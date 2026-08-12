use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use fennec_core::types::TimedSegment;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

#[derive(Debug, Clone)]
pub enum AppleLiveEvent {
    Partial {
        stream_id: String,
        text: String,
    },
    Final {
        stream_id: String,
        segment: TimedSegment,
    },
}

type ProgressFn = Box<dyn Fn(f64) + Send>;
type LiveFn = Arc<dyn Fn(AppleLiveEvent) + Send + Sync>;

struct Pending {
    tx: mpsc::Sender<Value>,
    on_progress: Option<Arc<Mutex<ProgressFn>>>,
}

#[derive(Default)]
struct Router {
    pending: HashMap<u64, Pending>,
    live: HashMap<String, LiveFn>,
}

pub struct SidecarClient {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    router: Arc<Mutex<Router>>,
    next_id: AtomicU64,
}

impl SidecarClient {
    pub fn spawn(helper_path: &Path) -> Result<Self> {
        let mut child = Command::new(helper_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn helper {}", helper_path.display()))?;
        let stdin = child.stdin.take().context("helper stdin unavailable")?;
        let stdout = child.stdout.take().context("helper stdout unavailable")?;

        let router = Arc::new(Mutex::new(Router::default()));
        let reader_router = Arc::clone(&router);
        std::thread::Builder::new()
            .name("fennec-helper-reader".into())
            .spawn(move || Self::reader_loop(stdout, reader_router))
            .context("failed to spawn helper reader thread")?;

        Ok(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            router,
            next_id: AtomicU64::new(1),
        })
    }

    fn reader_loop(stdout: ChildStdout, router: Arc<Mutex<Router>>) {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(msg) => Self::route(&router, msg),
                Err(_) => eprintln!("fennec-helper: unparsable output line: {line}"),
            }
        }
        // helper終了時にpendingを破棄してrecv側をエラーで起こす
        let mut guard = router.lock().unwrap();
        guard.pending.clear();
        guard.live.clear();
    }

    fn route(router: &Arc<Mutex<Router>>, msg: Value) {
        if let Some(event) = msg.get("event").and_then(Value::as_str) {
            match event {
                "progress" => {
                    let Some(id) = msg.get("id").and_then(Value::as_u64) else {
                        return;
                    };
                    let fraction = msg.get("fraction").and_then(Value::as_f64).unwrap_or(0.0);
                    let cb = router
                        .lock()
                        .unwrap()
                        .pending
                        .get(&id)
                        .and_then(|p| p.on_progress.clone());
                    if let Some(cb) = cb {
                        (cb.lock().unwrap())(fraction);
                    }
                }
                "partial" | "final" => {
                    let Some(stream_id) = msg.get("streamId").and_then(Value::as_str) else {
                        return;
                    };
                    let cb = router.lock().unwrap().live.get(stream_id).cloned();
                    let Some(cb) = cb else { return };
                    if event == "partial" {
                        let text = msg
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        cb(AppleLiveEvent::Partial {
                            stream_id: stream_id.to_string(),
                            text,
                        });
                    } else if let Some(segment) = msg
                        .get("segment")
                        .cloned()
                        .and_then(|v| serde_json::from_value::<TimedSegment>(v).ok())
                    {
                        cb(AppleLiveEvent::Final {
                            stream_id: stream_id.to_string(),
                            segment,
                        });
                    }
                }
                _ => {}
            }
            return;
        }

        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            let pending = router.lock().unwrap().pending.remove(&id);
            if let Some(p) = pending {
                let _ = p.tx.send(msg);
            }
        }
    }

    fn send_line(&self, value: &Value) -> Result<()> {
        let mut stdin = self.stdin.lock().unwrap();
        serde_json::to_writer(&mut *stdin, value).context("failed to write to helper stdin")?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    fn request(&self, method: &str, params: Option<Value>, on_progress: Option<ProgressFn>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.router.lock().unwrap().pending.insert(
            id,
            Pending {
                tx,
                on_progress: on_progress.map(|f| Arc::new(Mutex::new(f))),
            },
        );

        let mut msg = json!({ "id": id, "method": method });
        if let Some(params) = params {
            msg["params"] = params;
        }
        if let Err(e) = self.send_line(&msg) {
            self.router.lock().unwrap().pending.remove(&id);
            return Err(e);
        }

        let resp = rx
            .recv()
            .map_err(|_| anyhow!("fennec-helper terminated before responding to {method}"))?;
        if resp.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(resp.get("data").cloned().unwrap_or(Value::Null))
        } else {
            let error = resp
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            bail!("{method} failed: {error}")
        }
    }

    pub fn capabilities(&self) -> Result<Value> {
        self.request("capabilities", None, None)
    }

    pub fn supported_locales(&self) -> Result<Vec<String>> {
        let data = self.request("supportedLocales", None, None)?;
        Ok(data
            .get("locales")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn batch_transcribe(
        &self,
        file: &Path,
        locale: &str,
        on_progress: Box<dyn Fn(f64) + Send>,
    ) -> Result<(String, Vec<TimedSegment>)> {
        let params = json!({ "file": file.to_string_lossy(), "locale": locale });
        let data = self.request("batchTranscribe", Some(params), Some(on_progress))?;
        let text = data
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let segments = parse_segments(data.get("segments"))?;
        Ok((text, segments))
    }

    pub fn live_start(
        &self,
        stream_id: &str,
        locale: &str,
        sample_rate: u32,
        on_event: Box<dyn Fn(AppleLiveEvent) + Send + Sync>,
    ) -> Result<()> {
        self.router
            .lock()
            .unwrap()
            .live
            .insert(stream_id.to_string(), Arc::from(on_event));
        let params = json!({
            "streamId": stream_id,
            "locale": locale,
            "sampleRate": sample_rate,
            "channels": 1,
        });
        if let Err(e) = self.request("liveStart", Some(params), None) {
            self.router.lock().unwrap().live.remove(stream_id);
            return Err(e);
        }
        Ok(())
    }

    pub fn live_audio(&self, stream_id: &str, samples: &[f32]) -> Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let pcm = base64::engine::general_purpose::STANDARD.encode(bytes);
        self.send_line(&json!({
            "method": "liveAudio",
            "params": { "streamId": stream_id, "pcm": pcm },
        }))
    }

    pub fn live_stop(&self, stream_id: &str) -> Result<Vec<TimedSegment>> {
        let result = self.request("liveStop", Some(json!({ "streamId": stream_id })), None);
        self.router.lock().unwrap().live.remove(stream_id);
        let data = result?;
        parse_segments(data.get("segments"))
    }

    pub fn generate_title(&self, text: &str, lang: &str) -> Result<Option<String>> {
        let data = self.request("generateTitle", Some(json!({ "text": text, "lang": lang })), None)?;
        Ok(data
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    pub fn translate(&self, texts: &[String], target: &str) -> Result<Vec<Option<String>>> {
        let data = self.request("translate", Some(json!({ "texts": texts, "target": target })), None)?;
        let translations = data
            .get("translations")
            .and_then(Value::as_array)
            .context("translate response missing translations array")?;
        Ok(translations
            .iter()
            .map(|v| v.as_str().map(str::to_string))
            .collect())
    }
}

impl Drop for SidecarClient {
    fn drop(&mut self) {
        drop(self.stdin.lock().map(|mut s| s.flush()));
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn parse_segments(value: Option<&Value>) -> Result<Vec<TimedSegment>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value.clone()).context("failed to parse segments")
}
