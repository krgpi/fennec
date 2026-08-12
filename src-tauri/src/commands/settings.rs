use crate::state::AppState;
use fennec_core::settings::Settings;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.read().unwrap().clone()
}

#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) {
    let old = {
        let mut current = state.settings.write().unwrap();
        std::mem::replace(&mut *current, settings.clone())
    };
    state.persist_settings();

    if old.launch_at_login != settings.launch_at_login {
        sync_autostart(&app, settings.launch_at_login);
    }

    if old.show_in_menu_bar != settings.show_in_menu_bar
        || old.hide_from_dock != settings.hide_from_dock
    {
        crate::tray::apply(&app, &settings);
    }

    if old.hide_from_dock != settings.hide_from_dock {
        crate::window::apply_dock_policy(&app);
    }
}

pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    let autostart = app.autolaunch();
    if autostart.is_enabled().unwrap_or(false) == enabled {
        return;
    }
    let result = if enabled {
        autostart.enable()
    } else {
        autostart.disable()
    };
    if let Err(e) = result {
        log::warn!("failed to update autostart: {e}");
    }
}
