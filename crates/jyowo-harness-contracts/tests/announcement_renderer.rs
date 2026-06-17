use harness_contracts::{
    AnnouncementRenderInput, AnnouncementRenderer, XmlTaskNotificationRenderer,
};

#[test]
fn xml_task_notification_renderer_escapes_summary_and_keeps_stable_id() {
    let input = AnnouncementRenderInput::new("subagent", "done <ok> & \"safe\"")
        .with_status("Completed")
        .with_label("subagent_id", "subagent-1")
        .with_rewrite_hint("Rewrite <internal> output.");

    let rendered = XmlTaskNotificationRenderer.render(&input);

    assert_eq!(rendered.renderer_id, "xml-task-notification");
    assert!(rendered.user_message.contains("<task-notification>"));
    assert!(rendered.user_message.contains("<rewrite-hint>"));
    assert!(rendered
        .user_message
        .contains("done &lt;ok&gt; &amp; &quot;safe&quot;"));
    assert!(rendered
        .user_message
        .contains("Rewrite &lt;internal&gt; output."));
}
