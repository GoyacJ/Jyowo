use std::path::Path;

use harness_tool::{skill_script_local_path_authorized, SkillScriptLocalPathPolicy};

#[test]
fn skill_script_local_runner_path_authorization_stays_inside_authorized_roots() {
    let policy = SkillScriptLocalPathPolicy {
        authorized_roots: vec!["/workspace/project".into()],
    };

    assert!(skill_script_local_path_authorized(
        Path::new("/workspace/project/scripts/run.sh"),
        &policy
    ));
    assert!(skill_script_local_path_authorized(
        Path::new("/workspace/project/tmp/../scripts/run.sh"),
        &policy
    ));
    assert!(!skill_script_local_path_authorized(
        Path::new("/workspace/other/run.sh"),
        &policy
    ));
}
