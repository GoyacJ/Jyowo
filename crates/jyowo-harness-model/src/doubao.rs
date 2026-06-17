use crate::openai_compatible::openai_compatible_provider;

openai_compatible_provider!(
    provider = DoubaoProvider,
    provider_id = "doubao",
    env = DOUBAO_API_KEY_ENV => "DOUBAO_API_KEY",
    base_url = "https://ark.cn-beijing.volces.com/api/v3",
    path = "/chat/completions",
    context_window = 128_000,
    max_output_tokens = 16_384,
    models = [
        ("doubao-seed-1.6", "Doubao Seed 1.6"),
        ("doubao-seed-1.6-thinking", "Doubao Seed 1.6 Thinking"),
        ("doubao-seed-1.6-flash", "Doubao Seed 1.6 Flash"),
    ]
);
