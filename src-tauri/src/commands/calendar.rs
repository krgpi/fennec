use fennec_core::calendar::CalendarInfo;

#[tauri::command]
pub async fn calendar_request_access() -> bool {
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(crate::calendar::eventkit::request_access)
            .await
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[tauri::command]
pub async fn calendar_list() -> Vec<CalendarInfo> {
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(crate::calendar::eventkit::list_calendars)
            .await
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}
