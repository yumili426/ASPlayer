# 本地翻译引擎（Ollama 模型下载渠道）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在「翻译」设置页提供 Ollama 本地翻译模型的探测 / 列表 / 流式下载渠道 + 一键接通，保留现有 OpenAI 兼容 API 翻译。

**Architecture:** 纯 HTTP 驱动（不碰 ollama CLI）：`GET /api/version`+`/api/tags` 探测列表、`POST /api/pull` 流式拉取（NDJSON 逐行）。Rust 新建 `ollama.rs` 复用 `models.rs` 的 `LazyLock<Mutex<state>>` + 后台线程 + `app.emit` 范式；前端新建 `api/ollama.ts`，在 `SettingsPanel.vue`「翻译」tab 叠加本地引擎区块。

**Tech Stack:** Tauri 2 + Rust（reqwest blocking / serde_json / rusqlite）+ Vue 3 TS。`reqwest` 的 `json` feature 已由 workspace 的 `asplayer-transcribe` 合并启用，`.json()` 可直接用。

**前提事实（设计稿已核实）：**
- `POST {base}/api/pull` body `{"model":"<name>"}`, `stream` 默认 true → NDJSON 逐行：`{"status":"pulling <digest>","digest":"...","total":u64,"completed":u64}`（进度行），`{"status":"success"}` / `{"status":"already exists"}`（完成），顶层 `{"error":"..."}`（失败）。
- `GET /api/tags` → `{"models":[{"name":"qwen2.5:7b","size":u64},...]}`；`GET /api/version` → `{"version":"..."}`。
- OpenAI 兼容端点 `{base}/v1/chat/completions`。

---

## 文件结构

- 创建 `app/src-tauri/src/ollama.rs`：类型 + 状态机 + 命令 + NDJSON 解析（含单测）。
- 修改 `app/src-tauri/src/lib.rs`：`mod ollama;` + `generate_handler` 注册 3 个命令。
- 创建 `app/src/api/ollama.ts`：invoke + 事件监听封装。
- 修改 `app/src/types.ts`：新增 `OllamaModel` / `PullState` / `OllamaStatus` / `OllamaProgress`。
- 修改 `app/src/components/SettingsPanel.vue`：「翻译」tab 新增「本地翻译引擎」区块。

设置不用 DB 迁移：`ollama_base` 走现有 `save_setting`/`get_setting`（settings 表 k/v）。

---

## 自审 / 校验清单

- `cargo build -p app --lib`、`cargo test -p asplayer-dict`（现有 10 个不挂）、`vue-tsc --noEmit`、`vite build` 全绿。
- 手动：本地引擎连接检测、推荐模型下载（进度+取消+完成）、已拉取列表、一键接通后翻译页回显、用本地模型「转写并翻译」实际出中文。

---

## Milestone 1：Rust `ollama.rs`

### Task 1: 纯函数（NDJSON 解析 + base 规范化）—— TDD

**Files:**
- Create: `app/src-tauri/src/ollama.rs`
- Test: 同文件 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试 + 最小骨架**

先建模块骨架，含类型与待测函数（先让编译过、测试挂）：

```rust
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
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p app --lib ollama`（在 `app/src-tauri` 下）
Expected: FAIL，因为 `serde_json` 尚未作为依赖引入？实际 serde_json 已是 app 依赖。此时 module 未注册，`cargo test -p app --lib` 不编译 ollama.rs（未 `mod`），所以先看下一步。若想立即验证测试内容，可临时 `mod ollama;` 注册（见 Task 3），此处保持不注册。

> 说明：Parse/state 单测在 Task 3 注册模块后一并运行（`cargo test -p app --lib ollama`）。Step 2 仅确认「未注册前这些测试尚未运行」符合预期，不强行先红后绿。真正坚持 TDD 的纯函数验证在 Task 3 Step 2 一起跑。若实现者要严格先红：给 `mod ollama;` 一个空模块即可，上面骨架已含全部需测函数，注册后 `cargo test -p app --lib ollama` 应全 PASS（首写即绿，因测试与实现同步给出）。此任务以「编译通过 + 测试逻辑正确」为准。

### Task 2: 命令 + 后台拉取主流程

**Files:**
- Modify: `app/src-tauri/src/ollama.rs`

- [ ] **Step 1: 追加 client、状态/进度广播、失败/完成/取消收口、命令与拉取线程**

在 Task 1 骨架之后追加：

```rust
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
```

- [ ] **Step 2: 编译通过（先不注册，纯 check）**

Run: `cargo build -p app --lib`（在 `app/src-tauri`）
由于 `mod ollama` 尚未声明，ollama.rs 不参与编译。为单独校验本文件语法，可临时在 `lib.rs` 加 `mod ollama;`（下一任务正式加上）。若想此刻仅校验，可先仅做 Step 3 一起验证。

### Task 3: 注册模块与命令

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: 声明模块**

在 lib.rs 顶部模块声明区（`mod dict;` 等附近）加：

```rust
mod ollama;
```

- [ ] **Step 2: 注册命令**

在 `invoke_handler(tauri::generate_handler![...])` 内、`dict::dict_lookup` 之后加：

```rust
            // 本地翻译引擎（Ollama）
            ollama::ollama_status,
            ollama::ollama_pull,
            ollama::cancel_ollama_pull
```

- [ ] **Step 3: 运行测试（首写即绿验证纯函数）+ 构建**

Run: `cargo build -p app --lib && cargo test -p app --lib ollama`
Expected: build 成功；`ollama` 测试全 PASS（parse_pull_line / base_of / pull_state_transitions 共 6 个）。现有 `asplayer-dict` 10 个测试仍绿。

---

## Milestone 2：前端

### Task 4: api/ollama.ts + types

**Files:**
- Create: `app/src/api/ollama.ts`
- Modify: `app/src/types.ts`

- [ ] **Step 1: 追加类型定义**

在 `app/src/types.ts` 末尾追加：

```ts
export interface OllamaModel { name: string; size: number }
export type PullStatus = "idle" | "downloading" | "done" | "failed" | "canceled";
export interface PullState {
  model: string | null;
  status: PullStatus;
  bytes: number;
  total: number;
  error?: string | null;
}
export interface OllamaStatus {
  connected: boolean;
  version?: string | null;
  models: OllamaModel[];
  pulling: PullState;
}
export interface OllamaProgress { model: string; bytes: number; total: number; percent: number }
```

- [ ] **Step 2: 创建 `app/src/api/ollama.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { OllamaProgress, OllamaStatus, PullState } from "../types";

export function ollamaStatus(): Promise<OllamaStatus> {
  return invoke<OllamaStatus>("ollama_status");
}

export function ollamaPull(model: string) {
  return invoke<void>("ollama_pull", { model });
}

export function ollamaPullCancel() {
  return invoke<boolean>("cancel_ollama_pull");
}

export function onOllamaStatus(cb: (s: PullState) => void): Promise<UnlistenFn> {
  return listen<PullState>("ollama://status", (ev) => cb(ev.payload));
}

export function onOllamaProgress(cb: (p: OllamaProgress) => void): Promise<UnlistenFn> {
  return listen<OllamaProgress>("ollama://progress", (ev) => cb(ev.payload));
}
```

- [ ] **Step 3: 校验**

Run: `vue-tsc --noEmit`（在 `app` 下，PATH 含 node 目录，用 `node_modules/.bin/vue-tsc`）
Expected: 类型检查通过（新类型/API 无引用错误）。

### Task 5: SettingsPanel「翻译」tab 本地引擎区块

**Files:**
- Modify: `app/src/components/SettingsPanel.vue`

- [ ] **Step 1: import 与脚本状态**

在 `<script setup>` 顶部 import 区（`import type { DictStatus, DictProgress } ...` 附近）加：

```ts
import { ollamaStatus, ollamaPull, ollamaPullCancel, onOllamaStatus, onOllamaProgress } from "../api/ollama";
import type { OllamaStatus, PullState, OllamaProgress } from "../types";
```

在 `const dictUrlTimer ...` 之后加本地引擎状态：

```ts
// ---- 本地翻译引擎（Ollama）----
const ollamaBase = ref("http://localhost:11434");
const ollamaBaseSaved = ref(false);
let ollamaTimer: ReturnType<typeof setTimeout> | null = null;
const ollamaInfo = ref<OllamaStatus | null>(null);
const ollamaPullState = ref<PullState | null>(null);
const ollamaProgress = ref<OllamaProgress | null>(null);
let ollamaInit = false;
const OLLAMA_RECOMMENDED = [
  { model: "qwen2.5:3b", label: "小 · 约 1.9 GB" },
  { model: "qwen2.5:7b", label: "中 · 约 4.7 GB" },
] as const;
```

- [ ] **Step 2: 订阅一次 + 刷新函数**

加：

```ts
async function initOllama() {
  if (ollamaInit) return;
  await onOllamaStatus((s) => (ollamaPullState.value = s));
  await onOllamaProgress((p) => (ollamaProgress.value = p));
  ollamaInit = true;
}

async function loadOllama() {
  try {
    ollamaInfo.value = await ollamaStatus();
  } catch {
    /* ignore */
  }
}

async function onPullLocal(model: string) {
  try {
    await ollamaPull(model);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 拉取本地翻译模型失败:", e);
  }
}

async function onCancelPullLocal() {
  try {
    await ollamaPullCancel();
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 取消拉取本地翻译模型失败:", e);
  }
}

async function onSaveOllamaBase() {
  try {
    await saveSettings({ ollama_base: ollamaBase.value });
    ollamaBaseSaved.value = true;
    if (ollamaTimer) clearTimeout(ollamaTimer);
    ollamaTimer = setTimeout(() => (ollamaBaseSaved.value = false), 1500);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 保存 Ollama 地址失败:", e);
  }
}

// 一键接通：把翻译配置指向本地模型（api_base 用 base+"/v1"，api_key 留空）
async function onUseLocal(model: string) {
  await saveSettings({
    api_base: ollamaBase.value.replace(/\/+$/, "") + "/v1",
    api_model: model,
    api_key: "",
  });
  apiBase.value = ollamaBase.value.replace(/\/+$/, "") + "/v1";
  apiModel.value = model;
  apiKey.value = "";
  providerIdx.value = -1; // 不匹配任何云端预设，落回「自定义」
}

const ollamaConnected = computed(() => !!ollamaInfo.value?.connected);
const localPulling = computed(() => ollamaPullState.value?.status === "downloading");
const localPercent = computed(() => {
  const s = ollamaPullState.value;
  if (!s || !s.total) return 0;
  return Math.min(100, Math.round((s.bytes / s.total) * 100));
});
```

在 `load()` 中（读 settings 处）加回显 ollama_base：

```ts
ollamaBase.value = s.ollama_base ?? "http://localhost:11434";
```

在 `watch(open)` 打开分支（在 `await loadDict();` 后）加：

```ts
await initOllama();
await loadOllama();
```

- [ ] **Step 3: 模板——「翻译」tab 加本地引擎区块**

在「翻译」section 内、`<button class="save-btn" ... @click="onSave">` 之后加：

```html
      <div class="section-divider"></div>
      <div class="section-label">本地翻译引擎（Ollama）</div>

      <label class="field">
        <span>Ollama 地址</span>
        <div class="key-wrap">
          <input v-model="ollamaBase" type="text" placeholder="http://localhost:11434" />
          <button class="key-eye" title="保存" @click="onSaveOllamaBase">保存</button>
        </div>
        <small class="field-desc">默认 http://localhost:11434，改端口后点「保存」。</small>
      </label>

      <div v-if="!ollamaConnected" class="local-warn">
        未检测到 Ollama 服务。请先<a href="https://ollama.com" target="_blank" rel="noopener">安装 Ollama</a> 并启动后点「重新检测」。
      </div>
      <div v-else class="local-ok">
        已连接 Ollama{{ ollamaInfo?.version ? `（v${ollamaInfo.version}）` : "" }}。
        <span v-if="ollamaInfo?.models.length">{{ ollamaInfo.models.length }} 个模型</span>
      </div>
      <div class="local-actions">
        <button class="rs-btn" @click="loadOllama">重新检测</button>
        <span v-if="ollamaBaseSaved" class="saved-hint">已保存</span>
      </div>

      <!-- 已拉取模型 -->
      <div v-if="ollamaInfo?.models.length" class="model-list">
        <div v-for="m in ollamaInfo.models" :key="m.name" class="sc-item">
          <span class="sc-label">{{ m.name }}</span>
          <span class="sc-controls">
            <button class="sc-key sel" :disabled="localPulling" @click="onUseLocal(m.name)">用这个翻译</button>
            <span class="dl-mid">{{ fmtBytes(m.size) }}</span>
          </span>
        </div>
      </div>

      <!-- 推荐模型下载 -->
      <div class="adv-item">推荐翻译模型</div>
      <div class="model-list">
        <div v-for="r in OLLAMA_RECOMMENDED" :key="r.model" class="sc-item">
          <span class="sc-label">{{ r.model }}</span>
          <span class="sc-controls">
            <template v-if="localPulling && ollamaPullState?.model === r.model">
              <span class="dl-mid">{{ localPercent }}%</span>
              <button class="sc-clear" title="取消" @click="onCancelPullLocal">×</button>
            </template>
            <template v-else>
              <span class="dl-mid">{{ r.label }}</span>
              <button class="sc-key" :disabled="localPulling" @click="onPullLocal(r.model)">下载</button>
            </template>
          </span>
        </div>
      </div>
      <div v-if="localPulling && ollamaPullState?.model && !OLLAMA_RECOMMENDED.some((r) => r.model === ollamaPullState?.model)" class="model-list">
        <div class="sc-item">
          <span class="sc-label">{{ ollamaPullState.model }}</span>
          <span class="sc-controls">
            <span class="dl-mid">{{ localPercent }}%</span>
            <button class="sc-clear" title="取消" @click="onCancelPullLocal">×</button>
          </span>
        </div>
      </div>

      <div v-if="localPulling && ollamaPullState" class="model-progress">
        <div class="model-bar" :style="{ width: localPercent + '%' }"></div>
      </div>
      <p v-if="ollamaPullState?.status === 'failed' && ollamaPullState.error" class="dict-err">{{ ollamaPullState.error }}</p>
```

- [ ] **Step 4: 样式**

在 `<style scoped>` 末尾、`.dict-err` 之后加：

```css
.section-divider {
  margin: 16px 0 12px;
  border-top: 1px solid var(--line);
  padding-top: 12px;
}
.local-warn {
  font-size: 12px;
  line-height: 1.5;
  color: var(--accent);
  background: var(--accent-dim);
  border: 1px solid var(--accent);
  border-radius: 9px;
  padding: 8px 12px;
  margin-bottom: 12px;
}
.local-warn a { color: var(--accent); }
.local-ok {
  font-size: 12px;
  color: var(--fg-2);
  margin-bottom: 10px;
}
.local-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
}
.local-actions .rs-btn { width: auto; padding: 6px 14px; flex: 0 0 auto; }
.local-actions .saved-hint { margin: 0; }
```

- [ ] **Step 5: 校验**

Run: `vue-tsc --noEmit && vite build`（`app` 下）
Expected: 类型检查 + 构建通过。

---

## Milestone 3：收尾

### Task 6: 全量验证 + squash

**Files:**
- None（只跑命令 + git）

- [ ] **Step 1: 全量构建与测试**

Run: `cargo build -p app --lib && cargo test -p asplayer-dict`（`app/src-tauri`）
Expected: build OK；`asplayer-dict` 10 测试全绿。

Run: `vue-tsc --noEmit && vite build`（`app`）
Expected: 类型 + 构建通过。

- [ ] **Step 2: 手动验证清单**

1. 设置→翻译→本地引擎：默认未检测到（未启动 Ollama）→ 启动 Ollama → 重新检测 → 显示「已连接」+ 版本。
2. 推荐模型「下载」→ 进度条 + 百分比增长 → success → 出现在「已拉取模型」列表。
3. 拉取中「取消」→ 状态复位。
4. 点某模型「用这个翻译」→ 翻译页 api_base/model/api_key 回显刷新（base=`http://localhost:11434/v1`，model=模型名，key 空）。
5. 对某媒体「转写并翻译」→ 走本地 Ollama 实际出中文。

- [ ] **Step 3: squash 成单个功能提交**

按 `[[feedback-commit-batching]]`：收尾把改动 squash 成一个 `feat(ollama): ...` 提交。仅包含 ollama.rs / lib.rs / api/ollama.ts / types.ts / SettingsPanel.vue / 本计划 + 设计稿；不混入 Cargo.lock / capabilities / 其它 WIP。
