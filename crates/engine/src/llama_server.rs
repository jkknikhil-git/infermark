//! llama.cpp backend: runs the official `llama-server` as a child process on a random
//! localhost port and streams completions from it.
//!
//! The server only listens on 127.0.0.1, requires a random per-session API key, and is killed
//! when the model is unloaded or the backend is dropped.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use infermark_sdk::{
    Backend, GenerateRequest, GenerationStats, LoadOptions, ModelSpec, TokenEvent,
};
use serde_json::{json, Value};

use crate::http_error;
use crate::stream::StreamTracker;

const LOAD_TIMEOUT: Duration = Duration::from_secs(600);
const STDERR_LINES: usize = 40;

pub struct LlamaServer {
    exe: PathBuf,
    version: String,
    agent: ureq::Agent,
    running: Option<Running>,
}

struct Running {
    child: Child,
    base: String,
    api_key: String,
    load_ms: f64,
    stderr: Arc<Mutex<VecDeque<String>>>,
}

impl LlamaServer {
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        let exe = exe.into();
        let mut version = server_version(&exe).unwrap_or_else(|| "unknown".into());
        // Record which GPU the engine can use (llama.cpp offloads automatically).
        let devices = server_devices(&exe);
        if !devices.is_empty() {
            version = format!("{version} on {}", devices.join(", "));
        }
        Self {
            exe,
            version,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(600))
                .build(),
            running: None,
        }
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    fn stderr_tail(&self) -> String {
        self.running
            .as_ref()
            .map(|r| {
                let lines = r.stderr.lock().unwrap();
                let tail: Vec<&str> = lines.iter().rev().take(6).map(String::as_str).collect();
                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
            })
            .unwrap_or_default()
    }
}

/// The engine build, e.g. `b11669`, from `llama-server --version`.
pub fn server_version(exe: &Path) -> Option<String> {
    let text = run_for_output(exe, "--version")?;
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("version:"))?;
    parse_version(line.trim_start().trim_start_matches("version:").trim())
}

/// Older builds print `11669 (1a2b3c4)`, newer ones `0.7.0-dev (build 11669, commit 620d2225c)`.
fn parse_version(v: &str) -> Option<String> {
    if v.is_empty() {
        return None;
    }
    if let Some(rest) = v.split("build ").nth(1) {
        let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !n.is_empty() {
            return Some(format!("b{n}"));
        }
    }
    let first = v.split_whitespace().next().unwrap_or(v);
    if first.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("b{first}"))
    } else {
        Some(first.to_string())
    }
}

/// GPUs llama.cpp can use, e.g. `Intel Iris Xe Graphics (Vulkan)`; empty for CPU builds.
pub fn server_devices(exe: &Path) -> Vec<String> {
    let Some(text) = run_for_output(exe, "--list-devices") else {
        return Vec::new();
    };
    parse_devices(&text)
}

/// Lines like `  Vulkan0: Intel(R) Iris(R) Xe Graphics (8083 MiB, 7423 MiB free)`.
fn parse_devices(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let (api, rest) = l.trim().split_once(": ")?;
            // Device ids are an API name plus an index: `Vulkan0`, `CUDA1`, `Metal0`.
            let base = api.trim_end_matches(|c: char| c.is_ascii_digit());
            if base.is_empty() || base.len() == api.len() || api.contains(' ') {
                return None;
            }
            let name = rest.split(" (").next().unwrap_or(rest);
            let name = name.replace("(R)", "").replace("(TM)", "");
            Some(format!(
                "{} ({base})",
                name.split_whitespace().collect::<Vec<_>>().join(" ")
            ))
        })
        .collect()
}

fn run_for_output(exe: &Path, arg: &str) -> Option<String> {
    let out = hidden(Command::new(exe))
        .arg(arg)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    Some(format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// Don't flash a console window when the desktop app starts the server on Windows.
#[cfg_attr(not(windows), allow(unused_mut))]
fn hidden(mut cmd: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// 128 random bits, good enough to stop other local programs and web pages from using the
/// server while a benchmark runs.
fn random_key() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    (0..2)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(Instant::now().elapsed().as_nanos());
            format!("{:016x}", h.finish())
        })
        .collect()
}

impl Backend for LlamaServer {
    fn id(&self) -> &'static str {
        "llama-cpp"
    }

    fn version(&self) -> String {
        self.version.clone()
    }

    fn load(&mut self, model: &ModelSpec, options: &LoadOptions) -> Result<(), String> {
        self.unload();
        let port = free_port().map_err(|e| format!("no free port: {e}"))?;
        let api_key = random_key();
        let mut cmd = hidden(Command::new(&self.exe));
        cmd.arg("-m")
            .arg(&model.location)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--api-key", &api_key])
            .args(["-c", &options.context_size.max(256).to_string()])
            .args(["-np", "1"]);
        if let Some(t) = options.threads {
            cmd.args(["-t", &t.to_string()]);
        }
        if let Some(n) = options.gpu_layers {
            cmd.args(["-ngl", &n.to_string()]);
        }
        let start = Instant::now();
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start llama-server: {e}"))?;

        // Drain stderr on a thread so the pipe never blocks; keep the tail for error messages.
        let stderr = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_LINES)));
        if let Some(pipe) = child.stderr.take() {
            let sink = stderr.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let mut q = sink.lock().unwrap();
                    if q.len() == STDERR_LINES {
                        q.pop_front();
                    }
                    q.push_back(line);
                }
            });
        }
        let base = format!("http://127.0.0.1:{port}");
        self.running = Some(Running {
            child,
            base: base.clone(),
            api_key,
            load_ms: 0.0,
            stderr,
        });

        let health = format!("{base}/health");
        loop {
            let running = self.running.as_mut().expect("set above");
            if let Ok(Some(status)) = running.child.try_wait() {
                let tail = self.stderr_tail();
                self.running = None;
                return Err(format!(
                    "llama-server stopped ({status}) while loading:\n{tail}"
                ));
            }
            if let Ok(r) = self
                .agent
                .get(&health)
                .timeout(Duration::from_secs(2))
                .call()
            {
                if r.status() == 200 {
                    running.load_ms = start.elapsed().as_secs_f64() * 1000.0;
                    return Ok(());
                }
            }
            if start.elapsed() > LOAD_TIMEOUT {
                let tail = self.stderr_tail();
                self.unload();
                return Err(format!("model took more than 10 minutes to load:\n{tail}"));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn generate(
        &mut self,
        request: &GenerateRequest,
        on_token: &mut dyn FnMut(TokenEvent),
    ) -> Result<GenerationStats, String> {
        let running = self.running.as_ref().ok_or("no model loaded")?;
        let mut prompt = String::new();
        if let Some(system) = &request.system {
            prompt.push_str(system);
            prompt.push_str("\n\n");
        }
        prompt.push_str(&request.prompt);
        let body = json!({
            "prompt": prompt,
            "n_predict": request.max_tokens,
            "temperature": request.temperature,
            "stop": request.stop,
            "ignore_eos": request.ignore_eos,
            "cache_prompt": false,
            "stream": true,
        });
        let start = Instant::now();
        let response = self
            .agent
            .post(&format!("{}/completion", running.base))
            .set("Authorization", &format!("Bearer {}", running.api_key))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("llama-server: {}", http_error(e)))?;

        let mut tracker = StreamTracker::new(start);
        let mut timings: Option<Value> = None;
        for line in BufReader::new(response.into_reader()).lines() {
            let line = line.map_err(|e| format!("stream interrupted: {e}"))?;
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            let chunk: Value = serde_json::from_str(data).map_err(|e| format!("bad chunk: {e}"))?;
            if let Some(err) = chunk.get("error") {
                return Err(format!("llama-server: {err}"));
            }
            if let Some(ev) = chunk["content"]
                .as_str()
                .and_then(|t| tracker.piece(t, None))
            {
                on_token(ev);
            }
            if chunk["stop"].as_bool() == Some(true) {
                timings = Some(chunk["timings"].clone());
                break;
            }
        }
        let t = timings.ok_or("llama-server ended the stream without timings")?;
        let num = |k: &str| t[k].as_f64().unwrap_or(0.0);
        Ok(GenerationStats {
            prompt_tokens: num("prompt_n") as u32,
            output_tokens: num("predicted_n") as u32,
            thinking_tokens: tracker.thinking_pieces,
            load_ms: running.load_ms,
            prompt_eval_ms: num("prompt_ms"),
            generation_ms: num("predicted_ms"),
            time_to_first_token_ms: tracker
                .first_token_ms
                .unwrap_or_else(|| tracker.elapsed_ms()),
            time_to_first_visible_token_ms: tracker.first_visible_ms,
        })
    }

    fn unload(&mut self) {
        if let Some(mut r) = self.running.take() {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        self.unload();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_in_both_formats() {
        assert_eq!(parse_version("11669 (1a2b3c4)").as_deref(), Some("b11669"));
        assert_eq!(
            parse_version("0.7.0-dev (build 11669, commit 620d2225c)").as_deref(),
            Some("b11669")
        );
        assert_eq!(parse_version("").as_deref(), None);
    }

    #[test]
    fn device_list() {
        let text = "load_backend: loaded Vulkan backend\nAvailable devices:\n  Vulkan0: Intel(R) Iris(R) Xe Graphics (8083 MiB, 7423 MiB free)\n  CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7000 MiB free)\n";
        assert_eq!(
            parse_devices(text),
            [
                "Intel Iris Xe Graphics (Vulkan)",
                "NVIDIA GeForce RTX 4060 (CUDA)"
            ]
        );
        assert!(parse_devices("Available devices:\n").is_empty());
    }

    #[test]
    fn keys_are_random_and_long() {
        let a = random_key();
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_key());
    }

    #[test]
    fn missing_binary_is_a_clear_error() {
        let mut s = LlamaServer::new("definitely-not-llama-server-xyz");
        assert_eq!(s.version(), "unknown");
        let err = s
            .load(
                &ModelSpec {
                    id: "x".into(),
                    location: "x.gguf".into(),
                    sha256: None,
                },
                &LoadOptions::default(),
            )
            .unwrap_err();
        assert!(err.contains("could not start llama-server"), "{err}");
    }
}
