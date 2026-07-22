use std::path::{Path, PathBuf};

use bytes::Bytes;
use chrono::Utc;
use harness_contracts::{
    ArtifactCreatedEvent, ArtifactRevisionId, ArtifactSource, ArtifactStatus, BlobMeta,
    BlobRetention, BlobWriterCap, Event, ToolCapability,
};

use crate::{ToolContext, ToolEvent};

const DIFF_CONTEXT_LINES: usize = 3;
const MAX_DIFF_PREVIEW_CHARS: usize = 32_000;

pub(super) async fn text_artifact_event(
    ctx: &ToolContext,
    kind: &str,
    path: &Path,
    text: &str,
) -> Option<ToolEvent> {
    if text.is_empty() {
        return None;
    }
    let writer = ctx
        .cap_registry
        .get::<dyn BlobWriterCap>(&ToolCapability::BlobWriter)?;
    let bytes = text.as_bytes().to_vec();
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let content_hash = *blake3::hash(&bytes).as_bytes();
    let blob_ref = writer
        .write_blob(
            ctx.tenant_id,
            Bytes::from(bytes),
            BlobMeta {
                content_type: Some("text/plain".to_owned()),
                size,
                content_hash,
                created_at: Utc::now(),
                retention: BlobRetention::SessionScoped(ctx.session_id),
            },
        )
        .await
        .ok()?;
    Some(ToolEvent::Journal(Event::ArtifactCreated(
        ArtifactCreatedEvent {
            revision_id: ArtifactRevisionId::new(),
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            artifact_id: format!("{kind}:{}", ctx.tool_use_id),
            title: workspace_relative_path(path, &ctx.workspace_root),
            kind: kind.to_owned(),
            status: ArtifactStatus::Ready,
            source: ArtifactSource::Tool,
            source_message_id: None,
            source_tool_use_id: Some(ctx.tool_use_id),
            blob_ref: Some(blob_ref),
            preview: None,
            content_hash: Some(content_hash.to_vec()),
            at: Utc::now(),
        },
    )))
}

pub(super) fn unified_diff(
    path: &Path,
    workspace_root: &Path,
    before: &str,
    after: &str,
) -> String {
    if before == after {
        return String::new();
    }
    let before_lines = text_lines(before);
    let after_lines = text_lines(after);
    let mut common_prefix = 0;
    while before_lines.get(common_prefix) == after_lines.get(common_prefix)
        && common_prefix < before_lines.len()
        && common_prefix < after_lines.len()
    {
        common_prefix += 1;
    }
    let mut common_suffix = 0;
    while common_suffix < before_lines.len().saturating_sub(common_prefix)
        && common_suffix < after_lines.len().saturating_sub(common_prefix)
        && before_lines[before_lines.len() - common_suffix - 1]
            == after_lines[after_lines.len() - common_suffix - 1]
    {
        common_suffix += 1;
    }

    let context_before = common_prefix.min(DIFF_CONTEXT_LINES);
    let context_after = common_suffix.min(DIFF_CONTEXT_LINES);
    let old_changed_end = before_lines.len().saturating_sub(common_suffix);
    let new_changed_end = after_lines.len().saturating_sub(common_suffix);
    let old_changed = &before_lines[common_prefix..old_changed_end];
    let new_changed = &after_lines[common_prefix..new_changed_end];
    let old_hunk_start_index = common_prefix.saturating_sub(context_before);
    let new_hunk_start_index = common_prefix.saturating_sub(context_before);
    let old_count = context_before + old_changed.len() + context_after;
    let new_count = context_before + new_changed.len() + context_after;
    let old_start = if old_count == 0 {
        0
    } else {
        old_hunk_start_index + 1
    };
    let new_start = if new_count == 0 {
        0
    } else {
        new_hunk_start_index + 1
    };
    let label = workspace_relative_path(path, workspace_root);
    let mut diff = format!(
        "diff --git a/{label} b/{label}\n--- a/{label}\n+++ b/{label}\n@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
    );
    for line in &before_lines[old_hunk_start_index..common_prefix] {
        push_diff_line(&mut diff, ' ', line);
    }
    for line in old_changed {
        push_diff_line(&mut diff, '-', line);
    }
    for line in new_changed {
        push_diff_line(&mut diff, '+', line);
    }
    let suffix_end = common_suffix.min(context_after);
    for line in &after_lines[new_changed_end..new_changed_end + suffix_end] {
        push_diff_line(&mut diff, ' ', line);
    }
    bounded_diff(diff)
}

pub(super) fn created_file_diff(path: &Path, workspace_root: &Path, content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let lines = text_lines(content);
    let label = workspace_relative_path(path, workspace_root);
    let mut diff = format!(
        "diff --git a/{label} b/{label}\nnew file mode 100644\n--- /dev/null\n+++ b/{label}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        push_diff_line(&mut diff, '+', line);
    }
    bounded_diff(diff)
}

fn text_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split_terminator('\n').collect()
}

fn workspace_relative_path(path: &Path, workspace_root: &Path) -> String {
    let display = path
        .strip_prefix(workspace_root)
        .map_or_else(|_| PathBuf::from(path), PathBuf::from);
    display.to_string_lossy().replace('\\', "/")
}

fn push_diff_line(diff: &mut String, marker: char, line: &str) {
    diff.push(marker);
    diff.push_str(line);
    diff.push('\n');
}

fn bounded_diff(diff: String) -> String {
    if diff.chars().count() <= MAX_DIFF_PREVIEW_CHARS {
        return diff;
    }
    let mut bounded = diff
        .chars()
        .take(MAX_DIFF_PREVIEW_CHARS)
        .collect::<String>();
    bounded.push_str("\n# Diff preview truncated\n");
    bounded
}

#[cfg(test)]
mod tests {
    use super::{created_file_diff, unified_diff};
    use std::path::Path;

    #[test]
    fn creates_a_bounded_context_diff() {
        let before = "one\ntwo\nthree\nfour\nfive\n";
        let after = "one\ntwo\nTHREE\nfour\nfive\n";
        let diff = unified_diff(
            Path::new("/workspace/src/example.rs"),
            Path::new("/workspace"),
            before,
            after,
        );

        assert!(diff.contains("diff --git a/src/example.rs b/src/example.rs"));
        assert!(diff.contains("@@ -1,5 +1,5 @@"));
        assert!(diff.contains("-three\n+THREE"));
    }

    #[test]
    fn creates_a_new_file_diff() {
        let diff = created_file_diff(
            Path::new("/workspace/notes.md"),
            Path::new("/workspace"),
            "first\nsecond\n",
        );

        assert!(diff.contains("--- /dev/null"));
        assert!(diff.contains("@@ -0,0 +1,2 @@"));
        assert!(diff.contains("+first\n+second"));
    }
}
