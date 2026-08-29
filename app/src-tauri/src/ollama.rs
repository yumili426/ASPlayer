//! 本地翻译引擎：Ollama 模型探测 / 列表 / 流式下载（纯 HTTP 接口，不依赖 ollama CLI）。
//! 复用 models.rs 的 LazyLock<Mutex<state>> + 后台线程 + app.emit 范式。

use serde::Serialize;
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub const EVENT_STATUS: &str = "ollama://status";
pub const EVENT_PROGRESS: &str = "ollama://progress";
const SETTING_BASE: &str = "ollama_base";
const DEFAULT_BASE: &str = "http://localhost:11434";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PullStatus { Idle, Downloading, Done, Failed, Canceled }

#[derive(Debug, Clone, Serialize)]
pub struct PullState {
    pub model: Option<String>,
    pub status: PullStatus,
    pub bytes: u64,
    pub total: u64,
    pub error: Option<String>,
}

impl Default for PullState {
    fn default() -> Self {
        Self { model: None, status: PullStatus::Idle, bytes: 0, total: 0, error: None }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaModel { pub name: String, pub size: u64 }

#[derive(Debug, Clone, Serialize)]
pub struct OllamaStatus {
    pub connected: bool,
    pub version: Option<String>,
    pub models: Vec<OllamaModel>,
    pub pulling: Option<PullState>,
}

#[derive(Debug, Clone, Serialize)]
struct PullProgress { model: String, bytes: u64, total: u64, percent: u8 }

static STATE: LazyLock<Mutex<PullState>> = LazyLock::new(|| Mutex::new(PullState::default()));
static ACTIVE: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));
static CANCEL: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

fn with_state<R>(f: impl FnOnce(&mut PullState) -> R) -> Option<R> {
    STATE.lock().ok().map(|mut g| f(&mut g))
}
fn current_state() -> PullState {
    STATE.lock().ok().map(|g| g.clone()).unwrap_or_default()
}
fn is_canceled() -> bool { CANCEL.lock().map(|g| *g).unwrap_or(false) }
fn request_cancel() { if let Ok(mut g) = CANCEL.lock() { *g = true; } }
fn clear_cancel() { if let Ok(mut g) = CANCEL.lock() { *g = false; } }
fn release_active() { if let Ok(mut g) = ACTIVE.lock() { *g = None; } }

/// 规范化设置值：空/空白 → 默认 base
fn base_of(raw: Option<String>) -> String {
    match raw {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => DEFAULT_BASE.to_string(),
    }
}

#[derive(Debug, PartialEq)]
enum PullLine {
    Progress { total: u64, completed: u64 },
    Done,
    Error(String),
    Other,
}

/// 解析 /api/pull 的一行 NDJSON。容错：status 字符串 + 可选 total/completed + 顶层 error。
fn parse_pull_line(line: &str) -> PullLine {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return PullLine::Other;
    };
    if let Some(msg) = v.get("error").and_then(|e| e.as_str()) {
        return PullLine::Error(msg.to_string());
    }
    if v.get("total").and_then(|t| t.as_u64()).is_some() {
        let total = v.get("total").and_then(|t| t.as_u64()).unwrap_or(0);
        let completed = v.get("completed").and_then(|c| c.as_u64()).unwrap_or(0);
        return PullLine::Progress { total, completed };
    }
    if let Some(st) = v.get("status").and_then(|s| s.as_str()) {
        if st == "success" || st == "already exists" {
            return PullLine::Done;
        }
    }
    PullLine::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progress_line() {
        let line = r#"{"status":"pulling sha256:abc","digest":"sha256:abc","total":2142590208,"completed":241970}"#;
        assert_eq!(parse_pull_line(line), PullLine::Progress { total: 2142590208, completed: 241970 });
    }

    #[test]
    fn parse_missing_completed_defaults_zero() {
        assert_eq!(parse_pull_line(r#"{"status":"pulling sha256:x","total":1000}"#), PullLine::Progress { total: 1000, completed: 0 });
    }

    #[test]
    fn parse_success_and_error() {
        assert_eq!(parse_pull_line(r#"{"status":"success"}"#), PullLine::Done);
        assert_eq!(parse_pull_line(r#"{"status":"already exists"}"#), PullLine::Done);
        assert_eq!(parse_pull_line(r#"{"error":"pull model manifest: not found"}"#), PullLine::Error("pull model manifest: not found".into()));
    }

    #[test]
    fn parse_other_lines() {
        assert_eq!(parse_pull_line(r#"{"status":"pulling manifest"}"#), PullLine::Other);
        assert_eq!(parse_pull_line("garbage"), PullLine::Other);
        assert_eq!(parse_pull_line(""), PullLine::Other);
    }

    #[test]
    fn base_of_defaults_empty() {
        assert_eq!(base_of(None), DEFAULT_BASE);
        assert_eq!(base_of(Some("".into())), DEFAULT_BASE);
        assert_eq!(base_of(Some("  ".into())), DEFAULT_BASE);
        assert_eq!(base_of(Some("http://127.0.0.1:11434".into())), "http://127.0.0.1:11434");
    }

    #[test]
    fn pull_state_transitions() {
        clear_cancel();
        with_state(|d| { d.status = PullStatus::Downloading; d.error = Some("x".into()); });
        let s = current_state();
        assert_eq!(s.status, PullStatus::Downloading);
        assert_eq!(s.error.as_deref(), Some("x"));
        request_cancel();
        assert!(is_canceled());
        clear_cancel();
        assert!(!is_canceled());
    }
}

use crate::{AppState, CmdResult};
use std::io::{BufRead, BufReader};
use std::time::Instant;
use serde::Deserialize;

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

fn resolve_base(app: &AppHandle) -> String {
    let setting = app
        .state::<AppState>()
        .db
        .lock()
        .ok()
        .and_then(|db| db.get_setting(SETTING_BASE).ok().flatten());
    base_of(setting)
}

fn emit_status(app: &AppHandle) {
    let _ = app.emit(EVENT_STATUS, current_state());
}

fn emit_progress(app: &AppHandle, model: &str, bytes: u64, total: u64) {
    let percent = if total > 0 {
        ((bytes as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u8
    } else {
        0
    };
    let _ = app.emit(EVENT_PROGRESS, PullProgress {
        model: model.to_string(),
        bytes,
        total,
        percent,
    });
}

fn finish_done(app: &AppHandle) {
    with_state(|d| { d.status = PullStatus::Done; d.error = None; });
    emit_status(app);
    release_active();
}

fn finish_fail(app: &AppHandle, err: Option<String>) {
    with_state(|d| { d.status = PullStatus::Failed; d.error = err; });
    emit_status(app);
    release_active();
}

fn finish_cancel(app: &AppHandle) {
    clear_cancel();
    with_state(|d| { d.status = PullStatus::Canceled; d.error = None; });
    emit_status(app);
    release_active();
}

#[derive(Deserialize)]
struct VersionResp { version: String }
#[derive(Deserialize)]
struct TagModel { name: String, size: u64 }
#[derive(Deserialize)]
struct TagsResp { models: Vec<TagModel> }

#[tauri::command]
pub fn ollama_status(app: AppHandle) -> CmdResult<OllamaStatus> {
    let c = client();
    let b = resolve_base(&app).trim_end_matches('/').to_string();
    let mut out = OllamaStatus {
        connected: false,
        version: None,
        models: Vec::new(),
        pulling: Some(current_state()),
    };
    if let Ok(resp) = c.get(format!("{b}/api/version")).send() {
        if let Ok(v) = resp.json::<VersionResp>() {
            out.version = Some(v.version);
            out.connected = true;
        }
    }
    if let Ok(resp) = c.get(format!("{b}/api/tags")).send() {
        if let Ok(t) = resp.json::<TagsResp>() {
            out.connected = true;
            out.models = t.models.into_iter()
                .map(|m| OllamaModel { name: m.name, size: m.size })
                .collect();
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn ollama_pull(model: String, app: AppHandle) -> CmdResult<()> {
    if model.trim().is_empty() { return Err("模型名为空".into()); }
    {
        let mut g = ACTIVE.lock().map_err(e2)?;
        if g.is_some() { return Err("已有模型拉取在进行中".into()); }
        *g = Some(model.clone());
    }
    clear_cancel();
    with_state(|d| {
        d.model = Some(model.clone());
        d.status = PullStatus::Downloading;
        d.bytes = 0;
        d.total = 0;
        d.error = None;
    });
    emit_status(&app); // 立即广播，避免 UI 等流式首行才看到变化
    std::thread::spawn(move || pull_inner(app, model));
    Ok(())
}

#[tauri::command]
pub fn cancel_ollama_pull() -> CmdResult<bool> {
    if current_state().status != PullStatus::Downloading { return Ok(false); }
    request_cancel();
    Ok(true)
}

/// 后台拉取：POST /api/pull 流式读 NDJSON，逐行更新进度，success/error 收口。
fn pull_inner(app: AppHandle, model: String) {
    let c = client();
    let b = resolve_base(&app).trim_end_matches('/').to_string();
    let body = serde_json::json!({ "model": model });
    let resp = c.post(format!("{b}/api/pull")).json(&body).send();
    let Ok(resp) = resp else {
        finish_fail(&app, Some("请求失败".into()));
        return;
    };
    if !resp.status().is_success() {
        finish_fail(&app, Some(format!("HTTP {}", resp.status())));
        return;
    }
    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    let mut last = Instant::now();
    let mut saw_success = false;
    loop {
        if is_canceled() { finish_cancel(&app); return; }
        line.clear();
        let n = reader.read_line(&mut line).unwrap_or(0);
        if n == 0 { break; }
        let ln = line.trim();
        if ln.is_empty() { continue; }
        match parse_pull_line(ln) {
            PullLine::Progress { total, completed } => {
                with_state(|d| { d.total = total; d.bytes = completed; });
                if last.elapsed().as_millis() >= 250 {
                    emit_progress(&app, &model, completed, total);
                    last = Instant::now();
                }
            }
            PullLine::Done => saw_success = true,
            PullLine::Error(msg) => { finish_fail(&app, Some(msg)); return; }
            PullLine::Other => {}
        }
    }
    if saw_success {
        finish_done(&app);
    } else if is_canceled() {
        finish_cancel(&app);
    } else {
        finish_fail(&app, Some("流式响应意外中断".into()));
    }
}

fn e2<E: std::fmt::Display>(e: E) -> String { format!("{e}") }
