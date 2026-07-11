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
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
use super::memory::*;
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

#[must_use]
pub fn get_app_info_payload() -> AppInfoPayload {
    AppInfoPayload {
        name: "Jyowo",
        version: env!("CARGO_PKG_VERSION"),
        shell: "tauri2-react",
        harness: HarnessInfoPayload {
            sdk_crate: "jyowo_harness_sdk",
            mode: "in-process",
        },
    }
}

#[must_use]
pub fn harness_healthcheck_payload() -> HarnessHealthcheckPayload {
    let _sdk_marker = std::any::TypeId::of::<jyowo_harness_sdk::DesktopSettingsRuntime>();

    HarnessHealthcheckPayload {
        status: "available",
        sdk_crate: "jyowo_harness_sdk",
    }
}

pub fn list_eval_cases_payload() -> Result<ListEvalCasesResponse, CommandErrorPayload> {
    Err(runtime_unavailable(
        "Listing eval cases requires the eval runtime.",
    ))
}

pub fn list_eval_cases_with_runtime_state(
    _state: &DesktopRuntimeState,
) -> Result<ListEvalCasesResponse, CommandErrorPayload> {
    list_eval_cases_payload()
}
