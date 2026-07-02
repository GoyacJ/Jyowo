#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum OpenAiChatDialect {
    Plain,
    MiniMax,
    DeepSeek,
    Qwen,
    Doubao,
    Zhipu,
    Kimi,
    OpenRouter,
    LocalLlama,
}

impl Default for OpenAiChatDialect {
    fn default() -> Self {
        Self::Plain
    }
}

#[cfg(test)]
mod provider_dialect_tests {
    use super::OpenAiChatDialect;
    use crate::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};

    #[test]
    fn generic_openai_compatible_client_uses_plain_dialect() {
        let client = OpenAiCompatibleClient::from_api_key("provider-key", "http://localhost");

        assert_eq!(client.chat_dialect(), OpenAiChatDialect::Plain);
    }

    #[cfg(feature = "minimax")]
    #[test]
    fn minimax_provider_uses_minimax_dialect() {
        let provider = crate::minimax::MinimaxProvider::from_api_key("provider-key");

        assert_eq!(provider.client().chat_dialect(), OpenAiChatDialect::MiniMax);
    }

    #[cfg(feature = "deepseek")]
    #[test]
    fn deepseek_provider_uses_deepseek_dialect() {
        let provider = crate::deepseek::DeepSeekProvider::from_api_key("provider-key");

        assert_eq!(
            provider.client().chat_dialect(),
            OpenAiChatDialect::DeepSeek
        );
    }

    #[cfg(feature = "qwen")]
    #[test]
    fn qwen_provider_uses_qwen_dialect() {
        let provider = crate::qwen::QwenProvider::from_api_key("provider-key");

        assert_eq!(provider.client().chat_dialect(), OpenAiChatDialect::Qwen);
    }

    #[cfg(feature = "doubao")]
    #[test]
    fn doubao_provider_uses_doubao_dialect() {
        let provider = crate::doubao::DoubaoProvider::from_api_key("provider-key");

        assert_eq!(provider.client().chat_dialect(), OpenAiChatDialect::Doubao);
    }

    #[cfg(feature = "zhipu")]
    #[test]
    fn zhipu_provider_uses_zhipu_dialect() {
        let provider = crate::zhipu::ZhipuProvider::from_api_key("provider-key");

        assert_eq!(provider.client().chat_dialect(), OpenAiChatDialect::Zhipu);
    }

    #[cfg(feature = "km")]
    #[test]
    fn km_provider_uses_kimi_dialect() {
        let provider = crate::km::KmProvider::from_api_key("provider-key");

        assert_eq!(provider.client().chat_dialect(), OpenAiChatDialect::Kimi);
    }

    #[cfg(feature = "openrouter")]
    #[test]
    fn openrouter_provider_uses_openrouter_dialect() {
        let provider = crate::openrouter::OpenRouterProvider::from_api_key("provider-key");

        assert_eq!(
            provider.client().chat_dialect(),
            OpenAiChatDialect::OpenRouter
        );
    }

    #[cfg(feature = "local-llama")]
    #[test]
    fn local_llama_provider_uses_local_llama_dialect() {
        let provider = crate::local_llama::LocalLlamaProvider::default();

        assert_eq!(
            provider.client().chat_dialect(),
            OpenAiChatDialect::LocalLlama
        );
    }
}
