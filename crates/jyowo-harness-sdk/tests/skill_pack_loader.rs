use harness_skill::{SkillParamType, SkillSource};
use jyowo_harness_sdk::skill_pack_loader::{
    LockedSkillPackFile, LockedSkillVersionSnapshot, SkillPackLoaderAdapter,
};
use serde_json::json;

#[test]
fn skill_pack_loader_converts_locked_snapshot_without_reading_latest_state() {
    let snapshot = locked_snapshot();
    let skill = SkillPackLoaderAdapter::default()
        .load_skill(&snapshot)
        .expect("locked skill snapshot should load");

    assert_eq!(skill.name, "contract-review");
    assert_eq!(skill.description, "Review contracts for payment risk.");
    assert!(matches!(skill.source, SkillSource::Workspace(_)));
    assert_eq!(
        skill.frontmatter.category.as_deref(),
        Some("legal-operations")
    );
    assert_eq!(skill.frontmatter.tags, ["contracts", "payments"]);
    assert_eq!(skill.frontmatter.parameters[0].name, "jurisdiction");
    assert_eq!(
        skill.frontmatter.parameters[0].param_type,
        SkillParamType::String
    );
    assert!(skill.frontmatter.parameters[0].required);
    assert_eq!(skill.frontmatter.config[0].key, "github.token");
    assert_eq!(
        skill.frontmatter.config[0].value_type,
        SkillParamType::String
    );
    assert!(skill.frontmatter.config[0].secret);

    let jyowo = skill
        .frontmatter
        .metadata
        .get("jyowo")
        .expect("jyowo metadata should be present");
    assert_eq!(jyowo["skill_id"], "skill-123");
    assert_eq!(jyowo["skill_version_id"], "version-456");
    assert_eq!(jyowo["semantic_version"], "1.2.3");
    assert_eq!(jyowo["pack_hash"], "sha256:pack");
    assert_eq!(jyowo["manifest_hash"], "sha256:manifest");
    assert_eq!(jyowo["permissions_hash"], "sha256:permissions");
    assert_eq!(jyowo["source"], "agent_binding");
    assert_eq!(jyowo["manifest"]["metadata"]["owner"], "legal");

    assert!(skill.body.contains("Use the provided jurisdiction."));
    assert!(!skill.body.contains("risk_level: critical"));
    assert!(!skill.body.contains("required_tools"));
}

#[test]
fn skill_pack_loader_rejects_missing_required_files() {
    let mut snapshot = locked_snapshot();
    snapshot
        .files
        .retain(|file| file.path != "permissions.yaml");

    let error = SkillPackLoaderAdapter::default()
        .load_skill(&snapshot)
        .expect_err("missing permissions.yaml should be rejected");

    assert!(
        error
            .to_string()
            .contains("missing required skill pack file `permissions.yaml`"),
        "{error}"
    );
}

#[cfg(feature = "testing")]
#[test]
fn harness_registers_locked_skill_version_snapshot() {
    use std::sync::Arc;

    use futures::executor::block_on;
    use harness_contracts::NoopRedactor;
    use jyowo_harness_sdk::{prelude::*, testing::*};

    block_on(async {
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        harness
            .register_locked_skill_versions(&[locked_snapshot()])
            .expect("locked snapshot should register");

        assert!(harness.skill_registry().get("contract-review").is_some());
    });
}

fn locked_snapshot() -> LockedSkillVersionSnapshot {
    LockedSkillVersionSnapshot {
        source: "agent_binding".to_owned(),
        skill_id: "skill-123".to_owned(),
        skill_version_id: "version-456".to_owned(),
        semantic_version: "1.2.3".to_owned(),
        name: "contract-review".to_owned(),
        pack_hash: "sha256:pack".to_owned(),
        manifest_hash: "sha256:manifest".to_owned(),
        permissions_hash: "sha256:permissions".to_owned(),
        manifest: json!({
            "name": "contract-review",
            "version": "1.2.3",
            "description": "Review contracts for payment risk.",
            "category": "legal-operations",
            "entry": "SKILL.md",
            "tags": ["contracts", "payments"],
            "metadata": {
                "owner": "legal"
            },
            "parameters": [
                {
                    "name": "jurisdiction",
                    "type": "string",
                    "required": true,
                    "description": "Contract jurisdiction"
                }
            ],
            "config": [
                {
                    "key": "github.token",
                    "type": "string",
                    "secret": true,
                    "required": true
                }
            ]
        }),
        permissions_summary: json!({
            "required_tools": ["file:read"],
            "risk_level": "critical"
        }),
        files: vec![
            LockedSkillPackFile {
                path: "SKILL.md".to_owned(),
                kind: Some("skill_md".to_owned()),
                content_hash: Some("sha256:skill-md".to_owned()),
                content: r#"---
name: stale-name-from-skill-md
description: Stale description from SKILL.md.
---
Use the provided jurisdiction.
"#
                .to_owned(),
            },
            LockedSkillPackFile {
                path: "manifest.yaml".to_owned(),
                kind: Some("manifest".to_owned()),
                content_hash: Some("sha256:manifest".to_owned()),
                content: "name: contract-review\nversion: 1.2.3\n".to_owned(),
            },
            LockedSkillPackFile {
                path: "permissions.yaml".to_owned(),
                kind: Some("permissions".to_owned()),
                content_hash: Some("sha256:permissions".to_owned()),
                content: "risk_level: critical\nrequired_tools:\n  - file:read\n".to_owned(),
            },
        ],
    }
}
