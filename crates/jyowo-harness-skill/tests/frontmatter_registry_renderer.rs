#![allow(clippy::needless_raw_string_hashes)]

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::AgentId;
use harness_skill::{
    parse_skill_markdown, ConfigResolveError, DirectorySourceKind, RenderError,
    SkillConfigResolver, SkillFilter, SkillLoader, SkillParamType, SkillPlatform, SkillRegistry,
    SkillSource, SkillSourceConfig,
};
use serde_json::{json, Value};

#[test]
fn frontmatter_parses_standard_fields() {
    let skill = parse_skill_markdown(
        r#"---
name: daily-briefing
description: Generate a daily briefing
allowlist_agents: ["00000000000000000000000001"]
platforms: [macos, linux]
prerequisites:
  env_vars: [GITHUB_TOKEN]
  commands: [gh]
parameters:
  - name: topic
    type: string
    required: true
config:
  - key: github.org
    type: string
    required: true
metadata:
  jyowo:
    tags: ["briefing", "reporting"]
    category: operations
---
# Briefing
Topic: ${topic}
"#,
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");

    assert_eq!(skill.name, "daily-briefing");
    assert_eq!(skill.description, "Generate a daily briefing");
    assert_eq!(skill.frontmatter.parameters[0].name, "topic");
    assert_eq!(
        skill.frontmatter.parameters[0].param_type,
        SkillParamType::String
    );
    assert_eq!(
        skill.frontmatter.prerequisites.env_vars,
        vec!["GITHUB_TOKEN"]
    );
    assert_eq!(skill.frontmatter.tags, vec!["briefing", "reporting"]);
    assert_eq!(skill.frontmatter.category.as_deref(), Some("operations"));
    assert!(skill.body.contains("Topic: ${topic}"));
}

#[test]
fn frontmatter_rejects_platform_mismatch() {
    let error = parse_skill_markdown(
        r"---
name: linux-only
description: Linux only
platforms: [linux]
---
Body
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect_err("platform mismatch should reject");

    assert!(format!("{error}").contains("platform mismatch"));
}

#[test]
fn registry_filters_by_agent_allowlist() {
    let allowed_agent = AgentId::from_u128(1);
    let denied_agent = AgentId::from_u128(2);
    let skill = parse_skill_markdown(
        &format!(
            r#"---
name: review-pr
description: Review a pull request
allowlist_agents: ["{}"]
---
Review carefully.
"#,
            allowed_agent
        ),
        SkillSource::User("/tmp/user-skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");

    let registry = SkillRegistry::builder().with_skill(skill).build();

    assert_eq!(
        registry
            .list_summaries_for_agent(&allowed_agent, SkillFilter::default())
            .len(),
        1
    );
    assert!(registry
        .list_summaries_for_agent(&denied_agent, SkillFilter::default())
        .is_empty());
}

#[tokio::test]
async fn renderer_substitutes_parameters_and_config_values() {
    let skill = parse_skill_markdown(
        r"---
name: daily-briefing
description: Generate a daily briefing
parameters:
  - name: topic
    type: string
    required: true
config:
  - key: github.org
    type: string
    required: true
---
Topic: ${topic}
Org: ${config.github.org}
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");

    let renderer = harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver));
    let rendered = renderer
        .render(&skill, json!({ "topic": "release" }))
        .await
        .expect("render should succeed");

    assert_eq!(rendered.skill_name, "daily-briefing");
    assert!(rendered.content.contains("Topic: release"));
    assert!(rendered.content.contains("Org: jyowo"));
    assert_eq!(rendered.consumed_config_keys, vec!["github.org"]);
}

#[tokio::test]
async fn renderer_requires_declared_required_parameters() {
    let skill = parse_skill_markdown(
        r"---
name: daily-briefing
description: Generate a daily briefing
parameters:
  - name: topic
    type: string
    required: true
---
Topic: ${topic}
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");

    let renderer = harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver));
    let error = renderer
        .render(&skill, json!({}))
        .await
        .expect_err("missing required parameter should fail");

    assert!(matches!(error, RenderError::MissingParam(name) if name == "topic"));
}

#[tokio::test]
async fn renderer_replaces_disallowed_shell_with_safe_notice() {
    let skill = parse_skill_markdown(
        r"---
name: shell-example
description: Demonstrate shell notice
---
Today is !`printf hidden`.
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");

    let renderer = harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver));
    let rendered = renderer
        .render(&skill, json!({}))
        .await
        .expect("render should succeed");

    assert!(rendered.content.contains("[SHELL_NOT_ALLOWED]"));
    assert!(rendered.shell_invocations.is_empty());
}

#[tokio::test]
async fn loader_loads_directory_sources() {
    let root = unique_temp_dir("skill-loader");
    std::fs::create_dir_all(&root).expect("temp dir");
    std::fs::write(
        root.join("daily.md"),
        r"---
name: daily
description: Daily skill
---
Daily body
",
    )
    .expect("write skill");

    let report = SkillLoader::default()
        .with_source(SkillSourceConfig::Directory {
            path: root.clone(),
            source_kind: DirectorySourceKind::Workspace,
        })
        .with_runtime_platform(SkillPlatform::Macos)
        .load_all()
        .await
        .expect("load should succeed");

    assert_eq!(report.loaded.len(), 1);
    assert_eq!(report.loaded[0].name, "daily");
    assert!(report.rejected.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

struct TestConfigResolver;

#[async_trait]
impl SkillConfigResolver for TestConfigResolver {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError> {
        match key {
            "github.org" => Ok(json!("jyowo")),
            other => Err(ConfigResolveError::UnknownKey(other.to_owned())),
        }
    }

    async fn resolve_secret(&self, key: &str) -> Result<secrecy::SecretString, ConfigResolveError> {
        match key {
            "github.token" => Ok(secrecy::SecretString::new("secret-token".to_owned())),
            other => Err(ConfigResolveError::UnknownKey(other.to_owned())),
        }
    }
}

fn unique_temp_dir(name: &str) -> std::path::PathBuf {
    let nonce = format!(
        "{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    );
    std::env::temp_dir().join(nonce)
}
