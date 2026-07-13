#![allow(clippy::needless_raw_string_hashes)]

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::AgentId;
use harness_skill::{
    parse_skill_markdown, ConfigResolveError, SkillConfigResolver, SkillFilter, SkillPlatform,
    SkillRegistry, SkillRegistryService, SkillSource,
};
use serde_json::{json, Value};

#[test]
fn list_returns_metadata_without_body() {
    let registry = registry_with_skill(skill(
        "daily",
        "Daily skill",
        r#"
parameters:
  - name: topic
    type: string
    required: true
metadata:
  jyowo:
    tags: ["briefing"]
    category: operations
"#,
        "Daily ${topic}",
    ));
    let agent = AgentId::from_u128(1);

    let summaries = registry.list_summaries_for_agent(
        &agent,
        SkillFilter {
            tag: Some("briefing".to_owned()),
            include_prerequisite_missing: true,
            ..SkillFilter::default()
        },
    );

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].name, "daily");
    assert_eq!(summaries[0].description, "Daily skill");
    assert_eq!(summaries[0].category.as_deref(), Some("operations"));
}

#[test]
fn list_hides_prerequisite_missing_skills_unless_included() {
    let missing_env = format!("JYOWO_TEST_MISSING_ENV_{}", std::process::id());
    let registry = registry_with_skill(skill(
        "needs-env",
        "Needs env",
        &format!(
            r"
prerequisites:
  env_vars: [{missing_env}]
"
        ),
        "Body",
    ));
    let agent = AgentId::from_u128(1);

    assert!(registry
        .list_summaries_for_agent(&agent, SkillFilter::default())
        .is_empty());

    let summaries = registry.list_summaries_for_agent(
        &agent,
        SkillFilter {
            include_prerequisite_missing: true,
            ..SkillFilter::default()
        },
    );

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].name, "needs-env");
}

#[test]
fn view_controls_preview_and_full_body() {
    let body = "0123456789".repeat(140);
    let registry = registry_with_skill(skill("daily", "Daily skill", "", &body));
    let agent = AgentId::from_u128(1);

    let preview = registry
        .view(&agent, "daily", false)
        .expect("skill should be visible");
    assert_eq!(preview.body_preview.chars().count(), 1024);
    assert!(preview.body_full.is_none());

    let full = registry
        .view(&agent, "daily", true)
        .expect("skill should be visible");
    let expected_full_body = format!("{body}\n");
    assert_eq!(full.body_preview.chars().count(), 1024);
    assert_eq!(full.body_full.as_deref(), Some(expected_full_body.as_str()));
}

#[test]
fn view_exposes_config_keys_without_values_or_secret_flags() {
    let registry = registry_with_skill(skill(
        "configured",
        "Configured skill",
        r"
config:
  - key: github.token
    type: string
    secret: true
    required: true
  - key: github.org
    type: string
    default: jyowo
",
        "Token: ${config.github.token:secret}\nOrg: ${config.github.org}",
    ));

    let view = registry
        .view(&AgentId::from_u128(1), "configured", false)
        .expect("skill should be visible");

    assert_eq!(view.config_keys, vec!["github.token", "github.org"]);
    let serialized = serde_json::to_string(&view).expect("view serializes");
    assert!(!serialized.contains("jyowo"));
    assert!(!serialized.contains("secret-token"));
    assert!(!serialized.contains("\"secret\":true"));
}

#[tokio::test]
async fn invoke_returns_receipt_without_rendered_body() {
    let registry = registry_with_skill(skill(
        "daily",
        "Daily skill",
        r"
parameters:
  - name: topic
    type: string
    required: true
config:
  - key: github.org
    type: string
    required: true
",
        "Daily ${topic} for ${config.github.org}",
    ));
    let service = SkillRegistryService::new(
        registry,
        harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver)),
    );

    let receipt = service
        .invoke(&AgentId::from_u128(1), "daily", json!({ "topic": "M4" }))
        .await
        .expect("invoke should render and return a receipt");

    assert_eq!(receipt.skill_name, "daily");
    assert_eq!(receipt.bytes_injected, "Daily M4 for jyowo\n".len() as u64);
    assert_eq!(receipt.consumed_config_keys, vec!["github.org"]);
    assert!(!receipt.injection_id.0.is_empty());
}

#[tokio::test]
async fn invoke_respects_agent_allowlist() {
    let allowed = AgentId::from_u128(1);
    let denied = AgentId::from_u128(2);
    let registry = registry_with_skill(
        parse_skill_markdown(
            &format!(
                r#"---
name: private
description: Private skill
allowlist_agents: ["{}"]
---
Private body
"#,
                allowed
            ),
            SkillSource::Workspace("data/skills".into()),
            None,
            SkillPlatform::Macos,
        )
        .expect("skill should parse"),
    );
    let service = SkillRegistryService::new(
        registry,
        harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver)),
    );

    let error = service
        .invoke(&denied, "private", json!({}))
        .await
        .expect_err("denied agent should not invoke hidden skill");

    assert!(format!("{error}").contains("skill not visible"));
}

#[tokio::test]
async fn service_render_uses_snapshot_visibility_for_lookup() {
    let denied_in_current = AgentId::from_u128(2);
    let registry = registry_with_skill(skill("brief", "Snapshot skill", "", "snapshot body"));
    let turn_snapshot = registry.snapshot();
    let replacement = registry_with_skill(skill(
        "brief",
        "Current skill",
        &format!(
            r#"
allowlist_agents: ["{}"]
"#,
            AgentId::from_u128(1)
        ),
        "current body",
    ))
    .snapshot();
    registry
        .replace_source(
            SkillSource::Workspace("data/skills".into()),
            replacement
                .entries
                .values()
                .map(|skill| skill.as_ref().clone())
                .collect(),
        )
        .expect("current registry should be replaced");
    let service = SkillRegistryService::new(
        registry,
        harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver)),
    )
    .with_snapshot(turn_snapshot);

    let rendered = service
        .render(&denied_in_current, "brief", json!({}))
        .await
        .expect("render should use snapshot visibility");

    assert_eq!(rendered.content, "snapshot body\n");
}

#[tokio::test]
async fn capability_adapter_lists_views_and_renders() {
    let registry = registry_with_skill(skill(
        "daily",
        "Daily skill",
        r"
parameters:
  - name: topic
    type: string
    required: true
",
        "Daily ${topic}",
    ));
    let service = SkillRegistryService::new(
        registry,
        harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver)),
    );
    let cap: Arc<dyn harness_contracts::SkillRegistryCap> = Arc::new(service);
    let agent = AgentId::from_u128(1);

    let summaries = cap.list_summaries(
        &agent,
        harness_contracts::SkillFilter {
            include_prerequisite_missing: true,
            ..harness_contracts::SkillFilter::default()
        },
    );
    assert_eq!(summaries.len(), 1);

    let view = cap.view(&agent, "daily", false).expect("view");
    assert!(view.body_full.is_none());

    let rendered = cap
        .render(&agent, "daily".to_owned(), json!({ "topic": "M4" }))
        .await
        .expect("render");
    assert_eq!(rendered.content, "Daily M4\n");
}

#[tokio::test]
async fn script_preparation_resolves_only_declared_script_environment() {
    let package = tempfile::tempdir().unwrap();
    let script_dir = package.path().join("scripts");
    std::fs::create_dir_all(&script_dir).unwrap();
    std::fs::write(
        script_dir.join("collect.sh"),
        "#!/bin/sh\nprintf '%s:%s' \"$REGION\" \"$API_TOKEN\"\n",
    )
    .unwrap();
    let markdown = r#"---
name: collector
description: Collector
config:
  - key: region
    type: string
    required: true
  - key: apiToken
    type: string
    secret: true
    required: true
scripts:
  - id: collect
    path: scripts/collect.sh
    network: deny
    env:
      REGION:
        config: region
      API_TOKEN:
        config: apiToken
        secret: true
---
Token ${config.apiToken:secret}
"#;
    let skill_path = package.path().join("SKILL.md");
    std::fs::write(&skill_path, markdown).unwrap();
    let skill = parse_skill_markdown(
        markdown,
        SkillSource::Workspace(package.path().to_path_buf()),
        Some(skill_path),
        SkillPlatform::Macos,
    )
    .unwrap();
    let registry = registry_with_skill(skill);
    let service = SkillRegistryService::new(
        registry,
        harness_skill::SkillRenderer::new(Arc::new(ScriptConfigResolver)),
    );

    let prepared = service
        .prepare_script(
            &AgentId::from_u128(1),
            "collector",
            "collect",
            json!({ "query": "open" }),
        )
        .await
        .unwrap();

    assert_eq!(prepared.skill_name, "collector");
    assert_eq!(prepared.script_id, "collect");
    assert!(!prepared.package_hash.is_empty());
    assert_eq!(prepared.arguments, json!({ "query": "open" }));
    assert_eq!(prepared.env["REGION"], "cn-east");
    assert_eq!(prepared.env["API_TOKEN"], "secret-token");
    assert_eq!(
        prepared.declaration.secret_env_keys,
        std::collections::BTreeSet::from(["API_TOKEN".to_owned()])
    );
    assert!(prepared
        .files
        .iter()
        .any(|file| file.path == "scripts/collect.sh"));
    assert!(!format!("{prepared:?}").contains("secret-token"));

    let undeclared = service
        .prepare_script(&AgentId::from_u128(1), "collector", "missing", json!({}))
        .await
        .unwrap_err();
    assert!(undeclared.to_string().contains("undeclared"));

    let render_error = service
        .render(&AgentId::from_u128(1), "collector", json!({}))
        .await
        .unwrap_err();
    assert!(render_error.to_string().contains("cannot be interpolated"));
}

#[cfg(unix)]
#[tokio::test]
async fn script_preparation_rejects_package_symlinks() {
    use std::os::unix::fs::symlink;

    let package = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let script_dir = package.path().join("scripts");
    std::fs::create_dir_all(&script_dir).unwrap();
    symlink(outside.path(), script_dir.join("collect.sh")).unwrap();
    let markdown = r#"---
name: collector
description: Collector
scripts:
  - id: collect
    path: scripts/collect.sh
    network: deny
---
Collector
"#;
    let skill_path = package.path().join("SKILL.md");
    std::fs::write(&skill_path, markdown).unwrap();
    let skill = parse_skill_markdown(
        markdown,
        SkillSource::Workspace(package.path().to_path_buf()),
        Some(skill_path),
        SkillPlatform::Macos,
    )
    .unwrap();
    let service = SkillRegistryService::new(
        registry_with_skill(skill),
        harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver)),
    );

    let error = service
        .prepare_script(&AgentId::from_u128(1), "collector", "collect", json!({}))
        .await
        .expect_err("package symlink must be rejected");

    assert!(error.to_string().contains("symlink"));
}

struct TestConfigResolver;

struct ScriptConfigResolver;

#[async_trait]
impl SkillConfigResolver for ScriptConfigResolver {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError> {
        match key {
            "region" => Ok(json!("cn-east")),
            other => Err(ConfigResolveError::UnknownKey(other.to_owned())),
        }
    }

    async fn resolve_secret(&self, key: &str) -> Result<secrecy::SecretString, ConfigResolveError> {
        Err(ConfigResolveError::SecretInterpolationForbidden {
            skill_id: "workspace:collector".to_owned(),
            key: key.to_owned(),
        })
    }

    async fn resolve_secret_for_script(
        &self,
        skill_id: &harness_contracts::SkillId,
        key: &str,
    ) -> Result<secrecy::SecretString, ConfigResolveError> {
        assert_eq!(skill_id.0, "workspace:collector");
        match key {
            "apiToken" => Ok(secrecy::SecretString::from("secret-token".to_owned())),
            other => Err(ConfigResolveError::UnknownKey(other.to_owned())),
        }
    }
}

#[async_trait]
impl SkillConfigResolver for TestConfigResolver {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError> {
        match key {
            "github.org" => Ok(json!("jyowo")),
            other => Err(ConfigResolveError::UnknownKey(other.to_owned())),
        }
    }

    async fn resolve_secret(&self, key: &str) -> Result<secrecy::SecretString, ConfigResolveError> {
        Err(ConfigResolveError::UnknownKey(key.to_owned()))
    }
}

fn registry_with_skill(skill: harness_skill::Skill) -> SkillRegistry {
    SkillRegistry::builder().with_skill(skill).build()
}

fn skill(
    name: &str,
    description: &str,
    extra_frontmatter: &str,
    body: &str,
) -> harness_skill::Skill {
    parse_skill_markdown(
        &format!(
            r"---
name: {name}
description: {description}{extra_frontmatter}
---
{body}
"
        ),
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse")
}
