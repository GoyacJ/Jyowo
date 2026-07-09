use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{
    ActionResource, AssistantClarificationRequestedEvent, ClarifyChannelCap, ClarifyChoice,
    ClarifyPrompt, DecisionScope, Event, PermissionSubject, RequestId, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolResult,
    UiSafeText,
};
use harness_permission::PermissionCheck;
use serde_json::{json, Value};
use std::convert::TryFrom;

use crate::{AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct ClarifyTool {
    descriptor: ToolDescriptor,
}

impl Default for ClarifyTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_output_schema(
                super::descriptor(
                    "Clarify",
                    "Clarify",
                    "Ask the user for clarification through the session channel.",
                    ToolGroup::Clarification,
                    false,
                    false,
                    false,
                    8_000,
                    vec![ToolCapability::ClarifyChannel],
                    super::object_schema(
                        &["prompt"],
                        json!({
                            "prompt": { "type": "string" },
                            "choices": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["id", "label"],
                                    "properties": {
                                        "id": { "type": "string", "minLength": 1 },
                                        "label": { "type": "string", "minLength": 1 },
                                        "hint": { "type": "string" }
                                    }
                                }
                            },
                            "multiple": { "type": "boolean" },
                            "timeout_seconds": { "type": "integer", "minimum": 1 }
                        }),
                    ),
                ),
                json!({
                    "type": "object",
                    "required": ["answer", "chosen_ids", "answered_at"],
                    "properties": {
                        "answer": { "type": "string" },
                        "chosen_ids": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "answered_at": { "type": "string" }
                    },
                    "additionalProperties": false
                }),
            ),
        }
    }
}

#[async_trait]
impl Tool for ClarifyTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        prompt(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let prompt_hash = input
            .get("prompt")
            .and_then(Value::as_str)
            .map(|prompt| blake3::hash(prompt.as_bytes()).to_hex().to_string());
        super::generic_action_plan_with_resources(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            vec![ActionResource::Clarification {
                action: "ask".to_owned(),
                prompt_hash,
            }],
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let channel = ctx.capability::<dyn ClarifyChannelCap>(ToolCapability::ClarifyChannel)?;
        let prompt = prompt(authorized.raw_input()).map_err(validation_error)?;
        let request_id = RequestId::new();
        let event = Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
            run_id: ctx.run_id,
            request_id,
            prompt: UiSafeText::from_redacted_display(&prompt.prompt, ctx.redactor.as_ref()),
            at: chrono::Utc::now(),
        });
        let answer = stream::once(async move {
            match channel.ask(prompt).await {
                Ok(answer) => ToolEvent::Final(ToolResult::Structured(json!({
                    "answer": answer.answer,
                    "chosen_ids": answer.chosen_ids,
                    "answered_at": chrono::Utc::now()
                }))),
                Err(error) => ToolEvent::Error(error),
            }
        });
        Ok(Box::pin(
            stream::iter([ToolEvent::Journal(event)]).chain(answer),
        ))
    }
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn prompt(input: &Value) -> Result<ClarifyPrompt, ValidationError> {
    let prompt = input
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ValidationError::from("prompt is required"))?
        .to_owned();
    let choices = choices(input)?;
    Ok(ClarifyPrompt {
        prompt,
        choices,
        multiple: input
            .get("multiple")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        timeout_seconds: input
            .get("timeout_seconds")
            .map(timeout_seconds)
            .transpose()?,
    })
}

fn choices(input: &Value) -> Result<Vec<ClarifyChoice>, ValidationError> {
    let Some(value) = input.get("choices") else {
        return Ok(Vec::new());
    };
    let choices = value
        .as_array()
        .ok_or_else(|| ValidationError::from("choices must be an array"))?;
    choices
        .iter()
        .map(|choice| {
            let object = choice
                .as_object()
                .ok_or_else(|| ValidationError::from("choice must be an object"))?;
            let id = required_choice_string(object.get("id"), "choice.id is required")?;
            let label = required_choice_string(object.get("label"), "choice.label is required")?;
            let hint = match object.get("hint") {
                Some(value) => Some(
                    value
                        .as_str()
                        .ok_or_else(|| ValidationError::from("choice.hint must be a string"))?
                        .to_owned(),
                ),
                None => None,
            };
            Ok(ClarifyChoice { id, label, hint })
        })
        .collect()
}

fn required_choice_string(value: Option<&Value>, error: &str) -> Result<String, ValidationError> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ValidationError::from(error))
}

fn timeout_seconds(value: &Value) -> Result<u32, ValidationError> {
    let raw = value
        .as_u64()
        .ok_or_else(|| ValidationError::from("timeout_seconds must be a positive integer"))?;
    if raw == 0 {
        return Err(ValidationError::from(
            "timeout_seconds must be greater than 0",
        ));
    }
    u32::try_from(raw).map_err(|_| ValidationError::from("timeout_seconds must fit in u32"))
}
