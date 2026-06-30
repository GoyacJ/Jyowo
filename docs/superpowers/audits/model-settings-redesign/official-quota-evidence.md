# Official Provider Quota Evidence

Task 4 evidence for model settings redesign. Catalog ids enumerated from `crates/jyowo-harness-model/src/registry.rs::provider_catalog_entries()` on 2026-06-30.

```text
catalog provider ids source:
crates/jyowo-harness-model/src/registry.rs::provider_catalog_entries()

catalog provider ids:
anthropic, codex, deepseek, doubao, gemini, km, local-llama, minimax, openai, openrouter, qwen, zhipu
```

---

```text
provider id: anthropic
official account usage/quota API: none documented for API-key account balance or quota retrieval
official source URL: https://docs.anthropic.com/en/api/getting-started
accessed at: 2026-06-30
required credential scope: n/a
credential storage decision: existing
supported in this task: no
reason: Official Anthropic API docs describe inference endpoints and rate limits only; no account balance or quota query endpoint usable with the stored API key.
```

```text
provider id: codex
official account usage/quota API: OpenAI Organization Usage API (admin key required)
official source URL: https://platform.openai.com/docs/api-reference/usage
accessed at: 2026-06-30
required credential scope: organization admin API key (distinct from standard chat API key)
credential storage decision: extend
supported in this task: no
reason: Usage endpoints require an organization admin key not stored in current provider settings; adapter returns auth_required until a safe admin credential field exists.
```

```text
provider id: deepseek
official account usage/quota API: GET /user/balance
official source URL: https://api-docs.deepseek.com/api/get-user-balance
accessed at: 2026-06-30
required credential scope: Bearer API key (same as inference)
credential storage decision: existing
supported in this task: yes
reason: Balance endpoint accepts the same API key already stored for inference and returns account balance without provider-native payload retention.
```

```text
provider id: doubao
official account usage/quota API: none documented for API-key account balance retrieval
official source URL: https://www.volcengine.com/docs/82379/1494384
accessed at: 2026-06-30
required credential scope: n/a
credential storage decision: existing
supported in this task: no
reason: Volcengine model docs cover inference pricing and endpoints; no documented balance API callable with the stored API key alone.
```

```text
provider id: gemini
official account usage/quota API: none documented for API-key account balance retrieval
official source URL: https://ai.google.dev/gemini-api/docs/rate-limits
accessed at: 2026-06-30
required credential scope: n/a
credential storage decision: existing
supported in this task: no
reason: Gemini API docs document rate limits and billing via Google AI Studio console; no account quota API for the stored API key.
```

```text
provider id: km
official account usage/quota API: none documented for API-key account balance retrieval
official source URL: https://platform.moonshot.ai/docs
accessed at: 2026-06-30
required credential scope: n/a
credential storage decision: existing
supported in this task: no
reason: Moonshot/Kimi docs describe chat endpoints only; no documented balance or quota API for the stored API key.
```

```text
provider id: local-llama
official account usage/quota API: none (local runtime)
official source URL: https://ollama.com/library
accessed at: 2026-06-30
required credential scope: n/a
credential storage decision: existing
supported in this task: no
reason: Local Llama runs on the user's machine; Ollama has no account quota API.
```

```text
provider id: minimax
official account usage/quota API: none documented for API-key account balance retrieval
official source URL: https://platform.minimax.io/docs/faq/about-account
accessed at: 2026-06-30
required credential scope: n/a
credential storage decision: existing
supported in this task: no
reason: MiniMax account balance is managed in the web console; official API docs do not expose a balance endpoint for the stored pay-as-you-go API key.
```

```text
provider id: openai
official account usage/quota API: OpenAI Organization Usage API (admin key required)
official source URL: https://platform.openai.com/docs/api-reference/usage
accessed at: 2026-06-30
required credential scope: organization admin API key (distinct from standard chat API key)
credential storage decision: extend
supported in this task: no
reason: Usage endpoints require an organization admin key not stored in current provider settings; adapter returns auth_required until a safe admin credential field exists.
```

```text
provider id: openrouter
official account usage/quota API: GET /api/v1/key
official source URL: https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key
accessed at: 2026-06-30
required credential scope: Bearer API key (same as inference)
credential storage decision: existing
supported in this task: yes
reason: Key endpoint returns credit usage and remaining limits for the authenticated API key without a separate management credential.
```

```text
provider id: qwen
official account usage/quota API: none documented for DashScope API-key balance retrieval
official source URL: https://help.aliyun.com/zh/model-studio/models
accessed at: 2026-06-30
required credential scope: n/a
credential storage decision: existing
supported in this task: no
reason: DashScope docs cover inference; Alibaba Cloud BSS balance APIs require separate cloud account credentials, not the stored DashScope API key.
```

```text
provider id: zhipu
official account usage/quota API: none documented for API-key account balance retrieval
official source URL: https://docs.bigmodel.cn/api-reference/模型-api/对话补全
accessed at: 2026-06-30
required credential scope: n/a
credential storage decision: existing
supported in this task: no
reason: Zhipu BigModel docs describe chat completion only; no documented account balance API for the stored API key.
```
