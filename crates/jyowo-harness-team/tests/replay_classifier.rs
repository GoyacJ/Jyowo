use std::sync::Arc;

use harness_contracts::{AgentId, MessageId, NoopRedactor, Recipient, SessionId, TenantId};
use harness_journal::InMemoryEventStore;
use harness_team::{
    AgentMessage, FallbackMessageClassifier, MessageBus, MessageClass, MessagePayload,
    ReplayWindow, TeamAnnouncementRenderer, TeamBuilder, TeamJournalContext, Topology,
};

#[tokio::test]
async fn replay_window_classifier_and_announcement_renderer_are_deterministic() {
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
    };
    let from = AgentId::new();
    let to = AgentId::new();
    let spec = TeamBuilder::new("replay", Topology::PeerToPeer)
        .member(from, "from", harness_team::ContextVisibility::All)
        .member(to, "to", harness_team::ContextVisibility::All)
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store);

    bus.send(AgentMessage::text(
        spec.team_id,
        from,
        Recipient::Agent(to),
        "one",
    ))
    .await
    .unwrap();
    bus.send(AgentMessage::text(
        spec.team_id,
        from,
        Recipient::Agent(to),
        "two",
    ))
    .await
    .unwrap();
    let response = AgentMessage::new(
        spec.team_id,
        to,
        Recipient::Agent(from),
        MessagePayload::Response {
            in_reply_to: MessageId::new(),
            body: serde_json::json!({ "ok": true }),
        },
    );
    bus.send(response.clone()).await.unwrap();

    let window = bus.replay_window(ReplayWindow::last(2)).await;
    assert_eq!(window.len(), 2);
    assert_eq!(window[0].payload, MessagePayload::Text("two".to_owned()));
    assert_eq!(
        FallbackMessageClassifier.classify_kind(&response),
        MessageClass::Response
    );

    let rendered = TeamAnnouncementRenderer.member_joined(&spec.members[0]);
    assert!(rendered.contains("joined as from"));

    let rendered_response = TeamAnnouncementRenderer.worker_response(to, "done <ok>");
    assert_eq!(rendered_response.renderer_id, "xml-task-notification");
    assert!(rendered_response
        .user_message
        .contains("<task-notification>"));
    assert!(rendered_response.user_message.contains("<rewrite-hint>"));
    assert!(rendered_response.user_message.contains("done &lt;ok&gt;"));
}
