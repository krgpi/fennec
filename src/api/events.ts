import { listen } from "@tauri-apps/api/event";

export interface RecordingLevels {
  sys: number;
  mic: number;
}

export interface RecordingStatePayload {
  recording: boolean;
  duration: number;
  sessionId: string | null;
}

export interface TranscribeProgress {
  sessionId: string;
  phase: string;
  fraction: number | null;
}

export interface TranscribeDone {
  sessionId: string;
  error: string | null;
}

export interface ModelProgress {
  modelId: string;
  fraction: number;
  error: string | null;
  done: boolean;
}

export interface MinutesDone {
  sessionId: string;
  file: string | null;
  error: string | null;
}

export const events = {
  onRecordingLevels: (cb: (p: RecordingLevels) => void) =>
    listen<RecordingLevels>("recording://levels", (e) => cb(e.payload)),
  onRecordingState: (cb: (p: RecordingStatePayload) => void) =>
    listen<RecordingStatePayload>("recording://state", (e) => cb(e.payload)),
  onRecordingNoInput: (cb: (p: { sys: boolean; mic: boolean }) => void) =>
    listen<{ sys: boolean; mic: boolean }>("recording://no-input", (e) => cb(e.payload)),
  onRecordingAutoStopped: (cb: (p: { reason: string; minutes: number }) => void) =>
    listen<{ reason: string; minutes: number }>("recording://auto-stopped", (e) =>
      cb(e.payload),
    ),
  onLiveTranscript: (
    cb: (p: { source: string; finalized: string; volatile: string }) => void,
  ) =>
    listen<{ source: string; finalized: string; volatile: string }>(
      "live://transcript",
      (e) => cb(e.payload),
    ),
  onLiveTranslation: (cb: (p: { source: string; text: string }) => void) =>
    listen<{ source: string; text: string }>("live://translation", (e) => cb(e.payload)),
  onSessionsChanged: (cb: () => void) => listen("sessions://changed", () => cb()),
  onTranscribeProgress: (cb: (p: TranscribeProgress) => void) =>
    listen<TranscribeProgress>("transcribe://progress", (e) => cb(e.payload)),
  onTranscribeDone: (cb: (p: TranscribeDone) => void) =>
    listen<TranscribeDone>("transcribe://done", (e) => cb(e.payload)),
  onModelProgress: (cb: (p: ModelProgress) => void) =>
    listen<ModelProgress>("model://progress", (e) => cb(e.payload)),
  onMinutesDone: (cb: (p: MinutesDone) => void) =>
    listen<MinutesDone>("minutes://done", (e) => cb(e.payload)),
  onPlayerPosition: (cb: (p: import("./commands").PlayerPosition) => void) =>
    listen<import("./commands").PlayerPosition>("player://position", (e) => cb(e.payload)),
};
