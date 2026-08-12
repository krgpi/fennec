export type TranscriptSource = "system" | "mic";

export interface TranscriptEntry {
  source: TranscriptSource;
  text: string;
  startTime: number;
  speakerId?: number;
  translation?: string;
}

export interface RecordingSession {
  id: string;
  createdAt: string;
  folderPath: string;
  isLegacyFlat: boolean;
  systemAudioFile?: string | null;
  micAudioFile?: string | null;
  transcriptFile?: string | null;
  engineType?: string | null;
  modelId?: string | null;
  summary?: string | null;
  localeIdentifier?: string | null;
  minutesFile?: string | null;
  minutesPresetId?: string | null;
  minutesExportedAt?: string | null;
  minutesExportedPath?: string | null;
  transcriptPreview?: string | null;
  hasTranscript: boolean;
}

export type MinutesBackend = "claude" | "codex" | "gemini";

export interface MinutesPreset {
  id: string;
  name: string;
  contextFolder?: string | null;
  outputFolder?: string | null;
  backend: MinutesBackend;
  model: string;
  createdAt: string;
  lastUsedAt: string;
}

export type HookEvent =
  | "recordingStarted"
  | "recordingStopped"
  | "transcriptionCompleted"
  | "minutesGenerated";

export interface AutomationHook {
  id: string;
  event: HookEvent;
  command: string;
  enabled: boolean;
}

export interface Settings {
  saveDirectory?: string | null;
  appLanguage: string;
  showInMenuBar: boolean;
  hideFromDock: boolean;
  launchAtLogin: boolean;
  useSystemDefaultMic: boolean;
  micDeviceId?: string | null;
  transcriptionLocale?: string | null;
  liveTranscriptionEnabled: boolean;
  liveTranscriptionEngine: string;
  autoTranscribeEnabled: boolean;
  autoTranscribeEngine: string;
  translationEnabled: boolean;
  diarizationEnabled: boolean;
  silenceAutoStopEnabled: boolean;
  silenceAutoStopMinutes: number;
  meetingReminderEnabled: boolean;
  meetingReminderMinutesBefore: number;
  meetingReminderRequireMeetingUrl: boolean;
  meetingReminderRequireMic: boolean;
  meetingReminderCalendarIdentifiers?: string[] | null;
  calendarIcsUrls: string[];
  whisperEnabled: boolean;
  whisperModelId: string;
  whisperDownloadedModels: Record<string, string>;
  hookTimeoutSeconds: number;
  automationHooks: AutomationHook[];
  minutesPresets: MinutesPreset[];
}
