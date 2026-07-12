use std::{collections::VecDeque, sync::Mutex};

use async_trait::async_trait;
use harness_mcp::{
    McpConnection, McpError, McpListPage, McpPaginationLimits, McpPrompt, McpReadResourceResult,
    McpResource, McpToolDescriptor, McpToolResult,
};
use serde_json::{json, Value};

#[tokio::test]
async fn list_tools_all_collects_multiple_pages_and_empty_continuation_pages() {
    let connection = PagedConnection::new(vec![
        page(&["one"], Some("cursor-1")),
        page(&[], Some("cursor-2")),
        page(&["two"], None),
    ]);

    let tools = connection.list_tools_all().await.unwrap();

    assert_eq!(
        tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert_eq!(
        connection.seen_cursors(),
        vec![
            None,
            Some("cursor-1".to_owned()),
            Some("cursor-2".to_owned())
        ]
    );
}

#[tokio::test]
async fn list_tools_all_rejects_a_repeated_cursor() {
    let connection = PagedConnection::new(vec![
        page(&["one"], Some("same")),
        page(&["two"], Some("same")),
    ]);

    let error = connection.list_tools_all().await.unwrap_err();

    assert!(error.to_string().contains("repeated cursor"));
}

#[tokio::test]
async fn list_tools_all_enforces_page_limit() {
    let connection = PagedConnection::new(vec![
        page(&["one"], Some("one")),
        page(&["two"], Some("two")),
    ]);

    let error = connection
        .list_tools_all_with_limits(McpPaginationLimits {
            max_pages: 1,
            max_items: 10,
        })
        .await
        .unwrap_err();

    assert!(error.to_string().contains("page limit"));
}

#[tokio::test]
async fn list_tools_all_enforces_item_limit() {
    let connection = PagedConnection::new(vec![page(&["one", "two"], None)]);

    let error = connection
        .list_tools_all_with_limits(McpPaginationLimits {
            max_pages: 10,
            max_items: 1,
        })
        .await
        .unwrap_err();

    assert!(error.to_string().contains("item limit"));
}

#[tokio::test]
async fn resource_and_prompt_all_apis_follow_their_cursors() {
    let connection = MetadataPagedConnection {
        resource_pages: Mutex::new(
            vec![
                McpListPage {
                    items: vec![serde_json::from_value(json!({
                        "uri": "test://one",
                        "name": "one"
                    }))
                    .unwrap()],
                    next_cursor: Some("resources-2".to_owned()),
                },
                McpListPage {
                    items: vec![serde_json::from_value(json!({
                        "uri": "test://two",
                        "name": "two"
                    }))
                    .unwrap()],
                    next_cursor: None,
                },
            ]
            .into(),
        ),
        prompt_pages: Mutex::new(
            vec![
                McpListPage {
                    items: Vec::new(),
                    next_cursor: Some("prompts-2".to_owned()),
                },
                McpListPage {
                    items: vec![serde_json::from_value(json!({ "name": "triage" })).unwrap()],
                    next_cursor: None,
                },
            ]
            .into(),
        ),
        resource_cursors: Mutex::new(Vec::new()),
        prompt_cursors: Mutex::new(Vec::new()),
        read_result: serde_json::from_value(json!({
            "contents": [
                { "uri": "test://document", "text": "hello" },
                { "uri": "test://document/image", "blob": "AA==" }
            ],
            "_meta": { "trace": "resource-read" }
        }))
        .unwrap(),
    };

    let resources = connection.list_resources_all().await.unwrap();
    let prompts = connection.list_prompts_all().await.unwrap();

    assert_eq!(
        resources
            .into_iter()
            .map(|resource| resource.name)
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert_eq!(prompts[0].name, "triage");
    assert_eq!(
        connection.resource_cursors.lock().unwrap().as_slice(),
        [None, Some("resources-2".to_owned())]
    );
    assert_eq!(
        connection.prompt_cursors.lock().unwrap().as_slice(),
        [None, Some("prompts-2".to_owned())]
    );

    let read = connection.read_resource("test://document").await.unwrap();
    assert_eq!(read.contents.len(), 2);
    assert_eq!(
        serde_json::to_value(read).unwrap(),
        json!({
            "contents": [
                { "uri": "test://document", "text": "hello" },
                { "uri": "test://document/image", "blob": "AA==" }
            ],
            "_meta": { "trace": "resource-read" }
        })
    );
}

fn page(names: &[&str], next_cursor: Option<&str>) -> McpListPage<McpToolDescriptor> {
    McpListPage {
        items: names.iter().map(|name| tool(name)).collect(),
        next_cursor: next_cursor.map(str::to_owned),
    }
}

fn tool(name: &str) -> McpToolDescriptor {
    serde_json::from_value(json!({
        "name": name,
        "inputSchema": { "type": "object" }
    }))
    .unwrap()
}

struct PagedConnection {
    pages: Mutex<VecDeque<McpListPage<McpToolDescriptor>>>,
    cursors: Mutex<Vec<Option<String>>>,
}

impl PagedConnection {
    fn new(pages: Vec<McpListPage<McpToolDescriptor>>) -> Self {
        Self {
            pages: Mutex::new(pages.into()),
            cursors: Mutex::new(Vec::new()),
        }
    }

    fn seen_cursors(&self) -> Vec<Option<String>> {
        self.cursors.lock().unwrap().clone()
    }
}

#[async_trait]
impl McpConnection for PagedConnection {
    fn connection_id(&self) -> &str {
        "paged"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        self.list_tools_all().await
    }

    async fn list_tools_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpToolDescriptor>, McpError> {
        self.cursors.lock().unwrap().push(cursor.map(str::to_owned));
        self.pages
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| McpError::InvalidResponse("fixture ran out of pages".into()))
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("unused"))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

struct MetadataPagedConnection {
    resource_pages: Mutex<VecDeque<McpListPage<McpResource>>>,
    prompt_pages: Mutex<VecDeque<McpListPage<McpPrompt>>>,
    resource_cursors: Mutex<Vec<Option<String>>>,
    prompt_cursors: Mutex<Vec<Option<String>>>,
    read_result: McpReadResourceResult,
}

#[async_trait]
impl McpConnection for MetadataPagedConnection {
    fn connection_id(&self) -> &str {
        "metadata-paged"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(Vec::new())
    }

    async fn list_resources_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpResource>, McpError> {
        self.resource_cursors
            .lock()
            .unwrap()
            .push(cursor.map(str::to_owned));
        self.resource_pages
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| McpError::InvalidResponse("fixture ran out of resource pages".into()))
    }

    async fn list_prompts_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpPrompt>, McpError> {
        self.prompt_cursors
            .lock()
            .unwrap()
            .push(cursor.map(str::to_owned));
        self.prompt_pages
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| McpError::InvalidResponse("fixture ran out of prompt pages".into()))
    }

    async fn read_resource(&self, _uri: &str) -> Result<McpReadResourceResult, McpError> {
        Ok(self.read_result.clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("unused"))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}
