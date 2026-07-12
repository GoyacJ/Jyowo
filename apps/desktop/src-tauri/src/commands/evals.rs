#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
#[allow(unused_imports)]
use super::automations::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::plugins::*;
#[allow(unused_imports)]
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;

pub fn run_eval_case_payload(
    request: RunEvalCaseRequest,
) -> Result<RunEvalCaseResponse, CommandErrorPayload> {
    ensure_eval_case_id(&request.case_id)?;

    Err(runtime_unavailable(
        "Running eval cases requires the eval runtime.",
    ))
}

pub fn run_eval_case_with_runtime_state(
    request: RunEvalCaseRequest,
    _state: &DesktopRuntimeState,
) -> Result<RunEvalCaseResponse, CommandErrorPayload> {
    run_eval_case_payload(request)
}
