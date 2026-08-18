use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MeetingModel {
    pub id: String,
    pub title: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub folder_path: Option<String>,
    #[sqlx(default)]
    pub diarization_status: Option<String>,
    #[sqlx(default)]
    pub speaker_names: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct DateTimeUtc(pub DateTime<Utc>);

impl From<NaiveDateTime> for DateTimeUtc {
    fn from(naive: NaiveDateTime) -> Self {
        DateTimeUtc(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
    }
}

// Renamed from TranscriptSegment to Transcript to match the table name
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Transcript {
    pub id: String,
    pub meeting_id: String,
    pub transcript: String,
    pub timestamp: String,
    pub summary: Option<String>,
    pub action_items: Option<String>,
    pub key_points: Option<String>,
    // Recording-relative timestamps for audio-transcript synchronization
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
    pub duration: Option<f64>,
    pub source_device: Option<String>,
    pub speaker: Option<String>,
    pub speaker_label: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SummaryProcess {
    pub meeting_id: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub error: Option<String>,
    pub result: Option<String>, // JSON
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub chunk_count: i64,
    pub processing_time: f64,
    pub metadata: Option<String>, // JSON
    pub result_backup: Option<String>, // Backup of result before regeneration
    pub result_backup_timestamp: Option<chrono::DateTime<chrono::Utc>>, // When backup was created
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TranscriptChunk {
    pub meeting_id: String,
    pub meeting_name: Option<String>,
    pub transcript_text: String,
    pub model: String,
    pub model_name: String,
    pub chunk_size: Option<i64>,
    pub overlap: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Setting {
    pub id: String,
    pub provider: String,
    pub model: String,
    #[sqlx(rename = "whisperModel")]
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[sqlx(rename = "groqApiKey")]
    #[serde(rename = "groqApiKey")]
    pub groq_api_key: Option<String>,
    #[sqlx(rename = "openaiApiKey")]
    #[serde(rename = "openaiApiKey")]
    pub openai_api_key: Option<String>,
    #[sqlx(rename = "anthropicApiKey")]
    #[serde(rename = "anthropicApiKey")]
    pub anthropic_api_key: Option<String>,
    #[sqlx(rename = "ollamaApiKey")]
    #[serde(rename = "ollamaApiKey")]
    pub ollama_api_key: Option<String>,
    #[sqlx(rename = "openRouterApiKey")]
    #[serde(rename = "openRouterApiKey")]
    pub open_router_api_key: Option<String>,
    #[sqlx(rename = "ollamaEndpoint")]
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
    /// Custom OpenAI-compatible endpoint configuration stored as JSON
    #[sqlx(rename = "customOpenAIConfig")]
    #[serde(rename = "customOpenAIConfig")]
    pub custom_openai_config: Option<String>,
}

impl Setting {
    /// Parse the custom OpenAI config from JSON string
    pub fn get_custom_openai_config(&self) -> Option<crate::summary::CustomOpenAIConfig> {
        self.custom_openai_config.as_ref().and_then(|json| {
            serde_json::from_str(json).ok()
        })
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TranscriptSetting {
    pub id: String,
    pub provider: String,
    pub model: String,
    #[sqlx(rename = "whisperApiKey")]
    #[serde(rename = "whisperApiKey")]
    pub whisper_api_key: Option<String>,
    #[sqlx(rename = "deepgramApiKey")]
    #[serde(rename = "deepgramApiKey")]
    pub deepgram_api_key: Option<String>,
    #[sqlx(rename = "elevenLabsApiKey")]
    #[serde(rename = "elevenLabsApiKey")]
    pub eleven_labs_api_key: Option<String>,
    #[sqlx(rename = "groqApiKey")]
    #[serde(rename = "groqApiKey")]
    pub groq_api_key: Option<String>,
    #[sqlx(rename = "openaiApiKey")]
    #[serde(rename = "openaiApiKey")]
    pub openai_api_key: Option<String>,
}

// ===== Speaker identity registry (change: speaker-identity-registry) =====

/// Global speaker registry row. Identity is cross-meeting; names are unique
/// case-insensitively (enforced by idx_speakers_name_nocase).
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Speaker {
    pub id: String,
    pub name: String,
    /// Stored as INTEGER 0/1 in SQLite.
    pub is_me: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

/// Voiceprint row. One table, two owners: an enrolled prototype
/// (`speaker_id` set) or an unassigned per-meeting cluster cache
/// (`meeting_id` + `cluster_label` set). The CHECK constraint in the
/// migration guarantees exactly one owner kind.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SpeakerEmbedding {
    pub id: String,
    pub embedding: Vec<u8>,
    pub model: String,
    pub channel: String,
    pub duration_secs: f64,
    pub speaker_id: Option<String>,
    pub meeting_id: Option<String>,
    pub cluster_label: Option<String>,
    pub created_at: DateTimeUtc,
}

/// Per-meeting cluster -> person mapping. `centroid` is the recognition
/// target (mean of the cluster's exemplar embeddings). `matched_by` is
/// 'auto' (recognized above threshold), 'user' (manual edit), or NULL.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MeetingSpeaker {
    pub meeting_id: String,
    pub cluster_label: String,
    pub speaker_id: Option<String>,
    pub centroid: Option<Vec<u8>>,
    pub channel: Option<String>,
    pub matched_by: Option<String>,
    pub match_score: Option<f64>,
}

/// Expected-speaker allowlist row. An empty set for a meeting means
/// recognition matches against ALL registry speakers.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MeetingExpectedSpeaker {
    pub meeting_id: String,
    pub speaker_id: String,
}

/// Serialize a 256-d f32 embedding into a little-endian byte blob for storage.
pub fn embedding_to_bytes(emb: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(emb.len() * 4);
    for v in emb {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Deserialize a little-endian byte blob back into f32 embedding components.
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}
