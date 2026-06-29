//! `jyowo-harness-model`
//!
//! Model provider traits, credentials, token counting, and middleware.
//!
//! SPEC: docs/architecture/harness/crates/harness-model.md
//! Status: model provider contracts, middleware, credentials, pricing, and providers.

#![forbid(unsafe_code)]

#[cfg(feature = "anthropic")]
pub mod anthropic;
pub mod aux;
#[cfg(feature = "bedrock")]
pub mod bedrock;
#[cfg(feature = "cassette")]
pub mod cassette;
#[cfg(feature = "codex")]
pub mod codex;
pub mod credential;
pub mod credential_pool;
#[cfg(feature = "deepseek")]
pub mod deepseek;
#[cfg(feature = "doubao")]
pub mod doubao;
#[cfg(feature = "gemini")]
pub mod gemini;
#[cfg(feature = "km")]
pub mod km;
#[cfg(feature = "local-llama")]
pub mod local_llama;
pub mod metrics;
pub mod middleware;
#[cfg(feature = "minimax")]
pub mod minimax;
#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "openai-compatible")]
pub(crate) mod openai_compatible;
#[cfg(feature = "openrouter")]
pub mod openrouter;
pub mod provider;
#[cfg(feature = "qwen")]
pub mod qwen;
pub mod registry;
pub mod stream_aggregator;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod thinking_tag_normalizer;
pub mod token_counter;
#[cfg(feature = "zhipu")]
pub mod zhipu;

#[cfg(feature = "anthropic")]
pub use anthropic::*;
pub use aux::*;
#[cfg(feature = "bedrock")]
pub use bedrock::*;
#[cfg(feature = "cassette")]
pub use cassette::*;
#[cfg(feature = "codex")]
pub use codex::*;
pub use credential::*;
pub use credential_pool::*;
#[cfg(feature = "deepseek")]
pub use deepseek::*;
#[cfg(feature = "doubao")]
pub use doubao::*;
#[cfg(feature = "gemini")]
pub use gemini::*;
pub use harness_contracts::{
    ConversationModelCapability, ModelModality, ModelProtocol, ProviderAuthScheme,
    ProviderBaseUrlRegion, ProviderRuntimeCapability, ProviderServiceCapability,
    ProviderServiceCategory, ProviderServiceCostRisk, ProviderServiceExecution,
};
#[cfg(feature = "km")]
pub use km::*;
#[cfg(feature = "local-llama")]
pub use local_llama::*;
pub use metrics::*;
pub use middleware::*;
#[cfg(feature = "minimax")]
pub use minimax::*;
#[cfg(feature = "openai")]
pub use openai::*;
#[cfg(feature = "openrouter")]
pub use openrouter::*;
pub use provider::*;
#[cfg(feature = "qwen")]
pub use qwen::*;
pub use registry::*;
pub use stream_aggregator::*;
#[cfg(any(test, feature = "testing"))]
pub use testing::*;
pub use token_counter::*;
#[cfg(feature = "zhipu")]
pub use zhipu::*;
