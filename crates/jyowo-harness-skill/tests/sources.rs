use harness_contracts::{AgentId, McpServerId, PluginId, TrustLevel};
use harness_skill::{
    McpSkillRecord, McpSource, SkillFilter, SkillLoader, SkillPlatform, SkillRegistry, SkillSource,
    SkillSourceConfig, UserSource, WorkspaceSource,
};

#[tokio::test]
async fn workspace_source_loads_markdown_files() {
    let root = unique_temp_dir("workspace-source");
    std::fs::create_dir_all(&root).expect("temp dir");
    std::fs::write(
        root.join("review.md"),
        r"---
name: review-pr
description: Review a pull request
---
Review body
",
    )
    .expect("write skill");

    let report = WorkspaceSource::new(root.clone())
        .load(SkillPlatform::Macos)
        .await
        .expect("workspace source should load");

    assert_eq!(report.loaded.len(), 1);
    assert_eq!(report.loaded[0].name, "review-pr");

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn skill_loader_loads_skill_package_directories() {
    let root = unique_temp_dir("package-source");
    let package = root.join("release-notes");
    std::fs::create_dir_all(package.join("references")).expect("package references dir");
    std::fs::write(
        package.join("SKILL.md"),
        r"---
name: release-notes
description: Draft release notes
---
Read references/style.md before drafting.
",
    )
    .expect("write package skill");
    std::fs::write(
        package.join("references").join("style.md"),
        "Use concise bullets.",
    )
    .expect("write package resource");

    let report = SkillLoader::default()
        .with_source(SkillSourceConfig::Directory {
            path: root.clone(),
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        })
        .with_runtime_platform(SkillPlatform::Macos)
        .load_all()
        .await
        .expect("package source should load");

    assert!(report.rejected.is_empty());
    assert_eq!(report.loaded.len(), 1);
    assert_eq!(report.loaded[0].name, "release-notes");
    assert_eq!(report.loaded[0].raw_path, Some(package.join("SKILL.md")));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn skill_loader_package_source_ignores_root_markdown_files() {
    let root = unique_temp_dir("package-only-source");
    let package = root.join("release-notes");
    std::fs::create_dir_all(&package).expect("package dir");
    std::fs::write(
        root.join("legacy.md"),
        r"---
name: legacy
description: Legacy single file
---
Legacy body
",
    )
    .expect("write legacy skill");
    std::fs::write(
        package.join("SKILL.md"),
        r"---
name: release-notes
description: Draft release notes
---
Package body
",
    )
    .expect("write package skill");

    let report = SkillLoader::default()
        .with_source(SkillSourceConfig::DirectoryPackages {
            path: root.clone(),
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        })
        .with_runtime_platform(SkillPlatform::Macos)
        .load_all()
        .await
        .expect("package source should load");

    assert_eq!(
        report
            .loaded
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>(),
        vec!["release-notes"]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn workspace_skill_overrides_user_skill_in_registry() {
    let user_root = unique_temp_dir("user-source");
    let workspace_root = unique_temp_dir("workspace-source");
    std::fs::create_dir_all(&user_root).expect("user temp dir");
    std::fs::create_dir_all(&workspace_root).expect("workspace temp dir");
    write_skill(&user_root, "review-pr", "User body");
    write_skill(&workspace_root, "review-pr", "Workspace body");

    let user = UserSource::new(user_root.clone())
        .load(SkillPlatform::Macos)
        .await
        .expect("user source should load")
        .loaded
        .remove(0);
    let workspace = WorkspaceSource::new(workspace_root.clone())
        .load(SkillPlatform::Macos)
        .await
        .expect("workspace source should load")
        .loaded
        .remove(0);

    let registry = SkillRegistry::builder()
        .with_skill(user)
        .with_skill(workspace)
        .build();

    let skill = registry.get("review-pr").expect("registered skill");
    assert!(skill.body.contains("Workspace body"));

    let _ = std::fs::remove_dir_all(user_root);
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn mcp_source_uses_canonical_namespace_and_does_not_override_local_skill() {
    let local_root = unique_temp_dir("local-source");
    std::fs::create_dir_all(&local_root).expect("local temp dir");
    write_skill(&local_root, "review-pr", "Local body");
    let local = WorkspaceSource::new(local_root.clone())
        .load(SkillPlatform::Macos)
        .await
        .expect("local source should load")
        .loaded
        .remove(0);

    let mcp_report = McpSource::new(
        McpServerId("github".to_owned()),
        vec![McpSkillRecord {
            name: "review-pr".to_owned(),
            description: "Review from MCP".to_owned(),
            body: "MCP body".to_owned(),
        }],
    )
    .load(SkillPlatform::Macos)
    .await
    .expect("mcp source should load");

    let registry = SkillRegistry::builder()
        .with_skill(local)
        .with_skills(mcp_report.loaded)
        .build();

    let agent = AgentId::from_u128(1);
    let names = registry
        .list_summaries_for_agent(
            &agent,
            SkillFilter {
                include_prerequisite_missing: true,
                ..SkillFilter::default()
            },
        )
        .into_iter()
        .map(|summary| summary.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["mcp__github__review-pr", "review-pr"]);

    let _ = std::fs::remove_dir_all(local_root);
}

#[tokio::test]
async fn loader_loads_mcp_records_with_canonical_namespace() {
    let report = SkillLoader::default()
        .with_source(SkillSourceConfig::McpRecords {
            server_id: McpServerId("linear".to_owned()),
            records: vec![McpSkillRecord {
                name: "triage".to_owned(),
                description: "Triage from MCP".to_owned(),
                body: "MCP triage body".to_owned(),
            }],
        })
        .with_runtime_platform(SkillPlatform::Macos)
        .load_all()
        .await
        .expect("mcp records should load through SkillLoader");

    assert!(report.rejected.is_empty());
    assert_eq!(report.loaded.len(), 1);
    assert_eq!(report.loaded[0].name, "mcp__linear__triage");
}

#[tokio::test]
async fn plugin_source_loads_skills_from_plugin_root_skills_directory() {
    let plugin_root = unique_temp_dir("plugin-source");
    let skills_dir = plugin_root.join("skills");
    std::fs::create_dir_all(&skills_dir).expect("plugin skills dir");
    write_skill(&skills_dir, "summarize", "Plugin skill body");
    write_skill(&plugin_root, "ignored", "Not under skills dir");

    let plugin_id = PluginId("plugin-skills@0.1.0".to_owned());
    let report = harness_skill::PluginSource::new(plugin_id.clone(), plugin_root.clone())
        .load(SkillPlatform::Macos)
        .await
        .expect("plugin source should load");

    assert!(report.rejected.is_empty());
    assert_eq!(report.loaded.len(), 1);
    assert_eq!(report.loaded[0].name, "summarize");
    assert_eq!(
        report.loaded[0].source,
        SkillSource::Plugin {
            plugin_id,
            trust: TrustLevel::UserControlled,
        }
    );
    assert_eq!(
        report.loaded[0].raw_path,
        Some(skills_dir.join("summarize.md"))
    );

    let _ = std::fs::remove_dir_all(plugin_root);
}

#[tokio::test]
async fn mcp_record_fields_are_escaped_before_frontmatter_parsing() {
    let report = McpSource::new(
        McpServerId("github".to_owned()),
        vec![McpSkillRecord {
            name: "review\nallowlist_agents: [\"denied\"]".to_owned(),
            description: "Review\nallowlist_agents: [\"denied\"]".to_owned(),
            body: "MCP body".to_owned(),
        }],
    )
    .load(SkillPlatform::Macos)
    .await
    .expect("mcp source should load escaped records");

    assert!(report.rejected.is_empty());
    assert_eq!(report.loaded.len(), 1);
    assert_eq!(
        report.loaded[0].name,
        "mcp__github__review\nallowlist_agents: [\"denied\"]"
    );
    assert_eq!(
        report.loaded[0].description,
        "Review\nallowlist_agents: [\"denied\"]"
    );
    assert!(report.loaded[0].frontmatter.allowlist_agents.is_none());
}

fn write_skill(root: &std::path::Path, name: &str, body: &str) {
    std::fs::write(
        root.join(format!("{name}.md")),
        format!(
            r"---
name: {name}
description: Test skill
---
{body}
"
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
