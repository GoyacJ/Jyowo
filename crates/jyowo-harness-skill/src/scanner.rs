use harness_contracts::ThreatAction;
use harness_memory::MemoryThreatScanner;

use std::path::Path;

use crate::{loader::read_skill_package_files, Skill, SkillError, SkillSource};

pub(crate) fn auxiliary_skill_package_text(root: &Path) -> Result<Vec<String>, SkillError> {
    let mut text = Vec::new();
    for file in read_skill_package_files(root)? {
        if file.relative_path == Path::new("SKILL.md")
            || !is_supported_text_path(&file.relative_path)
        {
            continue;
        }
        if let Ok(content) = String::from_utf8(file.bytes) {
            text.push(content);
        }
    }
    Ok(text)
}

fn is_supported_text_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown"
                    | "txt"
                    | "rst"
                    | "adoc"
                    | "yaml"
                    | "yml"
                    | "json"
                    | "toml"
                    | "ini"
                    | "cfg"
                    | "sh"
                    | "bash"
                    | "zsh"
                    | "py"
                    | "js"
                    | "jsx"
                    | "ts"
                    | "tsx"
                    | "mjs"
                    | "cjs"
                    | "rs"
                    | "go"
                    | "java"
                    | "kt"
                    | "rb"
                    | "php"
                    | "pl"
                    | "lua"
                    | "sql"
                    | "html"
                    | "css"
                    | "xml"
                    | "csv"
            )
        })
}

pub fn apply_threat_scan(
    skill: &mut Skill,
    scanner: &MemoryThreatScanner,
) -> Result<(), SkillError> {
    if matches!(skill.source, SkillSource::Bundled) {
        return Ok(());
    }

    scan_text(&skill.description, scanner).map(|redacted| {
        if let Some(redacted) = redacted {
            skill.description = redacted.clone();
            skill.frontmatter.description = redacted;
        }
    })?;

    scan_text(&skill.body, scanner).map(|redacted| {
        if let Some(redacted) = redacted {
            skill.body = redacted;
        }
    })?;

    Ok(())
}

fn scan_text(content: &str, scanner: &MemoryThreatScanner) -> Result<Option<String>, SkillError> {
    let report = scanner.scan(content);
    if report.action == ThreatAction::Block {
        if let Some(hit) = report.hits.first() {
            return Err(SkillError::ThreatDetected {
                pattern_id: hit.pattern_id.clone(),
                category: hit.category,
            });
        }
    }

    Ok(report.redacted_content)
}
