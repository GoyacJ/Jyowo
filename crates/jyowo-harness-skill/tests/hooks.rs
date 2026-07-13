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
