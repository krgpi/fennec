use tauri::{AppHandle, Manager, WindowEvent};

pub fn setup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let handle = app.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                if !runs_in_background(&handle) {
                    finish_recording_before_exit(&handle);
                    return;
                }
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

// macOSはウィンドウを閉じてもDockから復帰できるが、Windows/Linuxは通知領域アイコンが
// 無いと復帰手段が無くなるため、その場合はウィンドウを閉じたら終了する
#[cfg(target_os = "macos")]
fn runs_in_background(_app: &AppHandle) -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
fn runs_in_background(app: &AppHandle) -> bool {
    app.try_state::<crate::state::AppState>()
        .map(|state| state.settings.read().unwrap().show_in_menu_bar)
        .unwrap_or(false)
}

fn finish_recording_before_exit(app: &AppHandle) {
    let recording = app
        .try_state::<crate::state::AppState>()
        .map(|state| state.recorder.lock().unwrap().is_some())
        .unwrap_or(false);
    if recording {
        if let Err(e) = crate::commands::recording::perform_stop(app) {
            log::warn!("failed to stop recording before exit: {e}");
        }
    }
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
