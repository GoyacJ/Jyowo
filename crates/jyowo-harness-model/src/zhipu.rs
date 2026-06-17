use crate::openai_compatible::openai_compatible_provider;

openai_compatible_provider!(
    provider = ZhipuProvider,
    provider_id = "zhipu",
    env = ZHIPU_API_KEY_ENV => "ZHIPU_API_KEY",
    base_url = "https://open.bigmodel.cn/api/paas/v4",
    path = "/chat/completions",
    context_window = 128_000,
    max_output_tokens = 8192,
    models = [
        ("glm-5", "GLM-5"),
        ("glm-5-turbo", "GLM-5 Turbo"),
        ("glm-4.7", "GLM-4.7"),
    ]
);
