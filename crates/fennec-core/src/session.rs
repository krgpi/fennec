use crate::format::transcript_preview;
use crate::types::TranscriptEntry;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub mod iso8601 {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(date: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&date.to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let s = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

pub mod iso8601_opt {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(date: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match date {
            Some(date) => s.serialize_str(&date.to_rfc3339_opts(SecondsFormat::Secs, true)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        let Some(s) = Option::<String>::deserialize(d)? else {
            return Ok(None);
        };
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    #[serde(with = "iso8601")]
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_audio_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_audio_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes_preset_id: Option<Uuid>,
    #[serde(
        default,
        with = "iso8601_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub minutes_exported_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes_exported_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes_exported_hash: Option<String>,
}

impl SessionMetadata {
    pub fn new(created_at: DateTime<Utc>) -> Self {
        Self {
            created_at,
            system_audio_file: None,
            mic_audio_file: None,
            transcript_file: None,
            engine_type: None,
            model_id: None,
            summary: None,
            locale_identifier: None,
            minutes_file: None,
            minutes_preset_id: None,
            minutes_exported_at: None,
            minutes_exported_path: None,
            minutes_exported_hash: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSession {
    pub id: String,
    #[serde(with = "iso8601")]
    pub created_at: DateTime<Utc>,
    pub folder_path: PathBuf,
    pub is_legacy_flat: bool,
    #[serde(flatten)]
    pub metadata: SessionMetadataFields,
    pub transcript_preview: Option<String>,
    pub has_transcript: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadataFields {
    #[serde(default)]
    pub system_audio_file: Option<String>,
    #[serde(default)]
    pub mic_audio_file: Option<String>,
    #[serde(default)]
    pub transcript_file: Option<String>,
    #[serde(default)]
    pub engine_type: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub locale_identifier: Option<String>,
    #[serde(default)]
    pub minutes_file: Option<String>,
    #[serde(default)]
    pub minutes_preset_id: Option<Uuid>,
    #[serde(default, with = "iso8601_opt")]
    pub minutes_exported_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub minutes_exported_path: Option<PathBuf>,
    #[serde(default)]
    pub minutes_exported_hash: Option<String>,
}

impl RecordingSession {
    pub fn system_audio_path(&self) -> Option<PathBuf> {
        self.metadata
            .system_audio_file
            .as_ref()
            .map(|f| self.folder_path.join(f))
    }

    pub fn mic_audio_path(&self) -> Option<PathBuf> {
        self.metadata
            .mic_audio_file
            .as_ref()
            .map(|f| self.folder_path.join(f))
    }

    pub fn transcript_path(&self) -> Option<PathBuf> {
        self.metadata
            .transcript_file
            .as_ref()
            .map(|f| self.folder_path.join(f))
    }

    pub fn transcript_json_path(&self) -> PathBuf {
        self.folder_path.join(format!("transcript_{}.json", self.id))
    }

    pub fn note_path(&self) -> PathBuf {
        self.folder_path.join(format!("note_{}.md", self.id))
    }

    pub fn minutes_path(&self) -> Option<PathBuf> {
        self.metadata
            .minutes_file
            .as_ref()
            .map(|f| self.folder_path.join(f))
    }
}

pub fn session_id_for(date: DateTime<Local>) -> String {
    date.format("%Y%m%d_%H%M%S").to_string()
}

pub fn parse_timestamp(ts: &str) -> Option<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(ts, "%Y%m%d_%H%M%S").ok()?;
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn load_metadata(folder: &Path) -> Option<SessionMetadata> {
    let data = fs::read_to_string(folder.join("session.json")).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_metadata(folder: &Path, metadata: &SessionMetadata) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(metadata)?;
    let tmp = folder.join("session.json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, folder.join("session.json"))
}

pub fn load_transcript_preview(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 500];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    let head = String::from_utf8_lossy(&buf);
    let head = head.trim_end_matches('\u{FFFD}');
    transcript_preview(head)
}

pub fn scan_sessions(base: &Path) -> Vec<RecordingSession> {
    let _ = fs::create_dir_all(base);
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };
    let contents: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !n.starts_with('.'))
        })
        .collect();

    let mut results = Vec::new();
    let mut folder_session_ids = std::collections::HashSet::new();

    for path in &contents {
        if !path.is_dir() {
            continue;
        }
        let Some(metadata) = load_metadata(path) else {
            continue;
        };
        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        folder_session_ids.insert(id.clone());
        results.push(build_session(id, path.clone(), metadata, false));
    }

    for path in &contents {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".m4a") || !name.starts_with("system_") {
            continue;
        }
        let ts = name
            .trim_end_matches(".m4a")
            .trim_start_matches("system_")
            .to_string();
        if folder_session_ids.contains(&ts) {
            continue;
        }

        let mic_file = format!("mic_{}.m4a", ts);
        let transcript_file = format!("transcript_{}.txt", ts);
        let mut metadata = SessionMetadata::new(parse_timestamp(&ts).unwrap_or_else(Utc::now));
        metadata.system_audio_file = Some(name.to_string());
        metadata.mic_audio_file = base.join(&mic_file).exists().then_some(mic_file);
        metadata.transcript_file = base.join(&transcript_file).exists().then_some(transcript_file);
        results.push(build_session(ts, base.to_path_buf(), metadata, true));
    }

    results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    results
}

fn build_session(
    id: String,
    folder: PathBuf,
    metadata: SessionMetadata,
    is_legacy_flat: bool,
) -> RecordingSession {
    let transcript_path = metadata.transcript_file.as_ref().map(|f| folder.join(f));
    let transcript_preview = transcript_path
        .as_ref()
        .and_then(|p| load_transcript_preview(p));
    let has_transcript = transcript_path.as_ref().is_some_and(|p| p.exists());
    RecordingSession {
        id,
        created_at: metadata.created_at,
        folder_path: folder,
        is_legacy_flat,
        metadata: SessionMetadataFields {
            system_audio_file: metadata.system_audio_file,
            mic_audio_file: metadata.mic_audio_file,
            transcript_file: metadata.transcript_file,
            engine_type: metadata.engine_type,
            model_id: metadata.model_id,
            summary: metadata.summary,
            locale_identifier: metadata.locale_identifier,
            minutes_file: metadata.minutes_file,
            minutes_preset_id: metadata.minutes_preset_id,
            minutes_exported_at: metadata.minutes_exported_at,
            minutes_exported_path: metadata.minutes_exported_path,
            minutes_exported_hash: metadata.minutes_exported_hash,
        },
        transcript_preview,
        has_transcript,
    }
}

pub fn metadata_from_fields(
    created_at: DateTime<Utc>,
    fields: &SessionMetadataFields,
) -> SessionMetadata {
    SessionMetadata {
        created_at,
        system_audio_file: fields.system_audio_file.clone(),
        mic_audio_file: fields.mic_audio_file.clone(),
        transcript_file: fields.transcript_file.clone(),
        engine_type: fields.engine_type.clone(),
        model_id: fields.model_id.clone(),
        summary: fields.summary.clone(),
        locale_identifier: fields.locale_identifier.clone(),
        minutes_file: fields.minutes_file.clone(),
        minutes_preset_id: fields.minutes_preset_id,
        minutes_exported_at: fields.minutes_exported_at,
        minutes_exported_path: fields.minutes_exported_path.clone(),
        minutes_exported_hash: fields.minutes_exported_hash.clone(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptDocument {
    pub version: u32,
    pub entries: Vec<TranscriptEntry>,
}

impl TranscriptDocument {
    pub fn new(entries: Vec<TranscriptEntry>) -> Self {
        Self { version: 1, entries }
    }

    pub fn load(path: &Path) -> Option<Self> {
        let data = fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn metadata_roundtrip_matches_swift_format() {
        let json = r#"{
  "createdAt" : "2026-08-09T10:30:00Z",
  "systemAudioFile" : "system_20260809_193000.m4a",
  "micAudioFile" : "mic_20260809_193000.m4a",
  "transcriptFile" : "transcript_20260809_193000.txt",
  "engineType" : "whisper",
  "modelId" : "openai_whisper-large-v3-v20240930_626MB",
  "summary" : "Q3ロードマップ定例",
  "localeIdentifier" : "ja_JP",
  "minutesFile" : "minutes.md",
  "minutesPresetId" : "6E4A1F1C-93B1-43A1-9E5A-111111111111"
}"#;
        let meta: SessionMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(
            meta.created_at,
            Utc.with_ymd_and_hms(2026, 8, 9, 10, 30, 0).unwrap()
        );
        assert_eq!(meta.engine_type.as_deref(), Some("whisper"));

        let out = serde_json::to_string_pretty(&meta).unwrap();
        let reparsed: SessionMetadata = serde_json::from_str(&out).unwrap();
        assert_eq!(meta, reparsed);
        assert!(out.contains("\"createdAt\": \"2026-08-09T10:30:00Z\""));
    }

    #[test]
    fn metadata_partial_fields_tolerated() {
        let json = r#"{"createdAt": "2026-01-01T00:00:00Z"}"#;
        let meta: SessionMetadata = serde_json::from_str(json).unwrap();
        assert!(meta.system_audio_file.is_none());
        let out = serde_json::to_string_pretty(&meta).unwrap();
        assert!(!out.contains("systemAudioFile"));
    }

    #[test]
    fn minutes_export_fields_roundtrip() {
        let json = r#"{
  "createdAt": "2026-08-09T10:30:00Z",
  "minutesFile": "minutes.md",
  "minutesExportedAt": "2026-08-10T05:00:00Z",
  "minutesExportedPath": "/Users/x/Minutes/teirei.md",
  "minutesExportedHash": "abc123"
}"#;
        let meta: SessionMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(
            meta.minutes_exported_at,
            Some(Utc.with_ymd_and_hms(2026, 8, 10, 5, 0, 0).unwrap())
        );
        assert_eq!(
            meta.minutes_exported_path.as_deref(),
            Some(Path::new("/Users/x/Minutes/teirei.md"))
        );

        let out = serde_json::to_string_pretty(&meta).unwrap();
        assert!(out.contains("\"minutesExportedAt\": \"2026-08-10T05:00:00Z\""));
        assert_eq!(serde_json::from_str::<SessionMetadata>(&out).unwrap(), meta);
    }

    #[test]
    fn minutes_export_fields_omitted_when_never_exported() {
        let meta = SessionMetadata::new(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap());
        let out = serde_json::to_string_pretty(&meta).unwrap();
        assert!(!out.contains("minutesExported"));
    }

    #[test]
    fn uuid_uppercase_roundtrip() {
        let json = r#"{"createdAt": "2026-01-01T00:00:00Z", "minutesPresetId": "6E4A1F1C-93B1-43A1-9E5A-111111111111"}"#;
        let meta: SessionMetadata = serde_json::from_str(json).unwrap();
        assert!(meta.minutes_preset_id.is_some());
    }

    #[test]
    fn scan_finds_folder_and_legacy_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        let folder = base.join("20260809_103000");
        fs::create_dir(&folder).unwrap();
        let meta = SessionMetadata {
            summary: Some("テスト".into()),
            transcript_file: Some("transcript_20260809_103000.txt".into()),
            ..SessionMetadata::new(Utc.with_ymd_and_hms(2026, 8, 9, 1, 30, 0).unwrap())
        };
        save_metadata(&folder, &meta).unwrap();
        fs::write(
            folder.join("transcript_20260809_103000.txt"),
            "[00:00] PC音声: こんにちは",
        )
        .unwrap();

        fs::write(base.join("system_20250101_090000.m4a"), b"x").unwrap();
        fs::write(base.join("mic_20250101_090000.m4a"), b"x").unwrap();

        fs::create_dir(base.join("no_metadata_dir")).unwrap();

        let sessions = scan_sessions(base);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "20260809_103000");
        assert_eq!(sessions[0].transcript_preview.as_deref(), Some("こんにちは"));
        assert!(sessions[0].has_transcript);
        assert!(!sessions[0].is_legacy_flat);

        assert_eq!(sessions[1].id, "20250101_090000");
        assert!(sessions[1].is_legacy_flat);
        assert_eq!(
            sessions[1].metadata.mic_audio_file.as_deref(),
            Some("mic_20250101_090000.m4a")
        );
        assert!(sessions[1].metadata.transcript_file.is_none());
    }

    #[test]
    fn legacy_skipped_when_folder_exists_for_same_id() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let folder = base.join("20260101_000000");
        fs::create_dir(&folder).unwrap();
        save_metadata(
            &folder,
            &SessionMetadata::new(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()),
        )
        .unwrap();
        fs::write(base.join("system_20260101_000000.m4a"), b"x").unwrap();

        let sessions = scan_sessions(base);
        assert_eq!(sessions.len(), 1);
        assert!(!sessions[0].is_legacy_flat);
    }

    #[test]
    fn parse_timestamp_roundtrip() {
        let ts = "20260809_103000";
        let parsed = parse_timestamp(ts).unwrap();
        let local = parsed.with_timezone(&Local);
        assert_eq!(session_id_for(local), ts);
        assert!(parse_timestamp("garbage").is_none());
    }
}
