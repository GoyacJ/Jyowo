use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
};

use harness_contracts::{SkillConfigDocument, SkillSecretMetadata};
use harness_skill::{SkillConfigDecl, SkillParamType};
use jyowo_harness_sdk::skill_config::{SecretString, SkillConfigSnapshot, SkillSecretStore};
use serde_json::Value;

use crate::commands::error::{
    invalid_payload, runtime_operation_failed, skill_config_commit_indeterminate,
    skill_config_compensation_failed, CommandErrorPayload,
};
use crate::storage_layout::StorageLayout;

use super::{ensure_app_dir_no_symlink, read_json_file, write_json_file_atomic};

static MUTATION_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

#[doc(hidden)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SkillConfigStoreFault {
    SaveFail,
    SaveAndReadbackFail,
}

struct SkillConfigStoreFaultState {
    fault: SkillConfigStoreFault,
    fail_next_readback: AtomicBool,
}

#[derive(Clone)]
pub struct DesktopSkillConfigStore {
    layout: StorageLayout,
    secret_store: Arc<dyn SkillSecretStore>,
    mutation_lock: Arc<Mutex<()>>,
    fault: Option<Arc<SkillConfigStoreFaultState>>,
    snapshot_load_hook: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl DesktopSkillConfigStore {
    pub fn new(layout: StorageLayout, secret_store: Arc<dyn SkillSecretStore>) -> Self {
        let mutation_lock = mutation_lock_for_path(layout.global_skill_config_file());
        Self {
            layout,
            secret_store,
            mutation_lock,
            fault: None,
            snapshot_load_hook: None,
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_fault_for_test(mut self, fault: SkillConfigStoreFault) -> Self {
        self.fault = Some(Arc::new(SkillConfigStoreFaultState {
            fault,
            fail_next_readback: AtomicBool::new(false),
        }));
        self
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_snapshot_load_hook_for_test(mut self, hook: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.snapshot_load_hook = Some(hook);
        self
    }

    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    pub fn load_document(&self) -> Result<SkillConfigDocument, CommandErrorPayload> {
        let path = self.layout.global_skill_config_file();
        let parent = path.parent().ok_or_else(|| {
            runtime_operation_failed("skill config path has no parent directory".to_owned())
        })?;
        ensure_app_dir_no_symlink(parent, "skill config directory")?;
        if self
            .fault
            .as_ref()
            .is_some_and(|fault| fault.fail_next_readback.swap(false, Ordering::SeqCst))
        {
            return Err(runtime_operation_failed(
                "injected skill config readback failure".to_owned(),
            ));
        }
        let document: SkillConfigDocument =
            read_json_file(&path, "skill config")?.unwrap_or_default();
        if document.version != SkillConfigDocument::CURRENT_VERSION {
            return Err(invalid_payload(format!(
                "unsupported skill config document version {}",
                document.version
            )));
        }
        Ok(document)
    }

    pub fn load_snapshot(&self) -> Result<SkillConfigSnapshot, CommandErrorPayload> {
        if let Some(hook) = &self.snapshot_load_hook {
            hook();
        }
        SkillConfigSnapshot::from_document(self.load_document()?, self.secret_store.clone())
            .map_err(secret_store_error)
    }

    pub fn set_public_value(
        &self,
        skill_id: &str,
        declaration: &SkillConfigDecl,
        value: Value,
    ) -> Result<(), CommandErrorPayload> {
        validate_declaration(skill_id, declaration, false)?;
        validate_public_value(declaration, &value)?;
        let _guard = self.lock_mutation()?;
        let mut document = self.load_document()?;
        let original_document = document.clone();
        let previous = self
            .secret_store
            .get(skill_id, &declaration.key)
            .map_err(secret_store_error)?;
        self.secret_store
            .delete(skill_id, &declaration.key)
            .map_err(secret_store_error)?;
        let entry = document.skills.entry(skill_id.to_owned()).or_default();
        entry.values.insert(declaration.key.clone(), value);
        entry.secrets.insert(
            declaration.key.clone(),
            SkillSecretMetadata { configured: false },
        );
        self.save_changed_document(
            &original_document,
            &document,
            skill_id,
            &declaration.key,
            previous,
        )
    }

    pub fn set_secret(
        &self,
        skill_id: &str,
        declaration: &SkillConfigDecl,
        value: SecretString,
    ) -> Result<(), CommandErrorPayload> {
        validate_declaration(skill_id, declaration, true)?;
        let _guard = self.lock_mutation()?;
        let key = declaration.key.as_str();
        let mut document = self.load_document()?;
        let original_document = document.clone();
        let previous = self
            .secret_store
            .get(skill_id, key)
            .map_err(secret_store_error)?;
        self.secret_store
            .set(skill_id, key, value)
            .map_err(secret_store_error)?;
        let entry = document.skills.entry(skill_id.to_owned()).or_default();
        entry.values.remove(key);
        entry
            .secrets
            .insert(key.to_owned(), SkillSecretMetadata { configured: true });
        self.save_changed_document(&original_document, &document, skill_id, key, previous)
    }

    pub fn clear_secret(
        &self,
        skill_id: &str,
        declaration: &SkillConfigDecl,
    ) -> Result<(), CommandErrorPayload> {
        validate_declaration(skill_id, declaration, true)?;
        let _guard = self.lock_mutation()?;
        let key = declaration.key.as_str();
        let mut document = self.load_document()?;
        let original_document = document.clone();
        let previous = self
            .secret_store
            .get(skill_id, key)
            .map_err(secret_store_error)?;
        self.secret_store
            .delete(skill_id, key)
            .map_err(secret_store_error)?;
        let entry = document.skills.entry(skill_id.to_owned()).or_default();
        entry.values.remove(key);
        entry
            .secrets
            .insert(key.to_owned(), SkillSecretMetadata { configured: false });
        self.save_changed_document(&original_document, &document, skill_id, key, previous)
    }

    fn save_document(&self, document: &SkillConfigDocument) -> Result<(), CommandErrorPayload> {
        if let Some(fault) = &self.fault {
            if fault.fault == SkillConfigStoreFault::SaveAndReadbackFail {
                fault.fail_next_readback.store(true, Ordering::SeqCst);
            }
            return Err(runtime_operation_failed(
                "injected skill config write failure".to_owned(),
            ));
        }
        write_json_file_atomic(
            &self.layout.global_skill_config_file(),
            "skill config",
            document,
        )
    }

    fn save_with_compensation(
        &self,
        document: &SkillConfigDocument,
        skill_id: &str,
        key: &str,
        previous: Option<SecretString>,
    ) -> Result<(), CommandErrorPayload> {
        let Err(error) = self.save_document(document) else {
            return Ok(());
        };
        match self.load_document() {
            Ok(committed) if committed == *document => Err(error),
            Ok(_) => {
                restore_secret(self.secret_store.as_ref(), skill_id, key, previous)?;
                Err(error)
            }
            Err(_) => Err(skill_config_commit_indeterminate()),
        }
    }

    fn save_changed_document(
        &self,
        original: &SkillConfigDocument,
        updated: &SkillConfigDocument,
        skill_id: &str,
        key: &str,
        previous: Option<SecretString>,
    ) -> Result<(), CommandErrorPayload> {
        if original == updated {
            return Ok(());
        }
        self.save_with_compensation(updated, skill_id, key, previous)
    }

    fn lock_mutation(&self) -> Result<std::sync::MutexGuard<'_, ()>, CommandErrorPayload> {
        self.mutation_lock
            .lock()
            .map_err(|_| runtime_operation_failed("skill config store unavailable".to_owned()))
    }
}

fn mutation_lock_for_path(path: PathBuf) -> Arc<Mutex<()>> {
    let locks = MUTATION_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&path).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(path, Arc::downgrade(&lock));
    lock
}

fn validate_declaration(
    skill_id: &str,
    declaration: &SkillConfigDecl,
    expect_secret: bool,
) -> Result<(), CommandErrorPayload> {
    if skill_id.trim().is_empty() {
        return Err(invalid_payload("skill id is required".to_owned()));
    }
    if declaration.key.trim().is_empty() {
        return Err(invalid_payload("skill config key is invalid".to_owned()));
    }
    if declaration.secret != expect_secret {
        return Err(invalid_payload(if expect_secret {
            "skill config is not secret".to_owned()
        } else {
            "secret skill config must use secret storage".to_owned()
        }));
    }
    Ok(())
}

fn validate_public_value(
    declaration: &SkillConfigDecl,
    value: &Value,
) -> Result<(), CommandErrorPayload> {
    let valid = match declaration.value_type {
        SkillParamType::String | SkillParamType::Path | SkillParamType::Url => value.is_string(),
        SkillParamType::Number => value.is_number(),
        SkillParamType::Boolean => value.is_boolean(),
    };
    if valid {
        Ok(())
    } else {
        Err(invalid_payload(format!(
            "skill config `{}` has an invalid value type",
            declaration.key
        )))
    }
}

fn secret_store_error(
    _: jyowo_harness_sdk::skill_config::SkillConfigStoreError,
) -> CommandErrorPayload {
    runtime_operation_failed("skill secret store operation failed".to_owned())
}

fn restore_secret(
    store: &dyn SkillSecretStore,
    skill_id: &str,
    key: &str,
    previous: Option<SecretString>,
) -> Result<(), CommandErrorPayload> {
    match previous {
        Some(secret) => store.set(skill_id, key, secret),
        None => store.delete(skill_id, key),
    }
    .map_err(|_| skill_config_compensation_failed())
}
