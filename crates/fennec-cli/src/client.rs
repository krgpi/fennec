use fennec_core::ipc::{Request, Response};
use interprocess::local_socket::{prelude::*, GenericFilePath, Stream};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

pub const EXIT_ERROR: i32 = 1;
pub const EXIT_NOT_RUNNING: i32 = 2;

pub struct Client;

impl Client {
    fn connect() -> Option<Stream> {
        // tauri buildはrelease CLIをtarget/debug/へも配置するため、自プロファイルの
        // ソケットに限定せず両方を試す（1つのCLIでDev/Release両アプリに接続できる）
        let candidates = [
            fennec_core::ipc::socket_name(cfg!(debug_assertions)),
            fennec_core::ipc::socket_name(!cfg!(debug_assertions)),
        ];
        for path in candidates {
            let name = match path.clone().to_fs_name::<GenericFilePath>() {
                Ok(name) => name,
                Err(e) => {
                    eprintln!("fennec: invalid socket name {path}: {e}");
                    continue;
                }
            };
            match Stream::connect(name) {
                Ok(stream) => return Some(stream),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound
                    || e.kind() == std::io::ErrorKind::ConnectionRefused => continue,
                Err(e) => {
                    eprintln!("fennec: failed to connect to {path}: {e}");
                    continue;
                }
            }
        }
        None
    }

    pub fn request(
        command: &str,
        args: HashMap<String, String>,
        mut on_progress: impl FnMut(&str, Option<f64>),
    ) -> Result<Value, ClientError> {
        let stream = Self::connect().ok_or(ClientError::NotRunning)?;
        Self::request_on(stream, command, args, &mut on_progress)
    }

    pub fn request_with_launch(
        command: &str,
        args: HashMap<String, String>,
        launch: bool,
        mut on_progress: impl FnMut(&str, Option<f64>),
    ) -> Result<Value, ClientError> {
        let stream = match Self::connect() {
            Some(s) => s,
            None if launch => {
                launch_app()?;
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    if let Some(s) = Self::connect() {
                        break s;
                    }
                    if Instant::now() >= deadline {
                        return Err(ClientError::NotRunning);
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
            None => return Err(ClientError::NotRunning),
        };
        Self::request_on(stream, command, args, &mut on_progress)
    }

    fn request_on(
        mut stream: Stream,
        command: &str,
        args: HashMap<String, String>,
        on_progress: &mut impl FnMut(&str, Option<f64>),
    ) -> Result<Value, ClientError> {
        let request = Request {
            command: command.to_string(),
            args,
        };
        let line = fennec_core::ipc::encode_line(&request)
            .map_err(|e| ClientError::Other(e.to_string()))?;
        stream
            .write_all(&line)
            .map_err(|e| ClientError::Other(e.to_string()))?;
        stream.flush().ok();

        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .map_err(|e| ClientError::Other(e.to_string()))?;
            if n == 0 {
                return Err(ClientError::Other("connection closed".into()));
            }
            let value: Value = serde_json::from_str(&line)
                .map_err(|e| ClientError::Other(format!("invalid response: {e}")))?;
            if let Some(progress) = value.get("progress").and_then(|v| v.as_str()) {
                let fraction = value.get("fraction").and_then(|v| v.as_f64());
                on_progress(progress, fraction);
                continue;
            }
            let response: Response = serde_json::from_value(value)
                .map_err(|e| ClientError::Other(format!("invalid response: {e}")))?;
            if response.ok {
                return Ok(response.data.unwrap_or(Value::Null));
            }
            return Err(ClientError::Server(
                response.error.unwrap_or_else(|| "unknown error".into()),
            ));
        }
    }
}

#[derive(Debug)]
pub enum ClientError {
    NotRunning,
    Server(String),
    Other(String),
}

impl ClientError {
    pub fn exit(&self) -> ! {
        match self {
            ClientError::NotRunning => {
                eprintln!("Fennec is not running. Start the app or pass --launch.");
                std::process::exit(EXIT_NOT_RUNNING);
            }
            ClientError::Server(msg) | ClientError::Other(msg) => {
                eprintln!("error: {msg}");
                std::process::exit(EXIT_ERROR);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn launch_app() -> Result<(), ClientError> {
    let status = std::process::Command::new("open")
        .args(["-g", "-b", "io.github.krgpi.fennec.tauri"])
        .status()
        .map_err(|e| ClientError::Other(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(ClientError::Other("failed to launch Fennec.app".into()))
    }
}

#[cfg(not(target_os = "macos"))]
fn launch_app() -> Result<(), ClientError> {
    Err(ClientError::Other(
        "--launch is not supported on this platform yet".into(),
    ))
}
