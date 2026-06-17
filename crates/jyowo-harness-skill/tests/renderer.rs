#![allow(clippy::needless_raw_string_hashes)]

use std::sync::Arc;

use async_trait::async_trait;
use harness_skill::{
    parse_skill_markdown, ConfigResolveError, RenderError, SkillConfigResolver, SkillPlatform,
    SkillSource,
};
use serde_json::{json, Value};

#[tokio::test]
async fn rejects_wrong_parameter_type() {
    let skill = parse_skill_markdown(
        r#"---
name: typed
description: Typed params
parameters:
  - name: retries
    type: number
    required: true
---
Retries: ${retries}
"#,
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");
    let renderer = harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver));

    let error = renderer
        .render(&skill, json!({ "retries": "three" }))
        .await
        .expect_err("wrong param type must fail");

    assert!(matches!(error, RenderError::InvalidParam { name, expected }
        if name == "retries" && expected == "number"));
}

#[tokio::test]
async fn shell_invocation_uses_structured_command_args() {
    let skill = parse_skill_markdown(
        r#"---
name: shell
description: Shell params
---
Today: !`printf "hi %s" jyowo`.
"#,
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");
    let renderer = harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver))
        .with_shell_allowlist(["printf".to_owned()]);

    let rendered = renderer
        .render(&skill, json!({}))
        .await
        .expect("render should succeed");

    assert!(rendered.content.contains("Today: hi jyowo."));
    assert_eq!(rendered.shell_invocations.len(), 1);
    assert_eq!(
        rendered.shell_invocations[0].command,
        r#"printf "hi %s" jyowo"#
    );
}

#[tokio::test]
async fn shell_metacharacters_are_not_executed() {
    let skill = parse_skill_markdown(
        r#"---
name: shell-block
description: Shell block
---
Value: !`printf ok; printf unsafe`.
"#,
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");
    let renderer = harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver))
        .with_shell_allowlist(["printf".to_owned()]);

    let rendered = renderer
        .render(&skill, json!({}))
        .await
        .expect("render should replace blocked shell");

    assert!(rendered.content.contains("[SHELL_NOT_ALLOWED]"));
    assert!(rendered.shell_invocations.is_empty());
}

#[tokio::test]
async fn shell_control_operators_subshells_and_redirects_are_blocked() {
    for command in [
        "printf ok | cat",
        "printf ok > /tmp/out",
        "printf $(whoami)",
        "printf ok && whoami",
        "printf ok\nwhoami",
    ] {
        let skill = parse_skill_markdown(
            &format!(
                r#"---
name: shell-boundary
description: Shell boundary
---
Value: !`{command}`.
"#
            ),
            SkillSource::Workspace("data/skills".into()),
            None,
            SkillPlatform::Macos,
        )
        .expect("skill should parse");
        let renderer = harness_skill::SkillRenderer::new(Arc::new(TestConfigResolver))
            .with_shell_allowlist(["printf".to_owned(), "cat".to_owned(), "whoami".to_owned()]);

        let rendered = renderer
            .render(&skill, json!({}))
            .await
            .expect("render should replace blocked shell");

        assert!(
            rendered.content.contains("[SHELL_NOT_ALLOWED]"),
            "command should be blocked: {command}"
        );
        assert!(
            rendered.shell_invocations.is_empty(),
            "blocked command must not execute: {command}"
        );
    }
}

struct TestConfigResolver;

#[async_trait]
impl SkillConfigResolver for TestConfigResolver {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError> {
        Err(ConfigResolveError::UnknownKey(key.to_owned()))
    }

    async fn resolve_secret(&self, key: &str) -> Result<secrecy::SecretString, ConfigResolveError> {
        Err(ConfigResolveError::UnknownKey(key.to_owned()))
    }
}
