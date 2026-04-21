use std::path::Path;

use async_trait::async_trait;
use axum::Json;
use axum::extract::{Multipart, State};
use base64::Engine as _;
use rustfin_core::error::ApiError;
use serde::Serialize;
use tokio::process::Command;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::transcription_agent::{self, AgentTranscribeChunkRequest};

pub const MAX_AI_TRANSCRIBE_BYTES: usize = 10 * 1024 * 1024;
const MAX_AI_TRANSCRIBE_SECONDS: f64 = 30.0;
const TARGET_SAMPLE_RATE_HZ: u32 = 16_000;
const MAX_AGENT_CHUNK_MS: i64 = 15_000;

#[derive(Debug, Serialize)]
pub struct AiTranscribeResponse {
    pub text: String,
}

#[async_trait]
trait TranscriptionClient: Send + Sync {
    async fn start_session(&self, state: &AppState, session_id: &str) -> Result<(), ApiError>;
    async fn stop_session(&self, state: &AppState, session_id: &str) -> Result<(), ApiError>;
    async fn transcribe_chunk(
        &self,
        state: &AppState,
        body: &AgentTranscribeChunkRequest,
    ) -> Result<Vec<transcription_agent::AgentTranscriptSegment>, ApiError>;
}

#[derive(Debug, Clone, Copy, Default)]
struct HttpTranscriptionClient;

#[async_trait]
impl TranscriptionClient for HttpTranscriptionClient {
    async fn start_session(&self, state: &AppState, session_id: &str) -> Result<(), ApiError> {
        transcription_agent::start_session(state, session_id).await
    }

    async fn stop_session(&self, state: &AppState, session_id: &str) -> Result<(), ApiError> {
        transcription_agent::stop_session(state, session_id).await
    }

    async fn transcribe_chunk(
        &self,
        state: &AppState,
        body: &AgentTranscribeChunkRequest,
    ) -> Result<Vec<transcription_agent::AgentTranscriptSegment>, ApiError> {
        transcription_agent::transcribe_chunk(state, body).await
    }
}

pub async fn transcribe_audio(
    user: AuthUser,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<AiTranscribeResponse>, AppError> {
    let client = HttpTranscriptionClient;
    transcribe_audio_with_client(user, state, multipart, &client).await
}

async fn transcribe_audio_with_client<C: TranscriptionClient + ?Sized>(
    user: AuthUser,
    state: AppState,
    mut multipart: Multipart,
    client: &C,
) -> Result<Json<AiTranscribeResponse>, AppError> {
    let mut file_name: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut payload: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("invalid multipart form: {e}")))?
    {
        let field_name = field.name().unwrap_or_default().to_string();
        if field_name != "file" || payload.is_some() {
            continue;
        }
        file_name = field.file_name().map(|value| value.to_string());
        content_type = field.content_type().map(|value| value.to_string());
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::BadRequest(format!("invalid audio upload: {e}")))?;
        if bytes.is_empty() {
            return Err(ApiError::BadRequest("uploaded audio is empty".into()).into());
        }
        if bytes.len() > MAX_AI_TRANSCRIBE_BYTES {
            return Err(ApiError::BadRequest("audio exceeds the 10MB upload limit".into()).into());
        }
        payload = Some(bytes.to_vec());
    }

    let payload = payload
        .ok_or_else(|| ApiError::BadRequest("multipart form requires a file field".into()))?;
    let decoded = decode_audio_upload(
        &state,
        &payload,
        file_name.as_deref(),
        content_type.as_deref(),
    )
    .await?;

    let duration_seconds = (decoded.pcm_s16le.len() as f64 / 2.0) / decoded.sample_rate_hz as f64;
    if duration_seconds <= 0.0 || duration_seconds > MAX_AI_TRANSCRIBE_SECONDS {
        return Err(ApiError::BadRequest(
            "audio duration must be between 1ms and 30 seconds".into(),
        )
        .into());
    }

    let session_id = format!("ai-input-{}", uuid::Uuid::new_v4());
    client.start_session(&state, &session_id).await?;

    let transcript_result =
        transcribe_pcm_chunks_with_client(&state, &user, &session_id, &decoded, client).await;
    let _ = client.stop_session(&state, &session_id).await;
    let text = transcript_result?;

    Ok(Json(AiTranscribeResponse { text }))
}

#[derive(Debug)]
pub(crate) struct DecodedAudio {
    pub sample_rate_hz: u32,
    pub pcm_s16le: Vec<u8>,
}

pub(crate) async fn transcribe_pcm_chunks_to_segments(
    state: &AppState,
    user: &AuthUser,
    session_id: &str,
    decoded: &DecodedAudio,
    first_chunk_started_ts_ms: i64,
) -> Result<Vec<transcription_agent::AgentTranscriptSegment>, AppError> {
    let client = HttpTranscriptionClient;
    transcribe_pcm_chunks_to_segments_with_client(
        state,
        user,
        session_id,
        decoded,
        first_chunk_started_ts_ms,
        &client,
    )
    .await
}

async fn transcribe_pcm_chunks_to_segments_with_client<C: TranscriptionClient + ?Sized>(
    state: &AppState,
    user: &AuthUser,
    session_id: &str,
    decoded: &DecodedAudio,
    first_chunk_started_ts_ms: i64,
    client: &C,
) -> Result<Vec<transcription_agent::AgentTranscriptSegment>, AppError> {
    let samples_per_chunk =
        ((decoded.sample_rate_hz as i64 * MAX_AGENT_CHUNK_MS) / 1000).max(1) as usize;
    let bytes_per_chunk = samples_per_chunk.saturating_mul(2);
    let mut chunk_start_ms = first_chunk_started_ts_ms.max(1);
    let mut transcript_segments = Vec::new();

    for chunk in decoded.pcm_s16le.chunks(bytes_per_chunk.max(2)) {
        let chunk_duration_ms =
            ((chunk.len() as f64 / 2.0) / decoded.sample_rate_hz as f64 * 1000.0).round() as i64;
        let chunk_end_ms = chunk_start_ms + chunk_duration_ms.max(1);
        let segments = client
            .transcribe_chunk(
                state,
                &AgentTranscribeChunkRequest {
                    session_id: session_id.to_string(),
                    user_id: user.user_id.clone(),
                    username: user.username.clone(),
                    sample_rate_hz: decoded.sample_rate_hz,
                    started_ts_ms: chunk_start_ms.max(1),
                    ended_ts_ms: chunk_end_ms.max(chunk_start_ms + 1),
                    pcm_s16le_base64: base64::engine::general_purpose::STANDARD.encode(chunk),
                    language: None,
                },
            )
            .await?;

        for segment in segments {
            let text = segment.text.trim();
            if !text.is_empty() {
                transcript_segments.push(transcription_agent::AgentTranscriptSegment {
                    started_ts_ms: segment.started_ts_ms,
                    ended_ts_ms: segment.ended_ts_ms,
                    text: text.to_string(),
                });
            }
        }

        chunk_start_ms = chunk_end_ms.max(chunk_start_ms + 1);
    }

    Ok(transcript_segments)
}

async fn transcribe_pcm_chunks(
    state: &AppState,
    user: &AuthUser,
    session_id: &str,
    decoded: &DecodedAudio,
) -> Result<String, AppError> {
    let client = HttpTranscriptionClient;
    transcribe_pcm_chunks_with_client(state, user, session_id, decoded, &client).await
}

async fn transcribe_pcm_chunks_with_client<C: TranscriptionClient + ?Sized>(
    state: &AppState,
    user: &AuthUser,
    session_id: &str,
    decoded: &DecodedAudio,
    client: &C,
) -> Result<String, AppError> {
    let segments =
        transcribe_pcm_chunks_to_segments_with_client(state, user, session_id, decoded, 1, client)
            .await?;

    let transcript_parts = segments
        .into_iter()
        .map(|segment| segment.text)
        .collect::<Vec<_>>();

    Ok(transcript_parts
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" "))
}

pub(crate) async fn decode_audio_upload(
    state: &AppState,
    payload: &[u8],
    file_name: Option<&str>,
    content_type: Option<&str>,
) -> Result<DecodedAudio, AppError> {
    let looks_like_wav = content_type
        .map(|value| {
            value.eq_ignore_ascii_case("audio/wav") || value.eq_ignore_ascii_case("audio/x-wav")
        })
        .unwrap_or(false)
        || file_name
            .and_then(|value| Path::new(value).extension().and_then(|ext| ext.to_str()))
            .map(|ext| ext.eq_ignore_ascii_case("wav"))
            .unwrap_or(false)
        || (payload.len() >= 12 && &payload[0..4] == b"RIFF" && &payload[8..12] == b"WAVE");

    if looks_like_wav {
        return decode_wav_payload(payload);
    }

    transcode_audio_with_ffmpeg(state, payload, file_name).await
}

fn decode_wav_payload(payload: &[u8]) -> Result<DecodedAudio, AppError> {
    if payload.len() < 44 || &payload[0..4] != b"RIFF" || &payload[8..12] != b"WAVE" {
        return Err(ApiError::BadRequest("invalid WAV upload".into()).into());
    }

    let mut channels = None;
    let mut sample_rate_hz = None;
    let mut bits_per_sample = None;
    let mut data = None;
    let mut cursor = 12_usize;

    while cursor + 8 <= payload.len() {
        let chunk_id = &payload[cursor..cursor + 4];
        let chunk_len = u32::from_le_bytes([
            payload[cursor + 4],
            payload[cursor + 5],
            payload[cursor + 6],
            payload[cursor + 7],
        ]) as usize;
        cursor += 8;
        if cursor + chunk_len > payload.len() {
            return Err(ApiError::BadRequest("invalid WAV chunk length".into()).into());
        }

        if chunk_id == b"fmt " {
            if chunk_len < 16 {
                return Err(ApiError::BadRequest("invalid WAV fmt chunk".into()).into());
            }
            let audio_format = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]);
            if audio_format != 1 {
                return Err(
                    ApiError::BadRequest("only PCM WAV uploads are supported".into()).into(),
                );
            }
            channels = Some(u16::from_le_bytes([
                payload[cursor + 2],
                payload[cursor + 3],
            ]));
            sample_rate_hz = Some(u32::from_le_bytes([
                payload[cursor + 4],
                payload[cursor + 5],
                payload[cursor + 6],
                payload[cursor + 7],
            ]));
            bits_per_sample = Some(u16::from_le_bytes([
                payload[cursor + 14],
                payload[cursor + 15],
            ]));
        } else if chunk_id == b"data" {
            data = Some(payload[cursor..cursor + chunk_len].to_vec());
        }

        cursor += chunk_len;
        if chunk_len % 2 == 1 {
            cursor += 1;
        }
    }

    let channels =
        channels.ok_or_else(|| ApiError::BadRequest("WAV fmt chunk is missing".into()))?;
    let sample_rate_hz =
        sample_rate_hz.ok_or_else(|| ApiError::BadRequest("WAV sample rate is missing".into()))?;
    let bits_per_sample =
        bits_per_sample.ok_or_else(|| ApiError::BadRequest("WAV bit depth is missing".into()))?;
    let data = data.ok_or_else(|| ApiError::BadRequest("WAV data chunk is missing".into()))?;

    if bits_per_sample != 16 {
        return Err(
            ApiError::BadRequest("only 16-bit PCM WAV uploads are supported".into()).into(),
        );
    }
    if !(1..=8).contains(&channels) {
        return Err(ApiError::BadRequest("unsupported WAV channel count".into()).into());
    }
    if data.len() % (channels as usize * 2) != 0 {
        return Err(ApiError::BadRequest("invalid WAV PCM frame length".into()).into());
    }

    let pcm_s16le = if channels == 1 {
        data
    } else {
        let mut mono = Vec::with_capacity(data.len() / channels as usize);
        for frame in data.chunks_exact(channels as usize * 2) {
            let mut sum = 0_i32;
            for sample in frame.chunks_exact(2) {
                sum += i16::from_le_bytes([sample[0], sample[1]]) as i32;
            }
            let avg = (sum / channels as i32) as i16;
            mono.extend_from_slice(&avg.to_le_bytes());
        }
        mono
    };

    Ok(DecodedAudio {
        sample_rate_hz,
        pcm_s16le,
    })
}

async fn transcode_audio_with_ffmpeg(
    state: &AppState,
    payload: &[u8],
    file_name: Option<&str>,
) -> Result<DecodedAudio, AppError> {
    let extension = file_name
        .and_then(|value| Path::new(value).extension().and_then(|ext| ext.to_str()))
        .unwrap_or("bin");
    let temp_path = std::env::temp_dir().join(format!(
        "rustyfin-ai-upload-{}.{}",
        uuid::Uuid::new_v4(),
        extension
    ));
    tokio::fs::write(&temp_path, payload)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to stage uploaded audio: {e}")))?;

    let output = Command::new(&state.ffmpeg_path)
        .args([
            "-v",
            "error",
            "-i",
            temp_path.to_string_lossy().as_ref(),
            "-ac",
            "1",
            "-ar",
            &TARGET_SAMPLE_RATE_HZ.to_string(),
            "-f",
            "s16le",
            "-",
        ])
        .output()
        .await;
    let _ = tokio::fs::remove_file(&temp_path).await;

    let output = output.map_err(|e| {
        ApiError::Internal(format!("failed to invoke ffmpeg for AI transcription: {e}"))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ApiError::BadRequest(format!(
            "Rustyfin could not decode that audio upload{}",
            if stderr.is_empty() {
                ".".to_string()
            } else {
                format!(": {stderr}")
            }
        ))
        .into());
    }
    if output.stdout.is_empty() {
        return Err(ApiError::BadRequest("decoded audio was empty".into()).into());
    }

    Ok(DecodedAudio {
        sample_rate_hz: TARGET_SAMPLE_RATE_HZ,
        pcm_s16le: output.stdout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::DefaultBodyLimit;
    use axum::routing::post;
    use axum_test::TestServer;
    use axum_test::multipart::{MultipartForm, Part};
    use sqlx::postgres::PgPoolOptions;
    use std::sync::{Arc, Mutex};

    fn test_state(transcription_agent_url: String) -> AppState {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@127.0.0.1/rustfin_test")
            .expect("lazy postgres pool");
        let tc_config = rustfin_transcoder::TranscoderConfig {
            transcode_dir: std::env::temp_dir().join(format!(
                "rustyfin-ai-transcribe-test-{}",
                uuid::Uuid::new_v4()
            )),
            max_concurrent: 1,
            ..Default::default()
        };
        let ffmpeg_path = tc_config.ffmpeg_path.clone();
        let ffprobe_path = tc_config.ffprobe_path.clone();
        let transcoder = Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));
        let (events_tx, _) = tokio::sync::broadcast::channel(8);

        AppState {
            db: pool,
            rustyvault: crate::state::RustyVaultRuntimeState::available(),
            jwt_secret: "test-secret".to_string(),
            http: reqwest::Client::builder().build().unwrap(),
            runtime_metrics: crate::runtime_metrics::RuntimeMetrics::new(),
            tmdb_agent_url: "http://127.0.0.1:8100".to_string(),
            tmdb_agent_token: None,
            youtube_agent_url: "http://127.0.0.1:8101".to_string(),
            youtube_agent_token: None,
            transcription_agent_url,
            transcription_agent_token: None,
            servers_agent_url: None,
            servers_agent_token: None,
            model_dir: Arc::new(tokio::sync::RwLock::new(
                std::env::temp_dir().join("rustyfin-ai-models-test"),
            )),
            engine: Arc::new(tokio::sync::Mutex::new(crate::ai::EngineState::default())),
            transcoder,
            ffmpeg_path,
            ffprobe_path,
            transcoder_hw_accel: None,
            transcoder_hw_accel_required: false,
            cache_dir: std::env::temp_dir().join(format!(
                "rustyfin-ai-transcribe-cache-{}",
                uuid::Uuid::new_v4()
            )),
            watch_party_audio_dir: std::env::temp_dir().join(format!(
                "rustyfin-ai-transcribe-watch-audio-{}",
                uuid::Uuid::new_v4()
            )),
            events: events_tx,
            watch_party: Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
            channel_manager: Arc::new(crate::channels::manager::ChannelManager::new()),
        }
    }

    fn auth_header_value(secret: &str) -> axum::http::HeaderValue {
        let token = crate::auth::issue_token("user-1", "tester", "user", secret).unwrap();
        axum::http::HeaderValue::from_str(&format!("Bearer {token}")).unwrap()
    }

    fn wav_bytes(samples: &[i16], sample_rate_hz: u32, channels: u16) -> Vec<u8> {
        let block_align = channels * 2;
        let byte_rate = sample_rate_hz * block_align as u32;
        let data_len = (samples.len() * 2) as u32;
        let riff_len = 36 + data_len;
        let bits_per_sample = 16_u16;

        let mut bytes = Vec::with_capacity((44 + data_len) as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_len.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate_hz.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FakeClientCall {
        StartSession(String),
        TranscribeChunk {
            session_id: String,
            started_ts_ms: i64,
            ended_ts_ms: i64,
            sample_rate_hz: u32,
            pcm_len_bytes: usize,
        },
        StopSession(String),
    }

    #[derive(Debug, Default)]
    struct FakeTranscriptionClient {
        calls: Mutex<Vec<FakeClientCall>>,
        transcript_text: String,
    }

    impl FakeTranscriptionClient {
        fn with_transcript(text: impl Into<String>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                transcript_text: text.into(),
            }
        }

        fn calls(&self) -> Vec<FakeClientCall> {
            self.calls.lock().expect("fake client calls").clone()
        }
    }

    #[async_trait]
    impl TranscriptionClient for FakeTranscriptionClient {
        async fn start_session(&self, _state: &AppState, session_id: &str) -> Result<(), ApiError> {
            self.calls
                .lock()
                .expect("fake client calls")
                .push(FakeClientCall::StartSession(session_id.to_string()));
            Ok(())
        }

        async fn stop_session(&self, _state: &AppState, session_id: &str) -> Result<(), ApiError> {
            self.calls
                .lock()
                .expect("fake client calls")
                .push(FakeClientCall::StopSession(session_id.to_string()));
            Ok(())
        }

        async fn transcribe_chunk(
            &self,
            _state: &AppState,
            body: &AgentTranscribeChunkRequest,
        ) -> Result<Vec<transcription_agent::AgentTranscriptSegment>, ApiError> {
            let chunk = base64::engine::general_purpose::STANDARD
                .decode(&body.pcm_s16le_base64)
                .map_err(|e| ApiError::Internal(format!("failed to decode fake chunk: {e}")))?;
            self.calls
                .lock()
                .expect("fake client calls")
                .push(FakeClientCall::TranscribeChunk {
                    session_id: body.session_id.clone(),
                    started_ts_ms: body.started_ts_ms,
                    ended_ts_ms: body.ended_ts_ms,
                    sample_rate_hz: body.sample_rate_hz,
                    pcm_len_bytes: chunk.len(),
                });
            Ok(vec![transcription_agent::AgentTranscriptSegment {
                started_ts_ms: body.started_ts_ms,
                ended_ts_ms: body.ended_ts_ms,
                text: self.transcript_text.clone(),
            }])
        }
    }

    #[test]
    fn decode_wav_payload_downmixes_stereo_pcm() {
        let payload = wav_bytes(&[1_000, -1_000, 2_000, 0], 16_000, 2);
        let decoded = decode_wav_payload(&payload).expect("wav should decode");

        assert_eq!(decoded.sample_rate_hz, 16_000);
        let samples = decoded
            .pcm_s16le
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        assert_eq!(samples, vec![0, 1_000]);
    }

    #[test]
    fn decode_wav_payload_rejects_non_pcm_audio() {
        let mut payload = wav_bytes(&[0, 1], 16_000, 1);
        payload[20] = 3;
        payload[21] = 0;
        let error = decode_wav_payload(&payload).expect_err("non-pcm wav should fail");
        assert!(
            error
                .0
                .to_string()
                .contains("only PCM WAV uploads are supported")
        );
    }

    #[tokio::test]
    async fn transcribe_route_accepts_small_wav_uploads() {
        let fake_client = Arc::new(FakeTranscriptionClient::with_transcript("hello rustyfin"));
        let state = test_state("http://127.0.0.1:9".to_string());
        let app = Router::new()
            .route(
                "/transcribe",
                post({
                    let fake_client = Arc::clone(&fake_client);
                    move |user: AuthUser, State(state): State<AppState>, multipart: Multipart| {
                        let fake_client = Arc::clone(&fake_client);
                        async move {
                            transcribe_audio_with_client(
                                user,
                                state,
                                multipart,
                                fake_client.as_ref(),
                            )
                            .await
                        }
                    }
                })
                .layer(DefaultBodyLimit::max(MAX_AI_TRANSCRIBE_BYTES)),
            )
            .with_state(state.clone());
        let server = TestServer::new(app).unwrap();

        let wav = wav_bytes(&vec![0_i16; 16_000], 16_000, 1);
        let form = MultipartForm::new().add_part(
            "file",
            Part::bytes(wav)
                .file_name("voice.wav")
                .mime_type("audio/wav"),
        );

        let response = server
            .post("/transcribe")
            .add_header(
                axum::http::header::AUTHORIZATION,
                auth_header_value(&state.jwt_secret),
            )
            .multipart(form)
            .await;
        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["text"].as_str(), Some("hello rustyfin"));
        let calls = fake_client.calls();
        assert_eq!(calls.len(), 3);
        let session_id = match &calls[0] {
            FakeClientCall::StartSession(session_id) => session_id.clone(),
            other => panic!("expected start session call, got {other:?}"),
        };
        match &calls[1] {
            FakeClientCall::TranscribeChunk {
                session_id: chunk_session_id,
                started_ts_ms,
                ended_ts_ms,
                sample_rate_hz,
                pcm_len_bytes,
            } => {
                assert_eq!(chunk_session_id, &session_id);
                assert_eq!(*started_ts_ms, 1);
                assert!(*ended_ts_ms > *started_ts_ms);
                assert_eq!(*sample_rate_hz, 16_000);
                assert!(*pcm_len_bytes > 0);
            }
            other => panic!("expected transcribe chunk call, got {other:?}"),
        }
        assert_eq!(calls[2], FakeClientCall::StopSession(session_id.clone()));
    }

    #[tokio::test]
    async fn transcribe_route_rejects_oversized_uploads() {
        let state = test_state("http://127.0.0.1:9".to_string());
        let app = Router::new()
            .route(
                "/transcribe",
                post(transcribe_audio).layer(DefaultBodyLimit::max(MAX_AI_TRANSCRIBE_BYTES)),
            )
            .with_state(state.clone());
        let server = TestServer::new(app).unwrap();

        let form = MultipartForm::new().add_part(
            "file",
            Part::bytes(vec![0_u8; MAX_AI_TRANSCRIBE_BYTES + 1])
                .file_name("voice.bin")
                .mime_type("application/octet-stream"),
        );

        let response = server
            .post("/transcribe")
            .add_header(
                axum::http::header::AUTHORIZATION,
                auth_header_value(&state.jwt_secret),
            )
            .multipart(form)
            .await;
        response.assert_status(axum::http::StatusCode::BAD_REQUEST);
        let body: serde_json::Value = response.json();
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("invalid multipart form")
        );
    }
}
