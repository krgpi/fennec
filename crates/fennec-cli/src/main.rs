mod client;

use clap::{Parser, Subcommand};
use client::{Client, ClientError};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;

#[derive(Parser)]
#[command(name = "fennec", about = "Control the Fennec recording app from the command line.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Status {
        #[arg(long)]
        launch: bool,
        #[arg(long)]
        json: bool,
    },
    Record {
        #[command(subcommand)]
        action: RecordAction,
    },
    Sessions {
        #[command(subcommand)]
        action: Option<SessionsAction>,
        #[arg(long)]
        json: bool,
    },
    Transcribe {
        session_id: String,
        #[arg(long)]
        engine: Option<String>,
    },
    Minutes {
        session_id: String,
        #[arg(long)]
        preset: Option<String>,
        #[arg(long)]
        backend: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    Preset {
        #[command(subcommand)]
        action: PresetAction,
    },
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand)]
enum RecordAction {
    Start,
    Stop,
    Status,
}

#[derive(Subcommand)]
enum SessionsAction {
    List {
        #[arg(long)]
        json: bool,
    },
    Show {
        session_id: String,
    },
    Delete {
        session_id: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum PresetAction {
    List,
    Show {
        name: String,
    },
    Create {
        name: String,
        #[arg(long)]
        context: Option<String>,
        #[arg(long)]
        output: Option<String>,
        #[arg(long)]
        backend: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    Delete {
        name: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    List,
    Get { key: String },
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum ModelAction {
    List,
    Download { model_id: String },
}

#[derive(Subcommand)]
enum HookAction {
    List,
    Add { event: String, command: String },
    Enable { id: String },
    Disable { id: String },
    Delete { id: String },
}

fn args(pairs: &[(&str, Option<&str>)]) -> HashMap<String, String> {
    pairs
        .iter()
        .filter_map(|(k, v)| v.map(|v| (k.to_string(), v.to_string())))
        .collect()
}

fn stderr_progress(message: &str, fraction: Option<f64>) {
    match fraction {
        Some(f) => eprint!("\r{message} {:>3}%", (f * 100.0) as u32),
        None => eprint!("\r{message}"),
    }
    let _ = std::io::stderr().flush();
}

fn finish_progress() {
    eprintln!();
}

fn run(command: &str, request_args: HashMap<String, String>) -> Value {
    Client::request(command, request_args, |_, _| {}).unwrap_or_else(|e| e.exit())
}

fn run_with_progress(command: &str, request_args: HashMap<String, String>) -> Value {
    let mut any_progress = false;
    let result = Client::request(command, request_args, |msg, fraction| {
        any_progress = true;
        stderr_progress(msg, fraction);
    });
    if any_progress {
        finish_progress();
    }
    result.unwrap_or_else(|e| e.exit())
}

fn format_duration(seconds: f64) -> String {
    let total = seconds as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn print_status_text(data: &Value) {
    let recording = data["recording"].as_bool().unwrap_or(false);
    let transcribing = data["transcribing"].as_bool().unwrap_or(false);
    if recording {
        let duration = data["duration"].as_f64().unwrap_or(0.0);
        let id = data["sessionId"].as_str().unwrap_or("?");
        println!("recording ({}, {})", format_duration(duration), id);
    } else if transcribing {
        let id = data["transcribingSessionId"].as_str().unwrap_or("?");
        println!("transcribing ({id})");
    } else {
        println!("idle");
    }
}

fn print_sessions_text(data: &Value) {
    let Some(sessions) = data["sessions"].as_array() else {
        return;
    };
    if sessions.is_empty() {
        println!("no sessions");
        return;
    }
    for s in sessions {
        let id = s["id"].as_str().unwrap_or("?");
        let has_transcript = s["hasTranscript"].as_bool().unwrap_or(false);
        let has_minutes = s.get("minutesFile").is_some();
        let flags = format!(
            "[{}{}]",
            if has_transcript { "T" } else { "-" },
            if has_minutes { "M" } else { "-" }
        );
        let duration = s["duration"]
            .as_f64()
            .map(format_duration)
            .unwrap_or_else(|| "--:--".to_string());
        let summary = s["summary"].as_str().unwrap_or("");
        println!("{id} {flags} {duration} {summary}");
    }
}

fn print_json(data: &Value) {
    println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Status { launch, json } => {
            let data = Client::request_with_launch("status", HashMap::new(), launch, |_, _| {})
                .unwrap_or_else(|e| e.exit());
            if json {
                print_json(&data);
            } else {
                print_status_text(&data);
            }
        }
        Command::Record { action } => match action {
            RecordAction::Start => {
                let data = run("record.start", HashMap::new());
                println!("recording started: {}", data["sessionId"].as_str().unwrap_or("?"));
            }
            RecordAction::Stop => {
                let data = run("record.stop", HashMap::new());
                println!("recording stopped: {}", data["sessionId"].as_str().unwrap_or("?"));
            }
            RecordAction::Status => print_json(&run("record.status", HashMap::new())),
        },
        Command::Sessions { action, json } => match action {
            None | Some(SessionsAction::List { .. }) => {
                let json_flag = json
                    || matches!(action, Some(SessionsAction::List { json: true }));
                let data = run("sessions.list", HashMap::new());
                if json_flag {
                    print_json(&data);
                } else {
                    print_sessions_text(&data);
                }
            }
            Some(SessionsAction::Show { session_id }) => {
                print_json(&run("sessions.show", args(&[("sessionId", Some(&session_id))])));
            }
            Some(SessionsAction::Delete { session_id, force }) => {
                if !force {
                    eprint!("delete session {session_id}? [y/N] ");
                    let _ = std::io::stderr().flush();
                    let mut answer = String::new();
                    let _ = std::io::stdin().read_line(&mut answer);
                    if !answer.trim().eq_ignore_ascii_case("y") {
                        eprintln!("aborted");
                        std::process::exit(1);
                    }
                }
                let data = run("sessions.delete", args(&[("sessionId", Some(&session_id))]));
                println!("deleted: {}", data["deleted"].as_str().unwrap_or("?"));
            }
        },
        Command::Transcribe { session_id, engine } => {
            let data = run_with_progress(
                "transcribe",
                args(&[("sessionId", Some(&session_id)), ("engine", engine.as_deref())]),
            );
            print_json(&data);
        }
        Command::Minutes {
            session_id,
            preset,
            backend,
            model,
        } => {
            let data = run_with_progress(
                "minutes",
                args(&[
                    ("sessionId", Some(&session_id)),
                    ("preset", preset.as_deref()),
                    ("backend", backend.as_deref()),
                    ("model", model.as_deref()),
                ]),
            );
            println!("{}", data["outputFile"].as_str().unwrap_or("?"));
        }
        Command::Preset { action } => match action {
            PresetAction::List => print_json(&run("preset.list", HashMap::new())),
            PresetAction::Show { name } => {
                print_json(&run("preset.show", args(&[("name", Some(&name))])));
            }
            PresetAction::Create {
                name,
                context,
                output,
                backend,
                model,
            } => {
                let data = run(
                    "preset.create",
                    args(&[
                        ("name", Some(&name)),
                        ("context", context.as_deref()),
                        ("output", output.as_deref()),
                        ("backend", backend.as_deref()),
                        ("model", model.as_deref()),
                    ]),
                );
                print_json(&data);
            }
            PresetAction::Delete { name } => {
                let data = run("preset.delete", args(&[("name", Some(&name))]));
                println!("deleted: {}", data["deleted"].as_str().unwrap_or("?"));
            }
        },
        Command::Config { action } => match action {
            ConfigAction::List => print_json(&run("config.list", HashMap::new())),
            ConfigAction::Get { key } => {
                let data = run("config.get", args(&[("key", Some(&key))]));
                println!("{}", data["value"].as_str().unwrap_or(""));
            }
            ConfigAction::Set { key, value } => {
                run("config.set", args(&[("key", Some(&key)), ("value", Some(&value))]));
                println!("{key} = {value}");
            }
        },
        Command::Model { action } => match action {
            ModelAction::List => print_json(&run("model.list", HashMap::new())),
            ModelAction::Download { model_id } => {
                let data = run_with_progress("model.download", args(&[("model", Some(&model_id))]));
                println!("downloaded: {}", data["path"].as_str().unwrap_or("?"));
            }
        },
        Command::Hook { action } => match action {
            HookAction::List => print_json(&run("hook.list", HashMap::new())),
            HookAction::Add { event, command } => {
                let data = run(
                    "hook.add",
                    args(&[("event", Some(&event)), ("hookCommand", Some(&command))]),
                );
                print_json(&data);
            }
            HookAction::Enable { id } => {
                print_json(&run(
                    "hook.set-enabled",
                    args(&[("id", Some(&id)), ("enabled", Some("true"))]),
                ));
            }
            HookAction::Disable { id } => {
                print_json(&run(
                    "hook.set-enabled",
                    args(&[("id", Some(&id)), ("enabled", Some("false"))]),
                ));
            }
            HookAction::Delete { id } => {
                print_json(&run("hook.delete", args(&[("id", Some(&id))])));
            }
        },
    }
}
