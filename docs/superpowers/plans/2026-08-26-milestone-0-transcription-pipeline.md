# ASPlayer 里程碑 0：转写验证管线 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建最小命令行管线 `媒体文件 → ffmpeg 抽音轨 → whisper.cpp 转写 → SRT/JSON → LLM 批量翻译`，在真实 ASMR 样本上验证识别率与翻译质量（对应设计文档 §12 里程碑 0），通过后才进入 UI 开发。

**Architecture:** 独立 Cargo workspace 中的单个 crate `asplayer-transcribe`。纯函数（SRT 渲染、prompt 构建）走 TDD；ffmpeg 与 Whisper 属集成边界，用小型集成测试 + 手动真实样本验证。此 crate 的模块（audio/srt/whisper/translate）在 M2 将被原样复用进 Tauri 应用的 Rust 层。

**Tech Stack:** Rust stable、whisper-rs（whisper.cpp 绑定）、hound（WAV 读取）、clap（CLI）、reqwest blocking（OpenAI 兼容 API）、serde/serde_json。

**前置知识：**
- whisper.cpp 的 GGUF/GGML 模型从 https://huggingface.co/ggerganov/whisper.cpp 下载；国内镜像把域名替换为 https://hf-mirror.com
- whisper-rs 返回的时间戳单位是 10ms（厘秒），乘 10 得毫秒
- 音频要求：16kHz 单声道 f32 采样

---

## File Structure（本计划锁定）

```
ASPlayer/
├─ Cargo.toml                      # workspace 根
└─ crates/
   └─ asplayer-transcribe/
      ├─ Cargo.toml
      ├─ src/
      │  ├─ main.rs                # clap CLI：transcribe / translate / pipeline 三个子命令
      │  ├─ audio.rs               # ffmpeg 抽音轨 + 读 WAV 为 f32
      │  ├─ srt.rs                 # SRT 时间戳格式化与渲染（纯函数）
      │  ├─ whisper.rs             # whisper-rs 封装
      │  └─ translate.rs           # LLM 批量翻译（prompt 构建为纯函数）
      └─ tests/
         └─ integration.rs         # WAV 读写往返测试
```

后续计划（仅记录，不在本计划内）：M1 Tauri 骨架与媒体库、M2 字幕管线入应用、M3 悬浮窗+全局快捷键、M4 播放模式/主题/i18n/外部字幕。

---

### Task 1: 环境自检

**Files:** 无代码变更，只验证环境。

- [ ] **Step 1: 验证 Rust 工具链**

Run: `cargo --version && rustc --version`
Expected: cargo 1.8x+，rustc 1.8x+。若缺失：到 https://rustup.rs 安装后重开终端再验。

- [ ] **Step 2: 验证 ffmpeg**

Run: `ffmpeg -version`
Expected: 打印版本信息。若缺失：下载 GPL 构建版（https://www.gyan.dev/ffmpeg/builds/ 的 release-essentials），解压后把 bin 目录加入 PATH。

- [ ] **Step 3: 准备 Whisper 模型**

Run:
```bash
mkdir -p "$USERPROFILE/.asplayer/models"
curl -L -o "$USERPROFILE/.asplayer/models/ggml-small.bin" https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
```
Expected: 得到约 466MB 的 ggml-small.bin。多语言任务必须用带 `.bin` 的多语言模型（不要用 `.en` 后缀模型）。

- [ ] **Step 4: 准备真实测试样本**

准备 2~3 个 ASMR 样本放入 `$USERPROFILE/.asplayer/samples/`：一个耳语型、一个正常说话型、一个带 BGM 型（mp4 或音频均可）。记录每个样本的实际语言内容供 Task 8 人工评估对照。

---

### Task 2: Cargo workspace 与 CLI 骨架

**Files:**
- Create: `Cargo.toml`（workspace 根）
- Create: `crates/asplayer-transcribe/Cargo.toml`
- Create: `crates/asplayer-transcribe/src/main.rs`

- [ ] **Step 1: 创建 workspace 根 Cargo.toml**

```toml
[workspace]
resolver = "2"
members = ["crates/asplayer-transcribe"]

[profile.release]
lto = true
```

- [ ] **Step 2: 创建 crate 清单 crates/asplayer-transcribe/Cargo.toml**

```toml
[package]
name = "asplayer-transcribe"
version = "0.1.0"
edition = "2021"

[dependencies]
whisper-rs = "0.13"
anyhow = "1"
clap = { version = "4", features = ["derive"] }
hound = "3.5"
reqwest = { version = "0.12", features = ["blocking", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tempfile = "3"

[dev-dependencies]
assert_cmd = "2"
```

注意：若 `whisper-rs = "0.13"` 编译报 API 不存在，查 https://docs.rs/whisper-rs 的最新版本号并同步调整 Task 5 中的 API 用法（该 crate 近年小版本间有 API 变动；核心类型名 WhisperContext / FullParams / SamplingStrategy 保持稳定）。

- [ ] **Step 3: 写最小 CLI 骨架 src/main.rs**

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "asplayer-transcribe", about = "ASPlayer 里程碑0 转写验证管线")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 媒体 → 音轨 → Whisper → SRT + segments.json
    Transcribe {
        /// 媒体文件路径
        #[arg(long)]
        media: String,
        /// whisper 模型路径
        #[arg(long)]
        model: String,
        /// 语言代码，如 ja/en；缺省自动检测
        #[arg(long)]
        lang: Option<String>,
        /// 输出目录，默认当前目录
        #[arg(long, default_value = ".")]
        out: String,
    },
    /// 对 transcribe 产物 segments.json 做批量翻译，输出双语 srt/txt
    Translate {
        /// transcribe 生成的 segments.json
        #[arg(long)]
        input: String,
        /// OpenAI 兼容 API 地址，如 https://api.openai.com/v1
        #[arg(long, env = "ASPLAYER_API_BASE")]
        api_base: String,
        /// API Key
        #[arg(long, env = "ASPLAYER_API_KEY")]
        api_key: String,
        /// 模型名，如 gpt-4o-mini
        #[arg(long, default_value = "gpt-4o-mini")]
        model: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Transcribe { .. } => anyhow::bail!("Task 5 实现"),
        Cmd::Translate { .. } => anyhow::bail!("Task 7 实现"),
    }
}
```

- [ ] **Step 4: 编译验证**

Run: `cargo build`
Expected: 编译通过（首次拉取依赖较慢；whisper-rs 会编译 C++ 代码，需要 MSVC Build Tools——VS2022 已装则无碍）。若 MSVC 缺失报错，安装 Visual Studio 2022 的"使用 C++ 的桌面开发"工作负载。

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/
git commit -m "feat(m0): cargo workspace 与转译 CLI 骨架"
```

---

### Task 3: SRT 纯函数（TDD）

**Files:**
- Create: `crates/asplayer-transcribe/src/srt.rs`
- Modify: `crates/asplayer-transcribe/src/main.rs`（注册模块）

- [ ] **Step 1: 在 main.rs 顶部注册模块（放在 use 之后）**

```rust
mod srt;
```

- [ ] **Step 2: 写失败测试 src/srt.rs（含实现文件一起创建，先只写测试与空壳）**

```rust
/// 一条字幕段。start/end 用毫秒。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// 毫秒 → "HH:MM:SS,mmm"
pub fn format_timestamp(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let msec = ms % 1000;
    format!("{h:02}:{m:02}:{s:02},{msec:03}")
}

/// 渲染整份 SRT 文本
pub fn render_srt(segments: &[Segment]) -> String {
    let mut out = String::new();
    for (i, seg) in segments.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            format_timestamp(seg.start_ms),
            format_timestamp(seg.end_ms),
            seg.text.trim()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_zero() {
        assert_eq!(format_timestamp(0), "00:00:00,000");
    }

    #[test]
    fn timestamp_with_hours_and_ms() {
        assert_eq!(format_timestamp(3_661_001), "01:01:01,001");
        assert_eq!(format_timestamp(59_999), "00:00:59,999");
    }

    #[test]
    fn render_numbered_blocks() {
        let segs = vec![
            Segment { start_ms: 0, end_ms: 1500, text: " おやすみ ".into() },
            Segment { start_ms: 2000, end_ms: 4000, text: "good night".into() },
        ];
        let s = render_srt(&segs);
        assert_eq!(
            s,
            "1\n00:00:00,000 --> 00:00:01,500\nおやすみ\n\n\
             2\n00:00:02,000 --> 00:00:04,000\ngood night\n\n"
        );
    }
}
```

- [ ] **Step 3: 运行测试确认通过**

Run: `cargo test -p asplayer-transcribe srt`
Expected: 3 个测试 PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/asplayer-transcribe/src/srt.rs crates/asplayer-transcribe/src/main.rs
git commit -m "feat(m0): SRT 时间戳格式化与渲染（纯函数+单测）"
```

---

### Task 4: 音频抽取与读取

**Files:**
- Create: `crates/asplayer-transcribe/src/audio.rs`
- Create: `crates/asplayer-transcribe/tests/integration.rs`
- Modify: `crates/asplayer-transcribe/src/main.rs`（注册模块）

- [ ] **Step 1: main.rs 注册模块**

```rust
mod audio;
```

- [ ] **Step 2: 实现 src/audio.rs**

```rust
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 用 ffmpeg 把任意媒体抽成 16kHz 单声道 WAV
pub fn extract_wav(media: &Path, out_dir: &Path) -> Result<PathBuf> {
    let name = media
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .to_string();
    let out = out_dir.join(format!("{name}.asplayer.wav"));
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(media)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "wav"])
        .arg(&out)
        .output()
        .context("启动 ffmpeg 失败：请确认 ffmpeg 已安装且在 PATH 中")?;
    if !status.status.success() {
        bail!("ffmpeg 抽音轨失败:\n{}", String::from_utf8_lossy(&status.stderr));
    }
    Ok(out)
}

/// 读 16kHz 单声道 WAV 为 whisper 所需的 f32 采样
pub fn read_samples_f32(wav: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(wav)?;
    let spec = *reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 {
        bail!("期望 16kHz 单声道 WAV，实际 {ch} 声道 {sr}Hz", ch = spec.channels, sr = spec.sample_rate);
    }
    let i16s: Vec<i16> = reader.samples::<i16>().collect::<Result<_, _>>()?;
    Ok(whisper_rs::convert_integer_to_float_audio(&i16s)?)
}
```

- [ ] **Step 3: 写集成测试 tests/integration.rs（用 hound 合成 WAV 验证往返）**

```rust
use anyhow::Result;
use std::path::Path;

#[test]
fn wav_roundtrip_16k_mono() -> Result<()> {
    // 合成 1 秒 440Hz 正弦波 16k 单声道
    let dir = tempfile::tempdir()?;
    let wav_path = dir.path().join("tone.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&wav_path, spec)?;
    for i in 0..16_000u32 {
        let v = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16_000.0).sin() * 10_000.0) as i16;
        writer.write_sample(v)?;
    }
    writer.finalize()?;

    let samples = asplayer_transcribe::audio::read_samples_f32(Path::new(&wav_path))?;
    assert_eq!(samples.len(), 16_000);
    assert!(samples.iter().all(|s| s.abs() <= 1.0));
    Ok(())
}
```

要让集成测试能引用 crate 内部模块，需在 Cargo.toml 定义库目标并暴露模块——修改 `crates/asplayer-transcribe/Cargo.toml` 追加：

```toml
[lib]
name = "asplayer_transcribe"
path = "src/lib.rs"
```

新建 `src/lib.rs`，把 main.rs 中的 `mod audio; mod srt;` 移到这里并改为公开：

```rust
pub mod audio;
pub mod srt;
pub mod whisper; // Task 5 创建后再加
pub mod translate; // Task 7 创建后再加
```

main.rs 删除 mod 声明，改为 `use asplayer_transcribe::{audio, srt};`。

- [ ] **Step 4: 运行测试**

Run: `cargo test -p asplayer-transcribe`
Expected: srt 3 个 + integration 1 个全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/asplayer-transcribe/
git commit -m "feat(m0): ffmpeg 抽音轨与 f32 采样读取"
```

---

### Task 5: Whisper 封装与 transcribe 子命令

**Files:**
- Create: `crates/asplayer-transcribe/src/whisper.rs`
- Modify: `crates/asplayer-transcribe/src/lib.rs`（确认注册 pub mod whisper）
- Modify: `crates/asplayer-transcribe/src/main.rs`（实现 Transcribe 分支）

- [ ] **Step 1: 实现 src/whisper.rs**

```rust
use crate::srt::Segment;
use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// 对 f32 采样做转写。language 为 None 时自动检测。
pub fn transcribe(model_path: &str, language: Option<&str>, samples: &[f32]) -> Result<Vec<Segment>> {
    let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .with_context(|| format!("加载模型失败: {model_path}"))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(language);
    params.set_print_progress(false);
    params.set_print_special(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    let mut state = ctx.create_state()?;
    state.full(params, samples).context("whisper 推理失败")?;

    let n = state.full_n_segments()?;
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        // whisper-rs 时间戳单位为 10ms（厘秒），×10 转毫秒
        let start_ms = state.full_get_segment_t0(i)? as u64 * 10;
        let end_ms = state.full_get_segment_t1(i)? as u64 * 10;
        let text = state.full_get_segment_text(i)?;
        if !text.trim().is_empty() {
            out.push(Segment { start_ms, end_ms, text });
        }
    }
    Ok(out)
}
```

- [ ] **Step 2: main.rs 实现 Transcribe 分支**

把 `Cmd::Transcribe { .. } => anyhow::bail!("Task 5 实现"),` 替换为：

```rust
Cmd::Transcribe { media, model, lang, out } => {
    let media_path = std::path::PathBuf::from(&media);
    let out_dir = std::path::PathBuf::from(&out);
    std::fs::create_dir_all(&out_dir)?;

    println!("[1/3] ffmpeg 抽取音轨…");
    let wav = audio::extract_wav(&media_path, &out_dir)?;

    println!("[2/3] whisper.cpp 转写中（模型：{model}）…");
    let samples = audio::read_samples_f32(&wav)?;
    let segments = whisper::transcribe(&model, lang.as_deref(), &samples)?;

    println!("[3/3] 写出结果（{} 段）", segments.len());
    let stem = media_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output").to_string();
    let srt_path = out_dir.join(format!("{stem}.srt"));
    let json_path = out_dir.join(format!("{stem}.segments.json"));
    std::fs::write(&srt_path, srt::render_srt(&segments))?;
    std::fs::write(&json_path, serde_json::to_string_pretty(&segments)?)?;
    println!("完成：\n  {}\n  {}", srt_path.display(), json_path.display());
}
```

并在 main.rs 的 use 行改为：

```rust
use asplayer_transcribe::{audio, srt, whisper};
```

- [ ] **Step 3: 编译 + 全部测试回归**

Run: `cargo test -p asplayer-transcribe`
Expected: 全部 PASS。

- [ ] **Step 4: 用真实样本手动验证（里程碑 0 核心验证点①）**

Run（路径按实际调整；先跑正常说话型）:
```powershell
cargo run --release -- transcribe --media "C:\Users\Yumili\.asplayer\samples\normal.mp4" --model "C:\Users\Yumili\.asplayer\models\ggml-small.bin" --out "C:\Users\Yumili\.asplayer\samples"
```
Expected: 生成 `.srt` 与 `.segments.json`，时间戳单调递增、文本语言正确。再用耳语型样本跑一次，记录识别率主观评分（能懂大意=及格）。若耳语效果差：先试 `--lang ja` 显式指定语言、再换 ggml-medium.bin 对比；仍差则按设计文档 §12 Plan B 处理，并如实记入 Task 8 评估表。

- [ ] **Step 5: Commit**

```bash
git add crates/asplayer-transcribe/
git commit -m "feat(m0): whisper.cpp 转写封装与 transcribe 子命令"
```

---

### Task 6: LLM 批量翻译模块（prompt 纯函数 TDD）

**Files:**
- Create: `crates/asplayer-transcribe/src/translate.rs`
- Modify: `crates/asplayer-transcribe/src/lib.rs`（注册 pub mod translate）

- [ ] **Step 1: 实现 src/translate.rs**

```rust
use crate::srt::Segment;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const BATCH_SIZE: usize = 25;

/// 构建一个批次的翻译 prompt（纯函数，可单测）。
/// batch 元素为 (全局序号, 原文)。
pub fn build_prompts(batch: &[(usize, &str)], context_before: &str, target_lang: &str) -> (String, String) {
    let system = concat!(
        "You are a professional subtitle translator. Translate each numbered subtitle line ",
        "into the target language naturally and conversationally, as native spoken content. ",
        "Preserve tone (including soft/whispered ASMR style). Use surrounding lines only as context. ",
        "Reply with STRICT JSON only: an object mapping each input index (as string) to the translated string. No extra keys, no commentary."
    );
    let lines: Vec<String> = batch.iter().map(|(i, t)| format!("{i}. {t}")).collect();
    let ctx = if context_before.trim().is_empty() {
        String::new()
    } else {
        format!("[Context before, do not translate]:\n{context_before}\n\n")
    };
    let user = format!(
        "{ctx}[Target language]: {target_lang}\n[Lines]:\n{}\n\nReply JSON now.",
        lines.join("\n")
    );
    (system.to_string(), user)
}

fn call_api(api_base: &str, api_key: &str, model: &str, system: &str, user: &str) -> Result<String> {
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "temperature": 0.3,
        "response_format": {"type": "json_object"}
    });
    let resp = reqwest::blocking::Client::new()
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .context("翻译 API 请求失败")?;
    let status = resp.status();
    let v: Value = resp.json().context("翻译 API 返回非 JSON")?;
    if !status.is_success() {
        bail!("翻译 API 错误 {status}: {v}");
    }
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .context("翻译 API 响应缺少 choices[0].message.content")?;
    Ok(content.to_string())
}

/// 解析模型返回的 JSON 映射，容忍代码围栏包裹；缺失的 idx 被容忍。
pub fn parse_mapping(raw: &str, expected_idx: &[usize]) -> Result<HashMap<usize, String>> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let v: Value = serde_json::from_str(cleaned).context("模型返回的不是合法 JSON")?;
    let obj = v.as_object().context("JSON 顶层不是对象")?;
    let mut map = HashMap::new();
    for idx in expected_idx {
        if let Some(s) = obj.get(idx.to_string()).and_then(Value::as_str) {
            map.insert(*idx, s.to_string());
        }
    }
    Ok(map)
}

/// 整体翻译：每批 BATCH_SIZE 句、带前 5 句上下文、解析失败自动重试至多 3 次。
pub fn translate_segments(
    segments: &[Segment],
    api_base: &str,
    api_key: &str,
    model: &str,
    target_lang: &str,
) -> Result<HashMap<usize, String>> {
    let mut result = HashMap::new();
    for chunk in segments.chunks(BATCH_SIZE) {
        let start_global = result.len();
        let batch: Vec<(usize, &str)> =
            chunk.iter().enumerate().map(|(i, s)| (start_global + i, s.text.as_str())).collect();
        let from = start_global.saturating_sub(5);
        let before = segments[from..start_global]
            .iter()
            .map(|s| s.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let (sys, usr) = build_prompts(&batch, &before, target_lang);

        let expected: Vec<usize> = batch.iter().map(|(i, _)| *i).collect();
        let mut attempt = 0;
        loop {
            attempt += 1;
            let raw = call_api(api_base, api_key, model, &sys, &usr)?;
            match parse_mapping(&raw, &expected) {
                Ok(m) => {
                    result.extend(m);
                    break;
                }
                Err(e) if attempt < 3 => eprintln!("批次解析失败（第{attempt}次），重试: {e}"),
                Err(e) => return Err(e),
            }
        }
    }
    Ok(result)
}
```

- [ ] **Step 2: 在 translate.rs 底部追加测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_contain_indices_and_context() {
        let batch = vec![(7, "おやすみ"), (8, "またね")];
        let (sys, usr) = build_prompts(&batch, "前の文", "Simplified Chinese");
        assert!(sys.contains("STRICT JSON"));
        assert!(usr.contains("[Context before"));
        assert!(usr.contains("7. おやすみ"));
        assert!(usr.contains("8. またね"));
        assert!(usr.contains("Simplified Chinese"));
    }

    #[test]
    fn parse_plain_and_fenced_json() {
        let m = parse_mapping(r#"{"0": "晚安", "1": "再见"}"#, &[0, 1]).unwrap();
        assert_eq!(m[&0], "晚安");
        let fenced = "```json\n{\"5\": \"好的\"}\n```";
        let m2 = parse_mapping(fenced, &[5]).unwrap();
        assert_eq!(m2[&5], "好的");
    }

    #[test]
    fn parse_missing_index_is_tolerated() {
        let m = parse_mapping("{\"0\":\"x\"}", &[0, 1]).unwrap();
        assert!(!m.contains_key(&1));
    }
}
```

- [ ] **Step 3: lib.rs 注册并回归测试**

确认 lib.rs 含 `pub mod translate;`。

Run: `cargo test -p asplayer-transcribe`
Expected: 全部 PASS（新增 3 个）。

- [ ] **Step 4: Commit**

```bash
git add crates/asplayer-transcribe/
git commit -m "feat(m0): LLM 批量上下文翻译（prompt构建+JSON解析+重试）"
```

---

### Task 7: Translate 子命令与双语输出

**Files:**
- Modify: `crates/asplayer-transcribe/src/main.rs`

- [ ] **Step 1: 实现 Translate 分支**

把 `Cmd::Translate { .. } => anyhow::bail!("Task 7 实现"),` 替换为：

```rust
Cmd::Translate { input, api_base, api_key, model } => {
    let raw = std::fs::read_to_string(&input)?;
    let segments: Vec<srt::Segment> = serde_json::from_str(&raw)?;
    println!("共 {} 段，开始批量翻译（每批 {} 句）…", segments.len(), translate::BATCH_SIZE);
    let map = translate::translate_segments(
        &segments, &api_base, &api_key, &model, "Simplified Chinese",
    )?;

    let mut bilingual = String::new();
    for (i, seg) in segments.iter().enumerate() {
        match map.get(&i) {
            Some(trans) => bilingual.push_str(&format!("{}\n{}\n\n", seg.text.trim(), trans)),
            None => bilingual.push_str(&format!("{}\n[未翻译]\n\n", seg.text.trim())),
        }
    }
    let out_txt = std::path::Path::new(&input).with_extension("bilingual.txt");
    std::fs::write(&out_txt, &bilingual)?;
    println!("完成：{}", out_txt.display());
}
```

并把 use 行改为：

```rust
use asplayer_transcribe::{audio, srt, translate, whisper};
```

- [ ] **Step 2: 编译 + 测试回归 + release 构建**

Run: `cargo test -p asplayer-transcribe && cargo build --release`
Expected: 全部 PASS，release 构建成功。

- [ ] **Step 3: 端到端验证（里程碑 0 核心验证点②）**

Run（用 Task 5 产物；按实际服务商填 key）:
```powershell
$env:ASPLAYER_API_BASE="https://api.openai.com/v1"; $env:ASPLAYER_API_KEY="sk-..."
cargo run --release -- translate --input "C:\Users\Yumili\.asplayer\samples\normal.segments.json" --model gpt-4o-mini
```
Expected: 生成 `.bilingual.txt`；人工评估译文自然、无漏翻、上下文连贯（尤其耳语语气词）。

- [ ] **Step 4: Commit**

```bash
git add crates/asplayer-transcribe/src/main.rs
git commit -m "feat(m0): translate 子命令输出双语对照文本"
```

---

### Task 8: 里程碑 0 验收

**Files:**
- Create: `docs/milestone-0-evaluation.md`

- [ ] **Step 1: 三类样本完整过管线**

对耳语型 / 正常说话型 / 带 BGM 型各执行一次 Task 5 的 transcribe 与 Task 7 的 translate。

- [ ] **Step 2: 记录评估表 docs/milestone-0-evaluation.md**

```markdown
# 里程碑 0 转写验证评估

| 样本 | 类型 | 模型 | 耗时 | 识别率(1-5) | 翻译质量(1-5) | 备注 |
|---|---|---|---|---|---|---|
| sample-whisper | 耳语型 | ggml-small | | | | |
| sample-normal | 正常型 | ggml-small | | | | |
| sample-bgm | 带BGM | ggml-small | | | | |

## 结论
[通过/未通过] 及理由；若触发 Plan B 写明方向。
```

**通过标准**：正常说话型 ≥3 分（能理解大意），耳语型 ≥2 分且 Plan B 方向明确。未达标则停止，先讨论 §12 Plan B（loudnorm 预处理 / 更大模型 / faster-whisper 引擎），不得进入 M1。

- [ ] **Step 3: 提交并打标签**

```bash
git add docs/milestone-0-evaluation.md
git commit -m "docs: 里程碑0 转写验证评估记录"
git tag milestone-0
```

---

## Self-Review 记录

1. **规格覆盖**：本计划仅覆盖设计文档 §12"里程碑0"，是多计划序列的第一个（M1 Tauri骨架+媒体库 → M2 字幕管线入应用 → M3 悬浮窗+全局快捷键 → M4 播放模式/主题/i18n/外部字幕），后续另立计划 ✅
2. **占位符扫描**：无 TBD/TODO/"适当处理"；所有代码步骤含完整代码 ✅
3. **类型一致性**：`Segment{start_ms,end_ms,text}` 在 srt/whisper/translate/main 一致；`read_samples_f32` 返回 `Vec<f32>` 与 whisper 入参一致；translate 返回 `HashMap<usize,String>` 以全局序号为键，main.rs 按 enumerate 序号取用一致 ✅
4. **已标注的已知风险**：whisper-rs 小版本 API 可能变动（Task 2 注意事项）、时间戳厘秒单位换算（Task 5 注释）


