use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{Event, McpServerId, PluginId, TrustLevel};
use harness_skill::{
    BuiltinHookKind, DirectorySourceKind, SkillCompatMode, SkillConfigResolver, SkillEventSink,
    SkillHookTransport, SkillLoader, SkillPlatform, SkillPrefetchStrategy, SkillPrefetcher,
    SkillRegistration, SkillRegistry, SkillRejectReason, SkillSource, SkillSourceConfig,
    SkillValidator,
};
use parking_lot::Mutex;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

#[test]
fn frontmatter_parses_hook_transports() {
    let skill = harness_skill::parse_skill_markdown(
        r##"---
name: audit-skill
description: Skill with hooks
hooks:
  - id: audit
    events: [SessionStart]
    transport:
      type: builtin
      kind: AuditLog
  - id: notify
    events: [PostToolUse]
    transport:
      type: exec
      command: /usr/local/bin/notify
      args: ["--json"]
      timeout_ms: 5000
  - id: webhook
    events: [PostToolUseFailure]
    transport:
      type: http
      url: https://hooks.example.test/audit
      timeout_ms: 3000
      allowlist: ["hooks.example.test"]
---
Body
"##,
        SkillSource::Bundled,
        None,
        SkillPlatform::Macos,
    )
    .expect("hook transports should parse");

    assert!(matches!(
        skill.frontmatter.hooks[0].transport,
        SkillHookTransport::Builtin(BuiltinHookKind::AuditLog)
    ));
    assert!(matches!(
        skill.frontmatter.hooks[1].transport,
        SkillHookTransport::Exec(_)
    ));
    assert!(matches!(
        skill.frontmatter.hooks[2].transport,
        SkillHookTransport::Http(_)
    ));
}

#[test]
fn frontmatter_parses_nested_http_security() {
    let skill = harness_skill::parse_skill_markdown(
        r##"---
name: secure-webhook
description: Skill with secure hook
hooks:
  - id: webhook
    events: [PostToolUse]
    transport:
      type: http
      url: https://hooks.example.test/audit
      timeout_ms: 3000
      security:
        allowlist: ["hooks.example.test"]
        ssrf_guard: true
        max_redirects: 1
        max_body_bytes: 4096
        mtls_required: true
---
Body
"##,
        SkillSource::Bundled,
        None,
        SkillPlatform::Macos,
    )
    .expect("nested transport security should parse");

    let SkillHookTransport::Http(spec) = &skill.frontmatter.hooks[0].transport else {
        panic!("expected http transport");
    };
    assert_eq!(spec.security.allowlist, vec!["hooks.example.test"]);
    assert!(spec.security.ssrf_guard);
    assert_eq!(spec.security.max_redirects, 1);
    assert_eq!(spec.security.max_body_bytes, 4096);
    assert!(spec.security.mtls_required);
}

#[test]
fn strict_mode_rejects_legacy_http_allowlist_without_security() {
    let error = harness_skill::parse_skill_markdown_with_options(
        r##"---
name: strict-webhook
description: Strict webhook
hooks:
  - id: webhook
    events: [PostToolUse]
    transport:
      type: http
      url: https://hooks.example.test/audit
      allowlist: ["hooks.example.test"]
---
Body
"##,
        SkillSource::Bundled,
        None,
        SkillPlatform::Macos,
        SkillCompatMode::Strict,
    )
    .expect_err("strict mode should require nested transport.security");

    assert!(format!("{error}").contains("transport.security"));
}

#[tokio::test]
async fn skill_validator_enforces_reload_trust_matrix() {
    let validator = SkillValidator::default().with_runtime_platform(SkillPlatform::Macos);

    let user_exec = SkillRegistration {
        skill: harness_skill::parse_skill_markdown(
            &exec_hook_skill("user-exec"),
            SkillSource::User("home/skills".into()),
            None,
            SkillPlatform::Macos,
        )
        .expect("skill should parse"),
        force_allowlist: None,
    };
    let error = validator
        .validate_registration(&user_exec)
        .await
        .expect_err("user exec hook should reject");
    assert!(matches!(
        SkillRejectReason::from_error(&error),
        SkillRejectReason::HookTransportNotPermitted {
            trust: TrustLevel::UserControlled
        }
    ));

    let mcp_builtin = SkillRegistration {
        skill: harness_skill::parse_skill_markdown(
            r"---
name: mcp-hook
description: MCP hook
hooks:
  - id: audit
    events: [SessionStart]
---
Body
",
            SkillSource::Mcp(McpServerId("remote".to_owned())),
            None,
            SkillPlatform::Macos,
        )
        .expect("skill should parse"),
        force_allowlist: None,
    };
    let error = validator
        .validate_registration(&mcp_builtin)
        .await
        .expect_err("mcp builtin hook should reject");
    assert!(matches!(
        SkillRejectReason::from_error(&error),
        SkillRejectReason::HookTransportNotPermitted { .. }
    ));

    let trusted_http = SkillRegistration {
        skill: harness_skill::parse_skill_markdown(
            &http_hook_skill("trusted-http"),
            SkillSource::Plugin {
                plugin_id: PluginId("trusted-plugin".to_owned()),
                trust: TrustLevel::AdminTrusted,
            },
            None,
            SkillPlatform::Macos,
        )
        .expect("skill should parse"),
        force_allowlist: None,
    };
    validator
        .validate_registration(&trusted_http)
        .await
        .expect("trusted plugin http hook should validate");
}

#[tokio::test]
async fn loader_enforces_hook_trust_matrix() {
    let user_root = unique_temp_dir("skill-user-exec");
    std::fs::create_dir_all(&user_root).expect("temp dir");
    std::fs::write(user_root.join("unsafe.md"), exec_hook_skill("unsafe")).expect("write skill");

    let report = SkillLoader::default()
        .with_source(SkillSourceConfig::Directory {
            path: user_root.clone(),
            source_kind: DirectorySourceKind::User,
        })
        .with_runtime_platform(SkillPlatform::Macos)
        .load_all()
        .await
        .expect("load should continue after rejection");

    assert!(report.loaded.is_empty());
    assert!(matches!(
        report.rejected[0].reason,
        SkillRejectReason::HookTransportNotPermitted {
            trust: TrustLevel::UserControlled
        }
    ));

    let plugin_root = unique_temp_dir("skill-plugin-http");
    std::fs::create_dir_all(&plugin_root).expect("temp dir");
    std::fs::write(plugin_root.join("safe.md"), http_hook_skill("safe")).expect("write skill");

    let report = SkillLoader::default()
        .with_source(SkillSourceConfig::Directory {
            path: plugin_root.clone(),
            source_kind: DirectorySourceKind::Plugin {
                plugin_id: PluginId("trusted-plugin".to_owned()),
                trust: TrustLevel::AdminTrusted,
            },
        })
        .with_runtime_platform(SkillPlatform::Macos)
        .load_all()
        .await
        .expect("trusted plugin hook should load");

    assert!(report.rejected.is_empty());
    assert_eq!(report.loaded.len(), 1);

    let _ = std::fs::remove_dir_all(user_root);
    let _ = std::fs::remove_dir_all(plugin_root);
}

#[tokio::test]
async fn bundled_parse_failure_is_hard_fail() {
    let error = SkillLoader::default()
        .with_source(SkillSourceConfig::BundledRecords {
            records: vec![harness_skill::BundledSkillRecord {
                name: "broken: [".to_owned(),
                description: "broken".to_owned(),
                body: "Body".to_owned(),
            }],
        })
        .load_all()
        .await
        .expect_err("broken bundled skill should hard fail");

    assert!(format!("{error}").contains("parse frontmatter"));
}

#[test]
fn registry_same_source_duplicate_policy_requires_explicit_reload_for_changed_content() {
    let first = simple_skill("dup", SkillSource::Workspace("data/skills".into()));
    let same = simple_skill("dup", SkillSource::Workspace("data/skills".into()));
    let mut changed = simple_skill("dup", SkillSource::Workspace("data/skills".into()));
    changed.body = "changed body".to_owned();
    let registry = SkillRegistry::builder().with_skill(first).build();
    let before = registry.snapshot();

    registry
        .register(same)
        .expect("same source duplicate with unchanged content should be idempotent");
    let after_same = registry.snapshot();
    assert_eq!(before.generation, after_same.generation);
    assert_eq!(after_same.entries.len(), 1);

    let error = registry
        .register(changed.clone())
        .expect_err("changed same source duplicate should require explicit reload");
    let after_changed_register = registry.snapshot();

    assert!(matches!(error, harness_skill::SkillError::Duplicate(name) if name == "dup"));
    assert_eq!(before.generation, after_changed_register.generation);

    let candidate = registry
        .candidate_snapshot(&[SkillRegistration {
            skill: changed,
            force_allowlist: None,
        }])
        .expect("explicit reload path should accept changed same source skill");
    registry.commit_snapshot(candidate);
    let after_reload = registry.snapshot();
    assert_eq!(after_reload.generation, before.generation + 1);
    assert_eq!(
        after_reload.entries.get("dup").expect("skill").body,
        "changed body"
    );
}

#[test]
fn snapshot_keeps_old_generation_after_later_registration() {
    let registry = SkillRegistry::builder()
        .with_skill(simple_skill("one", SkillSource::Bundled))
        .build();
    let old = registry.snapshot();

    registry
        .register(simple_skill("two", SkillSource::User("home/skills".into())))
        .expect("new name should register");
    let new = registry.snapshot();

    assert_eq!(old.generation, 1);
    assert_eq!(old.entries.len(), 1);
    assert_eq!(new.generation, 2);
    assert_eq!(new.entries.len(), 2);
}

#[test]
fn deregister_from_plugin_returns_skill_bound_hook_handler_ids() {
    let plugin_id = PluginId("plugin@1.0.0".to_owned());
    let skill = harness_skill::parse_skill_markdown(
        r"---
name: plugin-skill
description: Plugin skill
hooks:
  - id: audit
    events: [SessionStart]
---
Body
",
        SkillSource::Bundled,
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");
    let registry = SkillRegistry::builder().build();

    registry
        .register_from_plugin(plugin_id.clone(), TrustLevel::AdminTrusted, skill)
        .expect("plugin skill should register");
    let removed = registry.deregister_from_plugin(&plugin_id, "plugin-skill");

    assert_eq!(removed, vec!["skill:plugin-skill:audit"]);
    assert!(registry.get("plugin-skill").is_none());
}

#[test]
fn strict_compat_rejects_unknown_top_level_frontmatter() {
    let error = harness_skill::parse_skill_markdown_with_options(
        r##"---
name: strict
description: Strict skill
unknown_top_level: true
---
Body
"##,
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
        SkillCompatMode::Strict,
    )
    .expect_err("strict mode should reject unknown top-level field");

    assert!(format!("{error}").contains("unknown top-level frontmatter field"));
}

#[tokio::test]
async fn command_prerequisite_missing_emits_advisory_event() {
    let root = unique_temp_dir("skill-command-advisory");
    std::fs::create_dir_all(&root).expect("temp dir");
    std::fs::write(
        root.join("needs-command.md"),
        r"---
name: needs-command
description: Needs missing command
prerequisites:
  commands: [jyowo_missing_command_for_test]
---
Body
",
    )
    .expect("write skill");
    let sink = Arc::new(RecordingSink::default());

    let report = SkillLoader::default()
        .with_source(SkillSourceConfig::Directory {
            path: root.clone(),
            source_kind: DirectorySourceKind::Workspace,
        })
        .with_runtime_platform(SkillPlatform::Macos)
        .with_event_sink(sink.clone())
        .load_all()
        .await
        .expect("advisory should not reject skill");

    assert_eq!(report.loaded.len(), 1);
    assert!(sink.events.lock().iter().any(|event| {
        matches!(event, Event::SkillPrerequisiteAdvisory(advisory)
            if advisory.skill_name == "needs-command"
                && advisory.commands == vec!["jyowo_missing_command_for_test"])
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn secret_resolver_uses_secret_string() {
    let skill = harness_skill::parse_skill_markdown(
        r"---
name: secret-render
description: Secret render
config:
  - key: github.token
    type: string
    secret: true
---
Token: ${config.github.token:secret}
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");

    let rendered = harness_skill::SkillRenderer::new(Arc::new(SecretResolver))
        .render(&skill, Value::Null)
        .await
        .expect("render should resolve secret");

    assert!(rendered.content.contains("s3cr3t"));
    assert_eq!(
        SecretString::new("s3cr3t".to_owned()).expose_secret(),
        "s3cr3t"
    );
}

#[tokio::test]
async fn prefetcher_eager_loads_registry_without_context_injection() {
    let root = unique_temp_dir("skill-prefetcher");
    std::fs::create_dir_all(&root).expect("temp dir");
    std::fs::write(root.join("prefetched.md"), simple_markdown("prefetched")).expect("write skill");
    let registry = SkillRegistry::builder().build();
    let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
        path: root.clone(),
        source_kind: DirectorySourceKind::Workspace,
    });

    let report = SkillPrefetcher::new(loader, registry.clone(), SkillPrefetchStrategy::Eager)
        .prefetch_all()
        .await
        .expect("prefetch should load");

    assert_eq!(report.loaded, 1);
    assert!(registry.get("prefetched").is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn prefetcher_strategies_control_registry_loading() {
    let root = unique_temp_dir("skill-prefetcher-strategies");
    std::fs::create_dir_all(&root).expect("temp dir");
    for name in ["one", "two", "three"] {
        std::fs::write(root.join(format!("{name}.md")), simple_markdown(name))
            .expect("write skill");
    }

    let disabled = SkillRegistry::builder().build();
    let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
        path: root.clone(),
        source_kind: DirectorySourceKind::Workspace,
    });
    let report = SkillPrefetcher::new(
        loader.clone(),
        disabled.clone(),
        SkillPrefetchStrategy::Disabled,
    )
    .prefetch_all()
    .await
    .expect("disabled prefetch should succeed");
    assert_eq!(report.loaded, 0);
    assert!(disabled.get("one").is_none());

    let lazy = SkillRegistry::builder().build();
    let report = SkillPrefetcher::new(
        loader.clone(),
        lazy.clone(),
        SkillPrefetchStrategy::LazyPerTurn { concurrency: 2 },
    )
    .prefetch_all()
    .await
    .expect("lazy prefetch should load up to concurrency");
    assert_eq!(report.loaded, 2);
    assert_eq!(lazy.snapshot().entries.len(), 2);

    let hinted = SkillRegistry::builder().build();
    let report = SkillPrefetcher::new(loader, hinted.clone(), SkillPrefetchStrategy::HintDriven)
        .prefetch_hints(["three"])
        .await
        .expect("hinted prefetch should load matching skill");
    assert_eq!(report.loaded, 1);
    assert!(hinted.get("three").is_some());
    assert!(hinted.get("one").is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn lazy_per_turn_does_not_force_load_beyond_concurrency() {
    let registry = SkillRegistry::builder().build();
    let loader = SkillLoader::default().with_source(SkillSourceConfig::BundledRecords {
        records: vec![
            harness_skill::BundledSkillRecord {
                name: "first".to_owned(),
                description: "First skill".to_owned(),
                body: "Body".to_owned(),
            },
            harness_skill::BundledSkillRecord {
                name: "broken: [".to_owned(),
                description: "Broken skill".to_owned(),
                body: "Body".to_owned(),
            },
        ],
    });

    let report = SkillPrefetcher::new(
        loader,
        registry.clone(),
        SkillPrefetchStrategy::LazyPerTurn { concurrency: 1 },
    )
    .prefetch_all()
    .await
    .expect("lazy prefetch should not parse records beyond concurrency");

    assert_eq!(report.loaded, 1);
    assert!(registry.get("first").is_some());
}

#[test]
fn registry_insert_and_skills_list_smoke_1000() {
    let registry = SkillRegistry::builder().build();
    for index in 0..1000 {
        registry
            .register(simple_skill(
                &format!("skill-{index:04}"),
                SkillSource::Bundled,
            ))
            .expect("skill should register");
    }

    let summaries = registry.list_summaries_for_agent(
        &harness_contracts::AgentId::from_u128(7),
        harness_skill::SkillFilter {
            include_prerequisite_missing: true,
            ..harness_skill::SkillFilter::default()
        },
    );

    assert_eq!(summaries.len(), 1000);
}

#[tokio::test]
#[ignore = "manual perf smoke; avoids machine-dependent CI timing"]
async fn markdown_load_and_skills_list_perf_1000() {
    let root = unique_temp_dir("skill-markdown-perf-1000");
    std::fs::create_dir_all(&root).expect("temp dir");
    for index in 0..1000 {
        std::fs::write(
            root.join(format!("skill-{index:04}.md")),
            simple_markdown(&format!("skill-{index:04}")),
        )
        .expect("write skill");
    }

    let registry = SkillRegistry::builder().build();
    let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
        path: root.clone(),
        source_kind: DirectorySourceKind::Workspace,
    });
    let report = loader
        .load_all()
        .await
        .expect("markdown skills should load");
    registry
        .register_batch(report.loaded)
        .expect("loaded skills should register");
    let listed = registry.list_summaries_for_agent(
        &harness_contracts::AgentId::from_u128(7),
        harness_skill::SkillFilter {
            include_prerequisite_missing: true,
            ..harness_skill::SkillFilter::default()
        },
    );

    assert_eq!(listed.len(), 1000);
    let _ = std::fs::remove_dir_all(root);
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

#[async_trait]
impl SkillEventSink for RecordingSink {
    async fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

struct SecretResolver;

#[async_trait]
impl SkillConfigResolver for SecretResolver {
    async fn resolve(&self, key: &str) -> Result<Value, harness_skill::ConfigResolveError> {
        Err(harness_skill::ConfigResolveError::UnknownKey(
            key.to_owned(),
        ))
    }

    async fn resolve_secret(
        &self,
        _key: &str,
    ) -> Result<SecretString, harness_skill::ConfigResolveError> {
        Ok(SecretString::new("s3cr3t".to_owned()))
    }
}

fn simple_skill(name: &str, source: SkillSource) -> harness_skill::Skill {
    harness_skill::parse_skill_markdown(&simple_markdown(name), source, None, SkillPlatform::Macos)
        .expect("simple skill should parse")
}

fn simple_markdown(name: &str) -> String {
    format!(
        r"---
name: {name}
description: Test skill
---
Body
"
    )
}

fn exec_hook_skill(name: &str) -> String {
    format!(
        r"---
name: {name}
description: Exec hook skill
hooks:
  - id: audit
    events: [SessionStart]
    transport:
      type: exec
      command: /usr/local/bin/audit
---
Body
"
    )
}

fn http_hook_skill(name: &str) -> String {
    format!(
        r##"---
name: {name}
description: Http hook skill
hooks:
  - id: audit
    events: [PostToolUse]
    transport:
      type: http
      url: https://hooks.example.test/audit
      allowlist: ["hooks.example.test"]
---
Body
"##
    )
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
