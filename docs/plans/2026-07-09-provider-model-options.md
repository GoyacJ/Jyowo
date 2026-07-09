# Provider Model Options Structured Configuration Implementation Plan

Goal: replace raw model options JSON in model settings with structured provider/model options.

Architecture: the frontend renders provider-aware controls and converts them into the existing internal `modelOptions` and `providerDefaults` request shapes. The backend validates those internal shapes and maps supported non-OpenAI protocol options into real provider request bodies.

Key implementation points:
- Remove user-visible raw JSON editing.
- Keep `providerDefaults` as internal storage/runtime input.
- Expose `supportedParameters` in model catalog payloads.
- Support OpenRouter dynamic `supported_parameters`.
- Add Bedrock to the provider catalog.
- Allow provider defaults for supported providers with allowlist validation.
- Map Anthropic, Gemini, and Bedrock provider defaults into native request bodies.

Verification:
- Rust tests for catalog, provider defaults validation, OpenRouter inventory, and request encoders.
- Frontend tests for structured settings payloads and JSON editor removal.
