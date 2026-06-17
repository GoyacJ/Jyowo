#![cfg(feature = "prometheus")]

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use harness_contracts::UsageSnapshot;
use harness_observability::{Observer, UsageScope};

#[test]
fn prometheus_exporter_renders_usage_scrape() {
    let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let observer = Observer::builder().with_prometheus(bind).build().unwrap();

    observer.usage.record(
        UsageScope::Model("mock/usage-model".to_owned()),
        None,
        UsageSnapshot {
            input_tokens: 3,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 7,
            tool_calls: 0,
        },
    );

    let exporter = observer.prometheus.as_ref().unwrap();
    assert_eq!(exporter.bind_addr(), bind);
    let scrape = exporter.scrape();

    assert!(scrape.contains("jyowo_harness_usage_input_tokens"));
    assert!(scrape.contains("scope=\"model\",id=\"mock/usage-model\"} 3"));
    assert!(scrape.contains("jyowo_harness_usage_cost_micros{scope=\"model\""));
}

#[test]
fn prometheus_exporter_renders_model_metrics_scrape() {
    let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let observer = Observer::builder().with_prometheus(bind).build().unwrap();

    observer.model_metrics.record_infer(
        "mock/usage-model",
        Duration::from_millis(42),
        &UsageSnapshot {
            input_tokens: 3,
            output_tokens: 5,
            cache_read_tokens: 2,
            cache_write_tokens: 7,
            cost_micros: 0,
            tool_calls: 0,
        },
    );
    observer
        .model_metrics
        .record_credential_pool_cooldown("mock/usage-model");
    observer
        .model_metrics
        .record_model_error("mock/usage-model", "timeout");
    observer
        .model_metrics
        .record_stream_error("mock/usage-model", "provider");
    observer
        .model_metrics
        .record_aux_queue_wait("mock/usage-model", Duration::from_millis(9));

    let scrape = observer.prometheus.as_ref().unwrap().scrape();

    assert!(scrape.contains("jyowo_harness_model_infer_duration_ms{model=\"mock/usage-model\"} 42"));
    assert!(scrape.contains("jyowo_harness_model_tokens_input{model=\"mock/usage-model\"} 3"));
    assert!(scrape.contains("jyowo_harness_model_tokens_output{model=\"mock/usage-model\"} 5"));
    assert!(
        scrape.contains("jyowo_harness_model_cache_creation_tokens{model=\"mock/usage-model\"} 7")
    );
    assert!(scrape.contains("jyowo_harness_model_cache_read_tokens{model=\"mock/usage-model\"} 2"));
    assert!(scrape
        .contains("jyowo_harness_credential_pool_cooldowns_total{model=\"mock/usage-model\"} 1"));
    assert!(scrape.contains(
        "jyowo_harness_model_errors_total{model=\"mock/usage-model\",class=\"timeout\"} 1"
    ));
    assert!(scrape.contains(
        "jyowo_harness_model_stream_error_total{model=\"mock/usage-model\",class=\"provider\"} 1"
    ));
    assert!(scrape.contains("jyowo_harness_aux_model_queue_wait_ms{model=\"mock/usage-model\"} 9"));
}
