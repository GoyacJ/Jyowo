use harness_contracts::HookEventKind;
use harness_skill::{parse_skill_markdown, SkillPlatform, SkillRegistry, SkillSource};

#[test]
fn skill_registry_exposes_hook_bindings() {
    let skill = parse_skill_markdown(
        r"---
name: audit-skill
description: Skill with hooks
hooks:
  - id: audit
    events: [SessionStart, PostToolUse]
    transport:
      type: builtin
      kind: AuditLog
---
Body
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");
    let registry = SkillRegistry::builder().with_skill(skill).build();

    let bindings = registry.hook_bindings();

    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].logical_id, "skill:audit-skill:audit");
    assert!(bindings[0]
        .handler_id
        .starts_with("skill:audit-skill:audit:"));
    assert_eq!(
        bindings[0].events,
        vec![HookEventKind::SessionStart, HookEventKind::PostToolUse]
    );
}

#[test]
fn hook_handler_id_changes_when_source_or_declaration_changes() {
    let workspace = hook_skill(
        SkillSource::Workspace("data/skills".into()),
        "events: [SessionStart]",
    );
    let user = hook_skill(
        SkillSource::User("home/skills".into()),
        "events: [SessionStart]",
    );
    let declaration_changed = hook_skill(
        SkillSource::Workspace("data/skills".into()),
        "events: [PostToolUse]",
    );

    let workspace_binding = SkillRegistry::builder()
        .with_skill(workspace)
        .build()
        .hook_bindings()
        .remove(0);
    let user_binding = SkillRegistry::builder()
        .with_skill(user)
        .build()
        .hook_bindings()
        .remove(0);
    let changed_binding = SkillRegistry::builder()
        .with_skill(declaration_changed)
        .build()
        .hook_bindings()
        .remove(0);

    assert_eq!(workspace_binding.logical_id, user_binding.logical_id);
    assert_eq!(workspace_binding.logical_id, changed_binding.logical_id);
    assert_ne!(workspace_binding.handler_id, user_binding.handler_id);
    assert_ne!(workspace_binding.handler_id, changed_binding.handler_id);
}

#[test]
fn hook_handler_id_changes_when_transport_changes() {
    let builtin = hook_skill_with_transport(
        r"transport:
      type: builtin
      kind: AuditLog",
    );
    let http = hook_skill_with_transport(
        r##"transport:
      type: http
      url: https://hooks.example.test/audit
      security:
        allowlist: ["hooks.example.test"]"##,
    );

    let builtin_binding = SkillRegistry::builder()
        .with_skill(builtin)
        .build()
        .hook_bindings()
        .remove(0);
    let http_binding = SkillRegistry::builder()
        .with_skill(http)
        .build()
        .hook_bindings()
        .remove(0);

    assert_eq!(builtin_binding.logical_id, http_binding.logical_id);
    assert_ne!(builtin_binding.handler_id, http_binding.handler_id);
}

#[cfg(unix)]
#[test]
fn hook_handler_id_preserves_non_utf8_source_path_bytes() {
    use std::os::unix::ffi::OsStringExt;

    let first = hook_skill(
        SkillSource::Workspace(std::ffi::OsString::from_vec(vec![b'a', 0x80]).into()),
        "events: [SessionStart]",
    );
    let second = hook_skill(
        SkillSource::Workspace(std::ffi::OsString::from_vec(vec![b'a', 0x81]).into()),
        "events: [SessionStart]",
    );

    let first_id = SkillRegistry::builder()
        .with_skill(first)
        .build()
        .hook_bindings()
        .remove(0)
        .handler_id;
    let second_id = SkillRegistry::builder()
        .with_skill(second)
        .build()
        .hook_bindings()
        .remove(0)
        .handler_id;

    assert_ne!(first_id, second_id);
}

#[test]
fn hook_handler_id_separates_adjacent_transport_fields() {
    let first_args = ["ab", "c"];
    let second_args = ["a", "bc"];
    assert_eq!(first_args.concat(), second_args.concat());

    let first = hook_skill_with_transport(
        r"transport:
      type: exec
      command: /usr/bin/audit
      args: [ab, c]",
    );
    let second = hook_skill_with_transport(
        r"transport:
      type: exec
      command: /usr/bin/audit
      args: [a, bc]",
    );

    let first_id = SkillRegistry::builder()
        .with_skill(first)
        .build()
        .hook_bindings()
        .remove(0)
        .handler_id;
    let second_id = SkillRegistry::builder()
        .with_skill(second)
        .build()
        .hook_bindings()
        .remove(0)
        .handler_id;

    assert_ne!(first_id, second_id);
}

fn hook_skill(source: SkillSource, events: &str) -> harness_skill::Skill {
    parse_skill_markdown(
        &format!(
            r"---
name: audit-skill
description: Skill with hooks
hooks:
  - id: audit
    {events}
    transport:
      type: builtin
      kind: AuditLog
---
Body
"
        ),
        source,
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse")
}

fn hook_skill_with_transport(transport: &str) -> harness_skill::Skill {
    parse_skill_markdown(
        &format!(
            r"---
name: audit-skill
description: Skill with hooks
hooks:
  - id: audit
    events: [SessionStart]
    {transport}
---
Body
"
        ),
        SkillSource::Plugin {
            plugin_id: harness_contracts::PluginId("trusted-plugin".to_owned()),
            trust: harness_contracts::TrustLevel::AdminTrusted,
        },
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse")
}
