use crate::openai_compatible::openai_compatible_provider;

openai_compatible_provider!(
    provider = QwenProvider,
    provider_id = "qwen",
    env = QWEN_API_KEY_ENV => "QWEN_API_KEY",
    base_url = "https://dashscope.aliyuncs.com/compatible-mode",
    path = "/v1/chat/completions",
    context_window = 128_000,
    max_output_tokens = 8192,
    models = [
        ("qwen3-max", "Qwen3 Max"),
        ("qwen3-coder-plus", "Qwen3 Coder Plus"),
    ]
);
