use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoPayload {
    pub name: &'static str,
    pub version: &'static str,
    pub shell: &'static str,
    pub harness: HarnessInfoPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessInfoPayload {
    pub sdk_crate: &'static str,
    pub mode: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessHealthcheckPayload {
    pub status: &'static str,
    pub sdk_crate: &'static str,
}

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
    let _sdk_marker = std::any::TypeId::of::<jyowo_harness_sdk::Harness>();

    HarnessHealthcheckPayload {
        status: "available",
        sdk_crate: "jyowo_harness_sdk",
    }
}

#[tauri::command]
pub fn get_app_info() -> AppInfoPayload {
    get_app_info_payload()
}

#[tauri::command]
pub fn harness_healthcheck() -> HarnessHealthcheckPayload {
    harness_healthcheck_payload()
}
