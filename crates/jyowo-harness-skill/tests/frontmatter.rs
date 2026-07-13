use std::path::PathBuf;

use harness_skill::{parse_skill_markdown, SkillPlatform, SkillScriptNetworkPolicy, SkillSource};

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

#[test]
fn parses_declared_script_policy() {
    let skill = parse_skill_markdown(
        r"---
name: script-policy
description: Declared script policy
config:
  - key: apiToken
    type: string
    secret: true
scripts:
  - id: collect
    path: scripts/collect.sh
    timeoutSeconds: 12
    network: deny
    env:
      API_TOKEN:
        config: apiToken
        secret: true
    maxStdoutBytes: 100
    maxStderrBytes: 101
    maxOutputBytes: 150
    maxArtifactCount: 2
    maxArtifactBytes: 512
---
Body
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("valid script declaration should parse");

    let script = skill
        .frontmatter
        .scripts
        .first()
        .expect("script should be declared");
    assert_eq!(script.id, "collect");
    assert_eq!(script.path, PathBuf::from("scripts/collect.sh"));
    assert_eq!(script.timeout_seconds, 12);
    assert_eq!(script.network, SkillScriptNetworkPolicy::Deny);
    assert_eq!(script.env["API_TOKEN"].config, "apiToken");
    assert!(script.env["API_TOKEN"].secret);
    assert_eq!(script.max_stdout_bytes, 100);
    assert_eq!(script.max_stderr_bytes, 101);
    assert_eq!(script.max_output_bytes, 150);
    assert_eq!(script.max_artifact_count, 2);
    assert_eq!(script.max_artifact_bytes, 512);
}

#[test]
fn defaults_script_to_bounded_network_denied_policy() {
    let skill = parse_skill_markdown(
        r"---
name: script-defaults
description: Default script policy
scripts:
  - id: collect
    path: scripts/collect.sh
---
Body
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("script defaults should parse");

    let script = &skill.frontmatter.scripts[0];
    assert_eq!(script.network, SkillScriptNetworkPolicy::Deny);
    assert!((1..=300).contains(&script.timeout_seconds));
    assert!(script.max_stdout_bytes > 0);
    assert!(script.max_stderr_bytes > 0);
    assert!(script.max_output_bytes > 0);
    assert!(script.max_artifact_count > 0);
    assert!(script.max_artifact_bytes > 0);
}

#[test]
fn rejects_duplicate_script_ids() {
    let error = parse_script_error(
        r"scripts:
  - id: collect
    path: scripts/collect.sh
  - id: collect
    path: scripts/other.sh",
    );

    assert!(error.contains("duplicate script id: collect"));
}

#[test]
fn rejects_script_paths_outside_package() {
    for path in ["/tmp/run.sh", "../run.sh", "scripts/../../run.sh"] {
        let error = parse_script_error(&format!("scripts:\n  - id: collect\n    path: {path}"));
        assert!(error.contains("must be a relative package path"), "{error}");
    }
}

#[test]
fn rejects_reserved_script_path_components() {
    for path in [
        ".jyowo-input.json",
        "scripts/.jyowo-run.sh",
        "scripts/.jyowo-cache/run.sh",
    ] {
        let error = parse_script_error(&format!("scripts:\n  - id: collect\n    path: {path}"));
        assert!(
            error.contains("uses reserved .jyowo- path component"),
            "{error}"
        );
    }
}

#[test]
fn rejects_unsupported_script_network_policy() {
    let error = parse_script_error(
        r"scripts:
  - id: collect
    path: scripts/collect.sh
    network: allow",
    );

    assert!(error.contains("unsupported script network policy: allow"));
}

#[test]
fn rejects_script_secret_env_mapping_without_secret_config_declaration() {
    let error = parse_skill_markdown(
        r"---
name: script-secret
description: Invalid secret environment
config:
  - key: apiToken
    type: string
scripts:
  - id: collect
    path: scripts/collect.sh
    env:
      API_TOKEN:
        config: apiToken
        secret: true
---
Body
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect_err("secret environment mapping must target secret config");

    assert!(format!("{error}").contains("must reference config declared secret"));
}

#[test]
fn rejects_script_secret_config_mapping_not_marked_secret() {
    let error = parse_skill_markdown(
        r"---
name: script-secret-marker
description: Missing secret marker
config:
  - key: apiToken
    type: string
    secret: true
scripts:
  - id: collect
    path: scripts/collect.sh
    env:
      API_TOKEN:
        config: apiToken
---
Body
",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect_err("secret config environment mapping must be explicit");

    assert!(format!("{error}").contains("must set secret: true"));
}

#[test]
fn rejects_unknown_script_and_script_env_fields() {
    let script_error = parse_script_error(
        r"scripts:
  - id: collect
    path: scripts/collect.sh
    memoryMb: 64",
    );
    assert!(script_error.contains("unknown script field: memoryMb"));

    let env_error = parse_script_error(
        r"config:
  - key: token
    type: string
scripts:
  - id: collect
    path: scripts/collect.sh
    env:
      TOKEN:
        config: token
        fallback: unsafe",
    );
    assert!(env_error.contains("unknown script env field: fallback"));
}

#[test]
fn rejects_unbounded_script_timeout_and_limits() {
    for field in [
        "timeoutSeconds: 0",
        "timeoutSeconds: 301",
        "maxStdoutBytes: 0",
        "maxStderrBytes: 0",
        "maxOutputBytes: 0",
        "maxArtifactCount: 0",
        "maxArtifactBytes: 0",
    ] {
        let error = parse_script_error(&format!(
            "scripts:\n  - id: collect\n    path: scripts/collect.sh\n    {field}"
        ));
        assert!(error.contains("must be between"), "{field}: {error}");
    }
}

fn parse_script_error(extra_frontmatter: &str) -> String {
    let markdown = format!(
        "---\nname: script-error\ndescription: Invalid script\n{extra_frontmatter}\n---\nBody\n"
    );
    parse_skill_markdown(
        &markdown,
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect_err("script declaration should be rejected")
    .to_string()
}
