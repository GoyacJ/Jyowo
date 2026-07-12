use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_contracts::{
    AgentToolPolicy, AgentUsePolicy, SubagentRunnerCap, TeamId, ToolCapability,
};
use harness_journal::EventStore;
use harness_subagent::{
    DefaultSubagentRunner, DelegationPolicy, SubagentEngineFactory, SubagentRunner,
    SubagentRunnerCapAdapter,
};

const DEFAULT_SUBAGENT_WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);
const MAX_ALLOWED_DEPTH: u8 = 8;

#[derive(Clone)]
pub struct SubagentRunnerAssemblyInput {
    pub agent_tool_policy: AgentToolPolicy,
    pub engine_factory: Arc<dyn SubagentEngineFactory>,
    pub event_store: Arc<dyn EventStore>,
    pub workspace_root: PathBuf,
    pub team_attribution: Option<SubagentTeamAttribution>,
    pub daemon_runner: Option<Arc<dyn SubagentRunner>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentTeamAttribution {
    pub team_id: TeamId,
    pub team_member_profile_id: String,
}

#[must_use]
pub fn should_install_subagent_runner(options: &AgentToolPolicy) -> bool {
    options.subagents == AgentUsePolicy::Allowed
        && options.max_depth > 0
        && options.max_concurrent_subagents > 0
}

#[must_use]
pub fn delegation_policy_from_run_options(options: &AgentToolPolicy) -> DelegationPolicy {
    DelegationPolicy {
        max_depth: options.max_depth,
        depth_cap: options.max_depth.saturating_add(1).min(MAX_ALLOWED_DEPTH),
        max_concurrent_children: options.max_concurrent_subagents as usize,
        max_global_children: 128,
        blocklist: harness_subagent::DelegationBlocklist::default(),
    }
}

#[must_use]
pub fn assemble_subagent_runner(input: SubagentRunnerAssemblyInput) -> Arc<dyn SubagentRunner> {
    if let Some(runner) = input.daemon_runner {
        return runner;
    }
    let policy = delegation_policy_from_run_options(&input.agent_tool_policy);
    Arc::new(
        DefaultSubagentRunner::new_with_engine_factory(
            input.engine_factory,
            input.event_store,
            input.workspace_root,
            policy,
        )
        .with_watchdog_interval(DEFAULT_SUBAGENT_WATCHDOG_INTERVAL),
    )
}

pub fn install_subagent_runner_capability(
    registry: &mut harness_contracts::CapabilityRegistry,
    runner: Arc<dyn SubagentRunner>,
    team_attribution: Option<SubagentTeamAttribution>,
) {
    let runner_cap = match team_attribution {
        Some(attribution) => SubagentRunnerCapAdapter::from_runner_with_team_attribution(
            runner,
            attribution.team_id,
            attribution.team_member_profile_id,
        ),
        None => SubagentRunnerCapAdapter::from_runner(runner),
    };
    registry.install::<dyn SubagentRunnerCap>(ToolCapability::SubagentRunner, runner_cap);
}
