#[cfg(not(feature = "builtin-toolset"))]
use harness_tool::{BuiltinToolset, ToolRegistry};

#[test]
#[cfg(not(feature = "builtin-toolset"))]
fn default_toolset_is_empty_without_builtin_toolset_feature() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    assert_eq!(registry.snapshot().iter_sorted().count(), 0);
}

#[test]
#[cfg(not(feature = "builtin-toolset"))]
fn builtin_specific_toolsets_fail_closed_without_builtin_toolset_feature() {
    let shell_error = match ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Shell)
        .build()
    {
        Ok(_) => panic!("shell toolset should fail without builtin-toolset"),
        Err(error) => error,
    };
    assert!(shell_error
        .to_string()
        .contains("shell tools feature is not enabled"));

    let clarification_error = match ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Clarification)
        .build()
    {
        Ok(_) => panic!("clarification toolset should fail without builtin-toolset"),
        Err(error) => error,
    };
    assert!(clarification_error
        .to_string()
        .contains("clarification tools feature is not enabled"));
}

#[test]
#[cfg(not(any(feature = "builtin-toolset", feature = "skill-tools")))]
fn skills_toolset_fails_closed_without_any_skill_registration_feature() {
    let error = match ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Skills)
        .build()
    {
        Ok(_) => panic!("skills toolset should fail without skill registration features"),
        Err(error) => error,
    };

    assert!(error
        .to_string()
        .contains("skill tools feature is not enabled"));
}

#[test]
#[cfg(all(feature = "skill-tools", not(feature = "builtin-toolset")))]
fn skill_tools_feature_registers_only_skills_toolset() {
    let default_registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    assert_eq!(default_registry.snapshot().iter_sorted().count(), 0);

    let skills_registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Skills)
        .build()
        .unwrap();
    let snapshot = skills_registry.snapshot();
    let names = snapshot
        .iter_sorted()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["skills_invoke", "skills_list", "skills_view"]);
}
