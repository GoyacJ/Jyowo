use crate::openai_compatible::openai_compatible_provider;

openai_compatible_provider!(
    provider = KmProvider,
    provider_id = "km",
    env = KM_API_KEY_ENV => "KM_API_KEY",
    base_url = "https://api.moonshot.cn",
    path = "/v1/chat/completions",
    context_window = 200_000,
    max_output_tokens = 16_384,
    models = [
        ("kimi-k2.5", "Kimi K2.5"),
        ("kimi-k2-thinking", "Kimi K2 Thinking"),
        ("kimi-k2-thinking-turbo", "Kimi K2 Thinking Turbo"),
    ]
);
