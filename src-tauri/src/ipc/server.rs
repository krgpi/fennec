use crate::state::AppState;
use fennec_core::ipc::{Progress, Request, Response};
use fennec_core::minutes::{MinutesBackend, MinutesPreset};
use fennec_core::models::{whisper_model, WHISPER_MODELS};
use fennec_core::session::{scan_sessions, RecordingSession};
use interprocess::local_socket::{prelude::*, GenericFilePath, ListenerOptions, Stream};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("fennec-ipc".into())
        .spawn(move || {
            if let Err(e) = run(app) {
                log::error!("IPC server failed: {e:#}");
            }
        })
        .ok();
}

fn socket_path() -> PathBuf {
    PathBuf::from(fennec_core::ipc::socket_name(cfg!(debug_assertions)))
}

fn run(app: AppHandle) -> anyhow::Result<()> {
    let path = socket_path();

    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(&path);
    }

    let name = path.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new().name(name).create_sync()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    log::info!("IPC server listening on {}", path.display());
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let app = app.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(app, stream) {
                        log::debug!("IPC connection error: {e:#}");
                    }
                });
            }
            Err(e) => log::debug!("IPC accept error: {e}"),
        }
    }
    Ok(())
}

fn handle_connection(app: AppHandle, stream: Stream) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.len() > 1_000_000 || line.trim().is_empty() {
        return Ok(());
    }
    let mut stream = reader.into_inner();

    let response = match serde_json::from_str::<Request>(&line) {
        Ok(request) => {
            let mut send_progress = |message: &str, fraction: Option<f64>| {
                if let Ok(data) = fennec_core::ipc::encode_line(&Progress {
                    progress: message.to_string(),
                    fraction,
                }) {
                    let _ = stream.write_all(&data);
                    let _ = stream.flush();
                }
            };
            dispatch(&app, &request, &mut send_progress)
        }
        Err(e) => Response::err(format!("invalid request: {e}")),
    };

    let data = fennec_core::ipc::encode_line(&response)?;
    stream.write_all(&data)?;
    stream.flush()?;
    Ok(())
}

type ProgressSink<'a> = dyn FnMut(&str, Option<f64>) + 'a;

fn dispatch(app: &AppHandle, request: &Request, progress: &mut ProgressSink) -> Response {
    let arg = |key: &str| request.args.get(key).cloned();
    match request.command.as_str() {
        "status" | "record.status" => status_response(app),
        "record.start" => match crate::commands::recording::perform_start(app) {
            Ok(session_id) => Response::ok(json!({ "sessionId": session_id })),
            Err(e) => Response::err(e),
        },
        "record.stop" => match crate::commands::recording::perform_stop(app) {
            Ok(session_id) => Response::ok(json!({ "sessionId": session_id })),
            Err(e) => Response::err(e),
        },
        "sessions.list" => {
            let sessions = scan_sessions(&save_dir(app));
            let list: Vec<Value> = sessions.iter().map(session_dict).collect();
            Response::ok(json!({ "sessions": list }))
        }
        "sessions.show" => match resolve_session(app, arg("sessionId").as_deref()) {
            Ok(session) => Response::ok(session_dict(&session)),
            Err(e) => Response::err(e),
        },
        "sessions.delete" => match resolve_session(app, arg("sessionId").as_deref()) {
            Ok(session) => {
                match crate::commands::sessions::delete_session_at(&save_dir(app), &session.id) {
                    Ok(()) => {
                        crate::events::emit(app, crate::events::SESSIONS_CHANGED, ());
                        Response::ok(json!({ "deleted": session.id }))
                    }
                    Err(e) => Response::err(e),
                }
            }
            Err(e) => Response::err(e),
        },
        "transcribe" => handle_transcribe(
            app,
            arg("sessionId").as_deref(),
            arg("engine").as_deref(),
            progress,
        ),
        "minutes" => handle_minutes(
            app,
            arg("sessionId").as_deref(),
            arg("preset").as_deref(),
            arg("backend").as_deref(),
            arg("model").as_deref(),
            progress,
        ),
        "preset.list" => {
            let state = app.state::<AppState>();
            let presets = state.settings.read().unwrap().minutes_presets.clone();
            Response::ok(json!({ "presets": presets.iter().map(preset_dict).collect::<Vec<_>>() }))
        }
        "preset.show" => {
            let state = app.state::<AppState>();
            let name = arg("name").unwrap_or_default();
            let presets = state.settings.read().unwrap().minutes_presets.clone();
            match presets.iter().find(|p| p.name == name) {
                Some(p) => Response::ok(preset_dict(p)),
                None => Response::err(format!("preset not found: {name}")),
            }
        }
        "preset.create" => handle_preset_create(app, &request.args),
        "preset.delete" => {
            let state = app.state::<AppState>();
            let name = arg("name").unwrap_or_default();
            let removed = {
                let mut settings = state.settings.write().unwrap();
                let before = settings.minutes_presets.len();
                settings.minutes_presets.retain(|p| p.name != name);
                before != settings.minutes_presets.len()
            };
            if removed {
                state.persist_settings();
                Response::ok(json!({ "deleted": name }))
            } else {
                Response::err(format!("preset not found: {name}"))
            }
        }
        "config.list" => {
            let state = app.state::<AppState>();
            let settings = state.settings.read().unwrap();
            let map: serde_json::Map<String, Value> = settings
                .config_list()
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect();
            Response::ok(json!({ "config": map }))
        }
        "config.get" => {
            let state = app.state::<AppState>();
            let key = arg("key").unwrap_or_default();
            let settings = state.settings.read().unwrap();
            match settings.config_get(&key) {
                Some(value) => Response::ok(json!({ "key": key, "value": value })),
                None => Response::err(format!("unknown config key: {key}")),
            }
        }
        "config.set" => {
            let state = app.state::<AppState>();
            let key = arg("key").unwrap_or_default();
            let value = arg("value").unwrap_or_default();
            let result = {
                let mut settings = state.settings.write().unwrap();
                settings.config_set(&key, &value)
            };
            match result {
                Ok(()) => {
                    state.persist_settings();
                    Response::ok(json!({ "key": key, "value": value }))
                }
                Err(e) => Response::err(e),
            }
        }
        "model.list" => {
            let state = app.state::<AppState>();
            let settings = state.settings.read().unwrap();
            let models: Vec<Value> = WHISPER_MODELS
                .iter()
                .map(|m| {
                    json!({
                        "id": m.id,
                        "name": m.name,
                        "size": m.size,
                        "downloaded": settings
                            .whisper_downloaded_models
                            .get(m.id)
                            .map(|p| PathBuf::from(p).exists())
                            .unwrap_or(false),
                        "selected": settings.whisper_model_id == m.id,
                    })
                })
                .collect();
            Response::ok(json!({ "models": models }))
        }
        "model.download" => handle_model_download(app, arg("model").as_deref(), progress),
        "hook.list" => {
            let state = app.state::<AppState>();
            let hooks = state.settings.read().unwrap().automation_hooks.clone();
            Response::ok(json!({ "hooks": hooks.iter().map(hook_dict).collect::<Vec<_>>() }))
        }
        "hook.add" => {
            let event_raw = arg("event").unwrap_or_default();
            let Some(event) = fennec_core::hooks::HookEvent::from_raw(&event_raw) else {
                return Response::err(format!("unknown event: {event_raw}"));
            };
            let Some(command) = arg("hookCommand").filter(|c| !c.is_empty()) else {
                return Response::err("hookCommand is required");
            };
            let hook = fennec_core::hooks::AutomationHook::new(event, command);
            let dict = hook_dict(&hook);
            let state = app.state::<AppState>();
            state.settings.write().unwrap().automation_hooks.push(hook);
            state.persist_settings();
            Response::ok(dict)
        }
        "hook.set-enabled" => {
            let enabled = match arg("enabled")
                .as_deref()
                .and_then(fennec_core::ipc::parse_bool_arg)
            {
                Some(v) => v,
                None => return Response::err("invalid enabled value"),
            };
            with_hook_by_prefix(app, arg("id").as_deref().unwrap_or(""), |hook| {
                hook.enabled = enabled;
                hook_dict(hook)
            })
        }
        "hook.delete" => {
            let id_prefix = arg("id").unwrap_or_default();
            let state = app.state::<AppState>();
            let result = {
                let mut settings = state.settings.write().unwrap();
                let matches: Vec<usize> = settings
                    .automation_hooks
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| h.id.to_string().to_lowercase().starts_with(&id_prefix.to_lowercase()))
                    .map(|(i, _)| i)
                    .collect();
                match matches.len() {
                    0 => Err(format!("hook not found: {id_prefix}")),
                    1 => {
                        let hook = settings.automation_hooks.remove(matches[0]);
                        Ok(hook_dict(&hook))
                    }
                    _ => Err(format!("ambiguous hook id: {id_prefix}")),
                }
            };
            match result {
                Ok(dict) => {
                    state.persist_settings();
                    Response::ok(dict)
                }
                Err(e) => Response::err(e),
            }
        }
        other => Response::err(format!("unknown command: {other}")),
    }
}

fn save_dir(app: &AppHandle) -> PathBuf {
    let state = app.state::<AppState>();
    state.save_directory(app)
}

fn status_response(app: &AppHandle) -> Response {
    let state = app.state::<AppState>();
    let (recording, duration, session_id) = {
        let recorder = state.recorder.lock().unwrap();
        match recorder.as_ref() {
            Some(c) => (true, Some(c.duration()), Some(c.session_id.clone())),
            None => (false, None, None),
        }
    };
    let (transcribing, transcribing_session_id) = {
        let job = state.transcribe_job.lock().unwrap();
        match job.as_ref() {
            Some(j) => (true, Some(j.session_id.clone())),
            None => (false, None),
        }
    };
    let mut data = json!({
        "running": true,
        "recording": recording,
        "transcribing": transcribing,
    });
    if let Some(d) = duration {
        data["duration"] = json!(d);
    }
    if let Some(id) = session_id {
        data["sessionId"] = json!(id);
    }
    if let Some(id) = transcribing_session_id {
        data["transcribingSessionId"] = json!(id);
    }
    Response::ok(data)
}

fn resolve_session(app: &AppHandle, id: Option<&str>) -> Result<RecordingSession, String> {
    let id = id.filter(|s| !s.is_empty()).ok_or("sessionId is required")?;
    let sessions = scan_sessions(&save_dir(app));
    if id == "latest" {
        return sessions
            .into_iter()
            .next()
            .ok_or_else(|| "no sessions".to_string());
    }
    sessions
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("session not found: {id}"))
}

fn session_dict(session: &RecordingSession) -> Value {
    let mut dict = json!({
        "id": session.id,
        "createdAt": session
            .created_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "folder": session.folder_path.to_string_lossy(),
        "hasTranscript": session.has_transcript,
    });
    if let Some(v) = &session.metadata.summary {
        dict["summary"] = json!(v);
    }
    if let Some(v) = &session.metadata.engine_type {
        dict["engine"] = json!(v);
    }
    if let Some(v) = &session.metadata.minutes_file {
        dict["minutesFile"] = json!(v);
    }
    if let Some(v) = &session.metadata.transcript_file {
        dict["transcriptFile"] = json!(v);
    }
    if let Some(path) = session.system_audio_path().or_else(|| session.mic_audio_path()) {
        if path.extension().and_then(|e| e.to_str()) == Some("wav") {
            if let Ok(reader) = hound::WavReader::open(&path) {
                let spec = reader.spec();
                if spec.sample_rate > 0 {
                    dict["duration"] =
                        json!(reader.duration() as f64 / spec.sample_rate as f64);
                }
            }
        }
    }
    dict
}

fn preset_dict(preset: &MinutesPreset) -> Value {
    let mut dict = json!({
        "id": preset.id.to_string(),
        "name": preset.name,
        "backend": preset.backend,
        "model": preset.model,
    });
    if let Some(v) = &preset.context_folder {
        dict["contextFolder"] = json!(v.to_string_lossy());
    }
    if let Some(v) = &preset.output_folder {
        dict["outputFolder"] = json!(v.to_string_lossy());
    }
    dict
}

fn hook_dict(hook: &fennec_core::hooks::AutomationHook) -> Value {
    json!({
        "id": hook.id.to_string(),
        "event": hook.event.raw_value(),
        "command": hook.command,
        "enabled": hook.enabled,
    })
}

fn with_hook_by_prefix(
    app: &AppHandle,
    id_prefix: &str,
    update: impl FnOnce(&mut fennec_core::hooks::AutomationHook) -> Value,
) -> Response {
    let state = app.state::<AppState>();
    let result = {
        let mut settings = state.settings.write().unwrap();
        let matches: Vec<usize> = settings
            .automation_hooks
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                h.id.to_string()
                    .to_lowercase()
                    .starts_with(&id_prefix.to_lowercase())
            })
            .map(|(i, _)| i)
            .collect();
        match matches.len() {
            0 => Err(format!("hook not found: {id_prefix}")),
            1 => Ok(update(&mut settings.automation_hooks[matches[0]])),
            _ => Err(format!("ambiguous hook id: {id_prefix}")),
        }
    };
    match result {
        Ok(dict) => {
            state.persist_settings();
            Response::ok(dict)
        }
        Err(e) => Response::err(e),
    }
}

fn handle_preset_create(app: &AppHandle, args: &std::collections::HashMap<String, String>) -> Response {
    let Some(name) = args.get("name").filter(|s| !s.is_empty()) else {
        return Response::err("name is required");
    };
    let backend = match args.get("backend") {
        Some(raw) => match MinutesBackend::from_str_arg(raw) {
            Some(b) => b,
            None => return Response::err(format!("unknown backend: {raw}")),
        },
        None => MinutesBackend::Claude,
    };
    let context = args.get("context").map(PathBuf::from);
    let output = args.get("output").map(PathBuf::from);
    for dir in [&context, &output].into_iter().flatten() {
        if !dir.is_dir() {
            return Response::err(format!("folder not found: {}", dir.display()));
        }
    }

    let mut preset = MinutesPreset::new(name.clone(), backend, chrono::Utc::now());
    preset.context_folder = context;
    preset.output_folder = output;
    if let Some(model) = args.get("model") {
        preset.model = model.clone();
    }
    let dict = preset_dict(&preset);

    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.write().unwrap();
        settings.minutes_presets.retain(|p| p.name != preset.name);
        settings.minutes_presets.push(preset);
    }
    state.persist_settings();
    Response::ok(dict)
}

fn handle_transcribe(
    app: &AppHandle,
    session_id: Option<&str>,
    engine: Option<&str>,
    progress: &mut ProgressSink,
) -> Response {
    let session = match resolve_session(app, session_id) {
        Ok(s) => s,
        Err(e) => return Response::err(e),
    };
    if session.is_legacy_flat {
        return Response::err("legacy flat sessions cannot be retranscribed");
    }

    if let Err(e) = crate::transcription::job::start_with_engine(
        app.clone(),
        session.id.clone(),
        session.folder_path.clone(),
        engine.map(String::from),
    ) {
        return Response::err(e);
    }

    let status = {
        let state = app.state::<AppState>();
        let job = state.transcribe_job.lock().unwrap();
        job.as_ref().map(|j| j.status.clone())
    };
    let Some(status) = status else {
        return Response::err("transcription did not start");
    };

    while !status.done.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let (phase, fraction) = status.phase.lock().unwrap().clone();
        if !phase.is_empty() {
            progress(&format!("transcribing ({phase})"), fraction);
        }
    }

    let error = status.error.lock().unwrap().clone();
    match error {
        Some(e) => Response::err(e),
        None => match resolve_session(app, Some(&session.id)) {
            Ok(session) => Response::ok(session_dict(&session)),
            Err(e) => Response::err(e),
        },
    }
}

fn handle_minutes(
    app: &AppHandle,
    session_id: Option<&str>,
    preset_name: Option<&str>,
    backend: Option<&str>,
    model: Option<&str>,
    progress: &mut ProgressSink,
) -> Response {
    let session = match resolve_session(app, session_id) {
        Ok(s) => s,
        Err(e) => return Response::err(e),
    };
    let Some(transcript_file) = session.metadata.transcript_file.clone() else {
        return Response::err("session has no transcript");
    };
    let transcript = match std::fs::read_to_string(session.folder_path.join(&transcript_file)) {
        Ok(t) => t,
        Err(e) => return Response::err(e.to_string()),
    };

    let state = app.state::<AppState>();
    let preset = match preset_name {
        Some(name) => {
            let presets = state.settings.read().unwrap().minutes_presets.clone();
            match presets.into_iter().find(|p| p.name == name) {
                Some(p) => Some(p),
                None => return Response::err(format!("preset not found: {name}")),
            }
        }
        None => None,
    };

    let backend = match backend {
        Some(raw) => match MinutesBackend::from_str_arg(raw) {
            Some(b) => b,
            None => return Response::err(format!("unknown backend: {raw}")),
        },
        None => preset.as_ref().map(|p| p.backend).unwrap_or(MinutesBackend::Claude),
    };
    let model = model
        .map(String::from)
        .or_else(|| preset.as_ref().map(|p| p.model.clone()))
        .unwrap_or_else(|| backend.default_model().to_string());

    {
        let job = state.minutes_job.lock().unwrap();
        if job.is_some() {
            return Response::err("minutes generation already running");
        }
    }

    progress("generating", None);
    let request = crate::services::minutes::MinutesRequest {
        session_id: session.id.clone(),
        session_folder: session.folder_path.clone(),
        transcript,
        backend,
        model,
        context_folder: preset.as_ref().and_then(|p| p.context_folder.clone()),
        output_folder: preset.as_ref().and_then(|p| p.output_folder.clone()),
    };
    let child_slot = Arc::new(std::sync::Mutex::new(None));
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));

    match crate::services::minutes::generate(&request, child_slot, cancelled) {
        Ok(outcome) => {
            crate::services::minutes::record_outcome(
                &session.folder_path,
                &outcome,
                preset.as_ref().map(|p| p.id),
            );
            crate::services::hooks::fire(
                state.hook_config(app),
                fennec_core::hooks::HookEvent::MinutesGenerated,
                None,
                std::collections::HashMap::from([(
                    "FENNEC_MINUTES_FILE".to_string(),
                    outcome.output_file.to_string_lossy().into_owned(),
                )]),
            );
            crate::events::emit(app, crate::events::SESSIONS_CHANGED, ());
            Response::ok(json!({ "outputFile": outcome.output_file.to_string_lossy() }))
        }
        Err(e) => Response::err(format!("{e:#}")),
    }
}

fn handle_model_download(
    app: &AppHandle,
    model_id: Option<&str>,
    progress: &mut ProgressSink,
) -> Response {
    let Some(model_id) = model_id.filter(|s| !s.is_empty()) else {
        return Response::err("model is required");
    };
    let Some(model) = whisper_model(model_id) else {
        return Response::err(format!("unknown model: {model_id}"));
    };

    let state = app.state::<AppState>();
    {
        let mut download = state.model_download.lock().unwrap();
        if download.is_some() {
            return Response::err("download already running");
        }
        *download = Some(Arc::new(std::sync::atomic::AtomicBool::new(false)));
    }
    let cancelled = state.model_download.lock().unwrap().clone().unwrap();

    let dest_dir = crate::commands::models::models_dir(app);
    let mut last_sent = std::time::Instant::now();
    let mut on_progress = |fraction: f64| {
        if last_sent.elapsed() >= std::time::Duration::from_secs(1) || fraction >= 1.0 {
            last_sent = std::time::Instant::now();
            progress("downloading", Some(fraction));
        }
    };
    let result = fennec_stt::download_model(model, &dest_dir, &mut on_progress, cancelled);
    *state.model_download.lock().unwrap() = None;

    match result {
        Ok(path) => {
            {
                let mut settings = state.settings.write().unwrap();
                settings
                    .whisper_downloaded_models
                    .insert(model.id.to_string(), path.to_string_lossy().into_owned());
            }
            state.persist_settings();
            Response::ok(json!({ "model": model.id, "path": path.to_string_lossy() }))
        }
        Err(e) => Response::err(format!("{e:#}")),
    }
}
