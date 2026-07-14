use std::collections::{BTreeMap, BTreeSet};
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

    async fn resolve_secret_for_script(
        &self,
        skill_id: &harness_contracts::SkillId,
        key: &str,
    ) -> Result<SecretString, ConfigResolveError> {
        Err(ConfigResolveError::Message(format!(
            "script secret resolution is unavailable for config `{key}` in skill `{}`",
            skill_id.0
        )))
    }
}

pub type SkillConfigResolverFactory =
    dyn Fn(&Skill) -> Arc<dyn SkillConfigResolver> + Send + Sync + 'static;

#[derive(Clone)]
enum SkillConfigResolverBinding {
    Shared(Arc<dyn SkillConfigResolver>),
    PerSkill(Arc<SkillConfigResolverFactory>),
}

impl SkillConfigResolverBinding {
    fn for_skill(&self, skill: &Skill) -> Arc<dyn SkillConfigResolver> {
        match self {
            Self::Shared(resolver) => Arc::clone(resolver),
            Self::PerSkill(factory) => factory(skill),
        }
    }
}

#[derive(Clone)]
pub struct SkillRenderer {
    config_resolver: SkillConfigResolverBinding,
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
            config_resolver: SkillConfigResolverBinding::Shared(config_resolver),
            metrics_sink: None,
        }
    }

    #[must_use]
    pub fn new_with_config_resolver_factory(
        config_resolver_factory: Arc<SkillConfigResolverFactory>,
    ) -> Self {
        Self {
            config_resolver: SkillConfigResolverBinding::PerSkill(config_resolver_factory),
            metrics_sink: None,
        }
    }

    #[must_use]
    pub fn with_policy(self, _policy: SkillRenderPolicy) -> Self {
        self
    }

    #[must_use]
    pub fn with_shell_allowlist(self, _cmds: impl IntoIterator<Item = String>) -> Self {
        self
    }

    #[must_use]
    pub fn with_max_shell_output(self, _max_shell_output: usize) -> Self {
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn SkillMetricsSink>) -> Self {
        self.metrics_sink = Some(metrics_sink);
        self
    }

    pub async fn render(&self, skill: &Skill, params: Value) -> Result<RenderedSkill, RenderError> {
        let started = Instant::now();
        let config_resolver = self.config_resolver.for_skill(skill);
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
                let value = config_resolver.resolve_for(&skill.id, &config.key).await?;
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

    pub(crate) async fn resolve_script_environment(
        &self,
        skill: &Skill,
        declaration: &crate::SkillScriptDecl,
    ) -> Result<(BTreeMap<String, String>, BTreeSet<String>), ConfigResolveError> {
        use secrecy::ExposeSecret;

        let resolver = self.config_resolver.for_skill(skill);
        let declarations = skill
            .frontmatter
            .config
            .iter()
            .map(|config| (config.key.as_str(), config))
            .collect::<BTreeMap<_, _>>();
        let mut env = BTreeMap::new();
        let mut secret_env_keys = BTreeSet::new();
        for (env_name, mapping) in &declaration.env {
            let config = declarations
                .get(mapping.config.as_str())
                .ok_or_else(|| ConfigResolveError::UnknownKey(mapping.config.clone()))?;
            if config.secret != mapping.secret {
                return Err(ConfigResolveError::Message(format!(
                    "script environment `{env_name}` does not match config secrecy"
                )));
            }
            let value = if mapping.secret {
                secret_env_keys.insert(env_name.clone());
                resolver
                    .resolve_secret_for_script(&skill.id, &mapping.config)
                    .await?
                    .expose_secret()
                    .to_owned()
            } else {
                let value = resolver.resolve_for(&skill.id, &mapping.config).await?;
                if !script_config_type_matches(&value, config.value_type) {
                    return Err(ConfigResolveError::InvalidType {
                        skill_id: skill.id.0.clone(),
                        key: mapping.config.clone(),
                        expected: expected_type_label(config.value_type),
                    });
                }
                render_value_for_template(&value)
            };
            env.insert(env_name.clone(), value);
        }
        Ok((env, secret_env_keys))
    }

    fn render_shell_blocks(
        &self,
        content: &str,
    ) -> Result<(String, Vec<ShellInvocation>), RenderError> {
        let mut output = String::with_capacity(content.len());
        let mut remaining = content;
        let invocations = Vec::new();

        while let Some(start) = remaining.find("!`") {
            output.push_str(&remaining[..start]);
            let after_start = &remaining[start + 2..];
            let Some(end) = after_start.find('`') else {
                output.push_str(&remaining[start..]);
                return Ok((output, invocations));
            };
            let command = &after_start[..end];
            if let Some(metrics) = &self.metrics_sink {
                metrics.skill_shell_blocked(command);
            }
            output.push_str("[SHELL_NOT_ALLOWED]");
            remaining = &after_start[end + 1..];
        }

        output.push_str(remaining);
        Ok((output, invocations))
    }
}

fn script_config_type_matches(value: &Value, value_type: SkillParamType) -> bool {
    match value_type {
        SkillParamType::String | SkillParamType::Path => value.is_string(),
        SkillParamType::Url => value
            .as_str()
            .is_some_and(|value| value.starts_with("http://") || value.starts_with("https://")),
        SkillParamType::Number => value.is_number(),
        SkillParamType::Boolean => value.is_boolean(),
    }
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

fn render_value_for_template(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}
