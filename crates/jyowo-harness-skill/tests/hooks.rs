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
    assert_eq!(bindings[0].handler_id, "skill:audit-skill:audit");
    assert_eq!(
        bindings[0].events,
        vec![HookEventKind::SessionStart, HookEventKind::PostToolUse]
    );
}
