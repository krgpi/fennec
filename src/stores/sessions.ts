import { create } from "zustand";
import { api } from "../api/commands";
import type { RecordingSession } from "../types";

interface SessionsStore {
  sessions: RecordingSession[];
  selectedId: string | null;
  load: () => Promise<void>;
  select: (id: string | null) => void;
}

export const useSessions = create<SessionsStore>((set, get) => ({
  sessions: [],
  selectedId: null,
  load: async () => {
    const sessions = await api.listSessions();
    const { selectedId } = get();
    const stillExists = selectedId != null && sessions.some((s) => s.id === selectedId);
    set({
      sessions,
      selectedId: stillExists ? selectedId : (sessions[0]?.id ?? null),
    });
  },
  select: (id) => set({ selectedId: id }),
}));
