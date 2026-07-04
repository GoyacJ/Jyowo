pub fn assert_agent_runtime_identity(prompt: &str) {
    assert!(prompt.contains("Jyowo"));
    assert!(prompt.contains("本地 agent runtime 工作空间"));
    assert!(prompt.contains("不能以底层 model provider 身份自称"));
    assert!(prompt.contains("Rust runtime"));
    assert!(prompt.contains("workspace instructions"));
    assert!(prompt.contains("memory 只是辅助上下文"));
    assert!(!prompt.contains("AI 编程伙伴"));
    assert!(!prompt.contains("本地项目工作空间里的 AI 编程伙伴"));
}

pub fn assert_runtime_context_contract(system: &str) {
    assert!(system.contains("<runtime-context>"));
    assert!(system.contains("permission_mode:"));
    assert!(system.contains("interactivity:"));
    assert!(system.contains("tool_search:"));
    assert!(system.contains("model_provider:"));
    assert!(system.contains("model_id:"));
    assert!(system.contains("model_protocol:"));
    assert!(system.contains("tool_calling:"));
    assert!(system.contains("builtin_memory:"));
    assert!(system.contains("sandbox:"));
    assert!(system.contains("subagent_tool:"));
    assert!(system.contains("tool_calling: enabled") || system.contains("tool_calling: disabled"));
    assert!(
        system.contains("builtin_memory: enabled") || system.contains("builtin_memory: disabled")
    );
    assert!(
        system.contains("subagent_tool: enabled") || system.contains("subagent_tool: disabled")
    );
    assert!(!system.contains("sk-"));
    let lower = system.to_lowercase();
    assert!(!lower.contains("api_key"));
    assert!(!lower.contains("credential"));
}
