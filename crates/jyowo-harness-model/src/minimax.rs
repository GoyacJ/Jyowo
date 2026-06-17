use crate::openai_compatible::openai_compatible_provider;

openai_compatible_provider!(
    provider = MinimaxProvider,
    provider_id = "minimax",
    env = MINIMAX_API_KEY_ENV => "MINIMAX_API_KEY",
    base_url = "https://api.minimaxi.com",
    path = "/v1/chat/completions",
    context_window = 200_000,
    max_output_tokens = 16_384,
    models = [
        ("MiniMax-M2.7", "MiniMax M2.7"),
        ("MiniMax-M2.5", "MiniMax M2.5"),
        ("MiniMax-M2.1", "MiniMax M2.1"),
        ("MiniMax-M2", "MiniMax M2"),
    ]
);
