use harness_contracts::ThreatAction;
use harness_memory::MemoryThreatScanner;

use std::path::Path;

use crate::{Skill, SkillError, SkillPackageSnapshot, SkillSource};

pub(crate) fn auxiliary_skill_package_text(snapshot: &SkillPackageSnapshot) -> Vec<String> {
    let mut text = Vec::new();
    for (relative_path, bytes) in snapshot.files() {
        if relative_path == Path::new("SKILL.md")
            || !is_supported_text_path(relative_path)
            || looks_binary(bytes)
        {
            continue;
        }
        if let Ok(content) = std::str::from_utf8(bytes) {
            text.push(content.to_owned());
        }
    }
    text
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().any(|byte| {
        matches!(byte, 0 | 0x7f) || (*byte < 0x20 && !matches!(byte, b'\t' | b'\n' | b'\r'))
    })
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
