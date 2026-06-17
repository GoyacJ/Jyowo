use std::collections::HashSet;

use harness_contracts::{Message, ModelError};
use serde_json::{json, Value};

use crate::ModelRequest;

pub(super) fn apply_prompt_cache(body: &mut Value, req: &ModelRequest) -> Result<(), ModelError> {
    if req.cache_breakpoints.is_empty() {
        return Ok(());
    }

    let system_count = usize::from(req.system.is_some());
    if system_count + req.cache_breakpoints.len() > 4 {
        return Err(ModelError::InvalidRequest(
            "Anthropic prompt cache supports at most 4 cache_control breakpoints".to_owned(),
        ));
    }

    let mut seen = HashSet::new();
    for breakpoint in &req.cache_breakpoints {
        if !seen.insert(breakpoint.after_message_id) {
            return Err(ModelError::InvalidRequest(
                "duplicate Anthropic cache breakpoint message id".to_owned(),
            ));
        }
    }

    if let Some(system) = &req.system {
        body["system"] = json!([{
            "type": "text",
            "text": system,
            "cache_control": { "type": "ephemeral" },
        }]);
    }

    for breakpoint in &req.cache_breakpoints {
        let index = req
            .messages
            .iter()
            .position(|message| message.id == breakpoint.after_message_id)
            .ok_or_else(|| {
                ModelError::InvalidRequest(format!(
                    "Anthropic cache breakpoint message not found: {}",
                    breakpoint.after_message_id
                ))
            })?;
        inject_message_cache_control(body, index, &req.messages[index])?;
    }

    Ok(())
}

fn inject_message_cache_control(
    body: &mut Value,
    index: usize,
    source: &Message,
) -> Result<(), ModelError> {
    let content = body
        .pointer_mut(&format!("/messages/{index}/content"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            ModelError::InvalidRequest(format!(
                "Anthropic cache breakpoint message has no content array: {}",
                source.id
            ))
        })?;
    let Some(last) = content.last_mut() else {
        return Err(ModelError::InvalidRequest(format!(
            "Anthropic cache breakpoint message has empty content: {}",
            source.id
        )));
    };

    let Some(object) = last.as_object_mut() else {
        return Err(ModelError::InvalidRequest(format!(
            "Anthropic cache breakpoint message content is not an object: {}",
            source.id
        )));
    };
    object.insert(
        "cache_control".to_owned(),
        json!({
            "type": "ephemeral",
        }),
    );
    Ok(())
}
