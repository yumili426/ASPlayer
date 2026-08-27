# 转写切片化（批处理内部切片 + 块间可取消/续跑） Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 off-line 转写从「一次 `state.full(全量)`」改为「VAD 静音切块 → whisper 逐步解码 → 每块即时幂等回写 → 块间可取消、中断后从断点续跑」，落实主设计 §8 的原意。

**Architecture:** `asplayer-transcribe` 新增纯函数 `vad::split_samples`（能量阈值静音切块，`max_chunk_ms` 硬切兜底）；`whisper.rs` 拆成 `Whisper::load`（模型只载一次）+ `Whisper::transcribe`（逐块复用）。app 层 `transcriber.rs` 负责编排：循环块、块间取消检查、时间戳偏移、锁内幂等 upsert、断点持久化、逐块进度事件。取消落地进 `partial` 状态，可续跑。

**Tech Stack:** Rust（rusqlite、tauri、reqwest 已就位）、whisper-rs 0.13、Vue 3 + Pinia。cargo 用 `$HOME/.cargo/bin/cargo`（不在 PATH）。

**环境注意**：`cargo` / `node` / `npm` 不在 PATH。跑测试用 `"$HOME/.cargo/bin/cargo" test -p <crate>`；前端类型检查用 `"$HOME/.cargo/bin/cargo" ... ` 之外，跑 `npx vue-tsc` 需先补 PATH：`export PATH="/c/Program Files/nodejs:$PATH"`。

---

## 文件结构

**新建**
- `crates/asplayer-transcribe/src/vad.rs` — 切块纯函数 + 单测。

**修改**
- `crates/asplayer-transcribe/src/lib.rs` — `pub mod vad;`。
- `crates/asplayer-transcribe/src/whisper.rs` — 拆 `Whisper` 结构体（load/transcribe），保留自由函数 `transcribe`。
- `crates/asplayer-transcribe/src/main.rs` — 复用改动后的自由函数（无行为变化）。
- `app/src-tauri/src/db.rs` — 迁移（`transcribe_next_ms` 列 + `subtitles` UNIQUE 去重）、断点读写、`list_media` 增字段、删 `rollback_after_cancel`。
- `app/src-tauri/src/media.rs` — `MediaItem` 加 `transcribe_next_ms` 字段。
- `app/src-tauri/src/transcriber.rs` — 分块编排、partial 取消、断点、进度。
- `app/src-tauri/src/lib.rs` — `transcribe_media(resume)` 签名、`cancel_transcribe` 崩溃恢复改 partial。
- `app/src/types.ts` — `subtitle_status` 加 `partial`；`MediaItem` 加 `transcribe_next_ms`。
- `app/src/api/subtitle.ts` — `transcribeMedia(id, lang?, resume?)`。
- `app/src/components/PlayerStage.vue` — 工具栏「从断点继续」按钮 + `doTranscribe` 传 resume。
- `app/src/App.vue` — `onTranscribeCanceled` 置 `partial`。
- `app/src/components/SettingsPanel.vue` — 新增「转写切片」4 个参数输入。

**测试断言**贯穿：vad.rs 单测（crate）；db.rs 迁移/幂等/断点（`cargo test -p app`）。

---

## Task 1: `vad.rs` 静音切块（纯函数 + 单测）

**Files:**
- Create: `crates/asplayer-transcribe/src/vad.rs`
- Modify: `crates/asplayer-transcribe/src/lib.rs:1-4`（加 `pub mod vad;`）

**设计要点**：自适应阈值用 **P90 参考电平 × 0.1**（非 P10——均匀响度无静音时 P10 会把全部帧误判为静音，P90 × 0.1 则正常）；静音孔在孔中点切一刀；忽略贴近首/末的边缘孔（避免切出纯静音块）；`< min_chunk_ms` 并入前块；`> max_chunk_ms` 硬切。

- [ ] **Step 1: 加模块声明**

修改 `crates/asplayer-transcribe/src/lib.rs`，使 `vad` 成为公共模块：

```rust
pub mod audio;
pub mod srt;
pub mod vad;
pub mod whisper;
pub mod translate;
```

- [ ] **Step 2: 写 `vad.rs` 实现**

写 `crates/asplayer-transcribe/src/vad.rs`：

```rust
/// 静音切块（能量阈值 VAD）。纯函数，可单测。

/// 一块音频区间（样本半开区间 + 毫秒）。块间首尾相接覆盖整段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub start_sample: usize,
    pub end_sample: usize,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// VAD 切块参数。`Default` 为面向 ASMR 的预设。
#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    pub sample_rate: u32,
    pub window_ms: u32,
    pub min_silence_ms: u32,
    pub min_chunk_ms: i64,
    pub max_chunk_ms: i64,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            window_ms: 30,
            min_silence_ms: 300,
            min_chunk_ms: 1000,
            max_chunk_ms: 30000,
        }
    }
}

/// 能量阈值静音切块：静音孔为主，`max_chunk_ms` 硬切兜底，`min_chunk_ms` 并入前块。
/// 返回首尾相接、覆盖整段音频的块；空音频返回空 Vec。
pub fn split_samples(samples: &[f32], cfg: &VadConfig) -> Vec<Chunk> {
    if samples.is_empty() {
        return Vec::new();
    }
    let total = samples.len();
    let ms = |s: usize| (s as i64 * 1000) / cfg.sample_rate as i64;
    let sms = |m: i64| ((m * cfg.sample_rate as i64).div_euclid(1000)) as usize;

    if ms(total) <= cfg.min_chunk_ms {
        return vec![Chunk {
            start_sample: 0,
            end_sample: total,
            start_ms: 0,
            end_ms: ms(total),
        }];
    }

    let win = ((cfg.window_ms as i64 * cfg.sample_rate as i64) / 1000) as usize;
    let win = win.max(1);

    // 逐帧 RMS + 帧起点样本
    let mut rms: Vec<f32> = Vec::new();
    let mut fstart: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < total {
        fstart.push(i);
        let end = (i + win).min(total);
        let c = &samples[i..end];
        let m = (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt();
        rms.push(m);
        i = end;
    }

    // 自适应阈值：P90 参考电平 × 0.1，带绝对下限。
    // 用 P90 而非 P10：均匀响度（无静音）时 P10 会把全部帧误判为静音，P90 × 0.1 则正常。
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p90 = sorted[(sorted.len() - 1) * 9 / 10];
    let threshold = (p90 * 0.1).max(1e-5);

    // 静音孔 → 在孔中点切一刀；忽略贴近首/末的边缘孔（避免切出纯静音块）
    let edge_guard = sms(cfg.min_chunk_ms);
    let mut cuts: Vec<usize> = Vec::new();
    let mut k = 0;
    while k < rms.len() {
        if rms[k] >= threshold {
            k += 1;
            continue;
        }
        let s0 = k;
        while k < rms.len() && rms[k] < threshold {
            k += 1;
        }
        let gap_frames = k - s0;
        if gap_frames as i64 * cfg.window_ms as i64 >= cfg.min_silence_ms as i64 {
            let mid = fstart[s0 + gap_frames / 2];
            if mid >= edge_guard && mid <= total.saturating_sub(edge_guard) {
                cuts.push(mid);
            }
        }
    }

    // 由切点构造原始块
    let mut bounds = vec![0];
    for c in cuts {
        if *bounds.last().unwrap() < c && c < total {
            bounds.push(c);
        }
    }
    bounds.push(total);
    let raw: Vec<Chunk> = bounds
        .windows(2)
        .map(|w| Chunk {
            start_sample: w[0],
            end_sample: w[1],
            start_ms: ms(w[0]),
            end_ms: ms(w[1]),
        })
        .collect();

    // 合并 < min_chunk_ms 的块到前块
    let mut merged: Vec<Chunk> = Vec::new();
    for c in raw {
        if let Some(last) = merged.last_mut() {
            if c.end_ms - c.start_ms < cfg.min_chunk_ms {
                last.end_sample = c.end_sample;
                last.end_ms = c.end_ms;
                continue;
            }
        }
        merged.push(c);
    }

    // 拆分 > max_chunk_ms 的块（硬切）
    let mut out: Vec<Chunk> = Vec::new();
    for c in merged {
        let len = c.end_ms - c.start_ms;
        if len <= cfg.max_chunk_ms {
            out.push(c);
            continue;
        }
        let mut a = c.start_sample;
        loop {
            let a_ms = ms(a);
            let b = sms((a_ms + cfg.max_chunk_ms).min(ms(c.end_sample)))
                .max(a + 1)
                .min(c.end_sample);
            out.push(Chunk {
                start_sample: a,
                end_sample: b,
                start_ms: ms(a),
                end_ms: ms(b),
            });
            if b >= c.end_sample {
                break;
            }
            a = b;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 16000;

    fn silence_ms(ms: i64) -> Vec<f32> {
        vec![0.0; (ms * SR as i64 / 1000) as usize]
    }
    fn tone_ms(ms: i64, freq: f32) -> Vec<f32> {
        let n = (ms * SR as i64 / 1000) as usize;
        (0..n).map(|i| 0.6 * (2.0 * PI * freq * i as f32 / SR as f32).sin()).collect()
    }

    #[test]
    fn empty_returns_empty() {
        assert_eq!(split_samples(&[], &VadConfig::default()), vec![]);
    }

    #[test]
    fn tiny_input_returns_single_chunk() {
        let s = tone_ms(500, 440.0); // 500ms < min_chunk 1000ms
        let chunks = split_samples(&s, &VadConfig::default());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_sample, 0);
        assert_eq!(chunks[0].end_sample, s.len());
        assert_eq!(chunks[0].start_ms, 0);
        assert_eq!(chunks[0].end_ms, 500);
    }

    #[test]
    fn splits_at_silence_gap() {
        let mut s = tone_ms(1000, 440.0);
        s.extend(silence_ms(500)); // 1.0s~1.5s 静音，≥ min_silence 300ms
        s.extend(tone_ms(1000, 440.0));
        let chunks = split_samples(&s, &VadConfig::default());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].start_ms, 0);
        assert_eq!(chunks[1].end_ms, 2500);
        // 静音孔 1.0s~1.5s 的中点约 1.25s，放宽容差
        assert!((1200..=1300).contains(&chunks[0].end_ms), "cut at {}", chunks[0].end_ms);
        assert_eq!(chunks[0].end_sample, chunks[1].start_sample);
    }

    #[test]
    fn short_silence_not_split() {
        let mut s = tone_ms(1000, 440.0);
        s.extend(silence_ms(200)); // < min_silence 300ms
        s.extend(tone_ms(1000, 440.0));
        let chunks = split_samples(&s, &VadConfig::default());
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn hard_split_max_chunk() {
        let s = tone_ms(90000, 440.0); // 90s 连续，无静音
        let chunks = split_samples(&s, &VadConfig::default());
        assert!(chunks.len() >= 3, "got {}", chunks.len());
        for c in &chunks {
            assert!(c.end_ms - c.start_ms <= 31000, "block too long: {}ms", c.end_ms - c.start_ms);
        }
        assert_eq!(chunks.last().unwrap().end_ms, 90000);
        // 首尾相接覆盖整段
        let mut prev = 0;
        for c in &chunks {
            assert_eq!(c.start_sample, prev);
            prev = c.end_sample;
        }
        assert_eq!(prev, s.len());
    }

    #[test]
    fn long_edge_silence_still_single_or_split_not_truncated() {
        // 首尾静音很长不重新切；首块必须从 0 开始、被完整覆盖
        let mut s = silence_ms(2000);
        s.extend(tone_ms(2000, 440.0));
        s.extend(silence_ms(2000));
        let chunks = split_samples(&s, &VadConfig::default());
        assert!(chunks.len() >= 1);
        assert_eq!(chunks[0].start_ms, 0);
        let mut prev = 0;
        for c in &chunks {
            assert_eq!(c.start_sample, prev);
            prev = c.end_sample;
        }
        assert_eq!(prev, s.len());
    }
}
```

- [ ] **Step 3: 运行测试验证失败/通过**

Run:
```bash
cd "d:/Coding Projects/ASPlayer" && "$HOME/.cargo/bin/cargo" test -p asplayer-transcribe vad
```
Expected: 编译通过，`vad` 的 6 个测试全 PASS。

- [ ] **Step 4: 提交**

```bash
git add crates/asplayer-transcribe/src/vad.rs crates/asplayer-transcribe/src/lib.rs
git commit -m "feat(transcribe): 静音切块 vad::split_samples（能量阈值 + 最大时长硬切）"
```

---

## Task 2: `whisper.rs` 拆分为 `Whisper`（模型只载一次）

**Files:**
- Modify: `crates/asplayer-transcribe/src/whisper.rs`

**驱动**：`WhisperContext::new_with_params` 会载入 GGUF（small≈466MB）。分块后绝不能每块重建 context，必须「载一次模型 + 逐块 `transcribe`」。

- [ ] **Step 1: 重写 `whisper.rs`**

将 `crates/asplayer-transcribe/src/whisper.rs` 全文替换为：

```rust
use crate::srt::Segment;
use anyhow::{Context, Result};
use std::sync::Once;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, whisper_rs_sys,
};

/// whisper-rs-sys 在 debug/force-debug 构建下会用 `-DWHISPER_DEBUG` 编译 whisper.cpp，
/// 使每条 token 推理都走 `WHISPER_LOG_DEBUG` 打日志，刷屏且无诊断价值。此回调全局安装一次，
/// 丢弃 DEBUG 级、其余（ERROR/WARN/INFO）照旧写 stderr，行为与默认回调一致。
pub fn setup_log_filter() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        whisper_rs::set_log_callback(Some(drop_debug_logs), std::ptr::null_mut());
    });
}

unsafe extern "C" fn drop_debug_logs(
    level: whisper_rs_sys::ggml_log_level,
    text: *const std::os::raw::c_char,
    _user_data: *mut std::os::raw::c_void,
) {
    if level == whisper_rs_sys::ggml_log_level_GGML_LOG_LEVEL_DEBUG {
        return;
    }
    if text.is_null() {
        return;
    }
    // SAFETY: whisper.cpp 保证传递的是以 \0 结尾的合法字符串。
    let msg = unsafe { std::ffi::CStr::from_ptr(text) }.to_string_lossy();
    eprint!("{msg}");
}

/// 复用同一个 `WhisperContext`（模型只载一次）逐块解码。
pub struct Whisper {
    ctx: WhisperContext,
}

impl Whisper {
    /// 载入模型一次（含安装日志过滤）。整个转写任务只调用这一次。
    pub fn load(model_path: &str) -> Result<Self> {
        setup_log_filter();
        let ctx = WhisperContext::new_with_params(
            model_path,
            WhisperContextParameters::default(),
        )
        .with_context(|| format!("加载模型失败: {model_path}"))?;
        Ok(Self { ctx })
    }

    /// 对单块 samples 解码，返回「相对该块起点」的段时间戳（毫秒，0 基）。
    /// 每个块用独立的 `WhisperState`，块间无上下文串扰。绝对偏移由调用方统一加。
    pub fn transcribe(&mut self, language: Option<&str>, samples: &[f32]) -> Result<Vec<Segment>> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(language);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let mut state = self.ctx.create_state()?;
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
}

/// 便捷封装：载模型一次并转写整段（CLI / 单次调用用）。
pub fn transcribe(
    model_path: &str,
    language: Option<&str>,
    samples: &[f32],
) -> Result<Vec<Segment>> {
    Whisper::load(model_path)?.transcribe(language, samples)
}
```

- [ ] **Step 2: 编译验证**

Run:
```bash
cd "d:/Coding Projects/ASPlayer" && "$HOME/.cargo/bin/cargo" build -p asplayer-transcribe
```
Expected: 编译通过（`main.rs` 仍调用自由函数 `transcribe`，无需改动）。

- [ ] **Step 3: 提交**

```bash
git add crates/asplayer-transcribe/src/whisper.rs
git commit -m "refactor(transcribe): 拆分 Whisper::load + Whisper::transcribe（模型只载一次）"
```

---

## Task 3: DB 迁移（断点列 + subtitles UNIQUE）+ 断点读写

**Files:**
- Modify: `app/src-tauri/src/db.rs`
- Modify: `app/src-tauri/src/media.rs`

**驱动**：续跑要求「增量幂等写、不重复」。`subtitles` 目前无 UNIQUE，`save_subtitle` 的 `ON CONFLICT DO UPDATE`（[db.rs:227](app/src-tauri/src/db.rs#L227)）从未触发，现靠「先 clear 再插」防重。必须补 `UNIQUE(media_id, start_ms)` 并去重；同时加 `transcribe_next_ms` 列存断点。

- [ ] **Step 1: 重写 `db.rs` 相关片段**

(a) 在 `init` 结尾（`Self::migrate_playback_params(conn)` 之后）追加去重+唯一索引迁移：

```rust
        // 迁移已存在的旧库：为其补充 M2 新增的列（CREATE TABLE IF NOT EXISTS 不会改旧表）
        Self::migrate(conn)?;
        Self::migrate_playback_params(conn)?;
        Self::migrate_transcribe(conn)
```

(b) 在 `migrate` 函数内（其在 `if !cols.iter().any(|c| c == "file_size") { ... }` 块之后）追加 `transcribe_next_ms` 列补列：

```rust
        if !cols.iter().any(|c| c == "transcribe_next_ms") {
            conn.execute_batch(
                "ALTER TABLE media_files ADD COLUMN transcribe_next_ms INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        Ok(())
    }
```

> 注：上面的 `Ok(())` 是 `migrate` 函数现有的收尾，`transcribe_next_ms` 补列应放在该 `Ok(())` 之前、`file_size` 补列之后。

(c) 新增迁移方法 + 断点读写方法（插在 `all_settings` 之后、`use std::path::Path;` 之前）：

```rust
    /// 幂等迁移：给 subtitles 补 UNIQUE(media_id, start_ms)（先按该组合去重）。
    /// 这是 save_subtitle 的 ON CONFLICT DO UPDATE 真正生效、支撑续跑幂等写的前提。
    pub fn migrate_transcribe(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "DELETE FROM subtitles WHERE id NOT IN (
                SELECT MIN(id) FROM subtitles GROUP BY media_id, start_ms
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_sub_unique ON subtitles(media_id, start_ms);",
        )?;
        Ok(())
    }

    /// 读取某媒体已转写的音频毫秒断点（0 = 无断点）
    pub fn get_transcribe_next_ms(&self, id: i64) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT transcribe_next_ms FROM media_files WHERE id = ?1",
            [&id.to_string()],
            |r| r.get(0),
        )
    }

    /// 写入某媒体的转写断点（完成后置 0 清除）
    pub fn set_transcribe_next_ms(&self, id: i64, next_ms: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_files SET transcribe_next_ms = ?1 WHERE id = ?2",
            rusqlite::params![next_ms, id],
        )?;
        Ok(())
    }
```

(d) `list_media` 的 SELECT 与构造：加 `m.transcribe_next_ms`。将 SELECT 第 11 列之后追加 `m.transcribe_next_ms`，并给 `MediaItem` 加对应字段（见 Step 2）。SELECT 现在变成：

```rust
            "SELECT m.id, m.path, m.title, m.media_type, m.duration_ms, m.playback_position,
                    m.file_size, m.subtitle_status, m.subtitle_lang,
                    (SELECT COUNT(*) FROM subtitles s WHERE s.media_id = m.id),
                    COALESCE(m.speed, 1.0), COALESCE(m.volume, 1.0),
                    m.transcribe_next_ms
             FROM media_files m ORDER BY m.added_at DESC, m.id DESC",
```
构造处追加：`transcribe_next_ms: r.get(12)?,`

(e) 删除 `rollback_after_cancel` 方法（已无调用，语义被 partial 逻辑取代）。删除该 `pub fn rollback_after_cancel` 整段。

- [ ] **Step 2: `media.rs` 加字段**

```rust
    pub subtitle_count: i64,     // 已转写字幕段数
    pub transcribe_next_ms: i64, // 转写断点（0 = 无）
    pub speed: f64,              // 每文件记住的播放速度（默认 1.0）
```

> 在 `subtitle_count` 与 `speed` 两行之间插入。

- [ ] **Step 3: 跑 `app` 现有测试（迁移兼容 + 新列）**

Run:
```bash
cd "d:/Coding Projects/ASPlayer" && "$HOME/.cargo/bin/cargo" test -p app
```
Expected: 现有 18+ 测试全 PASS（`subtitle_status_roundtrip`、`list_media_includes_subtitle_fields` 等仍通过）。

- [ ] **Step 4: 提交**

```bash
git add app/src-tauri/src/db.rs app/src-tauri/src/media.rs
git commit -m "feat(db): media_files 增 transcribe_next_ms；subtitles 补 UNIQUE 去重；移除 rollback_after_cancel"
```

---

## Task 4: 分块编排（transcriber.rs）

**Files:**
- Modify: `app/src-tauri/src/transcriber.rs`

**行为**：循环块 → 块间取消检查 → 锁外 `whisper.transcribe` → 时间戳加 `chunk.start_ms` → 锁内幂等 upsert + 更新断点 → 逐块进度。取消落地 `partial`。

- [ ] **Step 1: 加辅助函数 + 重写取消检查**

在 `transcribe_inner` 之前插入 `vad_config`，并删除 `check_canceled`：

```rust
/// 从 DB 设置读取 VAD 切块参数（缺省回退默认值）
fn vad_config(db: &MediaDb) -> asplayer_transcribe::vad::VadConfig {
    let mut cfg = asplayer_transcribe::vad::VadConfig::default();
    if let Some(v) = db.get_setting("vad_window_ms").ok().flatten() {
        if let Ok(n) = v.parse() { cfg.window_ms = n; }
    }
    if let Some(v) = db.get_setting("vad_min_silence_ms").ok().flatten() {
        if let Ok(n) = v.parse() { cfg.min_silence_ms = n; }
    }
    if let Some(v) = db.get_setting("vad_min_chunk_ms").ok().flatten() {
        if let Ok(n) = v.parse() { cfg.min_chunk_ms = n; }
    }
    if let Some(v) = db.get_setting("vad_max_chunk_ms").ok().flatten() {
        if let Ok(n) = v.parse() { cfg.max_chunk_ms = n; }
    }
    cfg
}
```

> 若 `check_canceled` 函数仍存在则删除它（分块循环用内联检查）。

- [ ] **Step 2: 重写 `transcribe_inner` 主体**

将 `transcribe_inner` 整段替换为：

```rust
/// 转写步骤主体（锁外耗时操作 + 数据库短锁）。resume=true 从断点继续，false 清空重转。
fn transcribe_inner(
    app: &AppHandle,
    db: &Arc<Mutex<MediaDb>>,
    media_id: i64,
    lang_str: &str,
    resume: bool,
    path: &str,
    tmp: &std::path::Path,
) -> Result<(), TranscribeStop> {
    let lang_opt = if lang_str.is_empty() { None } else { Some(lang_str) };

    // 清空/断点策略：fresh 则清字幕与断点；resume 保持已有断点
    let _ = with_db(db, |d| {
        if !resume {
            d.clear_subtitles(media_id)?;
            d.set_transcribe_next_ms(media_id, 0)?;
        }
        d.set_subtitle_status(media_id, "transcribing", lang_str)
    })
    .map_err(|e| fail(db, media_id, format!("数据库异常: {e}")))?;
    emit_progress(app, media_id, "extract", 5, "抽取音轨…");

    // 抽音轨到临时目录
    let _ = std::fs::create_dir_all(tmp);
    let wav = match asplayer_transcribe::audio::extract_wav(&PathBuf::from(path), tmp) {
        Ok(w) => w,
        Err(e) => return Err(fail(db, media_id, format!("抽音轨失败: {e}"))),
    };
    if !transcription_running(media_id) {
        return Err(TranscribeStop::Canceled);
    }

    emit_progress(app, media_id, "transcribe", 15, "Whisper 转写准备…");
    let samples = match asplayer_transcribe::audio::read_samples_f32(&wav) {
        Ok(s) => s,
        Err(e) => return Err(fail(db, media_id, format!("读取音频失败: {e}"))),
    };
    if !transcription_running(media_id) {
        return Err(TranscribeStop::Canceled);
    }

    let (model, cfg) = {
        let g = db.lock().map_err(|e| fail(db, media_id, format!("数据库锁异常: {e}")))?;
        (
            crate::models::resolve_model_path(&g).to_string_lossy().into_owned(),
            vad_config(&g),
        )
    };

    let chunks = asplayer_transcribe::vad::split_samples(&samples, &cfg);
    if chunks.is_empty() {
        return Err(fail(db, media_id, "音频为空，无法转写".to_string()));
    }
    let total = chunks.len();

    // 断点：跳过 end_ms <= next_ms 的已完成块
    let next_ms = with_db(db, |d| d.get_transcribe_next_ms(media_id)).unwrap_or(0);
    let start_idx = chunks.iter().position(|c| c.end_ms > next_ms).unwrap_or(total);

    let mut whisper = match asplayer_transcribe::whisper::Whisper::load(&model) {
        Ok(w) => w,
        Err(e) => return Err(fail(db, media_id, format!("加载模型失败: {e}"))),
    };

    for idx in start_idx..total {
        // 块间检查点：取消最迟在一个块内生效
        if !transcription_running(media_id) {
            return Err(TranscribeStop::Canceled);
        }
        let c = chunks[idx];
        let chunk_samples = &samples[c.start_sample..c.end_sample];
        let segs = match whisper.transcribe(lang_opt, chunk_samples) {
            Ok(s) => s,
            Err(e) => return Err(fail(db, media_id, format!("转写失败: {e}"))),
        };

        // 时间戳偏移（块内相对 → 绝对毫秒）+ 锁内幂等回写 + 断点推进
        let _ = with_db(db, |d| {
            for (i, s) in segs.iter().enumerate() {
                d.save_subtitle(
                    media_id,
                    s.start_ms as i64 + c.start_ms,
                    s.end_ms as i64 + c.start_ms,
                    &s.text,
                    "",
                    i as i64,
                )?;
            }
            d.set_transcribe_next_ms(media_id, c.end_ms)?;
            Ok(())
        })
        .map_err(|e| fail(db, media_id, format!("回写字幕失败: {e}")))?;

        let prog = 15 + 65 * (idx + 1) as u64 / total as u64;
        emit_progress(app, media_id, "transcribe", prog as u8, &format!("转写 {}/{} 块", idx + 1, total));
    }

    // 全部完成：清断点，置 done
    let _ = with_db(db, |d| {
        d.set_transcribe_next_ms(media_id, 0)?;
        d.set_subtitle_status(media_id, "done", lang_str)
    })
    .map_err(|e| fail(db, media_id, format!("数据库异常: {e}")))?;
    Ok(())
}
```

- [ ] **Step 3: 改 `run_transcription`（签名 + Canceled 分支）**

将 `run_transcription` 的签名与调用改掉；`lang` 保持可用（算一次 `lang_str`）。`transcribe_inner(..., &lang_str, ...)`，`Err(Canceled)` 分支按断点落地 `partial`/`none`：

```rust
/// 转写任务（后台线程调用）：抽音轨 → VAD 切块 → 逐块 whisper → 逐块写库 + 断点 → done。
/// 取消落地：有断点 → partial（可续跑）；无断点 → none。
pub fn run_transcription(
    app: AppHandle,
    db: Arc<Mutex<MediaDb>>,
    media_id: i64,
    lang: Option<String>,
    resume: bool,
) {
    // 同一媒体同时只允许一个转写任务
    if !register_transcription(media_id) {
        let _ = app.emit(EVENT_ERROR, format!("媒体 #{media_id} 已有转写任务在进行中"));
        return;
    }

    let path = match with_db(&db, |d| d.media_path(media_id)).map(|v| v.0) {
        Ok(p) => p,
        Err(e) => {
            unregister_transcription(media_id);
            let _ = app.emit(EVENT_ERROR, format!("找不到媒体: {e}"));
            return;
        }
    };

    let lang_str = lang.unwrap_or_default();
    let tmp = std::env::temp_dir().join(format!("asplayer-{media_id}"));
    let result = transcribe_inner(&app, &db, media_id, &lang_str, resume, &path, &tmp);

    let _ = std::fs::remove_dir_all(&tmp);
    unregister_transcription(media_id);

    match result {
        Ok(()) => {
            emit_progress(&app, media_id, "done", 100, "转写完成");
            let _ = app.emit(EVENT_DONE, media_id);
        }
        Err(TranscribeStop::Canceled) => {
            // 有断点 → partial（保留已完成块，可续跑）；否则 none
            let next = with_db(&db, |d| d.get_transcribe_next_ms(media_id)).unwrap_or(0);
            let _ = with_db(&db, |d| {
                if next > 0 {
                    d.set_subtitle_status(media_id, "partial", &lang_str)
                } else {
                    d.set_subtitle_status(media_id, "none", "")
                }
            });
            let _ = app.emit(EVENT_CANCELED, media_id);
        }
        Err(TranscribeStop::Error(msg)) => {
            let _ = app.emit(EVENT_ERROR, msg);
        }
    }
}
```

- [ ] **Step 4: 编译验证**

Run:
```bash
cd "d:/Coding Projects/ASPlayer" && "$HOME/.cargo/bin/cargo" build -p app
```
Expected: 编译通过（`lib.rs` 调用签名尚未改，此处 `build -p app` 会因 `run_transcription` 多一个参数报错——**先改 Task 5 再一起编译**；若流程需要，可先用 `cargo check -p asplayer-transcribe` 单独确认 crate 无碍）。为便于自测，直接进入 Task 5 后统一构建。

- [ ] **Step 5: 提交（与 Task 5 一起）**

---

## Task 5: 命令签名（transcribe_media(resume) + cancel_transcribe 崩溃恢复 partial）

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: 改 `transcribe_media`**

```rust
/// 触发后台转写（立即返回，进度/结果走事件）。resume=true 从断点继续，false 清空重转。
#[tauri::command]
fn transcribe_media(
    id: i64,
    lang: Option<String>,
    resume: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    // 同一媒体同时只允许一个转写任务（后台线程内也会兜底校验）
    if transcriber::transcription_running(id) {
        return Err("该媒体已有转写任务在进行中".into());
    }
    let db = state.db.clone();
    std::thread::spawn(move || {
        transcriber::run_transcription(app, db, id, lang, resume);
    });
    Ok(())
}
```

- [ ] **Step 2: 改 `cancel_transcribe`（崩溃恢复走 partial/none）**

```rust
/// 请求取消某媒体的转写任务。whisper 逐块解码，取消在一个块粒度内生效。
/// 落地：有断点 → partial（保留已转写块，可续跑）；无断点 → none。
/// 返回 true 表示取消请求已受理（或本就无任务、顺手修正了残留状态）。
#[tauri::command]
fn cancel_transcribe(id: i64, state: State<AppState>) -> CmdResult<bool> {
    if transcriber::transcription_running(id) {
        transcriber::request_cancel_transcription(id);
        return Ok(true);
    }
    // 不在运行中：修正残留的 transcribing 状态（崩溃恢复）→ 有断点置 partial，否则 none
    let db = state.db.lock().map_err(err_str)?;
    let (status, _) = db.get_subtitle_status(id).map_err(err_str)?;
    if status == "transcribing" {
        let next = db.get_transcribe_next_ms(id).map_err(err_str)?;
        if next > 0 {
            db.set_subtitle_status(id, "partial", "").map_err(err_str)?;
        } else {
            db.set_subtitle_status(id, "none", "").map_err(err_str)?;
        }
        return Ok(true);
    }
    Ok(false)
}
```

- [ ] **Step 3: 统一编译验证**

Run:
```bash
cd "d:/Coding Projects/ASPlayer" && "$HOME/.cargo/bin/cargo" build
```
Expected: `app` 与 `asplayer-transcribe` 均编译通过。

- [ ] **Step 4: 跑 `app` 测试**

Run:
```bash
cd "d:/Coding Projects/ASPlayer" && "$HOME/.cargo/bin/cargo" test -p app
```
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add app/src-tauri/src/transcriber.rs app/src-tauri/src/lib.rs
git commit -m "feat(transcribe): 分块编排 + 块间取消/断点续跑；transcribe_media 增 resume；取消落地 partial"
```

---

## Task 6: 前端 — partial 状态与「从断点继续」

**Files:**
- Modify: `app/src/types.ts`
- Modify: `app/src/api/subtitle.ts`
- Modify: `app/src/components/PlayerStage.vue`
- Modify: `app/src/App.vue`

- [ ] **Step 1: `types.ts` 加 `partial` 与 `transcribe_next_ms`**

```ts
export interface MediaItem {
  id: number;
  path: string;
  title: string;
  media_type: "video" | "audio";
  duration_ms: number;
  playback_position: number;
  file_size: number;
  subtitle_status: "none" | "transcribing" | "done" | "error" | "translating" | "partial";
  subtitle_lang: string;
  subtitle_count: number;
  transcribe_next_ms: number;
  speed: number;
  volume: number;
}
```

> 在 `subtitle_count` 与 `speed` 之间加 `transcribe_next_ms: number;`。

- [ ] **Step 2: `api/subtitle.ts` 加 `resume` 参数**

```ts
export function transcribeMedia(id: number, lang?: string, resume = false) {
  return invoke<void>("transcribe_media", { id, resume, ...(lang ? { lang } : {}) });
}
```

- [ ] **Step 3: `PlayerStage.vue` — `doTranscribe` 传 resume + 工具栏「继续」按钮**

将 `doTranscribe` 改为：

```ts
async function doTranscribe(withTranslate: boolean, resume = false) {
  if (!props.item || transcribing.value) return;
  sub.setStatus("transcribing", "transcribe", 0, resume ? "从断点继续转写…" : "正在转写…");
  if (withTranslate) sub.requestAutoTranslate(props.item.id);
  try {
    await transcribeMedia(props.item.id, undefined, resume);
  } catch (e) {
    console.error("[ASPlayer] 转写启动失败:", e);
    sub.setStatus("error", "", 0, String(e));
  }
}
```

在工具栏「转写」按钮（`@click="doTranscribe(false)"` 那一行）之前，插入「从断点继续」按钮：

```html
<button
  v-if="item && item.subtitle_status === 'partial'"
  class="iconbtn"
  title="从断点继续转写"
  @click="doTranscribe(false, true)"
><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 4v16M20 6l-10 6 10 6z"/></svg></button>
```

> `props.item` 在模板里写作 `item`（PlayerStage 已解构或 `props.item`，两处皆可，模板用 `item`）。

- [ ] **Step 4: `App.vue` — `onTranscribeCanceled` 置 partial**

把 `sub.setStatus("none", "canceled", 0, "已取消转写");` 改为：

```ts
sub.setStatus("partial", "canceled", sub.progress.value, "已取消，可继续转写");
```

- [ ] **Step 5: 前端类型检查**

Run:
```bash
export PATH="/c/Program Files/nodejs:$PATH" && cd "d:/Coding Projects/ASPlayer/app" && npx vue-tsc --noEmit
```
Expected: 0 error（`MediaItem.transcribe_next_ms` 被 `PlayerStage`/`App` 使用，一致）。

- [ ] **Step 6: 提交**

```bash
git add app/src/types.ts app/src/api/subtitle.ts app/src/components/PlayerStage.vue app/src/App.vue
git commit -m "feat(ui): partial 状态 + 工具栏「从断点继续转写」按钮"
```

---

## Task 7: 设置面板「转写切片」参数

**Files:**
- Modify: `app/src/components/SettingsPanel.vue`

**说明**：后端 `vad_config(db)` 已按设置键 `vad_window_ms` 等读取（Task 4）。此处提供 UI 输入并保存到 `settings`。

- [ ] **Step 1: script — 加 ref + load/save**

在脚本 `const apiModel = ref("deepseek-chat");` 附近加一组 ref：

```ts
const vadWindowMs = ref(30);
const vadMinSilenceMs = ref(300);
const vadMinChunkMs = ref(1000);
const vadMaxChunkMs = ref(30000);
```

在 `load()` 里（`apiModel.value = s.api_model ?? "";` 之后）补读数：

```ts
vadWindowMs.value = Number(s.vad_window_ms ?? 30);
vadMinSilenceMs.value = Number(s.vad_min_silence_ms ?? 300);
vadMinChunkMs.value = Number(s.vad_min_chunk_ms ?? 1000);
vadMaxChunkMs.value = Number(s.vad_max_chunk_ms ?? 30000);
```

在 `onSave()` 的 `saveSettings({ ... })` 里补 4 个键：

```ts
await saveSettings({
  api_base: apiBase.value,
  api_key: apiKey.value,
  api_model: apiModel.value,
  vad_window_ms: String(vadWindowMs.value),
  vad_min_silence_ms: String(vadMinSilenceMs.value),
  vad_min_chunk_ms: String(vadMinChunkMs.value),
  vad_max_chunk_ms: String(vadMaxChunkMs.value),
});
```

- [ ] **Step 2: template — 新增一个 tab + 区块**

在 `const tabs` 里追加一项（放在 `model` 之后）：

```ts
const tabs: { key: TabKey; label: string }[] = [
  // ...既有的 appearance/playback/subtitle/translate/model(可选)
  { key: "vad", label: "转写切片" },
];
```

> 注意：`TabKey` 联合类型需加 `"vad"`：`type TabKey = "appearance" | "playback" | "subtitle" | "translate" | "model" | "shortcuts" | "vad";`

在 `<div class="content">` 内、`v-if="activeTab === 'model'"` 区块之后，加一个 `v-if="activeTab === 'vad'"` 区块（沿用 `.field` 样式）：

```html
<section v-if="activeTab === 'vad'" class="panel-block">
  <h3 class="panel-title">转写切片</h3>
  <p class="panel-desc">切块参数影响转写进度粒度与取消响应速度，默认值即可正常使用。</p>
  <div class="field">
    <label>RMS 窗长 (ms)</label>
    <input v-model.number="vadWindowMs" type="number" min="10" max="200" />
  </div>
  <div class="field">
    <label>最小静音 (ms)</label>
    <input v-model.number="vadMinSilenceMs" type="number" min="100" max="5000" />
  </div>
  <div class="field">
    <label>最小块长 (ms)</label>
    <input v-model.number="vadMinChunkMs" type="number" min="200" max="10000" />
  </div>
  <div class="field">
    <label>最大块长 (ms)</label>
    <input v-model.number="vadMaxChunkMs" type="number" min="1000" max="120000" />
  </div>
</section>
```

> 若 `.panel-block`/`.panel-title`/`.panel-desc` 类不存在，可参考本文件其它 tab 区块使用的既有类名，或直接使用 `v-if="activeTab === 'vad'"` 包裹的 `.field` 栈。

- [ ] **Step 3: 前端类型检查**

Run:
```bash
export PATH="/c/Program Files/nodejs:$PATH" && cd "d:/Coding Projects/ASPlayer/app" && npx vue-tsc --noEmit
```
Expected: 0 error。

- [ ] **Step 4: 提交**

```bash
git add app/src/components/SettingsPanel.vue
git commit -m "feat(ui): 设置面板「转写切片」参数输入"
```

---

## 收尾：整仓验证 + 手动验收

- [ ] **Run: 全部测试**

```bash
cd "d:/Coding Projects/ASPlayer" && "$HOME/.cargo/bin/cargo" test
```
Expected: `asplayer-transcribe`（含 vad 6 测试 + 既有）+ `app` 全部 PASS。

- [ ] **Run: 前端类型检查 + 构建**

```bash
export PATH="/c/Program Files/nodejs:$PATH" && cd "d:/Coding Projects/ASPlayer/app" && npx vue-tsc --noEmit && npm run build
```
Expected: 0 error，build 成功。

**手动验收清单（来自设计 §7.4）：**
1. ≥5min 音频转写：进度平滑推进「x/y 块」，字幕逐块出现（无需等全程）。
2. 中途取消：一个块内停止；媒体行出现「已转写 X%（可继续）」。
3. 从断点继续：只解码剩余块；完成后 `done`。
4. 杀进程再触发：`cancel_transcribe` 崩溃恢复把 `transcribing` 修正为 `partial`。
5. 设置面板调 `max_chunk_ms` 更小：转写更细、可更频繁取消。
6. 重新生成字幕：字幕清空重转。
7. 回归：`ASPLAYER_MODEL` env 路径仍生效；翻译仅在 `done` 触发，`partial` 不误触发。

---

## 自审记录

**Spec 覆盖**：§1 目标（分块/逐块回写/块间取消/断点续跑）→ Task 1/2/4/5；§2 决策（VAD+硬切、partial 取消、默认参数、手动偏移）→ Task 1/4；§4.1 vad → Task 1；§4.2 Whisper 拆分 → Task 2；§4.3 编排 → Task 4；§4.4 DB 迁移/UNIQUE → Task 3；§4.5 状态机 → Task 5（+App.vue）；§4.6 事件/命令 → Task 5/6；§5.1 设置 → Task 7；§5.2 横幅/继续 → Task 6；§7 测试 → 各任务 Step + 收尾。

**占位符扫描**：无 TBD/TODO；所有步骤含完整代码。

**类型一致性**：
- `Chunk{start_sample,end_sample,start_ms,end_ms}` 在 Task 1 定义，Task 4 一致使用（`c.start_ms` 为 i64）。`Segment.start_ms: u64`，`s.start_ms as i64 + c.start_ms` 匹配。
- `Whisper::load`/`Whisper::transcribe(&mut self, Option<&str>, &[f32])` Task 2 定义，Task 4 一致调用。
- `run_transcription(..., lang: Option<String>, resume: bool)` Task 4 定义，Task 5 调用一致。
- `transcribe_media(id, lang: Option<String>, resume: bool, ...)` Task 5 定义，Task 6 前端 `invoke("transcribe_media",{id,resume,...})` 一致。
- `save_subtitle(media_id, start_ms: i64, end_ms: i64, text, translation, ordinal: i64)` Task 4 传入 `i as i64` 一致。
- `get_transcribe_next_ms/set_transcribe_next_ms` Task 3 定义，Task 4/5 使用一致。
- `MediaItem.transcribe_next_ms: i64` Task 3（后端）/ Task 6（前端 `number`）一致；`list_media` SELECT 加 `m.transcribe_next_ms` 且构造 `r.get(12)?` 与列序一致。
- `with_db`/`emit_progress`/`fail`/`register_transcription`/`unregister_transcription`/`transcription_running`/`TranscribeStop` 均为现有符号（[transcriber.rs](app/src-tauri/src/transcriber.rs)），计划仅复用不改名。`vad_config(&g)` 依赖 `MutexGuard<MediaDb>: Deref<Target=MediaDb>` 的解引用强制（与现行 `resolve_model_path(&g)` 同机制）。

**与设计文档的两处偏差（有意为之，需知悉）：**
1. **自适应切块阈值**：设计 §4.1/§9 写的是「P10 噪声底 × 1.5」，本计划改为 **P90 参考电平 × 0.1**（Task 1）。原因：P10 对**均匀响度（无静音）**的 ASMR 会把全帧误判为静音（P10≈响度均值，×1.5 后全低于阈值）；P90×0.1 则正确视为非静音。这是修掉设计里的一个实际缺陷，非功能删减。**建议回填设计 §4.1 与 §9 的口径**。
2. **取消 `get_transcribe_state` 命令**：设计 §4.6/§5.2 列出新增命令 `get_transcribe_state(id)` 返回 `{transcribe_next_ms, subtitle_status}`。本计划**不新增**该命令，改经 `list_media` 返回的 `MediaItem.transcribe_next_ms`（Task 3/6）+ 既有 `get_subtitle_status` 满足前端的「已转写 X%/继续」渲染：`X% = transcribe_next_ms / duration_ms`，两者均已在 `MediaItem` 上。避免重复轮询 API，符合 YAGNI。
