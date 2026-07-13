use std::sync::Mutex;

use jyowo_desktop_shell::commands::{
    list_browser_mcp_presets_with_store, save_browser_mcp_preset_with_store, BrowserMcpPresetId,
    CommandErrorPayload, McpServerConfigRecord, McpServerStore, McpServerTransportConfig,
    SaveBrowserMcpPresetRequest,
};

#[derive(Default)]
struct RecordingMcpServerStore {
    record: Mutex<Option<McpServerConfigRecord>>,
}

impl McpServerStore for RecordingMcpServerStore {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload> {
        Ok(self.record.lock().unwrap().clone().into_iter().collect())
    }

    fn save_record(&self, record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload> {
        *self.record.lock().unwrap() = Some(record.clone());
        Ok(())
    }

    fn delete_record(&self, id: &str) -> Result<(), CommandErrorPayload> {
        let mut record = self.record.lock().unwrap();
        if record.as_ref().is_some_and(|record| record.id == id) {
            *record = None;
        }
        Ok(())
    }
}

#[tokio::test]
async fn browser_mcp_preset_summaries_expose_pinned_versions() {
    let store = RecordingMcpServerStore::default();

    let response = list_browser_mcp_presets_with_store(&store)
        .await
        .expect("browser MCP presets should list");

    assert_eq!(response.presets[0].id, BrowserMcpPresetId::Playwright);
    assert_eq!(response.presets[0].version, "0.0.78");
    assert_eq!(response.presets[1].id, BrowserMcpPresetId::ChromeDevtools);
    assert_eq!(response.presets[1].version, "1.5.0");
}

#[tokio::test]
async fn browser_mcp_presets_are_pinned_and_optional() {
    let summary_store = RecordingMcpServerStore::default();
    let summaries = list_browser_mcp_presets_with_store(&summary_store)
        .await
        .expect("browser MCP presets should list")
        .presets;

    for (preset_id, package_name) in [
        (BrowserMcpPresetId::Playwright, "@playwright/mcp"),
        (BrowserMcpPresetId::ChromeDevtools, "chrome-devtools-mcp"),
    ] {
        let store = RecordingMcpServerStore::default();
        let summary = summaries
            .iter()
            .find(|summary| summary.id == preset_id)
            .expect("browser MCP preset summary should exist");

        save_browser_mcp_preset_with_store(
            SaveBrowserMcpPresetRequest {
                preset_id,
                enabled: false,
            },
            &store,
        )
        .await
        .expect("browser MCP preset should save");

        let record = store.record.lock().unwrap().clone().unwrap();
        assert!(!record.enabled);
        assert!(!record.required);
        assert!(matches!(
            record.transport,
            McpServerTransportConfig::Stdio {
                ref command,
                ref args,
                ref env,
                ref inherit_env,
                working_dir: None,
            } if command == "npx"
                && args == &vec![
                    "-y".to_owned(),
                    format!("{package_name}@{}", summary.version),
                ]
                && args.iter().all(|arg| !arg.contains("@latest"))
                && env.is_empty()
                && inherit_env == &vec![
                    "PATH".to_owned(),
                    "HOME".to_owned(),
                    "USER".to_owned(),
                    "TMPDIR".to_owned(),
                ]
        ));
    }
}
