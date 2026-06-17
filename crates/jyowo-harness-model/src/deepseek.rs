use crate::openai_compatible::openai_compatible_provider;

openai_compatible_provider!(
    provider = DeepSeekProvider,
    provider_id = "deepseek",
    env = DEEPSEEK_API_KEY_ENV => "DEEPSEEK_API_KEY",
    base_url = "https://api.deepseek.com",
    path = "/v1/chat/completions",
    context_window = 128_000,
    max_output_tokens = 8192,
    models = [
        ("deepseek-chat", "DeepSeek Chat"),
        ("deepseek-reasoner", "DeepSeek Reasoner"),
    ]
);
