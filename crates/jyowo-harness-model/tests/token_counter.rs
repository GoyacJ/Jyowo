use chrono::Utc;
use harness_contracts::{Message, MessageId, MessagePart, MessageRole};
use harness_model::{
    AnthropicCounter, ApproximateCounter, ImageDetail, ImageMeta, TiktokenCounter, TokenCounter,
};

fn message(parts: Vec<MessagePart>) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::User,
        parts,
        created_at: Utc::now(),
    }
}

#[test]
fn approximate_counter_counts_text_and_messages() {
    let counter = ApproximateCounter;
    assert_eq!(counter.count_tokens("hello", "unknown"), 2);
    assert_eq!(
        counter.count_messages(
            &[message(vec![MessagePart::Text("hello".to_owned())])],
            "unknown"
        ),
        8
    );
}

#[test]
fn anthropic_counter_counts_image_tokens() {
    let counter = AnthropicCounter;
    let image = ImageMeta {
        width: 1500,
        height: 750,
        mime: "image/png".to_owned(),
        detail: ImageDetail::Auto,
    };

    assert_eq!(
        counter.count_image(&image, "claude-3-5-sonnet-20241022"),
        Some(1500)
    );
}

#[test]
fn tiktoken_counter_counts_openai_vision_tokens() {
    let counter = TiktokenCounter;
    let low = ImageMeta {
        width: 4000,
        height: 2000,
        mime: "image/png".to_owned(),
        detail: ImageDetail::Low,
    };
    let high = ImageMeta {
        width: 1024,
        height: 1024,
        mime: "image/png".to_owned(),
        detail: ImageDetail::High,
    };

    assert_eq!(counter.count_image(&low, "gpt-4o"), Some(85));
    assert_eq!(counter.count_image(&high, "gpt-4o"), Some(765));
    assert_eq!(counter.count_image(&high, "unknown-model"), None);
}

#[test]
fn approximate_counter_counts_gemini_image_tiles() {
    let counter = ApproximateCounter;
    let image = ImageMeta {
        width: 1536,
        height: 769,
        mime: "image/jpeg".to_owned(),
        detail: ImageDetail::Auto,
    };

    assert_eq!(counter.count_image(&image, "gemini-2.5-flash"), Some(1032));
    assert_eq!(counter.count_image(&image, "unknown-model"), None);
}
