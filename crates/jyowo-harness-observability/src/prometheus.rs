use std::net::SocketAddr;
use std::sync::Arc;

use crate::{ModelMetricsAccumulator, UsageAccumulator};

#[derive(Clone)]
pub struct PrometheusExporter {
    bind: SocketAddr,
    usage: Arc<UsageAccumulator>,
    model_metrics: Arc<ModelMetricsAccumulator>,
}

impl PrometheusExporter {
    #[must_use]
    pub fn new(
        bind: SocketAddr,
        usage: Arc<UsageAccumulator>,
        model_metrics: Arc<ModelMetricsAccumulator>,
    ) -> Self {
        Self {
            bind,
            usage,
            model_metrics,
        }
    }

    #[must_use]
    pub fn bind_addr(&self) -> SocketAddr {
        self.bind
    }

    #[must_use]
    pub fn scrape(&self) -> String {
        let mut out = render_usage_metrics(&self.usage);
        out.push_str(&render_model_metrics(&self.model_metrics));
        out
    }
}

#[must_use]
pub fn render_usage_metrics(usage: &UsageAccumulator) -> String {
    let report = usage.report();
    let mut out = String::new();
    out.push_str("# HELP jyowo_harness_usage_input_tokens Input tokens recorded by Harness.\n");
    out.push_str("# TYPE jyowo_harness_usage_input_tokens counter\n");
    out.push_str("# HELP jyowo_harness_usage_output_tokens Output tokens recorded by Harness.\n");
    out.push_str("# TYPE jyowo_harness_usage_output_tokens counter\n");
    out.push_str("# HELP jyowo_harness_usage_cost_micros Cost recorded by Harness in micro currency units.\n");
    out.push_str("# TYPE jyowo_harness_usage_cost_micros counter\n");

    push_scope(
        &mut out,
        "global",
        "all",
        report.global.input_tokens,
        report.global.output_tokens,
        report.global.cost_micros,
    );
    for (tenant, snapshot) in report.tenants {
        push_scope(
            &mut out,
            "tenant",
            &tenant.to_string(),
            snapshot.input_tokens,
            snapshot.output_tokens,
            snapshot.cost_micros,
        );
    }
    for (session, snapshot) in report.sessions {
        push_scope(
            &mut out,
            "session",
            &session.to_string(),
            snapshot.input_tokens,
            snapshot.output_tokens,
            snapshot.cost_micros,
        );
    }
    for (run, snapshot) in report.runs {
        push_scope(
            &mut out,
            "run",
            &run.to_string(),
            snapshot.input_tokens,
            snapshot.output_tokens,
            snapshot.cost_micros,
        );
    }
    for (model, snapshot) in report.models {
        push_scope(
            &mut out,
            "model",
            &model,
            snapshot.input_tokens,
            snapshot.output_tokens,
            snapshot.cost_micros,
        );
    }
    out
}

#[must_use]
pub fn render_model_metrics(model_metrics: &ModelMetricsAccumulator) -> String {
    let report = model_metrics.report();
    let mut out = String::new();
    out.push_str(
        "# HELP jyowo_harness_model_infer_duration_ms Model inference duration in milliseconds.\n",
    );
    out.push_str("# TYPE jyowo_harness_model_infer_duration_ms counter\n");
    out.push_str("# HELP jyowo_harness_model_infer_total Model inference attempts.\n");
    out.push_str("# TYPE jyowo_harness_model_infer_total counter\n");
    out.push_str("# HELP jyowo_harness_model_tokens_input Input tokens recorded by model calls.\n");
    out.push_str("# TYPE jyowo_harness_model_tokens_input counter\n");
    out.push_str(
        "# HELP jyowo_harness_model_tokens_output Output tokens recorded by model calls.\n",
    );
    out.push_str("# TYPE jyowo_harness_model_tokens_output counter\n");
    out.push_str(
        "# HELP jyowo_harness_model_cache_creation_tokens Cache creation tokens recorded by model calls.\n",
    );
    out.push_str("# TYPE jyowo_harness_model_cache_creation_tokens counter\n");
    out.push_str(
        "# HELP jyowo_harness_model_cache_read_tokens Cache read tokens recorded by model calls.\n",
    );
    out.push_str("# TYPE jyowo_harness_model_cache_read_tokens counter\n");
    out.push_str(
        "# HELP jyowo_harness_credential_pool_cooldowns_total Credential pool cooldowns recorded by model calls.\n",
    );
    out.push_str("# TYPE jyowo_harness_credential_pool_cooldowns_total counter\n");
    out.push_str("# HELP jyowo_harness_model_errors_total Model inference errors.\n");
    out.push_str("# TYPE jyowo_harness_model_errors_total counter\n");
    out.push_str("# HELP jyowo_harness_model_stream_error_total Model stream errors.\n");
    out.push_str("# TYPE jyowo_harness_model_stream_error_total counter\n");
    out.push_str(
        "# HELP jyowo_harness_aux_model_queue_wait_ms Aux model queue wait in milliseconds.\n",
    );
    out.push_str("# TYPE jyowo_harness_aux_model_queue_wait_ms counter\n");
    out.push_str("# HELP jyowo_harness_aux_model_queue_wait_total Aux model queue waits.\n");
    out.push_str("# TYPE jyowo_harness_aux_model_queue_wait_total counter\n");

    for (model, metrics) in report.models {
        let model = escape_label(&model);
        out.push_str(&format!(
            "jyowo_harness_model_infer_duration_ms{{model=\"{model}\"}} {}\n",
            metrics.infer_duration_ms
        ));
        out.push_str(&format!(
            "jyowo_harness_model_infer_total{{model=\"{model}\"}} {}\n",
            metrics.infer_total
        ));
        out.push_str(&format!(
            "jyowo_harness_model_tokens_input{{model=\"{model}\"}} {}\n",
            metrics.input_tokens
        ));
        out.push_str(&format!(
            "jyowo_harness_model_tokens_output{{model=\"{model}\"}} {}\n",
            metrics.output_tokens
        ));
        out.push_str(&format!(
            "jyowo_harness_model_cache_creation_tokens{{model=\"{model}\"}} {}\n",
            metrics.cache_creation_tokens
        ));
        out.push_str(&format!(
            "jyowo_harness_model_cache_read_tokens{{model=\"{model}\"}} {}\n",
            metrics.cache_read_tokens
        ));
        out.push_str(&format!(
            "jyowo_harness_credential_pool_cooldowns_total{{model=\"{model}\"}} {}\n",
            metrics.credential_pool_cooldowns_total
        ));
        out.push_str(&format!(
            "jyowo_harness_aux_model_queue_wait_ms{{model=\"{model}\"}} {}\n",
            metrics.aux_queue_wait_ms
        ));
        out.push_str(&format!(
            "jyowo_harness_aux_model_queue_wait_total{{model=\"{model}\"}} {}\n",
            metrics.aux_queue_wait_total
        ));
    }

    for (key, count) in report.model_errors {
        let model = escape_label(&key.model);
        let class = escape_label(&key.class);
        out.push_str(&format!(
            "jyowo_harness_model_errors_total{{model=\"{model}\",class=\"{class}\"}} {count}\n"
        ));
    }

    for (key, count) in report.stream_errors {
        let model = escape_label(&key.model);
        let class = escape_label(&key.class);
        out.push_str(&format!(
            "jyowo_harness_model_stream_error_total{{model=\"{model}\",class=\"{class}\"}} {count}\n"
        ));
    }

    out
}

fn push_scope(
    out: &mut String,
    scope: &str,
    id: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_micros: u64,
) {
    let id = escape_label(id);
    out.push_str(&format!(
        "jyowo_harness_usage_input_tokens{{scope=\"{scope}\",id=\"{id}\"}} {input_tokens}\n"
    ));
    out.push_str(&format!(
        "jyowo_harness_usage_output_tokens{{scope=\"{scope}\",id=\"{id}\"}} {output_tokens}\n"
    ));
    out.push_str(&format!(
        "jyowo_harness_usage_cost_micros{{scope=\"{scope}\",id=\"{id}\"}} {cost_micros}\n"
    ));
}

fn escape_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
