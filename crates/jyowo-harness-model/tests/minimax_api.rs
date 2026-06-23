#![cfg(feature = "minimax")]

use harness_model::MinimaxApiClient;
use serde_json::{json, Value};
use wiremock::{
    matchers::{body_json, body_string_contains, header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
#[ignore = "requires MINIMAX_API_KEY and JYOWO_LIVE_MINIMAX=1; may incur provider charges"]
async fn minimax_live_smoke_uses_official_modules() {
    if std::env::var("JYOWO_LIVE_MINIMAX").ok().as_deref() != Some("1") {
        return;
    }
    let api_key = std::env::var("MINIMAX_API_KEY").expect("MINIMAX_API_KEY is required");
    let client = MinimaxApiClient::from_api_key(api_key);

    client
        .image_generation(json!({
            "model": "image-01",
            "prompt": "a tiny monochrome square icon",
            "response_format": "url"
        }))
        .await
        .expect("image generation should succeed");

    client
        .text_to_speech(json!({
            "model": "speech-2.8-turbo",
            "text": "hello",
            "voice_setting": {"voice_id": "Wise_Woman", "speed": 1.0, "vol": 1.0, "pitch": 0},
            "audio_setting": {"sample_rate": 32000, "bitrate": 128000, "format": "mp3"}
        }))
        .await
        .expect("sync tts should succeed");

    client
        .lyrics_generation(json!({"prompt": "two short lines about morning"}))
        .await
        .expect("lyrics generation should succeed");

    client
        .get_voice(json!({"voice_type": "all"}))
        .await
        .expect("voice list should succeed");

    if std::env::var("JYOWO_LIVE_MINIMAX_EXPENSIVE")
        .ok()
        .as_deref()
        == Some("1")
    {
        client
            .video_generation(json!({
                "model": "MiniMax-Hailuo-2.3-Fast",
                "prompt": "a single blue cube rotating slowly"
            }))
            .await
            .expect("video task creation should succeed");

        client
            .music_generation(json!({
                "model": "music-2.6",
                "prompt": "short calm instrumental loop",
                "is_instrumental": true
            }))
            .await
            .expect("music generation should succeed");
    }
}

#[tokio::test]
async fn minimax_api_client_covers_official_generation_endpoints() {
    let server = MockServer::start().await;
    let client = MinimaxApiClient::from_api_key("provider-key").with_base_url(server.uri());

    assert_post(
        &server,
        "/v1/image_generation",
        json!({"model": "image-01", "prompt": "tiny icon"}),
        json!({"id": "image-task"}),
    )
    .await;
    assert_eq!(
        client
            .image_generation(json!({"model": "image-01", "prompt": "tiny icon"}))
            .await
            .unwrap()["id"],
        "image-task"
    );

    assert_post(
        &server,
        "/v1/text/chatcompletion_v2",
        json!({"model": "MiniMax-M3", "messages": [{"role": "user", "content": "hi"}]}),
        json!({"choices": []}),
    )
    .await;
    client
        .text_generation(json!({
            "model": "MiniMax-M3",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/responses",
        json!({"model": "MiniMax-M3", "input": "hi"}),
        json!({"id": "response-1"}),
    )
    .await;
    client
        .responses(json!({"model": "MiniMax-M3", "input": "hi"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/responses/input_tokens",
        json!({"model": "MiniMax-M3", "input": "hi"}),
        json!({"input_tokens": 4}),
    )
    .await;
    client
        .responses_input_tokens(json!({"model": "MiniMax-M3", "input": "hi"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/anthropic/v1/messages",
        json!({"model": "MiniMax-M3", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 8}),
        json!({"id": "msg-1"}),
    )
    .await;
    client
        .anthropic_messages(json!({
            "model": "MiniMax-M3",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 8
        }))
        .await
        .unwrap();

    assert_post(
        &server,
        "/anthropic/v1/messages/count_tokens",
        json!({"model": "MiniMax-M3", "messages": [{"role": "user", "content": "hi"}]}),
        json!({"input_tokens": 4}),
    )
    .await;
    client
        .anthropic_count_tokens(json!({
            "model": "MiniMax-M3",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/video_generation",
        json!({"model": "MiniMax-Hailuo-2.3-Fast", "prompt": "wave"}),
        json!({"task_id": "video-task"}),
    )
    .await;
    client
        .video_generation(json!({"model": "MiniMax-Hailuo-2.3-Fast", "prompt": "wave"}))
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/query/video_generation"))
        .and(query_param("task_id", "video-task"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "Success"})))
        .mount(&server)
        .await;
    client.query_video_generation("video-task").await.unwrap();

    assert_post(
        &server,
        "/v1/video_template_generation",
        json!({"template_id": "tpl", "inputs": {}}),
        json!({"task_id": "template-task"}),
    )
    .await;
    client
        .video_template_generation(json!({"template_id": "tpl", "inputs": {}}))
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/query/video_template_generation"))
        .and(query_param("task_id", "template-task"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"video_url": "https://example.test/v.mp4"})),
        )
        .mount(&server)
        .await;
    client
        .query_video_template_generation("template-task")
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/t2a_v2",
        json!({"model": "speech-2.8-turbo", "text": "hi"}),
        json!({"data": {"audio": "AA=="}}),
    )
    .await;
    client
        .text_to_speech(json!({"model": "speech-2.8-turbo", "text": "hi"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/t2a_async_v2",
        json!({"model": "speech-2.8-turbo", "text": "long"}),
        json!({"task_id": "tts-task"}),
    )
    .await;
    client
        .text_to_speech_async(json!({"model": "speech-2.8-turbo", "text": "long"}))
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/query/t2a_async_query_v2"))
        .and(query_param("task_id", "tts-task"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "Success"})))
        .mount(&server)
        .await;
    client.query_text_to_speech_async("tts-task").await.unwrap();

    assert_post(
        &server,
        "/v1/voice_clone",
        json!({"file_id": "file-1", "voice_id": "clone-1"}),
        json!({"voice_id": "clone-1"}),
    )
    .await;
    client
        .voice_clone(json!({"file_id": "file-1", "voice_id": "clone-1"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/voice_design",
        json!({"prompt": "warm narrator"}),
        json!({"voice_id": "voice-1"}),
    )
    .await;
    client
        .voice_design(json!({"prompt": "warm narrator"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/get_voice",
        json!({"voice_type": "all"}),
        json!({"voices": []}),
    )
    .await;
    client
        .get_voice(json!({"voice_type": "all"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/delete_voice",
        json!({"voice_id": "voice-1"}),
        json!({"ok": true}),
    )
    .await;
    client
        .delete_voice(json!({"voice_id": "voice-1"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/lyrics_generation",
        json!({"prompt": "short"}),
        json!({"lyrics": "la"}),
    )
    .await;
    client
        .lyrics_generation(json!({"prompt": "short"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/music_generation",
        json!({"model": "music-2.6", "prompt": "short"}),
        json!({"audio_url": "https://example.test/music.mp3"}),
    )
    .await;
    client
        .music_generation(json!({"model": "music-2.6", "prompt": "short"}))
        .await
        .unwrap();

    assert_post(
        &server,
        "/v1/music_cover_preprocess",
        json!({"audio_url": "https://example.test/input.mp3"}),
        json!({"preview_url": "https://example.test/preview.mp3"}),
    )
    .await;
    client
        .music_cover_preprocess(json!({"audio_url": "https://example.test/input.mp3"}))
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    client.list_models().await.unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/models/MiniMax-M3"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "MiniMax-M3"})))
        .mount(&server)
        .await;
    client.retrieve_model("MiniMax-M3").await.unwrap();

    Mock::given(method("GET"))
        .and(path("/anthropic/v1/models"))
        .and(query_param("limit", "10"))
        .and(header("x-api-key", "provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    client
        .list_anthropic_models(Some(10), None, None)
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/anthropic/v1/models/MiniMax-M3"))
        .and(header("x-api-key", "provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "MiniMax-M3"})))
        .mount(&server)
        .await;
    client.retrieve_anthropic_model("MiniMax-M3").await.unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/models/minimax%2Fcustom"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "minimax/custom"})))
        .mount(&server)
        .await;
    client.retrieve_model("minimax/custom").await.unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/files/upload"))
        .and(header("authorization", "Bearer provider-key"))
        .and(header(
            "content-type",
            "multipart/form-data; boundary=jyowo-minimax-boundary",
        ))
        .and(body_string_contains("voice_clone"))
        .and(body_string_contains("voice.wav"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"file_id": "file-1"})))
        .mount(&server)
        .await;
    client
        .file_upload("voice_clone", "voice.wav", b"audio".to_vec())
        .await
        .unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/files/upload"))
        .and(header("authorization", "Bearer provider-key"))
        .and(body_string_contains("bad__name.wav"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"file_id": "file-2"})))
        .mount(&server)
        .await;
    client
        .file_upload("voice_clone", "bad\r\nname.wav", b"audio".to_vec())
        .await
        .unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/files/upload"))
        .and(query_param("GroupId", "group-1"))
        .and(header("authorization", "Bearer provider-key"))
        .and(body_string_contains("vision"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"file_id": "file-3"})))
        .mount(&server)
        .await;
    client
        .file_upload_with_group_id("vision", "image.png", b"image".to_vec(), Some("group-1"))
        .await
        .unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/files/retrieve"))
        .and(query_param("file_id", "file-1"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"file": {"id": "file-1"}})))
        .mount(&server)
        .await;
    client.file_retrieve("file-1").await.unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/files/retrieve_content"))
        .and(query_param("file_id", "file-1"))
        .and(query_param("GroupId", "group-1"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"binary".to_vec()))
        .mount(&server)
        .await;
    assert_eq!(
        client
            .file_retrieve_content("file-1", Some("group-1"))
            .await
            .unwrap(),
        b"binary".to_vec()
    );

    Mock::given(method("GET"))
        .and(path("/v1/files/list"))
        .and(query_param("purpose", "voice_clone"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    client.file_list(Some("voice_clone")).await.unwrap();

    assert_post(
        &server,
        "/v1/files/delete",
        json!({"file_id": "file-1"}),
        json!({"deleted": true}),
    )
    .await;
    client.file_delete("file-1").await.unwrap();
}

async fn assert_post(server: &MockServer, expected_path: &str, expected_body: Value, body: Value) {
    Mock::given(method("POST"))
        .and(path(expected_path))
        .and(header("authorization", "Bearer provider-key"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}
