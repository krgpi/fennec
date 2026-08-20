import { invoke } from "@tauri-apps/api/core";
import type { MinutesBackend, RecordingSession, Settings, TranscriptEntry } from "../types";

export interface InputDeviceInfo {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface WhisperModelStatus {
  id: string;
  name: string;
  size: string;
  detail: string;
  downloaded: boolean;
  selected: boolean;
}

export interface TranscriptionState {
  running: boolean;
  sessionId: string | null;
}

export interface RecordingStateResult {
  recording: boolean;
  duration: number;
  sessionId: string | null;
}

export interface PlayerPosition {
  sessionId: string;
  position: number;
  duration: number;
  playing: boolean;
  finished: boolean;
  hasSystem: boolean;
  hasMic: boolean;
}

export interface BackendInfo {
  backend: MinutesBackend;
  displayName: string;
  models: { id: string; name: string }[];
  defaultModel: string;
}

export interface MinutesDocument {
  content: string | null;
  exportedAt: string | null;
  exportedPath: string | null;
  dirty: boolean;
}

export interface GenerateMinutesArgs {
  sessionId: string;
  backend: MinutesBackend;
  model: string;
  contextFolder?: string | null;
  outputFolder?: string | null;
  customPrompt?: string | null;
  presetId?: string | null;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) => invoke<void>("update_settings", { settings }),

  listSessions: () => invoke<RecordingSession[]>("list_sessions"),
  readTranscript: (sessionId: string) =>
    invoke<TranscriptEntry[]>("read_transcript", { sessionId }),
  readNote: (sessionId: string) => invoke<string>("read_note", { sessionId }),
  saveNote: (sessionId: string, content: string) =>
    invoke<void>("save_note", { sessionId, content }),
  readMinutes: (sessionId: string) =>
    invoke<MinutesDocument>("read_minutes", { sessionId }),
  saveMinutes: (sessionId: string, content: string) =>
    invoke<MinutesDocument>("save_minutes", { sessionId, content }),
  exportMinutes: (sessionId: string, content: string, target: string) =>
    invoke<MinutesDocument>("export_minutes", { sessionId, content, target }),
  exportSessionAudio: (sessionId: string, target: string) =>
    invoke<void>("export_session_audio", { sessionId, target }),
  updateSummary: (sessionId: string, summary: string) =>
    invoke<void>("update_summary", { sessionId, summary }),
  deleteSession: (sessionId: string) => invoke<void>("delete_session", { sessionId }),
  revealSessionFolder: (sessionId: string) =>
    invoke<void>("reveal_session_folder", { sessionId }),

  listInputDevices: () => invoke<InputDeviceInfo[]>("list_input_devices"),
  startRecording: () => invoke<string>("start_recording"),
  stopRecording: () => invoke<string>("stop_recording"),
  getRecordingState: () => invoke<RecordingStateResult>("get_recording_state"),

  startTranscription: (sessionId: string) =>
    invoke<void>("start_transcription", { sessionId }),
  cancelTranscription: () => invoke<void>("cancel_transcription"),
  getTranscriptionState: () => invoke<TranscriptionState>("get_transcription_state"),

  playerLoad: (sessionId: string) => invoke<PlayerPosition>("player_load", { sessionId }),
  playerUnload: () => invoke<void>("player_unload"),
  playerPlay: () => invoke<void>("player_play"),
  playerPause: () => invoke<void>("player_pause"),
  playerSeek: (seconds: number) => invoke<void>("player_seek", { seconds }),
  playerSetRate: (rate: number) => invoke<void>("player_set_rate", { rate }),
  playerSetTrackMuted: (track: number, muted: boolean) =>
    invoke<void>("player_set_track_muted", { track, muted }),

  listMinutesBackends: (refresh?: boolean) =>
    invoke<BackendInfo[]>("list_minutes_backends", { refresh }),
  generateMinutes: (args: GenerateMinutesArgs) =>
    invoke<void>("generate_minutes", { args }),
  cancelMinutes: () => invoke<void>("cancel_minutes"),

  diarizationStatus: () => invoke<boolean>("diarization_status"),
  downloadDiarizationModels: () => invoke<void>("download_diarization_models"),
  deleteDiarizationModels: () => invoke<void>("delete_diarization_models"),

  listWhisperModels: () => invoke<WhisperModelStatus[]>("list_whisper_models"),
  downloadWhisperModel: (modelId: string) =>
    invoke<void>("download_whisper_model", { modelId }),
  cancelModelDownload: () => invoke<void>("cancel_model_download"),
  deleteWhisperModel: (modelId: string) =>
    invoke<void>("delete_whisper_model", { modelId }),

  openUrl: (url: string) => invoke<void>("plugin:opener|open_url", { url }),
};
