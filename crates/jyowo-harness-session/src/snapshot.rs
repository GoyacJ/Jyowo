use harness_contracts::SnapshotId;
use serde::Serialize;

use crate::{PermissionRecord, SessionProjection, ToolUseRecord};

pub(crate) fn projection_snapshot_id(projection: &SessionProjection) -> SnapshotId {
    let tool_uses = sorted_tool_uses(projection);
    let canonical = CanonicalProjection {
        session_id: projection.session_id.to_string(),
        tenant_id: projection.tenant_id.to_string(),
        messages: &projection.messages,
        tool_uses,
        permission_log: &projection.permission_log,
        usage: &projection.usage,
        allowlist: projection.allowlist.iter().collect(),
        end_reason: &projection.end_reason,
        last_offset: projection.last_offset.0,
        discovered_tools: projection.discovered_tools.iter().collect(),
        pending_deferred_tools_delta: projection.pending_deferred_tools_delta(),
    };
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let hash = blake3::hash(&bytes);
    let mut id = [0_u8; 16];
    id.copy_from_slice(&hash.as_bytes()[..16]);
    SnapshotId::from_u128(u128::from_be_bytes(id))
}

#[derive(Serialize)]
struct CanonicalProjection<'a> {
    session_id: String,
    tenant_id: String,
    messages: &'a [harness_contracts::Message],
    tool_uses: Vec<(String, &'a ToolUseRecord)>,
    permission_log: &'a [PermissionRecord],
    usage: &'a harness_contracts::UsageSnapshot,
    allowlist: Vec<&'a String>,
    end_reason: &'a Option<harness_contracts::EndReason>,
    last_offset: u64,
    discovered_tools: Vec<&'a String>,
    pending_deferred_tools_delta: Option<&'a harness_contracts::DeferredToolsDeltaAttachment>,
}

fn sorted_tool_uses(projection: &SessionProjection) -> Vec<(String, &ToolUseRecord)> {
    let mut tool_uses = projection
        .tool_uses
        .iter()
        .map(|(id, record)| (id.to_string(), record))
        .collect::<Vec<_>>();
    tool_uses.sort_by(|left, right| left.0.cmp(&right.0));
    tool_uses
}
