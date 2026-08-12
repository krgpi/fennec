mod calendar;
mod commands;
mod events;
mod ipc;
mod recording;
mod services;
mod state;
mod transcription;
mod tray;
mod window;

use state::AppState;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_main(app);
        }))
        .setup(|app| {
            let state = AppState::initialize(app.handle())?;
            app.manage(state);
            ipc::server::start(app.handle().clone());
            if let Err(e) = tray::setup(app.handle()) {
                log::warn!("failed to set up tray icon: {e}");
            }
            {
                let state = app.state::<AppState>();
                let settings = state.settings.read().unwrap();
                tray::apply(app.handle(), &settings);
                commands::settings::sync_autostart(app.handle(), settings.launch_at_login);
            }
            window::setup(app.handle());
            calendar::monitor::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::sessions::list_sessions,
            commands::sessions::read_transcript,
            commands::sessions::read_note,
            commands::sessions::save_note,
            commands::sessions::read_minutes,
            commands::sessions::save_minutes,
            commands::sessions::export_minutes,
            commands::sessions::update_summary,
            commands::sessions::delete_session,
            commands::sessions::reveal_session_folder,
            commands::recording::list_input_devices,
            commands::recording::start_recording,
            commands::recording::stop_recording,
            commands::recording::get_recording_state,
            commands::transcription::start_transcription,
            commands::transcription::cancel_transcription,
            commands::transcription::get_transcription_state,
            commands::models::list_whisper_models,
            commands::models::download_whisper_model,
            commands::models::cancel_model_download,
            commands::models::delete_whisper_model,
            commands::models::diarization_status,
            commands::models::download_diarization_models,
            commands::models::delete_diarization_models,
            commands::minutes::list_minutes_backends,
            commands::minutes::generate_minutes,
            commands::minutes::cancel_minutes,
            commands::playback::player_load,
            commands::playback::player_unload,
            commands::playback::player_play,
            commands::playback::player_pause,
            commands::playback::player_seek,
            commands::playback::player_set_rate,
            commands::playback::player_set_track_muted,
            commands::calendar::calendar_request_access,
            commands::calendar::calendar_list,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                window::show_main(_app);
            }
        });
}
