use fennec_core::format::format_time;
use fennec_core::settings::Settings;
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "main";

pub struct TrayHandles {
    pub toggle: MenuItem<Wry>,
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItemBuilder::with_id("toggle_recording", "録音開始").build(app)?;
    let show = MenuItemBuilder::with_id("show_window", "ウィンドウを表示").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "終了").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&toggle, &show, &quit])
        .build()?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle_recording" => {
                let recording = {
                    let state = app.state::<crate::state::AppState>();
                    let recorder = state.recorder.lock().unwrap();
                    recorder.is_some()
                };
                let result = if recording {
                    crate::commands::recording::perform_stop(app).map(|_| ())
                } else {
                    crate::commands::recording::perform_start(app).map(|_| ())
                };
                if let Err(e) = result {
                    log::warn!("tray toggle recording failed: {e}");
                }
            }
            "show_window" => crate::window::show_main(app),
            "quit" => {
                app.exit(0);
            }
            _ => {}
        });

    #[cfg(target_os = "macos")]
    {
        let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon@2x.png"))?;
        builder = builder.icon(icon).icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone()).icon_as_template(false);
    }
    builder.build(app)?;

    app.manage(TrayHandles { toggle });
    Ok(())
}

pub fn apply(app: &AppHandle, settings: &Settings) {
    // Dockアイコンを隠すとメニューバーが唯一の操作手段になるため、その場合は強制表示する
    let visible = settings.show_in_menu_bar || settings.hide_from_dock;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_visible(visible) {
            log::warn!("failed to update tray visibility: {e}");
        }
    }
    if visible {
        refresh(app);
    }
}

pub fn refresh(app: &AppHandle) {
    let (recording, duration) = {
        let state = app.state::<crate::state::AppState>();
        let recorder = state.recorder.lock().unwrap();
        match recorder.as_ref() {
            Some(c) => (true, c.duration()),
            None => (false, 0.0),
        }
    };
    set_recording(app, recording, duration);
}

pub fn set_recording(app: &AppHandle, recording: bool, duration: f64) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let title = if recording {
            format_time(duration)
        } else {
            String::new()
        };
        let _ = tray.set_title(Some(title));
    }
    if let Some(handles) = app.try_state::<TrayHandles>() {
        let label = if recording { "録音停止" } else { "録音開始" };
        let _ = handles.toggle.set_text(label);
    }
}
