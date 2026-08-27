# 模型下载器 设计文档

- **日期**：2026-08-27
- **状态**：设计评审中
- **对应主设计**：[2026-08-26-asplayer-design.md](../../specs/2026-08-26-asplayer-design.md) §11（本体不内嵌模型，内置下载器 + HF 镜像加速）
- **上游依赖**：M0 离线转写管线（`asplayer-transcribe`，进程内调用）已落地

---

## 1. 背景与目标

主设计 §11 已定调：**本体不内嵌模型**，沿用 LLPlayer 验证过的模式——首次使用由内置下载器拉取 Whisper GGUF 模型，显示进度，支持国内镜像加速；无显卡用户走云端 API 完全跳过本地模型。

当前现状（对账结论）：
- `model_path()`（`app/src-tauri/src/transcriber.rs:79`）只读「环境变量 `ASPLAYER_MODEL` > 默认 `~/.asplayer/models/ggml-small.bin`」两档，**无数据库设置档**，也无应用内模型选择/下载。
- DB 已有 `save_setting`/`get_setting`（`app/src-tauri/src/db.rs:338/348`）可做 KV 持久化。
- 前端 `SettingsPanel.vue` 已有翻译 API 预设（DeepSeek/OpenAI/Qwen/GLM/Kimi/Ollama）与 api_key/api_model，**无「模型」区块**。

### 目标
1. 应用内可下载 whisper.cpp 五个标准档 GGUF 模型，含进度、断点续传、可取消、下载后校验。
2. 模型选择（大小）持久化到 DB，`model_path()` 优先级改为 **环境变量 > DB 设置 > 默认 small**。
3. 设置面板新增「模型」卡片；模型缺失时主动引导下载（不阻塞转写触发）。

### 非目标（YAGNI）
- 不做量化/压缩档（q5_0 等），仅标准五档。
- 不做多版本/分支模型列表、不做模型排行榜。
- 不做并发多模型同时下载（一次一个，串行）。
- 不重复造已存在的产物缓存/指纹逻辑。

---

## 2. 产品决策（用户已确认）

| 决策点 | 结论 |
|---|---|
| 模型档位 | 五个标准档：`tiny` / `base` / `small` / `medium` / `large-v3`；默认 `small` |
| 文件 | 各档标准文件 `ggml-<size>.bin`（不含 q5_0 量化） |
| 下载地址 | 官方 HF → `hf-mirror.com` 镜像自动回退 |
| 下载器能力 | 断点续传 + 可取消 + 下载后校验 |
| 入口 | 设置面板「模型」卡片 + 无模型时引导 |

### 模型规模速览（帮助理解体积/精度取舍）

| 档位 | 标准 GGUF 体积 | q5_0 体积 | 相对精度 |
|---|---|---|---|
| tiny | ~75 MB | ~31 MB | 最低 |
| base | ~142 MB | ~54 MB | 低 |
| small | ~466 MB | ~181 MB | 中（默认推荐） |
| medium | ~1.5 GB | ~590 MB | 高 |
| large-v3 | ~3.1 GB | ~1.2 GB | 最高 |

> 注：本设计仅收录标准档（不收录 q5_0）。精度对 ASMR 耳语音最不利（主设计 §12 风险 #1），故默认 small 而非更小的档。

---

## 3. 架构：路线 A（全 Rust 后端 + DB 状态 + 事件推送）

延续项目铁律 **「Rust 是唯一事实来源」**（同悬浮窗 `floating.rs` 模式）：

```
┌─ 设置面板窗口 (Vue 3) ──────────────────────────┐
│ 模型卡片（modelStore.ts 驱动）                     │
│  · 列表：五档状态/大小/已选                       │
│  · 下载：进度条 / 取消 / 校验 / 重下              │
│  · 无模型引导横幅                                │
└─────────────┬────────────────────────────────────┘
              │ invoke ↔ 事件 (model://*)
┌─────────────▼─────────── Rust 核心层 ────────────┐
│ models.rs（新模块）                                │
│  · 全局下载状态：Mutex<HashMap<size, Download>>    │
│  · 单下载锁：串行，一次只跑一个                    │
│  · reqwest 流式下载（bytes_stream）+ 校验 + 续传  │
│  · 落盘 ~/.asplayer/models/ggml-<size>.bin        │
│  · save_setting("whisper_model", size)             │
└─────────────┬────────────────────────────────────┘
              │ 读 model_path() 增设 DB 档
        ┌─────▼───────────────────────────────┐
        │ transcriber.rs :: model_path()        │
        │ env ASPLAYER_MODEL > DB whisper_model │
        │ > 默认 ggml-small.bin                 │
        └───────────────────────────────────────┘
```

**为什么全 Rust 后端（vs 前端 XHR 直下）**：
- 断点续传（Range 续写）、取消、校验（GGML magic 头 + Content-Length 比对）需要可靠的文件管理工作，后端持有始终可靠。
- 已选模型要写 DB（`whisper_model`），这是转写管线的唯一事实来源，必须由 Rust 写，避免前端/后端双源漂移（呼应悬浮窗 Bug #4 的教训）。
- 进度事件经 `app.emit` 广播，与现有 `transcribe://progress` 同一套模式，前端 `listen` 订阅即可。

---

## 4. 后端设计（models.rs）

### 4.1 常量与路径

```rust
pub const MODEL_SIZES: [&str; 5] = ["tiny", "base", "small", "medium", "large-v3"];
const SETTING_MODEL: &str = "whisper_model";   // DB 键
const MAGIC: &[u8] = b"ggml";                  // GGUF 文件魔数头（whisper.cpp）

fn models_dir() -> PathBuf {
    home().join(".asplayer").join("models")
}
fn model_path(size: &str) -> PathBuf {
    models_dir().join(format!("ggml-{size}.bin"))
}
```

下载地址（官方 + 镜像回退）：
```rust
const URL_OFFICIAL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin";
const URL_MIRROR: &str   = "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin";
```

### 4.2 全局状态

```rust
pub enum DlStatus { Downloading, Done, Failed, Canceled }
pub struct Download {
    pub size: String,
    pub status: DlStatus,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,        // 0 = 未知
    pub error: Option<String>,
}

// AppState 新增两个字段（与 db 并列）：
pub struct AppState {
    pub db: Arc<Mutex<MediaDb>>,
    pub downloads: Arc<Mutex<HashMap<String, Download>>>,  // 各档状态
}
```

串行约束：新增 `ACTIVE: Mutex<Option<String>>` 表示正在下载的 size；`download_model` 若已有 ACTIVE 则报错「已有模型下载中」。取消置 `Download.status = Canceled`，流式循环在每 chunk 边界检查并终止。

### 4.3 下载流程（断点续传 + 校验）

```
download_model(size):
  1. 目标文件已存在且校验通过 → 直接置 Done，emit model://done
  2. 加 ACTIVE 锁（非空则报错）
  3. init Download{Downloading, 已存在字节数, ...}
  4. 循环两条源：
     for url in [official, mirror]:
        req = GET url
        if 本地已有 N 字节 → Header Range: bytes=N-   (断点续传)
        resp 206/200？ → 按需追加/新建 .bin.part
        流式读 chunks：
           write append → bytes_downloaded += n → 节流 emit model://progress
           每 chunk 检查 cancel 标志 → 终止
        HTTP 失败 / 校验失败 → 切下一条源
  5. 下载完 → 校验：文件头 == b"ggml" 且（total 已知时）文件尺寸 == total_bytes
        通过 → 重命名 .part → Done + emit model://done（不自动改选中，选中由 set_model 决定）
        失败 → 回退下一条源；两条都失败 → Failed + emit model://error
```

### 4.4 事件（`app.emit` 广播，命名空间 `model://`）

| 事件 | 载荷 | 时机 |
|---|---|---|
| `model://progress` | `{size, bytes_downloaded, total_bytes, percent}` | 节流约 4 次/s |
| `model://done` | `{size}` | 下载+校验通过 |
| `model://error` | `{size, error}` | 下载失败 |
| `model://canceled` | `{size}` | 用户取消 |
| `model://selected` | `{size}` | `set_model` 成功 |

### 4.5 Tauri 命令（lib.rs 注册）

| 命令 | 返回 | 说明 |
|---|---|---|
| `get_models_status` | `Vec<ModelStatus>` | 五档：已存在大小、下载态、已选 |
| `download_model` | `()` | 启动异步下载（后台线程） |
| `cancel_model_download` | `bool` | 置取消标志 |
| `set_model` | `()` | 写 `whisper_model` + emit selected；校验文件在盘，缺则引导下载 |
| `remove_model` | `()` | 仅删除磁盘 .bin；`whisper_model` 选中保持不变（转写时因缺文件而引导下载） |

`ModelStatus` 前端结构：
```ts
interface ModelStatus {
  size: string;               // "small"
  file_exists: boolean;       // 磁盘上是否已有
  file_bytes: number;         // 磁盘文件大小（0=无）
  selected: boolean;          // 是否当前选中
  status: 'downloading'|'done'|'failed'|'canceled'|'idle';
  bytes_downloaded: number;
  total_bytes: number;        // 0=未知
  error: string | null;
}
```

### 4.6 model_path() 优先级调整（transcriber.rs）

```rust
pub fn model_path(db: &MediaDb) -> PathBuf {
    if let Ok(m) = env ASPLAYER_MODEL { 非空 → 返回 }        // 档1 环境变量（调试/高级）
    let size = db.get_setting("whisper_model").ok().flatten()  // 档2 DB 设置
        .unwrap_or_else(|| "small".into());
    models_dir().join(format!("ggml-{size}.bin"))              // 档3 默认 small
}
```

- 调用点 `run_transcription` 内 `model_path()` 改为 `model_path(&db)`（该处已有 `db`）。
- 现有默认文件 `ggml-small.bin` 路径不变，向后兼容。

---

## 5. 前端设计（SettingsPanel + modelStore）

### 5.1 modelStore.ts（Pinia）
- state：`models: ModelStatus[]`、`downloading: boolean`、`busy: boolean`。
- actions：
  - `loadStatus()` → invoke `get_models_status`。
  - `download(size)` → invoke `download_model(size)`。
  - `cancel(size)` → invoke `cancel_model_download(size)`。
  - `select(size)` → invoke `set_model(size)`。
  - `remove(size)` → invoke `remove_model(size)`。
- listen：模块加载时订阅 `model://progress/done/error/canceled/selected`，定向更新对应 `size` 的状态，`progress` 节流写 `bytes_downloaded`（由后端已节流，前端只赋值）。

### 5.2 SettingsPanel「模型」卡片
- **无模型引导**：当前选中档文件不存在时，显示横幅「尚未下载模型，转写前请先下载」，按钮直接触发 `download(selected)`。
- **五档列表**：每行 `tiny | base | small | medium | large-v3`，显示体积估算+当前状态；未下载行显示「下载」，已下载行显示「选为当前」（selected 高亮），下载中行显示进度条 + 「取消」。
- **当前模型展示**：顶部显示 `当前模型：ggml-small.bin（466 MB）`，带「改用云端/下载」切换提示（云端走已有 API 配置）。
- 服务端/云端可在该卡片下方提示：「无 N 卡可跳过本地模型，直接配置云端 API（见上方 API 卡片）」——对应主设计 §11。

### 5.3 交互细节
- 已选模型被 `remove_model` 删除后，`whisper_model` 不自动改回；转写时若文件缺失，`transcribe_media` 返回友好错误「所选模型文件不存在，请在设置面板下载」，并指引到模型卡片。不阻塞其他功能。
- 下载中切换模型尺寸不中断，仅作 UI 状态展示。

---

## 6. 错误处理与边界

| 场景 | 处理 |
|---|---|
| 下载中再次 `download_model` | 报「已有模型下载中」 |
| 下载中断 | `.part` 保留，下次复用断点；`bytes_downloaded` 从文件实际大小恢复 |
| 官方源失败 | 自动切 `hf-mirror.com` |
| 两条源都失败 | `Failed` + emit error，前端显示可重试 |
| 校验失败（魔数/长度不符） | 视为源失败，切下一源；都失败则删 `.part` 重下 |
| 取消 | 删 `.part`，emit canceled |
| 转写时模型缺 | `transcribe_media` 返回指引下载的友好错误，不传后台线程 |

---

## 7. 测试与验收

### 7.1 Rust 单元测试
- `model_path()`：env 覆盖 > DB 设置 > 默认 small，三层各一测试。
- `save_setting/get_setting` 写读 `whisper_model` 往返。
- 校验逻辑：魔数头正确/错误、长度比对——用内存 buffer 构造。

### 7.2 命令级集成（mock 网络不可行，采用「最小可切源」）
- `get_models_status` 在无网络、无文件时返回 `file_exists=false`。
- `set_model` 在文件缺失时仍写 DB 并返回成功（选中允许超前），但触发转写时校验。

### 7.3 手动验收清单
1. 打开设置 → 模型卡片，五档状态正确（初始全 idle、文件全不存在）。
2. 下载 `small`：进度条增长、可取消、取消后 `bytes_downloaded` 归零且 `.part` 不残留。
3. 断点续传：下载中取消网络（或杀进程），重开应用再下载，从断点继续而非 0。
4. 下载完成：`file_exists=true`、体积正确、`ggml-small.bin` 存在且魔数头为 `ggml`。
5. 选中 `small` → 触发一次真实转写，`run_transcription` 使用该模型路径（日志确认）。
6. `ASPLAYER_MODEL` 环境变量设别处 → 优先于 DB 选中（回归，转发写仍用 env 路径）。
7. 删除模型后转写：返回「模型不存在」友好错误，前端弹指引。
8. 无 N 卡用户：不下载模型，仅配置云端 API → 转写跳过模型依赖（回归既有路径）。

---

## 8. 里程碑边界

**本设计属于里程碑 4（模型下载器）**，与已合并的 M0–M3 解耦，独立可验收。未含：量化模型、模型社区分享、在线播放实时管线、转码兜底。

---

## 9. 风险与对策

| 风险 | 对策 |
|---|---|
| HF 官方下载慢/被墙 | 镜像自动回退；`hf-mirror.com` 实测可用 |
| 大模型（large-v3）下载几 GB 中断 | 断点续传 + 可取消 + 校验，减少重下 |
| 校验弱（无全网 SHA-256） | 用 GGML 魔数 + Content-Length 近似校验；避免多 GB 全量哈希开销（主设计 §7 指纹思路同理） |
| 耳语音识别仍差 | 不属本里程碑；主设计 §12 Plan B（loudnorm/更大模型/faster-whisper）另立 |
