use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionResource, AskUserQuestion, AskUserQuestionCap, AskUserQuestionOption,
    AskUserQuestionRequest, RequestId, ToolActionPlan, ToolCapability, ToolDescriptor, ToolError,
    ToolExecutionChannel, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

const USER_INPUT_TIMEOUT_SECONDS: i64 = 300;
const TOOL_TIMEOUT_MS: u64 = 310_000;

#[derive(Clone)]
pub struct AskUserQuestionTool {
    descriptor: ToolDescriptor,
}

impl Default for AskUserQuestionTool {
    fn default() -> Self {
        let descriptor = super::with_configuration(
            super::with_output_schema(
                super::descriptor(
                "AskUserQuestion",
                "Ask User Question",
                "Ask the user one structured question only when their input is required to continue. Prefer a safe default when possible. Batch up to three questions only when they are independent, short, and answerable in one pass. Do not ask for information that can be inferred from context.",
                ToolGroup::Clarification,
                false,
                false,
                false,
                16_000,
                vec![ToolCapability::AskUserQuestion],
                super::object_schema(
                    &["questions"],
                    json!({
                        "questions": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": 3,
                            "description": "Prefer exactly one blocking question. Multiple questions must be independent; do not batch questions when a later question depends on an earlier answer.",
                            "items": {
                                "type": "object",
                                "required": ["id", "question"],
                                "properties": {
                                    "id": { "type": "string", "minLength": 1, "maxLength": 64 },
                                    "header": { "type": "string", "minLength": 1, "maxLength": 32 },
                                    "question": {
                                        "type": "string",
                                        "minLength": 1,
                                        "maxLength": 4096,
                                        "description": "Ask for one specific decision in plain language; do not combine multiple decisions in one question."
                                    },
                                    "options": {
                                        "type": "array",
                                        "maxItems": 4,
                                        "description": "Keep options on one decision dimension. Single-select options must be mutually exclusive; use multiSelect when choices may coexist.",
                                        "items": {
                                            "type": "object",
                                            "required": ["id", "label"],
                                            "properties": {
                                                "id": { "type": "string", "minLength": 1, "maxLength": 64 },
                                                "label": { "type": "string", "minLength": 1, "maxLength": 128 },
                                                "description": { "type": "string", "minLength": 1, "maxLength": 512 }
                                            },
                                            "additionalProperties": false
                                        }
                                    },
                                    "multiSelect": {
                                        "type": "boolean",
                                        "default": false,
                                        "description": "Set true only when more than one listed option may be selected."
                                    },
                                    "allowCustom": {
                                        "type": "boolean",
                                        "default": false,
                                        "description": "Allow an answer outside the listed options."
                                    }
                                },
                                "additionalProperties": false
                            }
                        }
                    }),
                ),
            ),
                json!({
                    "oneOf": [
                        {
                            "type": "object",
                            "required": ["status", "answers"],
                            "properties": {
                                "status": { "const": "answered" },
                                "answers": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "required": ["questionId", "selectedOptionIds"],
                                        "properties": {
                                            "questionId": { "type": "string" },
                                            "selectedOptionIds": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            },
                                            "text": { "type": "string" }
                                        },
                                        "additionalProperties": false
                                    }
                                }
                            },
                            "additionalProperties": false
                        },
                        {
                            "type": "object",
                            "required": ["status"],
                            "properties": {
                                "status": {
                                    "enum": ["declined", "timed_out", "cancelled"]
                                }
                            },
                            "additionalProperties": false
                        }
                    ]
                }),
            ),
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            json!({}),
            Some(TOOL_TIMEOUT_MS),
        );
        Self { descriptor }
    }
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        questions(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let prompt_hash = Some(
            blake3::hash(input.to_string().as_bytes())
                .to_hex()
                .to_string(),
        );
        super::generic_action_plan_with_resources(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::Allowed,
            vec![ActionResource::Clarification {
                action: "ask_user_question".to_owned(),
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
        let channel = ctx.capability::<dyn AskUserQuestionCap>(ToolCapability::AskUserQuestion)?;
        let questions = questions(authorized.raw_input()).map_err(validation_error)?;
        let request = AskUserQuestionRequest {
            request_id: RequestId::new(),
            tool_use_id: ctx.tool_use_id,
            run_id: ctx.run_id,
            session_id: ctx.session_id,
            actor_source: ctx.actor_source,
            questions,
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(USER_INPUT_TIMEOUT_SECONDS),
        };
        let result = channel.ask(request).await?;
        let value =
            serde_json::to_value(result).map_err(|error| ToolError::Internal(error.to_string()))?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(value),
        )])))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AskUserQuestionInput {
    questions: Vec<QuestionInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuestionInput {
    id: String,
    header: Option<String>,
    question: String,
    #[serde(default)]
    options: Vec<OptionInput>,
    #[serde(default)]
    multi_select: bool,
    #[serde(default)]
    allow_custom: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OptionInput {
    id: String,
    label: String,
    description: Option<String>,
}

fn questions(input: &Value) -> Result<Vec<AskUserQuestion>, ValidationError> {
    let input: AskUserQuestionInput = serde_json::from_value(input.clone())
        .map_err(|error| ValidationError::from(error.to_string()))?;
    if !(1..=3).contains(&input.questions.len()) {
        return Err(ValidationError::from(
            "questions must contain between 1 and 3 items",
        ));
    }
    let questions = input
        .questions
        .into_iter()
        .map(|question| AskUserQuestion {
            id: question.id,
            header: question.header,
            question: question.question,
            options: question
                .options
                .into_iter()
                .map(|option| AskUserQuestionOption {
                    id: option.id,
                    label: option.label,
                    description: option.description,
                })
                .collect(),
            multi_select: question.multi_select,
            allow_custom: question.allow_custom,
        })
        .collect::<Vec<_>>();
    validate_semantics(&questions)?;
    Ok(questions)
}

fn validate_semantics(questions: &[AskUserQuestion]) -> Result<(), ValidationError> {
    let mut question_ids = std::collections::HashSet::new();
    for question in questions {
        if question.id.trim().is_empty() || question.question.trim().is_empty() {
            return Err(ValidationError::from(
                "question id and text must not be empty",
            ));
        }
        if !question_ids.insert(question.id.as_str()) {
            return Err(ValidationError::from("question ids must be unique"));
        }
        if !question.options.is_empty() && !(2..=4).contains(&question.options.len()) {
            return Err(ValidationError::from(
                "options must be empty or contain between 2 and 4 items",
            ));
        }
        if question.multi_select && question.options.is_empty() {
            return Err(ValidationError::from(
                "multiSelect requires at least two options",
            ));
        }
        let mut option_ids = std::collections::HashSet::new();
        for option in &question.options {
            if option.id.trim().is_empty() || option.label.trim().is_empty() {
                return Err(ValidationError::from(
                    "option id and label must not be empty",
                ));
            }
            if !option_ids.insert(option.id.as_str()) {
                return Err(ValidationError::from(
                    "option ids must be unique within a question",
                ));
            }
        }
    }
    Ok(())
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}
