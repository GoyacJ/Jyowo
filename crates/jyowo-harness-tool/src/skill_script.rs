use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillScriptLocalPathPolicy {
    pub authorized_roots: Vec<PathBuf>,
}

#[must_use]
pub fn skill_script_local_path_authorized(
    candidate_path: &Path,
    policy: &SkillScriptLocalPathPolicy,
) -> bool {
    let candidate = lexical_normalize(candidate_path);
    policy
        .authorized_roots
        .iter()
        .map(|root| lexical_normalize(root))
        .any(|root| candidate.starts_with(root))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}
