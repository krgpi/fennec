import { create } from "zustand";
import { api } from "../api/commands";
import type { Settings } from "../types";

interface SettingsStore {
  settings: Settings | null;
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

export const useSettings = create<SettingsStore>((set, get) => ({
  settings: null,
  load: async () => {
    set({ settings: await api.getSettings() });
  },
  update: async (patch) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...patch };
    set({ settings: next });
    await api.updateSettings(next);
  },
}));
