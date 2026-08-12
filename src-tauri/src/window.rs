use tauri::{AppHandle, Manager, WindowEvent};

pub fn setup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let handle = app.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.hide();
                }
                apply_dock_policy(&handle);
            }
            WindowEvent::Focused(true) => apply_dock_policy(&handle),
            _ => {}
        });
    }
    apply_dock_policy(app);
}

pub fn show_main(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
        log::warn!("failed to set activation policy: {e}");
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(target_os = "macos")]
pub fn apply_dock_policy(app: &AppHandle) {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let hide = state.settings.read().unwrap().hide_from_dock;
    let has_visible_window = app
        .webview_windows()
        .values()
        .any(|w| w.is_visible().unwrap_or(false));
    let policy = if hide && !has_visible_window {
        tauri::ActivationPolicy::Accessory
    } else {
        tauri::ActivationPolicy::Regular
    };
    if let Err(e) = app.set_activation_policy(policy) {
        log::warn!("failed to set activation policy: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_dock_policy(_app: &AppHandle) {}
