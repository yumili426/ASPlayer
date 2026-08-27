# 模型下载器 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在应用内下载 whisper.cpp 五档 GGUF 模型（含进度/断点续传/可取消/校验），并把「选中的模型档位」接入 `model_path()` 优先级，设置面板新增「模型」卡片与无模型引导。

**Architecture:** 延续「Rust 是唯一事实来源」：新建 `models.rs` 用 `reqwest::blocking` 同步流式下载（后台线程），事件经 `app.emit` 广播 `model://*`；选中档持久化到 DB `settings.whisper_model`，`model_path()` 优先级改为环境变量 > DB 设置 > 默认 small。前端用 Pinia store 订阅事件驱动「模型」卡片。

**Tech Stack:** Tauri 2 (Rust) + reqwest 0.12(blocking) + rusqlite；Vue 3 + Pinia + vue-tsc。

**基准**：设计文档 `docs/superpowers/specs/2026-08-27-model-downloader-design.md`。转写管线为同步 `std::thread::spawn`，模型下载保持一致。

---

## 文件结构

| 文件 | 责任 | 动作 |
|---|---|---|
| `app/src-tauri/src/models.rs` | 模型下载引擎 + 状态 + 路径解析 + 5 条 Tauri 命令 | **Created** |
| `app/src-tauri/src/lib.rs` | 声明 `mod models` + 注册命令 | Modify |
| `app/src-tauri/src/transcriber.rs` | 删除旧 `model_path()`，改用 `models::resolve_model_path(&db)` | Modify |
| `app/src-tauri/Cargo.toml` | 追加 `reqwest = { version = "0.12", features = ["blocking"] }` | Modify |
| `app/src/types.ts` | `ModelStatus` / `ModelProgress` 接口 | Modify |
| `app/src/api/model.ts` | invoke 封装 + 事件订阅 | **Created** |
| `app/src/stores/model.ts` | 模型 state + 动作 + 事件订阅 | **Created** |
| `app/src/components/SettingsPanel.vue` | 新增「模型」tab 卡片 | Modify |

**依赖顺序**：Task 1（路径/状态就位并可编译）→ Task 2（reqwest 依赖）→ Task 3（校验/状态，TDD）→ Task 4（下载引擎）→ Task 5（命令接线）→ Task 6/7（前端）→ Task 8（验收）。

> 约定：`cargo` 命令在仓库根 `d:/Coding Projects/ASPlayer` 执行；前端命令在 `app/` 执行。事件载荷字段用 snake_case（图省事且与 `MediaItem` 一致）；`DlStatus` 枚举经 `#[serde(rename_all = "lowercase")]` 序列化为 `"downloading"|"done"|"failed"|"canceled"|"idle"`。

---

### Task 1: models.rs 路径解析 + 状态脚手架（打通 model_path 优先级）

**Files:**
- Create: `app/src-tauri/src/models.rs`
- Modify: `app/src-tauri/src/lib.rs`（加 `mod models;`）
- Modify: `app/src-tauri/src/transcriber.rs:78-87`（删 `model_path()`）、`transcriber.rs:178`（改用新函数）
- Test: `app/src-tauri/src/models.rs` 内 `#[cfg(test)]`

- [ ] **Step 1: 写失败测试**（先建文件，只放测试）

在 `app/src-tauri/src/models.rs` 写入：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MediaDb;

    #[test]
    fn model_file_path_format() {
        let dir = std::path::PathBuf::from("C:/ml");
        assert_eq!(model_file_path(&dir, "small"), dir.join("ggml-small.bin"));
        assert_eq!(model_file_path(&dir, "base"), dir.join("ggml-base.bin"));
    }

    #[test]
    fn selected_size_defaults_small() {
        let db = MediaDb::open_in_memory().unwrap();
        assert_eq!(selected_size(&db), "small");
    }

    #[test]
    fn selected_size_from_setting() {
        let db = MediaDb::open_in_memory().unwrap();
        db.save_setting("whisper_model", "base").unwrap();
        assert_eq!(selected_size(&db), "base");
    }
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd "d:/Coding Projects/ASPlayer" && cargo test -p app models -- --nocapture`
Expected: 编译失败（`models` 模块未声明 / `model_file_path`/`selected_size` 未定义 / 找不到模块）。

- [ ] **Step 3: 声明模块 + 写实现**

在 `app/src-tauri/src/lib.rs` 顶部（第 1 行 `mod db;` 附近）加：

```rust
mod models;
```

把下面内容**整段**追加到 `models.rs`（覆盖 Step 1 的测试区，测试保留并扩为完整文件）：

```rust
//! 模型下载器：whisper.cpp 五档 GGUF 模型的下载/校验/取消/选择。
//! 同步流式下载在后台线程跑，事件经 `app.emit` 广播，DB 设置 `whisper_model` 存选中档。

use crate::AppState;
use crate::db::MediaDb;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

pub const MODEL_SIZES: [&str; 5] = ["tiny", "base", "small", "medium", "large-v3"];
const SETTING_MODEL: &str = "whisper_model";
const MAGIC: &[u8] = b"ggml";
const URL_OFFICIAL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin";
const URL_MIRROR: &str = "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin";

const EVENT_PROGRESS: &str = "model://progress";
const EVENT_DONE: &str = "model://done";
const EVENT_ERROR: &str = "model://error";
const EVENT_CANCELED: &str = "model://canceled";
const EVENT_SELECTED: &str = "model://selected";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DlStatus {
    Idle,
    Downloading,
    Done,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Download {
    pub size: String,
    pub status: DlStatus,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub size: String,
    pub file_exists: bool,
    pub file_bytes: u64,
    pub selected: bool,
    pub status: DlStatus,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelProgress {
    size: String,
    bytes_downloaded: u64,
    total_bytes: u64,
    percent: u8,
}

#[derive(Debug, Clone, Serialize)]
struct ModelError {
    size: String,
    error: String,
}

static DOWNLOADS: LazyLock<Mutex<HashMap<String, Download>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));
static CANCEL: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 模型根目录：~/.asplayer/models
pub fn models_dir() -> PathBuf {
    home().join(".asplayer").join("models")
}

/// ggml-{size}.bin 的完整路径
pub fn model_file_path(dir: &Path, size: &str) -> PathBuf {
    dir.join(format!("ggml-{size}.bin"))
}

/// 从 DB 读选中档；未设置回退 "small"
fn selected_size(db: &MediaDb) -> String {
    db.get_setting(SETTING_MODEL)
        .ok()
        .flatten()
        .unwrap_or_else(|| "small".into())
}

/// 解析模型路径：环境变量 ASPLAYER_MODEL > DB 设置 whisper_model > 默认小模型
pub fn resolve_model_path(db: &MediaDb) -> PathBuf {
    if let Some(m) = std::env::var_os("ASPLAYER_MODEL") {
        if !m.is_empty() {
            return PathBuf::from(m);
        }
    }
    model_file_path(&models_dir(), &selected_size(db))
}

fn e2<E: std::fmt::Display>(e: E) -> String {
    format!("{e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MediaDb;

    #[test]
    fn model_file_path_format() {
        let dir = std::path::PathBuf::from("C:/ml");
        assert_eq!(model_file_path(&dir, "small"), dir.join("ggml-small.bin"));
        assert_eq!(model_file_path(&dir, "base"), dir.join("ggml-base.bin"));
    }

    #[test]
    fn selected_size_defaults_small() {
        let db = MediaDb::open_in_memory().unwrap();
        assert_eq!(selected_size(&db), "small");
    }

    #[test]
    fn selected_size_from_setting() {
        let db = MediaDb::open_in_memory().unwrap();
        db.save_setting("whisper_model", "base").unwrap();
        assert_eq!(selected_size(&db), "base");
    }
}
```

- [ ] **Step 4: 改 transcriber.rs，删旧函数、换调用点**

删除 `transcriber.rs:78-87` 的旧 `model_path()` 整个函数。

把 `transcriber.rs:178` 的：

```rust
    let model = model_path().to_string_lossy().into_owned();
```

替换为：

```rust
    let model = {
        let g = db.lock().map_err(|e| fail(db, media_id, format!("数据库锁异常: {e}")))?;
        crate::models::resolve_model_path(&g).to_string_lossy().into_owned()
    };
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cd "d:/Coding Projects/ASPlayer" && cargo test -p app models -- --nocapture`
Expected: 3 个测试 PASS。`db`、`transcriber` 现有测试仍通过（可 `cargo test -p app` 全量确认）。

- [ ] **Step 6: 提交**

```bash
cd "d:/Coding Projects/ASPlayer"
git add app/src-tauri/src/models.rs app/src-tauri/src/lib.rs app/src-tauri/src/transcriber.rs
git commit -m "feat(models): 模块脚手架 + model_path 优先级接入 DB 设置
环境变量 ASPLAYER_MODEL > DB whisper_model > 默认 small"

```

---

### Task 2: 追加 reqwest 依赖

**Files:**
- Modify: `app/src-tauri/Cargo.toml`

- [ ] **Step 1: 加依赖**

在 `app/src-tauri/Cargo.toml` 的 `[dependencies]` 段（`anyhow = "1"` 之后）加：

```toml
reqwest = { version = "0.12", features = ["blocking"] }
```

- [ ] **Step 2: 编译确认**

Run: `cd "d:/Coding Projects/ASPlayer" && cargo build -p app`
Expected: 编译成功（reqwest 与 asplayer-transcribe 共享同版本，无重复编译冲突）。

- [ ] **Step 3: 提交**

```bash
cd "d:/Coding Projects/ASPlayer"
git add app/src-tauri/Cargo.toml
git commit -m "chore(models): 追加 reqwest blocking 依赖"
```

---

### Task 3: 下载状态辅助 + 校验逻辑（TDD）

**Files:**
- Modify: `app/src-tauri/src/models.rs`
- Test: `app/src-tauri/src/models.rs` 内 `#[cfg(test)]`

- [ ] **Step 1: 写失败测试**

在 `models.rs` 的 `mod tests` 里追加：

```rust
    #[test]
    fn verify_detects_magic() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good");
        std::fs::write(&good, b"ggml###payload").unwrap();
        assert!(verify_file(&good));

        let bad = dir.path().join("bad");
        std::fs::write(&bad, b"XXXX###payload").unwrap();
        assert!(!verify_file(&bad));

        let empty = dir.path().join("empty");
        std::fs::write(&empty, b"").unwrap();
        assert!(!verify_file(&empty));
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cd "d:/Coding Projects/ASPlayer" && cargo test -p app models::tests::verify_detects_magic -- --nocapture`
Expected: FAIL（`verify_file` 未定义）。

- [ ] **Step 3: 实现状态辅助与校验**

在 `models.rs`（`fn e2` 之前）追加：

```rust
/// 校验 GGUF：非空 + 头 4 字节魔数 == b"ggml"
fn verify_file(path: &Path) -> bool {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4];
    f.read_exact(&mut head).is_ok() && &head == MAGIC
}

/// 对某档状态做一次改造（不存在则插入默认 Idle 档）
fn with_dl<R>(size: &str, f: impl FnOnce(&mut Download) -> R) -> Option<R> {
    let mut g = DOWNLOADS.lock().ok()?;
    Some(f(g.entry(size.to_string()).or_insert_with(|| Download {
        size: size.to_string(),
        status: DlStatus::Idle,
        bytes_downloaded: 0,
        total_bytes: 0,
        error: None,
    })))
}

/// 读取某档（深拷贝），无记录则 None
fn get_dl(size: &str) -> Option<Download> {
    DOWNLOADS.lock().ok().and_then(|g| g.get(size).cloned())
}

/// 是否收到取消请求
fn is_canceled(size: &str) -> bool {
    CANCEL.lock().map(|g| g.contains(size)).unwrap_or(false)
}

fn request_cancel(size: &str) {
    if let Ok(mut g) = CANCEL.lock() {
        g.insert(size.to_string());
    }
}

fn clear_cancel(size: &str) {
    if let Ok(mut g) = CANCEL.lock() {
        g.remove(size);
    }
}

fn release_active(size: &str) {
    if let Ok(mut g) = ACTIVE.lock() {
        if g.as_deref() == Some(size) {
            *g = None;
        }
    }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cd "d:/Coding Projects/ASPlayer" && cargo test -p app models -- --nocapture`
Expected: 4 个测试全 PASS。

- [ ] **Step 5: 提交**

```bash
cd "d:/Coding Projects/ASPlayer"
git add app/src-tauri/src/models.rs
git commit -m "feat(models): 下载状态容器与 GGML 魔数校验"
```

---

### Task 4: 同步流式下载引擎（含断点/取消/回退）

**Files:**
- Modify: `app/src-tauri/src/models.rs`

> 本任务为网络下载，无法单测；用「编译 + 手动验收」（Task 8）覆盖。代码必须可编译。

- [ ] **Step 1: 追加下载引擎**

在 `models.rs`（`fn release_active` 之后、`fn e2` 之前；若 `e2` 已提前，则紧跟在其后）追加：

```rust
enum StreamErr {
    Canceled,
    Network(String),
}

/// 流式下载单条源。`cur` 为断点字节；成功返回内容总长。
fn stream_download(
    client: &reqwest::blocking::Client,
    url: &str,
    size: &str,
    part: &Path,
    cur: &mut u64,
    app: &AppHandle,
) -> Result<u64, StreamErr> {
    let mut req = client.get(url);
    if *cur > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={}-", cur));
    }
    let resp = req.send().map_err(|e| StreamErr::Network(format!("请求失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(StreamErr::Network(format!("HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(0);
    with_dl(size, |d| d.total_bytes = total);

    // 206 为断点续传（追加）；服务器忽略 Range 返回 200 则从头覆盖
    let resume = *cur > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut file = if resume {
        std::fs::OpenOptions::new().create(true).append(true).open(part)
            .map_err(|e| StreamErr::Network(format!("写入失败: {e}")))?
    } else {
        *cur = 0;
        std::fs::File::create(part).map_err(|e| StreamErr::Network(format!("写入失败: {e}")))?
    };

    let mut reader = resp; // blocking Response 实现 Read
    let mut buf = [0u8; 64 * 1024];
    let mut last = Instant::now();
    loop {
        if is_canceled(size) {
            return Err(StreamErr::Canceled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| StreamErr::Network(format!("读取失败: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| StreamErr::Network(format!("写入失败: {e}")))?;
        *cur += n as u64;
        with_dl(size, |d| d.bytes_downloaded = *cur);
        if last.elapsed().as_millis() >= 250 {
            emit_progress(app, size, *cur, total);
            last = Instant::now();
        }
    }
    Ok(total)
}

fn emit_progress(app: &AppHandle, size: &str, bytes: u64, total: u64) {
    let percent = if total > 0 {
        ((bytes as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u8
    } else {
        0
    };
    let _ = app.emit(
        EVENT_PROGRESS,
        ModelProgress {
            size: size.into(),
            bytes_downloaded: bytes,
            total_bytes: total,
            percent,
        },
    );
}

/// 后台下载主流程：官方 → 镜像自动回退，校验通过即完工，任一路由取消则终止。
fn download_model_inner(app: AppHandle, size: String) {
    let target = model_file_path(&models_dir(), &size);
    // 已完整且校验通过 → 直接 Done（幂等）
    if target.is_file() && verify_file(&target) {
        let bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        with_dl(&size, |d| {
            d.status = DlStatus::Done;
            d.error = None;
            d.bytes_downloaded = bytes;
            d.total_bytes = bytes;
        });
        let _ = app.emit(EVENT_DONE, &size);
        release_active(&size);
        return;
    }

    let part = target.with_extension("bin.part");
    let _ = std::fs::create_dir_all(models_dir());
    let mut cur = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    with_dl(&size, |d| {
        d.status = DlStatus::Downloading;
        d.bytes_downloaded = cur;
        d.error = None;
    });

    let client = reqwest::blocking::Client::new();
    let urls = [URL_OFFICIAL.replace("{size}", &size), URL_MIRROR.replace("{size}", &size)];
    let mut last_err: Option<String> = None;
    let mut done = false;

    for url in urls {
        if is_canceled(&size) {
            break;
        }
        match stream_download(&client, &url, &size, &part, &mut cur, &app) {
            Ok(total) => {
                if is_canceled(&size) {
                    break;
                }
                if verify_file(&part) && total > 0 && cur == total {
                    if std::fs::rename(&part, &target).is_ok() {
                        done = true;
                        break;
                    }
                    last_err = Some("重命名模型文件失败".into());
                } else {
                    last_err = Some("校验失败（文件大小或魔数头不符）".into());
                }
            }
            Err(StreamErr::Canceled) => break,
            Err(StreamErr::Network(e)) => last_err = Some(e),
        }
    }

    if done {
        let bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        with_dl(&size, |d| {
            d.status = DlStatus::Done;
            d.bytes_downloaded = bytes;
            d.total_bytes = bytes;
            d.error = None;
        });
        let _ = app.emit(EVENT_DONE, &size);
    } else if is_canceled(&size) {
        let _ = std::fs::remove_file(&part);
        clear_cancel(&size);
        with_dl(&size, |d| {
            d.status = DlStatus::Canceled;
            d.error = None;
        });
        let _ = app.emit(EVENT_CANCELED, &size);
    } else {
        with_dl(&size, |d| {
            d.status = DlStatus::Failed;
            d.error = last_err.clone();
        });
        let _ = app.emit(
            EVENT_ERROR,
            ModelError {
                size: size.clone(),
                error: last_err.unwrap_or_else(|| "下载失败".into()),
            },
        );
        // 保留 .part，下次续传
    }
    release_active(&size);
}
```

- [ ] **Step 2: 编译确认**

Run: `cd "d:/Coding Projects/ASPlayer" && cargo build -p app`
Expected: 编译成功。

- [ ] **Step 3: 提交**

```bash
cd "d:/Coding Projects/ASPlayer"
git add app/src-tauri/src/models.rs
git commit -m "feat(models): 同步流式下载引擎（断点续传/取消/官方→镜像回退/校验）"
```

---

### Task 5: 5 条 Tauri 命令 + 注册

**Files:**
- Modify: `app/src-tauri/src/models.rs`（追命令）
- Modify: `app/src-tauri/src/lib.rs`（`invoke_handler` 注册）

- [ ] **Step 1: 追加命令**

在 `models.rs` 末尾（`fn e2` 之后，测试模块之前；也可放模块末尾）追加：

```rust
#[tauri::command]
pub fn get_models_status(state: State<AppState>) -> Result<Vec<ModelStatus>, String> {
    let db = state.db.lock().map_err(e2)?;
    let sel = selected_size(&db);
    let mut out = Vec::new();
    for size in MODEL_SIZES {
        let f = model_file_path(&models_dir(), size);
        let (file_exists, file_bytes) = match std::fs::metadata(&f) {
            Ok(m) => (true, m.len()),
            Err(_) => (false, 0),
        };
        let dl = get_dl(size);
        out.push(ModelStatus {
            size: size.into(),
            file_exists,
            file_bytes,
            selected: sel == size,
            status: dl.as_ref().map(|d| d.status.clone()).unwrap_or(DlStatus::Idle),
            bytes_downloaded: dl.as_ref().map(|d| d.bytes_downloaded).unwrap_or(file_bytes),
            total_bytes: dl.as_ref().map(|d| d.total_bytes).unwrap_or(0),
            error: dl.as_ref().and_then(|d| d.error.clone()),
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn download_model(size: String, app: AppHandle) -> Result<(), String> {
    if !MODEL_SIZES.contains(&size.as_str()) {
        return Err(format!("未知模型档位: {size}"));
    }
    {
        let mut g = ACTIVE.lock().map_err(e2)?;
        if g.is_some() {
            return Err("已有模型下载在进行中".into());
        }
        *g = Some(size.clone());
    }
    clear_cancel(&size);
    with_dl(&size, |d| {
        d.status = DlStatus::Downloading;
        d.bytes_downloaded =
            std::fs::metadata(&model_file_path(&models_dir(), &size)).map(|m| m.len()).unwrap_or(0);
        d.error = None;
    });
    std::thread::spawn(move || download_model_inner(app, size));
    Ok(())
}

#[tauri::command]
pub fn cancel_model_download(size: String) -> Result<bool, String> {
    if !MODEL_SIZES.contains(&size.as_str()) {
        return Err(format!("未知模型档位: {size}"));
    }
    let st = get_dl(&size).map(|d| d.status).unwrap_or(DlStatus::Idle);
    if st != DlStatus::Downloading {
        return Ok(false);
    }
    request_cancel(&size);
    Ok(true)
}

#[tauri::command]
pub fn set_model(size: String, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    if !MODEL_SIZES.contains(&size.as_str()) {
        return Err(format!("未知模型档位: {size}"));
    }
    let db = state.db.lock().map_err(e2)?;
    db.save_setting(SETTING_MODEL, &size).map_err(e2)?;
    let _ = app.emit(EVENT_SELECTED, &size);
    Ok(())
}

#[tauri::command]
pub fn remove_model(size: String) -> Result<(), String> {
    if !MODEL_SIZES.contains(&size.as_str()) {
        return Err(format!("未知模型档位: {size}"));
    }
    let f = model_file_path(&models_dir(), &size);
    match std::fs::remove_file(&f) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("删除模型失败: {e}")),
    }
}
```

- [ ] **Step 2: 注册命令**

在 `app/src-tauri/src/lib.rs` 的 `invoke_handler(tauri::generate_handler![ ... ])`（第 263-292 行）末尾，`floating::push_overlay_subtitle` 之后、`]` 之前追加：

```rust
            // M4 模型下载器
            models::get_models_status,
            models::download_model,
            models::cancel_model_download,
            models::set_model,
            models::remove_model
```

- [ ] **Step 3: 编译确认**

Run: `cd "d:/Coding Projects/ASPlayer" && cargo build -p app`
Expected: 编译成功。

- [ ] **Step 4: 提交**

```bash
cd "d:/Coding Projects/ASPlayer"
git add app/src-tauri/src/models.rs app/src-tauri/src/lib.rs
git commit -m "feat(models): 5 条 Tauri 命令并注册（状态/下载/取消/选中/删除）"
```

---

### Task 6: 前端类型 + api/model.ts + stores/model.ts

**Files:**
- Modify: `app/src/types.ts`
- Create: `app/src/api/model.ts`
- Create: `app/src/stores/model.ts`

- [ ] **Step 1: types.ts 追加接口**

在 `app/src/types.ts` 末尾追加：

```ts
export interface ModelStatus {
  size: string;
  file_exists: boolean;
  file_bytes: number;
  selected: boolean;
  status: "downloading" | "done" | "failed" | "canceled" | "idle";
  bytes_downloaded: number;
  total_bytes: number;
  error: string | null;
}

export interface ModelProgress {
  size: string;
  bytes_downloaded: number;
  total_bytes: number;
  percent: number;
}
```

- [ ] **Step 2: 新建 `app/src/api/model.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ModelProgress, ModelStatus } from "../types";

export function getModelsStatus(): Promise<ModelStatus[]> {
  return invoke<ModelStatus[]>("get_models_status");
}

export function downloadModel(size: string) {
  return invoke<void>("download_model", { size });
}

export function cancelModelDownload(size: string) {
  return invoke<boolean>("cancel_model_download", { size });
}

export function setModel(size: string) {
  return invoke<void>("set_model", { size });
}

export function removeModel(size: string) {
  return invoke<void>("remove_model", { size });
}

export function onModelProgress(cb: (e: ModelProgress) => void): Promise<UnlistenFn> {
  return listen<ModelProgress>("model://progress", (ev) => cb(ev.payload));
}

export function onModelDone(cb: (size: string) => void): Promise<UnlistenFn> {
  return listen<string>("model://done", (ev) => cb(ev.payload));
}

export function onModelError(
  cb: (e: { size: string; error: string }) => void
): Promise<UnlistenFn> {
  return listen<{ size: string; error: string }>("model://error", (ev) => cb(ev.payload));
}

export function onModelCanceled(cb: (size: string) => void): Promise<UnlistenFn> {
  return listen<string>("model://canceled", (ev) => cb(ev.payload));
}

export function onModelSelected(cb: (size: string) => void): Promise<UnlistenFn> {
  return listen<string>("model://selected", (ev) => cb(ev.payload));
}
```

- [ ] **Step 3: 新建 `app/src/stores/model.ts`**

```ts
import { reactive } from "vue";
import {
  cancelModelDownload,
  downloadModel,
  getModelsStatus,
  onModelCanceled,
  onModelDone,
  onModelError,
  onModelProgress,
  onModelSelected,
  removeModel,
  setModel,
} from "../api/model";
import type { ModelStatus } from "../types";

/** 五档体积估算（仅用于展示） */
export const MODEL_META: Record<string, string> = {
  tiny: "75MB",
  base: "142MB",
  small: "466MB",
  medium: "1.5GB",
  "large-v3": "3.1GB",
};

export const modelState = reactive<{
  models: ModelStatus[];
  selected: string;
  loading: boolean;
  activeSize: string | null;
}>({
  models: [],
  selected: "small",
  loading: false,
  activeSize: null,
});

let initialized = false;

/** 首次打开面板时调用一次：订阅后台事件并拉取一次状态 */
export async function initModel() {
  if (initialized) return;
  initialized = true;
  await onModelProgress((p) => {
    const m = modelState.models.find((x) => x.size === p.size);
    if (m) {
      m.bytes_downloaded = p.bytes_downloaded;
      m.total_bytes = p.total_bytes;
    }
  });
  await onModelDone((size) => settle(size, "done", null));
  await onModelError((e) => settle(e.size, "failed", e.error));
  await onModelCanceled((size) => settle(size, "canceled", null));
  await onModelSelected((size) => {
    modelState.selected = size;
  });
}

function settle(size: string, status: ModelStatus["status"], error: string | null) {
  const m = modelState.models.find((x) => x.size === size);
  if (m) {
    m.status = status;
    m.error = error;
  }
  if (status === "done" || status === "canceled" || status === "failed") {
    modelState.activeSize = null;
  }
  if (status === "done") {
    // 刷新 file_exists / file_bytes
    void loadModel();
  }
}

export async function loadModel() {
  modelState.loading = true;
  try {
    modelState.models = await getModelsStatus();
    const sel = modelState.models.find((m) => m.selected);
    modelState.selected = sel?.size ?? "small";
    const active = modelState.models.find((m) => m.status === "downloading");
    modelState.activeSize = active ? active.size : null;
  } finally {
    modelState.loading = false;
  }
}

export async function download(size: string) {
  modelState.activeSize = size;
  try {
    await downloadModel(size);
  } catch {
    modelState.activeSize = null;
  }
}

export async function cancel(size: string) {
  await cancelModelDownload(size);
}

export async function select(size: string) {
  await setModel(size);
  modelState.selected = size;
  await loadModel();
}

export async function remove(size: string) {
  await removeModel(size);
  await loadModel();
}

export function useModels() {
  return { modelState, initModel, loadModel, download, cancel, select, remove };
}
```

- [ ] **Step 4: 前端类型检查**

Run: `cd "d:/Coding Projects/ASPlayer/app" && npx vue-tsc --noEmit`
Expected: 无新增类型错误（旧文件未动）。

- [ ] **Step 5: 提交**

```bash
cd "d:/Coding Projects/ASPlayer"
git add app/src/types.ts app/src/api/model.ts app/src/stores/model.ts
git commit -m "feat(models): 前端模型 store 与 API 封装（订阅 model://* 事件）"
```

---

### Task 7: SettingsPanel 新增「模型」卡片

**Files:**
- Modify: `app/src/components/SettingsPanel.vue`

- [ ] **Step 1: script 引入 store + 助手**

在 `SettingsPanel.vue` 的 `<script setup lang="ts">` 中，`import type { ShortcutActionName } from "../types";` 之后追加：

```ts
import { computed } from "vue";
import { useModels, MODEL_META } from "../stores/model";
```

把既有 `TabKey` 类型改为含 `"model"`：

```ts
type TabKey = "appearance" | "playback" | "subtitle" | "translate" | "model" | "shortcuts";
```

在 `const pb = usePlayback();` 之后加：

```ts
const ms = useModels();
const selectedFileExists = computed(() => {
  const s = ms.modelState.models.find((m) => m.selected);
  return !!s && s.file_exists;
});
const activePercent = computed(() => {
  const s = ms.modelState.models.find((m) => m.size === ms.modelState.activeSize);
  if (!s || !s.total_bytes) return 0;
  return Math.min(100, Math.round((s.bytes_downloaded / s.total_bytes) * 100));
});
```

把既有 `tabs` 数组，在 `{ key: "translate", label: "翻译" }` 之后插入：

```ts
  { key: "model", label: "模型" },
```

把既有 `watch(() => props.open, ...)` 改为：

```ts
watch(
  () => props.open,
  async (open) => {
    if (open) {
      await ms.initModel();
      await ms.loadModel();
      load(); // 既有：载入翻译/API 设置
    }
  }
);
```

- [ ] **Step 2: 模板新增「模型」段**

在 `</nav>` 后的 `<div class="content">` 内，`<div class="section" v-show="activeTab === 'translate'">...</div>` 之后、`<div class="foot-hint">` 之前，插入：

```html
      <div class="section" v-show="activeTab === 'model'">
        <div class="section-label">模型</div>

        <div v-if="ms.modelState.selected && !selectedFileExists" class="model-warn">
          尚未下载所选模型「ggml-{{ ms.modelState.selected }}.bin」，转写前请先下载，或在「翻译」页改用云端 API。
        </div>

        <div class="row model-current">
          <span class="row-label">当前模型</span>
          <span class="model-path">ggml-{{ ms.modelState.selected }}.bin</span>
        </div>

        <div class="model-list">
          <div v-for="m in ms.modelState.models" :key="m.size" class="sc-item">
            <span class="sc-label">
              {{ m.size }}（{{ MODEL_META[m.size] }}）
              <span v-if="m.selected" class="model-badge">已选</span>
            </span>

            <span v-if="m.status !== 'downloading'" class="sc-controls">
              <template v-if="m.file_exists">
                <button class="sc-key" :class="{ sel: m.selected }" @click="ms.select(m.size)">
                  {{ m.selected ? "当前" : "选为当前" }}
                </button>
                <button class="sc-clear" title="删除模型" @click="ms.remove(m.size)">×</button>
              </template>
              <button v-else class="sc-key" @click="ms.download(m.size)">下载</button>
            </span>
            <span v-else class="sc-controls">
              <span class="dl-mid">
                {{
                  m.total_bytes
                    ? Math.round((m.bytes_downloaded / m.total_bytes) * 100) + "%"
                    : "下载中"
                }}
              </span>
              <button class="sc-clear" title="取消" @click="ms.cancel(m.size)">×</button>
            </span>
          </div>
        </div>

        <div v-if="ms.modelState.activeSize" class="model-progress">
          <div class="model-bar" :style="{ width: activePercent + '%' }"></div>
        </div>

        <p class="hint">
          建议用 small（466MB）兼顾体积与 ASMR 识别率。无 N 卡可跳过本地模型，直接在「翻译」页配置云端 API。
        </p>
      </div>
```

- [ ] **Step 3: 追加 scoped 样式**

在 `<style scoped>` 末尾、`</style>` 前追加：

```css
.model-current {
  margin-bottom: 12px;
}
.model-path {
  font-size: 13px;
  color: var(--fg-1);
  font-variant-numeric: tabular-nums;
}
.model-warn {
  background: var(--accent-dim);
  color: var(--accent);
  border: 1px solid var(--accent);
  border-radius: 9px;
  padding: 8px 12px;
  font-size: 12px;
  line-height: 1.5;
  margin-bottom: 12px;
}
.model-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 12px;
}
.model-badge {
  margin-left: 6px;
  font-size: 11px;
  color: var(--accent);
  background: var(--accent-dim);
  padding: 2px 6px;
  border-radius: 5px;
}
.sc-key.sel {
  border-color: var(--accent);
  color: var(--accent);
}
.dl-mid {
  font-size: 12px;
  color: var(--fg-2);
  font-variant-numeric: tabular-nums;
}
.model-progress {
  height: 6px;
  background: var(--bg-2);
  border-radius: 3px;
  overflow: hidden;
  margin-bottom: 12px;
}
.model-bar {
  height: 100%;
  background: var(--accent);
  transition: width 0.2s ease;
}
```

- [ ] **Step 4: 类型检查 + 构建**

Run: `cd "d:/Coding Projects/ASPlayer/app" && npm run build`
Expected: `vue-tsc --noEmit` 无错误，`vite build` 成功产出 dist。

- [ ] **Step 5: 提交**

```bash
cd "d:/Coding Projects/ASPlayer"
git add app/src/components/SettingsPanel.vue
git commit -m "feat(models): 设置面板新增「模型」卡片（五档下载/选中/删除/进度/无模型引导）"
```

---

### Task 8: 全量验收

**Files:** 无新增（仅运行与记录）

- [ ] **Step 1: Rust 全量测试**

Run: `cd "d:/Coding Projects/ASPlayer" && cargo test -p app`
Expected: 所有 tests PASS（db + models + transcriber）。

- [ ] **Step 2: 前端构建**

Run: `cd "d:/Coding Projects/ASPlayer/app" && npm run build`
Expected: 无 TS 错误，构建成功。

- [ ] **Step 3: 运行应用手动验收清单**

Run: `cd "d:/Coding Projects/ASPlayer/app" && npm run tauri dev`

逐项记录：
1. 打开设置 → 「模型」卡片：五档初始 `file_exists=false`，选中为 small，出现「尚未下载」引导横幅。
2. 点 small 的「下载」：进度条从 0 增长；期间点「取消」，`.part` 被清除、状态回 idle、不再残留下载任务。
3. 断点续传：下载中途退出应用，重开再点下载——从断点继续（`.part` 大小在重启后被读为起始偏移）。
4. 下载完成：显示「选为当前」+「已选」徽标高亮；`~/.asplayer/models/ggml-small.bin` 存在且前 4 字节为 `ggml`。
5. 选中 small 后触发一次真实转写：日志确认使用了 `~/.asplayer/models/ggml-small.bin` 路径（或监控 `ASPLAYER_MODEL` 缺失仍走选中档）。
6. 设 `ASPLAYER_MODEL` 指向别处 → 优先于 DB 选中（回归：转写仍用 env 路径）。
7. 删除模型后转写：返回友好错误「所选模型文件不存在…」，前端弹出引导。
8. 无 N 卡场景：不下载模型，仅「翻译」页配置云端 API → 转写跳过模型依赖（回归既有路径）。

> 若断网环境无法完善网络项，至少确认：状态加载、无模型引导、`ASPLAYER_MODEL` 回归、下载中途取消等不依赖真实外网的项通过；网络下载项标注「待联网实测」。

---

## Self-Review

**Spec coverage：**
- 五档/默认 small/标准文件 → Task 1, 7 ✓
- 官方→镜像回退 → Task 4 ✓
- 断点续传/取消/校验 → Task 3, 4 ✓
- 设置面板「模型」/无模型引导 → Task 7 ✓
- `model_path()` 优先级 env>DB>default → Task 1 ✓
- 事件 `model://*` + 前端 store → Task 4,6 ✓

**Placeholder scan：** 无 TBD/TODO；每步含完整代码；下载引擎因网络无法单测，已用「编译+手动验收」明确覆盖，非占位。

**Type consistency：** `resolve_model_path(&db)` / `model_file_path(dir,&size)` / `verify_file(&path)` 跨 Task 一致；前端 `ModelStatus`/`ModelProgress` 字段与后端 snake_case 序列化对齐；`DlStatus` 的 `"downloading"` 等字符串在 TS 联合类型中一一对应。
