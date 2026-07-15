#[allow(unused_imports)]
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
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
