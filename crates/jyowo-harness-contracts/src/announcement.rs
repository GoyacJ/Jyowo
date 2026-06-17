//! Shared announcement rendering contracts.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnouncementRenderInput {
    pub kind: String,
    pub summary: String,
    pub status: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub rewrite_hint: Option<String>,
}

impl AnnouncementRenderInput {
    #[must_use]
    pub fn new(kind: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            summary: summary.into(),
            status: None,
            labels: BTreeMap::new(),
            rewrite_hint: None,
        }
    }

    #[must_use]
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }

    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_rewrite_hint(mut self, rewrite_hint: impl Into<String>) -> Self {
        self.rewrite_hint = Some(rewrite_hint.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedAnnouncement {
    pub user_message: String,
    pub renderer_id: String,
}

pub trait AnnouncementRenderer: Send + Sync + 'static {
    fn renderer_id(&self) -> &str;

    fn render(&self, input: &AnnouncementRenderInput) -> RenderedAnnouncement;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct XmlTaskNotificationRenderer;

impl AnnouncementRenderer for XmlTaskNotificationRenderer {
    fn renderer_id(&self) -> &str {
        "xml-task-notification"
    }

    fn render(&self, input: &AnnouncementRenderInput) -> RenderedAnnouncement {
        let mut body = String::from("<task-notification>\n");
        body.push_str("  <kind>");
        body.push_str(&escape_xml(&input.kind));
        body.push_str("</kind>\n");
        if let Some(status) = &input.status {
            body.push_str("  <status>");
            body.push_str(&escape_xml(status));
            body.push_str("</status>\n");
        }
        for (key, value) in &input.labels {
            body.push_str("  <label key=\"");
            body.push_str(&escape_xml(key));
            body.push_str("\">");
            body.push_str(&escape_xml(value));
            body.push_str("</label>\n");
        }
        body.push_str("  <summary>");
        body.push_str(&escape_xml(&input.summary));
        body.push_str("</summary>\n");
        body.push_str("  <rewrite-hint>");
        body.push_str(&escape_xml(input.rewrite_hint.as_deref().unwrap_or(
            "Rewrite this internal task result before showing it to the user.",
        )));
        body.push_str("</rewrite-hint>\n");
        body.push_str("</task-notification>");

        RenderedAnnouncement {
            user_message: body,
            renderer_id: self.renderer_id().to_owned(),
        }
    }
}

#[must_use]
pub fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
