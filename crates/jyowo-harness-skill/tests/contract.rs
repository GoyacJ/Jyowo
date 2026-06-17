#![allow(clippy::needless_raw_string_hashes)]

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::AgentId;

use harness_skill::{
    ConfigResolveError, DirectorySourceKind, SkillConfigResolver, SkillFilter, SkillLoader,
    SkillPlatform, SkillRegistry, SkillSourceConfig,
};
use serde_json::{json, Value};

#[tokio::test]
async fn skill_contract_loads_lists_and_renders_workspace_skills() {
    let root = unique_temp_dir("skill-contract");
    std::fs::create_dir_all(&root).expect("temp dir");
    write_skill(&root, "daily", "briefing", "Daily ${topic}");
    write_skill(&root, "review", "code", "Review ${topic}");
    write_skill(&root, "release", "briefing", "Release ${topic}");

    let report = SkillLoader::default()
        .with_source(SkillSourceConfig::Directory {
            path: root.clone(),
            source_kind: DirectorySourceKind::Workspace,
        })
        .with_runtime_platform(SkillPlatform::Macos)
        .load_all()
        .await
        .expect("load should succeed");

    let registry = SkillRegistry::builder().with_skills(report.loaded).build();
    let summaries = registry.list_summaries_for_agent(
        &AgentId::from_u128(1),
        SkillFilter {
            tag: Some("briefing".to_owned()),
            include_prerequisite_missing: true,
            ..SkillFilter::default()
        },
    );

    assert_eq!(
        summaries
            .iter()
            .map(|summary| summary.name.as_str())
            .collect::<Vec<_>>(),
        vec!["daily", "release"]
    );

    let renderer = harness_skill::SkillRenderer::new(Arc::new(EmptyConfigResolver));
    let skill = registry.get("daily").expect("daily skill");
    let rendered = renderer
        .render(&skill, json!({ "topic": "M4" }))
        .await
        .expect("render should succeed");
    assert_eq!(rendered.content.trim(), "Daily M4");

    let _ = std::fs::remove_dir_all(root);
}

struct EmptyConfigResolver;

#[async_trait]
impl SkillConfigResolver for EmptyConfigResolver {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError> {
        Err(ConfigResolveError::UnknownKey(key.to_owned()))
    }

    async fn resolve_secret(&self, key: &str) -> Result<secrecy::SecretString, ConfigResolveError> {
        Err(ConfigResolveError::UnknownKey(key.to_owned()))
    }
}

fn write_skill(root: &std::path::Path, name: &str, tag: &str, body: &str) {
    std::fs::write(
        root.join(format!("{name}.md")),
        format!(
            r#"---
name: {name}
description: {name} skill
parameters:
  - name: topic
    type: string
    required: true
metadata:
  jyowo:
    tags: ["{tag}"]
---
{body}
"#
        ),
    )
    .expect("write skill");
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
