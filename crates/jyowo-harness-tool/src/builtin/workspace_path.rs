use std::path::{Path, PathBuf};

use harness_contracts::ToolError;
use harness_contracts::{DecisionScope, PermissionSubject};
use harness_permission::{DangerousPatternLibrary, PermissionCheck};
use serde_json::Value;

use crate::{ToolContext, ValidationError};

pub(super) fn input_path(input: &Value) -> Result<PathBuf, ValidationError> {
    input
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| ValidationError::from("path is required"))
}

pub(super) fn scope_path(input: &Value, ctx: &ToolContext) -> Result<PathBuf, ValidationError> {
    let path = input_path(input)?;
    Ok(anchored_path(path, ctx))
}

pub(super) fn dangerous_path_permission(
    input: &Value,
    ctx: &ToolContext,
    subject: PermissionSubject,
    scope: DecisionScope,
) -> Option<PermissionCheck> {
    let path = input_path(input).ok()?;
    let scoped = anchored_path(path, ctx);
    let scoped = scoped.to_string_lossy();
    let library = DangerousPatternLibrary::default_all();
    let rule = library.detect_path(scoped.as_ref())?;
    Some(PermissionCheck::DangerousPattern {
        kind: "path".to_owned(),
        pattern: rule.id.clone(),
        severity: rule.severity,
        subject,
        scope,
    })
}

pub(super) fn resolve_existing(input: &Value, ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    let path = input_path(input).map_err(validation_error)?;
    let candidate = anchored_path(path, ctx);
    let canonical = candidate
        .canonicalize()
        .map_err(|error| ToolError::Message(error.to_string()))?;
    ensure_inside_workspace(&canonical, ctx)?;
    Ok(canonical)
}

pub(super) fn resolve_writable(input: &Value, ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    let path = input_path(input).map_err(validation_error)?;
    let candidate = anchored_path(path, ctx);
    if candidate.exists() {
        let canonical = candidate
            .canonicalize()
            .map_err(|error| ToolError::Message(error.to_string()))?;
        ensure_inside_workspace(&canonical, ctx)?;
        return Ok(candidate);
    }

    let parent = candidate
        .parent()
        .ok_or_else(|| ToolError::Validation("path must have a parent directory".to_owned()))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| ToolError::Message(error.to_string()))?;
    ensure_inside_workspace(&canonical_parent, ctx)?;
    Ok(candidate)
}

pub(super) fn ensure_inside_workspace(path: &Path, ctx: &ToolContext) -> Result<(), ToolError> {
    let path = path
        .canonicalize()
        .map_err(|error| ToolError::Message(error.to_string()))?;
    let workspace = ctx
        .workspace_root
        .canonicalize()
        .map_err(|error| ToolError::Message(error.to_string()))?;
    if path == workspace || path.starts_with(&workspace) {
        return Ok(());
    }
    Err(ToolError::PermissionDenied(format!(
        "path escapes workspace: {}",
        path.display()
    )))
}

fn anchored_path(path: PathBuf, ctx: &ToolContext) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        ctx.workspace_root.join(path)
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}
