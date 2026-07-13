use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use secrecy::SecretString;
use serde_json::Value;

use crate::{
    ConfigResolveError, RenderError, Skill, SkillMetricsSink, SkillParamType, SkillRenderPolicy,
};

#[async_trait]
pub trait SkillConfigResolver: Send + Sync + 'static {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError>;
    async fn resolve_secret(&self, key: &str) -> Result<SecretString, ConfigResolveError>;

    async fn resolve_for(
        &self,
        _skill_id: &harness_contracts::SkillId,
        key: &str,
    ) -> Result<Value, ConfigResolveError> {
        self.resolve(key).await
    }

    async fn resolve_secret_for(
        &self,
        _skill_id: &harness_contracts::SkillId,
        key: &str,
    ) -> Result<SecretString, ConfigResolveError> {
        self.resolve_secret(key).await
    }
}

#[derive(Clone)]
pub struct SkillRenderer {
    config_resolver: Arc<dyn SkillConfigResolver>,
    shell_allowlist: HashSet<String>,
    max_shell_output: usize,
    metrics_sink: Option<Arc<dyn SkillMetricsSink>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedSkill {
    pub skill_id: harness_contracts::SkillId,
    pub skill_name: String,
    pub content: String,
    pub shell_invocations: Vec<ShellInvocation>,
    pub consumed_config_keys: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShellInvocation {
    pub command: String,
    pub stdout_truncated: bool,
    pub exit_code: i32,
}

impl SkillRenderer {
    #[must_use]
    pub fn new(config_resolver: Arc<dyn SkillConfigResolver>) -> Self {
        Self {
            config_resolver,
            shell_allowlist: SkillRenderPolicy::default()
                .shell_allowlist
                .into_iter()
                .collect(),
            max_shell_output: 4_000,
            metrics_sink: None,
        }
    }

    #[must_use]
    pub fn with_policy(mut self, policy: SkillRenderPolicy) -> Self {
        self.shell_allowlist = policy.shell_allowlist.into_iter().collect();
        self.max_shell_output = policy.max_shell_output;
        self
    }

    #[must_use]
    pub fn with_shell_allowlist(mut self, cmds: impl IntoIterator<Item = String>) -> Self {
        self.shell_allowlist = cmds.into_iter().collect();
        self
    }

    #[must_use]
    pub fn with_max_shell_output(mut self, max_shell_output: usize) -> Self {
        self.max_shell_output = max_shell_output;
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn SkillMetricsSink>) -> Self {
        self.metrics_sink = Some(metrics_sink);
        self
    }

    pub async fn render(&self, skill: &Skill, params: Value) -> Result<RenderedSkill, RenderError> {
        let started = Instant::now();
        for parameter in &skill.frontmatter.parameters {
            let value = params
                .get(&parameter.name)
                .cloned()
                .or_else(|| parameter.default.clone());
            if parameter.required && value.is_none() {
                return Err(RenderError::MissingParam(parameter.name.clone()));
            }
            if let Some(value) = &value {
                validate_value_type(&parameter.name, value, parameter.param_type)?;
            }
        }

        let mut content = skill.body.clone();
        let mut consumed_config_keys = Vec::new();

        for parameter in &skill.frontmatter.parameters {
            let value = params
                .get(&parameter.name)
                .cloned()
                .or_else(|| parameter.default.clone());
            if let Some(value) = value {
                content = content.replace(
                    &format!("${{{}}}", parameter.name),
                    &render_value_for_template(&value),
                );
            }
        }

        for config in &skill.frontmatter.config {
            let secret_pattern = format!("${{config.{}:secret}}", config.key);
            if content.contains(&secret_pattern) {
                return Err(ConfigResolveError::SecretInterpolationForbidden {
                    skill_id: skill.id.0.clone(),
                    key: config.key.clone(),
                }
                .into());
            }

            let pattern = format!("${{config.{}}}", config.key);
            if content.contains(&pattern) {
                if config.secret {
                    return Err(ConfigResolveError::SecretInterpolationForbidden {
                        skill_id: skill.id.0.clone(),
                        key: config.key.clone(),
                    }
                    .into());
                }
                let value = self
                    .config_resolver
                    .resolve_for(&skill.id, &config.key)
                    .await?;
                content = content.replace(&pattern, &render_value_for_template(&value));
                if !consumed_config_keys.iter().any(|key| key == &config.key) {
                    consumed_config_keys.push(config.key.clone());
                }
            }
        }

        let (content, shell_invocations) = self.render_shell_blocks(&content)?;

        let rendered = RenderedSkill {
            skill_id: skill.id.clone(),
            skill_name: skill.name.clone(),
            content,
            shell_invocations,
            consumed_config_keys,
        };
        if let Some(metrics) = &self.metrics_sink {
            metrics.skill_render_duration_ms(started.elapsed().as_millis() as u64);
            for invocation in &rendered.shell_invocations {
                metrics.skill_shell_invocation(&invocation.command);
            }
        }
        Ok(rendered)
    }

    fn render_shell_blocks(
        &self,
        content: &str,
    ) -> Result<(String, Vec<ShellInvocation>), RenderError> {
        let mut output = String::with_capacity(content.len());
        let mut remaining = content;
        let mut invocations = Vec::new();

        while let Some(start) = remaining.find("!`") {
            output.push_str(&remaining[..start]);
            let after_start = &remaining[start + 2..];
            let Some(end) = after_start.find('`') else {
                output.push_str(&remaining[start..]);
                return Ok((output, invocations));
            };
            let command = &after_start[..end];
            if let Some(command) = self.allowed_command(command) {
                let rendered = self.execute_shell(&command)?;
                output.push_str(&rendered.0);
                invocations.push(rendered.1);
            } else {
                if let Some(metrics) = &self.metrics_sink {
                    metrics.skill_shell_blocked(command);
                }
                output.push_str("[SHELL_NOT_ALLOWED]");
            }
            remaining = &after_start[end + 1..];
        }

        output.push_str(remaining);
        Ok((output, invocations))
    }

    fn allowed_command(&self, command: &str) -> Option<StructuredShellCommand> {
        let command = parse_shell_command(command).ok()?;
        self.shell_allowlist
            .contains(&command.program)
            .then_some(command)
    }

    fn execute_shell(
        &self,
        command: &StructuredShellCommand,
    ) -> Result<(String, ShellInvocation), RenderError> {
        let output = std::process::Command::new(&command.program)
            .args(&command.args)
            .output()?;
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let (stdout, stdout_truncated) = truncate_chars(&stdout, self.max_shell_output);
        Ok((
            stdout,
            ShellInvocation {
                command: command.original.clone(),
                stdout_truncated,
                exit_code,
            },
        ))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct StructuredShellCommand {
    original: String,
    program: String,
    args: Vec<String>,
}

fn validate_value_type(
    name: &str,
    value: &Value,
    param_type: SkillParamType,
) -> Result<(), RenderError> {
    let valid = match param_type {
        SkillParamType::String | SkillParamType::Path => value.as_str().is_some(),
        SkillParamType::Url => value
            .as_str()
            .is_some_and(|value| value.starts_with("http://") || value.starts_with("https://")),
        SkillParamType::Number => value.as_f64().is_some(),
        SkillParamType::Boolean => value.as_bool().is_some(),
    };
    if valid {
        Ok(())
    } else {
        Err(RenderError::InvalidParam {
            name: name.to_owned(),
            expected: expected_type_label(param_type),
        })
    }
}

fn expected_type_label(param_type: SkillParamType) -> &'static str {
    match param_type {
        SkillParamType::String => "string",
        SkillParamType::Number => "number",
        SkillParamType::Boolean => "boolean",
        SkillParamType::Path => "path string",
        SkillParamType::Url => "http or https url string",
    }
}

fn parse_shell_command(input: &str) -> Result<StructuredShellCommand, RenderError> {
    let original = input.trim().to_owned();
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = original.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (None, '\n' | '\r') => return Err(RenderError::ShellNotAllowed(original)),
            (None, ch) if ch.is_whitespace() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            (None, '\'' | '"') => quote = Some(ch),
            (Some(active), ch) if ch == active => quote = None,
            (None, '\\') | (Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (None, ';' | '|' | '&' | '<' | '>' | '$' | '(' | ')') => {
                return Err(RenderError::ShellNotAllowed(original));
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err(RenderError::ShellNotAllowed(original));
    }
    if !current.is_empty() {
        args.push(current);
    }
    if args.is_empty() {
        return Err(RenderError::ShellNotAllowed(original));
    }

    let program = args.remove(0);
    Ok(StructuredShellCommand {
        original,
        program,
        args,
    })
}

fn render_value_for_template(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    if value.chars().count() <= max_chars {
        return (value.to_owned(), false);
    }

    let truncated = value.chars().take(max_chars).collect::<String>();
    (format!("{truncated}[...truncated]"), true)
}
