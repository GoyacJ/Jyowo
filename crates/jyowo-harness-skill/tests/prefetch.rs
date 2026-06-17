use harness_skill::{SkillPrefetchPlan, SkillPrefetchStrategy};

#[test]
fn eager_prefetch_loads_registry_at_session_start() {
    assert_eq!(
        SkillPrefetchStrategy::Eager.plan_for_skill_count(10),
        SkillPrefetchPlan {
            load_on_session_start: true,
            load_limit: None,
        }
    );
}

#[test]
fn none_prefetch_defers_all_loading() {
    assert_eq!(
        SkillPrefetchStrategy::Disabled.plan_for_skill_count(10),
        SkillPrefetchPlan {
            load_on_session_start: false,
            load_limit: Some(0),
        }
    );
}

#[test]
fn hybrid_prefetch_only_eager_loads_small_skill_sets() {
    assert_eq!(
        SkillPrefetchStrategy::LazyPerTurn { concurrency: 20 }.plan_for_skill_count(10),
        SkillPrefetchPlan {
            load_on_session_start: true,
            load_limit: Some(20),
        }
    );
    assert_eq!(
        SkillPrefetchStrategy::LazyPerTurn { concurrency: 20 }.plan_for_skill_count(100),
        SkillPrefetchPlan {
            load_on_session_start: false,
            load_limit: Some(20),
        }
    );
}
