#![allow(dead_code)]
#![allow(unused_imports)]

use super::*;

pub(crate) fn automation_spec(
    id: &str,
    enabled: bool,
    missed_run_policy: MissedRunPolicy,
) -> AutomationSpec {
    automation_spec_at(id, enabled, missed_run_policy, chrono::Utc::now())
}

pub(crate) fn automation_spec_at(
    id: &str,
    enabled: bool,
    missed_run_policy: MissedRunPolicy,
    created_at: chrono::DateTime<chrono::Utc>,
) -> AutomationSpec {
    AutomationSpec {
        id: id.to_owned(),
        enabled,
        prompt: "Run checks".to_owned(),
        schedule: AutomationSchedule {
            interval_minutes: 30,
        },
        tool_profile: ToolProfile::Coding,
        permission_mode: PermissionMode::Default,
        sandbox_mode: SandboxMode::None,
        workspace_scope: AutomationWorkspaceScope::CurrentWorkspace,
        workspace_access: WorkspaceAccess::ReadOnly,
        missed_run_policy,
        created_at,
        updated_at: created_at,
    }
}
