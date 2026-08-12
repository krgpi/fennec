import { create } from "zustand";

interface TranscribeJobState {
  running: boolean;
  sessionId: string | null;
  phase: string | null;
  fraction: number | null;
  lastError: string | null;
}

interface ModelDownloadState {
  running: boolean;
  modelId: string | null;
  fraction: number;
  error: string | null;
}

interface MinutesJobState {
  running: boolean;
  sessionId: string | null;
  error: string | null;
}

interface JobsStore {
  transcribe: TranscribeJobState;
  modelDownload: ModelDownloadState;
  minutes: MinutesJobState;
  setTranscribe: (p: Partial<TranscribeJobState>) => void;
  setModelDownload: (p: Partial<ModelDownloadState>) => void;
  setMinutes: (p: Partial<MinutesJobState>) => void;
}

export const useJobs = create<JobsStore>((set) => ({
  transcribe: {
    running: false,
    sessionId: null,
    phase: null,
    fraction: null,
    lastError: null,
  },
  modelDownload: { running: false, modelId: null, fraction: 0, error: null },
  minutes: { running: false, sessionId: null, error: null },
  setTranscribe: (p) => set((s) => ({ transcribe: { ...s.transcribe, ...p } })),
  setModelDownload: (p) => set((s) => ({ modelDownload: { ...s.modelDownload, ...p } })),
  setMinutes: (p) => set((s) => ({ minutes: { ...s.minutes, ...p } })),
}));
