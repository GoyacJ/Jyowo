#![cfg(feature = "http")]

use harness_mcp::{SseDecoder, SseLimits};

#[test]
fn decoder_handles_chunk_boundaries_line_endings_bom_and_multiline_data() {
    let mut decoder = SseDecoder::new(SseLimits::default());
    let mut events = Vec::new();
    for chunk in [
        &b"\xef\xbb"[..],
        &b"\xbf: comment\r"[..],
        &b"\ndata: first\rdata:second\nid: cursor-1\r\nretry: 25\r\n\r"[..],
        &b"\n"[..],
    ] {
        events.extend(decoder.push(chunk).expect("valid SSE chunk"));
    }
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "first\nsecond");
    assert_eq!(events[0].id.as_deref(), Some("cursor-1"));
    assert_eq!(events[0].retry_ms, Some(25));
}

#[test]
fn decoder_discards_an_event_not_terminated_by_a_blank_line() {
    let mut decoder = SseDecoder::new(SseLimits::default());
    assert!(decoder
        .push(b"id: bad\0id\ndata: final")
        .expect("valid partial event")
        .is_empty());
    let events = decoder.finish().expect("valid SSE EOF");
    assert!(events.is_empty());
}

#[test]
fn decoder_preserves_empty_data_prime_ids_and_retry_only_updates() {
    let mut decoder = SseDecoder::new(SseLimits::default());
    let events = decoder
        .push(b"id: post-stream-1\ndata:\n\nretry: 75\n\n")
        .expect("valid SSE control events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].data, "");
    assert_eq!(events[0].id.as_deref(), Some("post-stream-1"));
    assert_eq!(events[1].retry_ms, Some(75));
}

#[test]
fn decoder_preserves_leading_empty_data_line() {
    let mut decoder = SseDecoder::new(SseLimits::default());
    let events = decoder
        .push(b"data:\ndata: x\n\n")
        .expect("valid multiline event");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "\nx");
}

#[test]
fn decoder_rejects_invalid_utf8_and_configured_limits() {
    let limits = SseLimits {
        max_line_bytes: 8,
        max_event_bytes: 16,
        max_data_bytes: 8,
    };
    let mut invalid_utf8 = SseDecoder::new(limits);
    assert!(invalid_utf8.push(b"data: \xff\n\n").is_err());

    let mut long_line = SseDecoder::new(limits);
    assert!(long_line.push(b"data: 123456789").is_err());

    let mut long_data = SseDecoder::new(limits);
    assert!(long_data.push(b"data: 1234\ndata: 5678\n\n").is_err());

    let mut long_event = SseDecoder::new(SseLimits {
        max_line_bytes: 16,
        max_event_bytes: 20,
        max_data_bytes: 16,
    });
    assert!(long_event.push(b"event: 123456\ndata: 123456\n\n").is_err());
}

#[test]
fn decoder_ignores_an_id_field_containing_nul() {
    let mut decoder = SseDecoder::new(SseLimits::default());
    let events = decoder
        .push(b"id: bad\0cursor\ndata: payload\n\n")
        .expect("NUL id is ignored");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "payload");
    assert_eq!(events[0].id, None);
}
