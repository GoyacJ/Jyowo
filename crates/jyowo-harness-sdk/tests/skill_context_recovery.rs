#![cfg(feature = "testing")]

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    ContentHash, ConversationContextReference, Event, ModelError, SkillContextAssembledEvent,
    SkillContextError, SkillContextPreparedEvent, SkillContextProviderAcceptedEvent,
    SkillSourceKind, TenantId, CURRENT_CONTEXT_REFERENCE_VERSION,
};
use harness_journal::{EventStore, ReplayCursor};
use harness_model::{ContentDelta, ModelStreamEvent, ScriptedProvider, ScriptedResponse};
use harness_skill::{parse_skill_markdown, SkillPlatform, SkillSource};
use jyowo_harness_sdk::{prelude::*, testing::*};

#[tokio::test]
async fn assembled_skill_context_is_delivered_again_after_provider_failure() {
    let workspace = unique_workspace("sdk-skill-context-recovery");
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(ScriptedProvider::new(vec![
        ScriptedResponse::Error(ModelError::Message("provider unavailable".to_owned())),
        ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]),
    ]));
    let skill = test_skill("RECOVERABLE SKILL BODY");
    let skill_id = skill.id.clone();
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .unwrap();
    harness.skill_registry().register(skill).unwrap();
    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .unwrap();
    let delivery_key = "recovery-delivery-key".to_owned();

    let first = harness
        .submit_conversation_turn_with_run_control_and_skill_context_delivery_keys(
            selected_skill_request(options.clone(), skill_id.clone()),
            RunId::new(),
            RunControlHandle::new(),
            vec![Some(delivery_key.clone())],
        )
        .await;
    assert!(first.is_err());

    harness
        .submit_conversation_turn_with_run_control_and_skill_context_delivery_keys(
            selected_skill_request(options, skill_id),
            RunId::new(),
            RunControlHandle::new(),
            vec![Some(delivery_key.clone())],
        )
        .await
        .expect("assembled delivery should be retried");

    let requests = model.requests().await;
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(
            request_text(request)
                .matches("RECOVERABLE SKILL BODY")
                .count(),
            1
        );
    }
    let events = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::SkillContextPrepared(_)))
            .count(),
        1
    );
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SkillContextConsumed(event) if event.delivery_key == delivery_key)));
}

#[tokio::test]
async fn prepared_skill_context_is_rendered_and_consumed_on_recovery() {
    let workspace = unique_workspace("sdk-skill-context-prepared-recovery");
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::MessageStop,
    ])]));
    let skill = test_skill("PREPARED RECOVERY BODY");
    let skill_id = skill.id.clone();
    let body_hash = ContentHash(*blake3::hash(skill.body.as_bytes()).as_bytes());
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .unwrap();
    harness.skill_registry().register(skill).unwrap();
    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .unwrap();
    let delivery_key = "prepared-recovery-delivery-key".to_owned();
    store
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::SkillContextPrepared(SkillContextPreparedEvent {
                session_id,
                run_id: RunId::new(),
                delivery_key: delivery_key.clone(),
                reference: skill_reference(skill_id.clone()),
                body_hash,
                at: harness_contracts::now(),
            })],
        )
        .await
        .unwrap();

    harness
        .submit_conversation_turn_with_run_control_and_skill_context_delivery_keys(
            selected_skill_request(options, skill_id),
            RunId::new(),
            RunControlHandle::new(),
            vec![Some(delivery_key.clone())],
        )
        .await
        .expect("prepared delivery should recover");

    assert_eq!(
        request_text(&model.requests().await[0])
            .matches("PREPARED RECOVERY BODY")
            .count(),
        1
    );
    let events = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SkillContextConsumed(event) if event.delivery_key == delivery_key)));
}

#[tokio::test]
async fn provider_accepted_skill_context_is_delivered_again_then_consumed() {
    let workspace = unique_workspace("sdk-skill-context-accepted-recovery");
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::MessageStop,
    ])]));
    let skill = test_skill("ACCEPTED RECOVERY BODY");
    let skill_id = skill.id.clone();
    let body_hash = ContentHash(*blake3::hash(skill.body.as_bytes()).as_bytes());
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .unwrap();
    harness.skill_registry().register(skill).unwrap();
    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .unwrap();
    let delivery_key = "accepted-recovery-delivery-key".to_owned();
    let seed_run_id = RunId::new();
    store
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::SkillContextPrepared(SkillContextPreparedEvent {
                    session_id,
                    run_id: seed_run_id,
                    delivery_key: delivery_key.clone(),
                    reference: skill_reference(skill_id.clone()),
                    body_hash,
                    at: harness_contracts::now(),
                }),
                Event::SkillContextAssembled(SkillContextAssembledEvent {
                    session_id,
                    run_id: seed_run_id,
                    delivery_key: delivery_key.clone(),
                    at: harness_contracts::now(),
                }),
                Event::SkillContextProviderAccepted(SkillContextProviderAcceptedEvent {
                    session_id,
                    run_id: seed_run_id,
                    delivery_key: delivery_key.clone(),
                    at: harness_contracts::now(),
                }),
            ],
        )
        .await
        .unwrap();

    harness
        .submit_conversation_turn_with_run_control_and_skill_context_delivery_keys(
            selected_skill_request(options, skill_id),
            RunId::new(),
            RunControlHandle::new(),
            vec![Some(delivery_key.clone())],
        )
        .await
        .expect("accepted delivery should be retried at least once");

    assert_eq!(
        request_text(&model.requests().await[0])
            .matches("ACCEPTED RECOVERY BODY")
            .count(),
        1
    );
    let events = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SkillContextConsumed(event) if event.delivery_key == delivery_key)));
}

#[tokio::test]
async fn recovery_rejects_changed_rendered_skill_body() {
    let workspace = unique_workspace("sdk-skill-context-integrity");
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(ScriptedProvider::new(vec![
        ScriptedResponse::Error(ModelError::Message("provider unavailable".to_owned())),
        ScriptedResponse::Stream(vec![ModelStreamEvent::MessageStop]),
    ]));
    let skill = test_skill("ORIGINAL SKILL BODY");
    let skill_id = skill.id.clone();
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .unwrap();
    harness.skill_registry().register(skill).unwrap();
    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .unwrap();
    let delivery_key = "integrity-delivery-key".to_owned();
    let _ = harness
        .submit_conversation_turn_with_run_control_and_skill_context_delivery_keys(
            selected_skill_request(options.clone(), skill_id.clone()),
            RunId::new(),
            RunControlHandle::new(),
            vec![Some(delivery_key.clone())],
        )
        .await;
    harness
        .skill_registry()
        .replace_source(SkillSource::Bundled, vec![test_skill("CHANGED SKILL BODY")])
        .unwrap();

    let error = harness
        .submit_conversation_turn_with_run_control_and_skill_context_delivery_keys(
            selected_skill_request(options, skill_id),
            RunId::new(),
            RunControlHandle::new(),
            vec![Some(delivery_key.clone())],
        )
        .await
        .expect_err("changed body must fail recovery");
    assert!(matches!(
        error,
        HarnessError::SkillContext(SkillContextError::IntegrityMismatch {
            delivery_key: actual
        }) if actual == delivery_key
    ));
    assert_eq!(model.requests().await.len(), 1);
    let serialized_events = serde_json::to_string(
        &store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await,
    )
    .unwrap();
    assert!(!serialized_events.contains("ORIGINAL SKILL BODY"));
    assert!(!serialized_events.contains("CHANGED SKILL BODY"));
}

#[tokio::test]
async fn selected_skill_validation_returns_typed_errors() {
    let missing_parameter = selected_skill_error(
        "---\nname: validation-skill\ndescription: Validation skill\nparameters:\n  - name: topic\n    type: string\n    required: true\n---\nReview ${topic}.\n",
    )
    .await;
    assert!(matches!(
        missing_parameter,
        HarnessError::SkillContext(SkillContextError::MissingParameter {
            parameter,
            ..
        }) if parameter == "topic"
    ));

    let missing_config = selected_skill_error(
        "---\nname: validation-skill\ndescription: Validation skill\nconfig:\n  - key: region\n    type: string\n    required: true\n---\nReview this.\n",
    )
    .await;
    assert!(matches!(
        missing_config,
        HarnessError::SkillContext(SkillContextError::MissingConfig { config_keys, .. })
            if config_keys == vec!["region"]
    ));

    let hidden = selected_skill_error(&format!(
        "---\nname: validation-skill\ndescription: Validation skill\nallowlist_agents: [\"{}\"]\n---\nReview this.\n",
        harness_contracts::AgentId::from_u128(2)
    ))
    .await;
    assert!(matches!(
        hidden,
        HarnessError::SkillContext(SkillContextError::NotVisible { .. })
    ));
}

fn selected_skill_request(
    options: SessionOptions,
    skill_id: harness_contracts::SkillId,
) -> ConversationTurnRequest {
    let mut request = ConversationTurnRequest::from_prompt(
        options,
        ConversationRunOptions::default(),
        "use selected skill",
    );
    request.input.context_references = vec![skill_reference(skill_id)];
    request
}

fn skill_reference(skill_id: harness_contracts::SkillId) -> ConversationContextReference {
    ConversationContextReference::Skill {
        version: CURRENT_CONTEXT_REFERENCE_VERSION,
        skill_id,
        label: "Recovery skill".to_owned(),
        parameters: BTreeMap::new(),
        source: Some(SkillSourceKind::Bundled),
    }
}

async fn selected_skill_error(markdown: &str) -> HarnessError {
    let workspace = unique_workspace("sdk-selected-skill-validation");
    let session_id = SessionId::new();
    let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::MessageStop,
    ])]));
    let skill =
        parse_skill_markdown(markdown, SkillSource::Bundled, None, SkillPlatform::Macos).unwrap();
    let skill_id = skill.id.clone();
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model)
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .unwrap();
    harness.skill_registry().register(skill).unwrap();
    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .unwrap();
    harness
        .submit_conversation_turn(selected_skill_request(options, skill_id))
        .await
        .expect_err("selected skill should fail validation")
}

fn test_skill(body: &str) -> harness_skill::Skill {
    parse_skill_markdown(
        &format!("---\nname: recovery-skill\ndescription: Recovery skill\n---\n{body}\n"),
        SkillSource::Bundled,
        None,
        SkillPlatform::Macos,
    )
    .unwrap()
}

fn request_text(request: &harness_model::ModelRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            harness_contracts::MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{name}-{}", SessionId::new()));
    std::fs::create_dir_all(&path).unwrap();
    path
}
