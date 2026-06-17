use harness_skill::{parse_skill_markdown, SkillPlatform, SkillSource};

#[test]
fn rejects_secret_config_plaintext_default() {
    let error = parse_skill_markdown(
        r"---
name: secret-default
description: Unsafe config
config:
  - key: github.token
    type: string
    secret: true
    default: plaintext-token
---
Body
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect_err("secret defaults must be rejected");

    assert!(format!("{error}").contains("secret config `github.token` cannot declare a default"));
}

#[test]
fn rejects_typed_default_mismatch() {
    let error = parse_skill_markdown(
        r"---
name: invalid-default
description: Invalid default
parameters:
  - name: retries
    type: number
    default: many
---
Body
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect_err("typed default mismatch must be rejected");

    assert!(format!("{error}").contains("default must be number"));
}
