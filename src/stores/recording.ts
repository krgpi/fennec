import { create } from "zustand";
import { api } from "../api/commands";

interface RecordingStore {
  recording: boolean;
  duration: number;
  sessionId: string | null;
  levels: { sys: number; mic: number };
  busy: boolean;
  error: string | null;
  noInput: { sys: boolean; mic: boolean } | null;
  live: Record<string, { finalized: string; volatile: string }>;
  liveTranslation: Record<string, string>;
  setState: (p: { recording: boolean; duration: number; sessionId: string | null }) => void;
  setLevels: (levels: { sys: number; mic: number }) => void;
  setNoInput: (p: { sys: boolean; mic: boolean } | null) => void;
  setLive: (source: string, text: { finalized: string; volatile: string }) => void;
  setLiveTranslation: (source: string, text: string) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  clearError: () => void;
}

export const useRecording = create<RecordingStore>((set, get) => ({
  recording: false,
  duration: 0,
  sessionId: null,
  levels: { sys: 0, mic: 0 },
  busy: false,
  error: null,
  noInput: null,
  live: {},
  liveTranslation: {},
  setState: (p) =>
    set({ recording: p.recording, duration: p.duration, sessionId: p.sessionId }),
  setLevels: (levels) => set({ levels }),
  setNoInput: (p) => set({ noInput: p }),
  setLive: (source, text) => set((s) => ({ live: { ...s.live, [source]: text } })),
  setLiveTranslation: (source, text) =>
    set((s) => ({ liveTranslation: { ...s.liveTranslation, [source]: text } })),
  start: async () => {
    if (get().busy) return;
    set({ busy: true, error: null, noInput: null, live: {}, liveTranslation: {} });
    try {
      const sessionId = await api.startRecording();
      set({ recording: true, sessionId, duration: 0 });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ busy: false });
    }
  },
  stop: async () => {
    if (get().busy) return;
    set({ busy: true });
    try {
      await api.stopRecording();
      set({ recording: false, duration: 0, levels: { sys: 0, mic: 0 }, noInput: null });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ busy: false });
    }
  },
  clearError: () => set({ error: null }),
}));
