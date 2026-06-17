use harness_contracts::ThreatAction;
use harness_memory::MemoryThreatScanner;

use crate::{Skill, SkillError, SkillSource};

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
