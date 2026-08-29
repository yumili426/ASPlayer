# 本地翻译引擎（Ollama 模型下载渠道）设计

> **状态：⭐ 已与作者确认方案（2026-08-29）。** 经会话逐节对齐，决定实施方案 B：设置页提供 Ollama 本地翻译模型的**探测 / 列表 / 流式下载**渠道，并**一键接通**翻译配置；**保留现有 OpenAI 兼容 API 翻译**。本稿冻结在此，后续据此进入 writing-plans。

**目标：** 在「翻译」设置页，像下载 ASR 模型一样，帮助用户使用本机 Ollama 做离线翻译：探测服务、列出已拉取模型、从推荐清单一键下载（流式进度、可取消），并把所选本地模型一键填进翻译配置；API 翻译（DeepSeek/OpenAI/Kimi 等 OpenAI 兼容服务商）原样保留，两条路并行。

---

## 1. 范围（做什么）

- **驱动方式：纯 HTTP 接口，不依赖 `ollama` 命令行在 PATH。** 只要求 Ollama 服务在跑（默认 `http://localhost:11434`，端口可在设置里改）。
- 在「翻译」设置页新增「本地翻译引擎」区块：
  - 顶部连接状态：已连接（Ollama 版本）/ 未检测到（引导去装 Ollama 并启动）。
  - 已拉取的模型列表（名称 + 体积，可删除）。
  - 「推荐模型」下载列表：小（qwen2.5:3b）/ 中（qwen2.5:7b），下载按钮 + 流式进度条 + 取消。
  - 「一键接通」：选中/拉取某本地模型后，一键把翻译配置指向该模型。
- 新增 settings 键 `ollama_base`（默认 `http://localhost:11434`），设置页可改并保存。
- 现有「翻译」页的 API 服务商（api_base/api_key/api_model + 预设下拉）**不改动**，仅在其上叠加本地区块。

## 2. 非目标（明确不做）

- **不打包 / 内嵌 Ollama 可执行文件**（方案 C），不自动启动 Ollama 进程。
- **不做 LM Studio 的探测 / 拉取**：LM Studio 既然暴露 OpenAI 兼容端点，走现有 API 预设即可；本次下载渠道只管 Ollama。
- **不改翻译管线本体**：`POST {api_base}/chat/completions` 已是事实，本地模型只是换一个 base，无需触碰 `crates/asplayer-transcribe`。
- 不做多模型并发下载、不做下载续传 UI（Ollama 本身会续传 / 共享进度）。

## 3. 技术要点

### 3.1 Ollama HTTP 接口（已对照真实文档核实）

- **探测 + 列表：** `GET {base}/api/tags`
  - 200 → 服务在跑；每个 model 含 `name`（如 `qwen2.5:7b`）、`size` 等。取 `name` + `size` 展示。
  - 连接失败 / 非 200 → 视为「未检测到」。
- **拉取：** `POST {base}/api/pull`，body `{"model":"<name>"}`（`stream` 默认 `true`）→ 返回 **NDJSON**，逐行：
  - `{"status":"pulling manifest"}`
  - `{"status":"pulling <digest>","digest":"sha256:...","total":<u64>,"completed":<u64>}` → percent = `completed/total`（同一模型多个 layer，取全局累计或当前 layer 均可；实现按「有 total 就出百分比，无 total 显示下载中」兜底，同词库 chunked 策略）。
  - `{"status":"success"}` → 完成。
  - 顶层 `{"error":"..."}` 行 → 失败。
  - 兼容 `stream:false` 时返回单个 `{"status":"success"}` 或 `{"error":...}` 对象。
- **OpenAI 兼容端点：** `{base}/v1/chat/completions`（一键接通时 `api_base` 填 `http://localhost:11434/v1`）。

### 3.2 Rust 侧（app crate）——复用模型下载器 `models.rs` 的范式

用 `LazyLock<Mutex<OllamaPullState>>` + 后台线程 + `app.emit(...)` 广播事件，与 `models.rs` / `dict.rs` 一致。

新增命令（`app/src-tauri/src/ollama.rs`）：

- `ollama_status(base: String) -> CmdResult<OllamaStatus>`
  - `GET {base}/api/tags`，返回 `OllamaStatus { connected, version: Option<String>, models: Vec<OllamaModel> }`；`OllamaModel { name, size }`。
  - `version` 可由 `GET {base}/api/version` 补取（可选，拿不到就 `None`）。
- `ollama_pull(base: String, model: String, app: AppHandle) -> CmdResult<()>`
  - ACTIVE 互斥（一次只拉一个）；置状态 Downloading → 立即广播 → `std::thread::spawn`。
  - 线程内 `reqwest::blocking` 发 `POST {base}/api/pull`，**流式逐行**读 NDJSON（`BufRead::read_line`），解析 status/total/completed/error；每 250ms 广播进度；`is_canceled` 检查（取消则关连接）。
  - 结束：Done / Failed（带 error 明细）/ Canceled。
- `cancel_ollama_pull() -> CmdResult<bool>`：受理取消（同 `dict.cancel_dict_download`）。

事件名（与现有引擎风格一致）：

- `ollama://status`：`OllamaStatus`（含拉取中状态 + 当前模型 + error）。
- `ollama://progress`：`OllamaProgress { model, bytes, total, percent }`。

### 3.3 前端

- `app/src/api/ollama.ts`：`ollamaStatus(base)` / `ollamaPull(base, model)` / `ollamaPullCancel()` / `onOllamaStatus(cb)` / `onOllamaProgress(cb)`。
- `app/src/types.ts`：`OllamaStatus { connected, error?, version?, models: OllamaModel[], pulling?: { model, status } }`、`OllamaModel { name, size }`、`OllamaProgress { model, bytes, total, percent }`。
- `SettingsPanel.vue` 「翻译」tab：
  - 顶部：`ollama_base` 输入 + 保存（同 `onSaveDictUrl` 范式）+ 连接状态 pill。
  - 连接后：已拉取模型列表（`name` + `fmtBytes(size)`，每项「用这个翻译」+「删除」钮）。
  - 推荐模型下载区：小 / 中 两类（名称 + 体积 + 下载按钮），下载中切进度条 + 取消。
  - **一键接通**：`saveSettings({ api_base: "{base}/v1", api_model: model, api_key: "" })`，并更新本页 `apiBase/apiModel/apiKey` refs 回显 + 轻提示「已切换到本地模型」。
- 订阅：仅面板打开时 `onOllamaStatus/onOllamaProgress` 订阅一次（同 `initDict` 思路）。

### 3.4 settings

- 新增键：`ollama_base`，默认 `http://localhost:11434`。读写走现有 `save_settings` / `get_setting`。

## 4. 推荐模型清单（翻译用途，中文友好）

| 档位 | 模型 | 约体积 |
|---|---|---|
| 小 | `qwen2.5:3b` | ~1.9 GB |
| 中 | `qwen2.5:7b` | ~4.7 GB |

> 仅列这两档，避免选择疲劳；完整模型名以 `:latest` 或具体 tag 由用户在「已拉取」列表看到。清单作为「可直接点下载」的入口，不强制。

## 5. 数据流（一键接通路径）

1. 用户在设置页启动 Ollama（服务在 `http://localhost:11434`）。
2. 「本地翻译引擎」探测到已连接 → 显示已拉取模型。
3. 用户点推荐清单某模型 → `ollama_pull(base, model)` → 流式进度 → `success`。
4. 用户点该模型行「用这个翻译」→ `saveSettings({ api_base, api_model, api_key: "" })` → 翻译页回显刷新。
5. 「转写并翻译」时，`translate_media` 走 `{base}/v1/chat/completions`，本地离线推理。

## 6. 风险 / 待确认

- **实现期用真实 Ollama 验证**：/api/tags 字段、/api/pull 的 layer 进度语义、`completed` 是否为缺省缺失、`stream:false` 返回形态；解析器按「status 字符串 + 可选 total/completed + 顶层 error」容错。
- 拉取时若模型已存在：Ollama 返回 `{"status":"already exists"}` 或直接 success，需当「已就绪」处理。
- 字节流可能一次读半行：用 `BufRead::read_line` 按 `\n` 切，每行 `serde_json::from_str`；健壮地忽略空白行。
- `total=0`（未给出）时进度显示「下载中」而非百分比。
- 取消语义：关闭流式响应即可；Ollama 会继续后台下载（服务端保留），下次可续传——UI 只保证「停止显示进度并复位状态」。

## 7. 验收要点

- `cargo build -p app --lib`、`cargo test -p asplayer-dict`（现有、不受影响）、`vue-tsc --noEmit`、`vite build` 全绿。
- 手动：设置页连接检测、推荐模型下载（进度+取消+完成）、已拉取列表、一键接通后翻译页回显、用本地模型「转写并翻译」实际出中文。
