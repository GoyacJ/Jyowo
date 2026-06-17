use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ToolDeferredPoolChangedEvent, ToolName, ToolPoolChangeSource};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeferredToolsDeltaAttachment {
    pub added_names: Vec<ToolName>,
    pub removed_names: Vec<ToolName>,
    pub source: ToolPoolChangeSource,
    pub at: DateTime<Utc>,
    pub initial: bool,
}

impl DeferredToolsDeltaAttachment {
    #[must_use]
    pub fn from_pool_change(event: &ToolDeferredPoolChangedEvent) -> Self {
        Self {
            added_names: event.added.iter().map(|hint| hint.name.clone()).collect(),
            removed_names: event.removed.clone(),
            source: event.source.clone(),
            at: event.at,
            initial: matches!(event.source, ToolPoolChangeSource::InitialClassification),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added_names.is_empty() && self.removed_names.is_empty()
    }

    pub fn remove_added_names(&mut self, names: &[ToolName]) {
        let materialized = names.iter().collect::<BTreeSet<_>>();
        self.added_names.retain(|name| !materialized.contains(name));
    }

    pub fn merge(&mut self, next: Self) {
        let next_added = next.added_names.iter().collect::<BTreeSet<_>>();
        let next_removed = next.removed_names.iter().collect::<BTreeSet<_>>();
        self.added_names.retain(|name| !next_removed.contains(name));
        self.removed_names.retain(|name| !next_added.contains(name));

        for name in next.added_names {
            if !self.added_names.contains(&name) {
                self.added_names.push(name);
            }
        }
        for name in next.removed_names {
            if !self.removed_names.contains(&name) {
                self.removed_names.push(name);
            }
        }
        self.source = next.source;
        self.at = next.at;
        self.initial |= next.initial;
    }

    #[must_use]
    pub fn to_attachment_text(&self) -> String {
        let mut text = if self.initial {
            format!(
                "<deferred-tools initial=\"true\" changed-at=\"{}\">\n",
                self.at.to_rfc3339()
            )
        } else {
            format!("<deferred-tools changed-at=\"{}\">\n", self.at.to_rfc3339())
        };
        if !self.added_names.is_empty() {
            text.push_str("  <added>\n");
            for name in &self.added_names {
                text.push_str("    ");
                text.push_str(name);
                text.push('\n');
            }
            text.push_str("  </added>\n");
        }
        if !self.removed_names.is_empty() {
            text.push_str("  <removed>\n");
            for name in &self.removed_names {
                text.push_str("    ");
                text.push_str(name);
                text.push('\n');
            }
            text.push_str("  </removed>\n");
        }
        text.push_str("</deferred-tools>");
        text
    }
}
