import { invoke } from "@tauri-apps/api/core";

export interface CalendarInfo {
  id: string;
  title: string;
  color?: string | null;
  source: string;
}

export const calendarApi = {
  requestAccess: () => invoke<boolean>("calendar_request_access"),
  listCalendars: () => invoke<CalendarInfo[]>("calendar_list"),
};
