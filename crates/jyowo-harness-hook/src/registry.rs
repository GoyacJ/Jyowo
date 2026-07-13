use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use harness_contracts::{HookEventKind, PluginId, TrustLevel};
use parking_lot::RwLock;

use crate::HookHandler;

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum RegistrationError {
    #[error("duplicate hook handler id: {0}")]
    Duplicate(String),
    #[error("hook handler is owned by another registry: {0}")]
    OwnershipConflict(String),
    #[error("invalid hook handler: {0}")]
    InvalidHandler(String),
}

#[derive(Clone)]
pub struct HookRegistry {
    inner: Arc<RwLock<HookRegistryInner>>,
}

#[derive(Default)]
struct HookRegistryInner {
    handlers: Vec<RegisteredHook>,
    ids: HashSet<String>,
    origins: HashMap<String, HookOrigin>,
    generation: u64,
}

#[derive(Clone)]
struct RegisteredHook {
    handler: Arc<dyn HookHandler>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HookOrigin {
    Host,
    Plugin {
        plugin_id: PluginId,
        trust: TrustLevel,
    },
    Skill {
        owner: Arc<str>,
    },
}

impl HookRegistry {
    pub fn builder() -> HookRegistryBuilder {
        HookRegistryBuilder::new()
    }

    pub fn register(&self, handler: Box<dyn HookHandler>) -> Result<(), RegistrationError> {
        self.register_with_origin(handler, HookOrigin::Host)
    }

    pub fn register_from_plugin(
        &self,
        plugin_id: PluginId,
        trust: TrustLevel,
        handler: Box<dyn HookHandler>,
    ) -> Result<(), RegistrationError> {
        self.register_with_origin(handler, HookOrigin::Plugin { plugin_id, trust })
    }

    pub fn register_from_skill(
        &self,
        owner: Arc<str>,
        handler: Box<dyn HookHandler>,
    ) -> Result<bool, RegistrationError> {
        validate_handler(handler.as_ref())?;

        let id = handler.handler_id().to_owned();
        let handler: Arc<dyn HookHandler> = handler.into();
        let mut inner = self.inner.write();
        if inner.ids.contains(&id) {
            if matches!(
                inner.origins.get(&id),
                Some(HookOrigin::Skill { owner: existing }) if existing == &owner
            ) {
                return Ok(false);
            }
            return Err(RegistrationError::Duplicate(id));
        }

        inner.ids.insert(id.clone());
        inner.origins.insert(id, HookOrigin::Skill { owner });
        inner.handlers.push(RegisteredHook { handler });
        inner.generation += 1;
        Ok(true)
    }

    pub fn reconcile_skill_handlers(
        &self,
        owner: Arc<str>,
        handlers: Vec<Box<dyn HookHandler>>,
        reusable_ids: &HashSet<String>,
        remove_ids: &HashSet<String>,
    ) -> Result<(), RegistrationError> {
        let mut additions = Vec::with_capacity(handlers.len());
        let mut addition_ids = HashSet::with_capacity(handlers.len());
        for handler in handlers {
            validate_handler(handler.as_ref())?;
            let id = handler.handler_id().to_owned();
            if !addition_ids.insert(id.clone()) {
                return Err(RegistrationError::Duplicate(id));
            }
            additions.push((id, Arc::<dyn HookHandler>::from(handler)));
        }
        if let Some(id) = addition_ids.intersection(remove_ids).next() {
            return Err(RegistrationError::InvalidHandler(format!(
                "handler cannot be added and removed in one update: {id}"
            )));
        }

        let mut inner = self.inner.write();
        for (id, _) in &additions {
            if !inner.ids.contains(id) {
                continue;
            }
            let reusable = reusable_ids.contains(id)
                && matches!(
                    inner.origins.get(id),
                    Some(HookOrigin::Skill { owner: existing }) if existing == &owner
                );
            if !reusable {
                return Err(RegistrationError::Duplicate(id.clone()));
            }
        }
        for id in remove_ids {
            match inner.origins.get(id) {
                None => {}
                Some(HookOrigin::Skill { owner: existing }) if existing == &owner => {}
                Some(_) => return Err(RegistrationError::OwnershipConflict(id.clone())),
            }
        }

        let mut changed = false;
        for (id, handler) in additions {
            if inner.ids.contains(&id) {
                continue;
            }
            inner.ids.insert(id.clone());
            inner.origins.insert(
                id,
                HookOrigin::Skill {
                    owner: Arc::clone(&owner),
                },
            );
            inner.handlers.push(RegisteredHook { handler });
            changed = true;
        }
        if !remove_ids.is_empty() {
            let before = inner.handlers.len();
            inner
                .handlers
                .retain(|registered| !remove_ids.contains(registered.handler.handler_id()));
            changed |= inner.handlers.len() != before;
            for id in remove_ids {
                inner.ids.remove(id);
                inner.origins.remove(id);
            }
        }
        if changed {
            inner.generation += 1;
        }
        Ok(())
    }

    fn register_with_origin(
        &self,
        handler: Box<dyn HookHandler>,
        origin: HookOrigin,
    ) -> Result<(), RegistrationError> {
        validate_handler(handler.as_ref())?;

        let id = handler.handler_id().to_owned();
        let handler: Arc<dyn HookHandler> = handler.into();
        let mut inner = self.inner.write();
        if !inner.ids.insert(id.clone()) {
            return Err(RegistrationError::Duplicate(id));
        }

        inner.origins.insert(id, origin.clone());
        inner.handlers.push(RegisteredHook { handler });
        inner.generation += 1;
        Ok(())
    }

    pub fn snapshot(&self) -> HookRegistrySnapshot {
        let inner = self.inner.read();
        HookRegistrySnapshot::from_handlers(inner.handlers.clone(), inner.generation)
    }

    pub fn origin_for(&self, id: &str) -> Option<HookOrigin> {
        self.inner.read().origins.get(id).cloned()
    }

    pub fn deregister(&self, id: &str) {
        let mut inner = self.inner.write();
        if !inner.ids.remove(id) {
            return;
        }
        inner.origins.remove(id);
        inner
            .handlers
            .retain(|registered| registered.handler.handler_id() != id);
        inner.generation += 1;
    }

    pub fn deregister_from_plugin(&self, plugin_id: &PluginId, id: &str) -> bool {
        let mut inner = self.inner.write();
        if !matches!(
            inner.origins.get(id),
            Some(HookOrigin::Plugin {
                plugin_id: owner,
                ..
            }) if owner == plugin_id
        ) {
            return false;
        }
        inner.ids.remove(id);
        inner.origins.remove(id);
        inner
            .handlers
            .retain(|registered| registered.handler.handler_id() != id);
        inner.generation += 1;
        true
    }

    pub fn deregister_from_skill(&self, owner: &str, id: &str) -> bool {
        let mut inner = self.inner.write();
        if !matches!(
            inner.origins.get(id),
            Some(HookOrigin::Skill { owner: existing }) if existing.as_ref() == owner
        ) {
            return false;
        }
        inner.ids.remove(id);
        inner.origins.remove(id);
        inner
            .handlers
            .retain(|registered| registered.handler.handler_id() != id);
        inner.generation += 1;
        true
    }
}

pub struct HookRegistryBuilder {
    handlers: Vec<Box<dyn HookHandler>>,
}

impl HookRegistryBuilder {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_hook(mut self, handler: Box<dyn HookHandler>) -> Self {
        self.handlers.push(handler);
        self
    }

    pub fn build(self) -> Result<HookRegistry, RegistrationError> {
        let registry = HookRegistry {
            inner: Arc::new(RwLock::new(HookRegistryInner::default())),
        };

        for handler in self.handlers {
            registry.register(handler)?;
        }

        Ok(registry)
    }
}

impl Default for HookRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Default)]
pub struct HookRegistrySnapshot {
    handlers_by_event: Arc<HashMap<HookEventKind, Vec<Arc<dyn HookHandler>>>>,
    generation: u64,
}

impl HookRegistrySnapshot {
    fn from_handlers(handlers: Vec<RegisteredHook>, generation: u64) -> Self {
        let mut handlers_by_event: HashMap<HookEventKind, Vec<Arc<dyn HookHandler>>> =
            HashMap::new();

        for registered in handlers {
            for event in registered.handler.interested_events() {
                handlers_by_event
                    .entry(event.clone())
                    .or_default()
                    .push(Arc::clone(&registered.handler));
            }
        }

        for handlers in handlers_by_event.values_mut() {
            handlers.sort_by(|left, right| {
                right
                    .priority()
                    .cmp(&left.priority())
                    .then_with(|| left.handler_id().cmp(right.handler_id()))
            });
        }

        Self {
            handlers_by_event: Arc::new(handlers_by_event),
            generation,
        }
    }

    pub fn handlers_for(&self, event: HookEventKind) -> Vec<Arc<dyn HookHandler>> {
        self.handlers_by_event
            .get(&event)
            .cloned()
            .unwrap_or_default()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

fn validate_handler(handler: &dyn HookHandler) -> Result<(), RegistrationError> {
    if handler.handler_id().trim().is_empty() {
        return Err(RegistrationError::InvalidHandler(
            "handler_id must not be empty".to_owned(),
        ));
    }
    if handler.interested_events().is_empty() {
        return Err(RegistrationError::InvalidHandler(
            "interested_events must not be empty".to_owned(),
        ));
    }
    Ok(())
}
