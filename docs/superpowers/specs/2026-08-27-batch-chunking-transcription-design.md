# 转写切片化 设计文档

- **日期**：2026-08-27
- **状态**：设计评审中
- **对应主设计**：[2026-08-26-asplayer-design.md](../../specs/2026-08-26-asplayer-design.md) §8「转写流程」（VAD 按静音切块 → 逐块识别 → 实时回写 → 可取消，已完成块不重跑）
- **上游依赖**：M4 模型下载器已落地（`resolve_model_path`）；本轮「whisper 日志去刷屏」的跨进程日志过滤已修（`setup_log_filter`）——否则逐块解码在 debug 下每 token 都要刷屏

---

## 1. 背景与目标

主设计 §8 的原始承诺是：**VAD 按静音切块 → whisper.cpp 逐块识别（语言自动检测或手动指定）→ 实时回写 segments 并推送进度 → 可取消，已完成块不重跑**。

但 M0 把这条管线**简化成了单次整体调用**，导致三处与原意脱节：

| 原意 | M0 现状 | 后果 |
|---|---|---|
| VAD 切块 + 逐块识别 | `whisper::transcribe()` 一次 `state.full(全量 samples)`（[transcriber.rs:172](app/src-tauri/src/transcriber.rs#L172)） | 无法控制粒度 |
| 实时回写 segments + 推送进度 | 推断全结束后一次性 `clear+insert`（[transcriber.rs:184](app/src-tauri/src/transcriber.rs#L184)），进度只有 0%/100% 端点 | 无逐块进展，取消/中断全丢 |
| 可取消，已完成块不重跑 | 取消最迟在整次推理结束后生效（[transcriber.rs:40](app/src-tauri/src/transcriber.rs#L40) 注释「单次整体调用不可中断」）；中断后从头重跑 | 取消无效、中断白跑 |

### 目标
1. 转写**按静音边界 + 最大时长硬切**切成若干块，whisper 逐块识别，块内时间戳绝对化。
2. 每块完成**立即幂等回写**到 `subtitles` ＋ 推送逐块进度。
3. **块间可取消**：取消在一个块粒度内生效，**已完成块保留**并进入可续跑状态（`partial`）。
4. **断点续跑**：中断/取消/崩溃/断电后再触发，跳过已完成块，不重跑。

### 非目标（YAGNI）
- 不做 whisper.cpp 流式实时转写（那是 LLPlayer 的路线，会重新引入「翻译不保存/无上下文」的痛点；本项目坚持**离线批处理 + 永久缓存**）。切片化是批处理内的粒度细化，不是转成流式。
- 不做 VAD 之外的音频预处理（loudnorm 增益等留到 §12 风险 #1 的 Plan B）。
- 不做跨块上下文携带（init prompt）。VAD 边界对齐句/静音，独立解码精度损失可忽略，且独立块才能干净地取消/续跑。
- 不做多模型并行转写。

---

## 2. 产品决策（用户已确认）

| 决策点 | 结论 |
|---|---|
| 切片策略 | **静音 VAD + 最大时长硬切**。静音边界切，无静音则每 `max_chunk_ms` 硬切一刀；`min_chunk_ms` 下限并入前块。防 ASMR 长耳语无静音导致切不出块 |
| 取消语义 | **取消保留已完成块 + 可续跑**。新增 `subtitle_status = 'partial'`；取消后已写块字幕保留，媒体行显示「已转写 X/Y（可继续）」，再次触发从断点续跑 |
| 默认档位参数 | `window_ms=30`、`min_silence_ms=300`、`min_chunk_ms=1000`、`max_chunk_ms=30000`、`sample_rate=16000`（可在设置里调，见 §5.1） |
| 时间戳 | 块内相对时间戳由 whisper 给出；**app 层按 `chunk.start_ms` 手动偏移**成绝对毫秒（不开 `params.offset_ms`，求纯净可测） |

### 为什么选「静音 VAD + 最大时长硬切」而不是纯静音/纯固定
- **ASMR 特性**：常有长时间无静音的连续耳语。纯静音 VAD 可能整段只切出 1–2 块，等于切片失效，取消依然接近无效；纯固定时长会在句中/词中硬切，丢词尾、边界质量下降。
- **静音 VAD 为主**：把块对齐到自然句/静音边界，避免切词，质量优先。
- **最大时长硬切兜底**：当静音太少、VAD 切不出边界时，按上限强制切，保证取消响应性。两者叠加才是 §8 原意的忠实实现。

---

## 3. 架构：延续「Rust 是唯一事实来源」

```
┌─ 设置面板/媒体库 (Vue 3) ───────────────────────────────┐
│  · 转写按钮 / 「从断点继续」横幅                        │
│  · 逐块进度 / partial 状态展示                          │
└──────────────┬──────────────────────────────────────────┘
               │ invoke + transcribe://progress 事件订阅
┌──────────────▼─────── Rust app (tauri) ─────────────────┐
│ transcriber.rs 编排（唯一事实来源）                     │
│  · 读 samples → vad::split_samples 切块                 │
│  · 从 transcribe_next_ms 断点之后的块循环：             │
│      块间 check_canceled → 锁外 whisper.decode          │
│      → 时间戳偏移 → 锁内幂等 upsert 至 subtitles         │
│      → 更新 transcribe_next_ms → emit 逐块进度          │
│  · 取消→partial；完成→done 并清断点                     │
└──────────────┬──────────────────────────────────────────┘
               │ 纯函数 crate（无 tauri，可单测）
┌──────────────▼─────── asplayer-transcribe ──────────────┐
│ vad.rs   :: VadConfig, split_samples() -> Vec<Chunk>    │
│ whisper.rs :: Whisper{ load() + decode() }（模型只载一次）│
└─────────────────────────────────────────────────────────┘
```

**为什么如此分层**：
- **crate 保持纯净**：VAD 切块、逐块 whisper 解码都是离散、可单测的纯逻辑，不碰 DB/事件，延续 M0 的管线定位。
- **app 层编排**：取消检查、DB 幂等回写、断点持久化、进度事件这些「唯一事实来源」必须由 Rust 后端持有（呼应悬浮窗 Bug #4 的教训），前端只订阅。
- **模型只载一次**：`WhisperContext` 加载约 466MB（small），绝不能每块重建。拆成 `Whisper::load()`（载模型）＋ `Whisper::decode()`（逐块复用），这是本里程碑的关键重构前提。

---

## 4. 后端设计

### 4.1 crate：`asplayer-transcribe/src/vad.rs`（新增）

```rust
pub struct Chunk {
    pub start_sample: usize,
    pub end_sample: usize,   // 开区间
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    pub sample_rate: u32,
    pub window_ms: u32,      // RMS 窗长，默认 30
    pub min_silence_ms: u32, // 判定为静音的最短时长，默认 300
    pub min_chunk_ms: i64,   // 块最小时长（不足则并入前块），默认 1000
    pub max_chunk_ms: i64,   // 块最大时长（超则硬切），默认 30000
}

impl Default for VadConfig { /* 上述默认值 + sample_rate=16000 */ }

/// 能量阈值静音切块：静音边界为主，max_chunk_ms 硬切兜底，min_chunk_ms 并入前块。
/// 返回首尾相接、覆盖整段音频的块序列；空音频返回空 Vec。
pub fn split_samples(samples: &[f32], cfg: &VadConfig) -> Vec<Chunk> { ... }
```

算法流水线（计划文档细化）：
1. 按 `window_ms` 切非重叠帧，逐帧算均方根（RMS）。
2. **自适应静音阈值**：取 RMS 分布高参考电平 **P90 × 0.1** 作为阈值，`threshold = max(p90 * 0.1, abs_floor)`；仅当块内无任何自适应帧时才退化为绝对阈值。用 P90×0.1 而非 P10×1.5 —— 均匀响度（无静音）的 ASMR 里 P10≈响度均值，×1.5 后全部帧低于阈值会被整段误判为静音；P90×0.1 则正确视为非静音。
3. 识别连续静音帧段：连续 rms < threshold 且时长 ≥ `min_silence_ms` 的段记为静音孔。
4. 在静音孔中点切一刀，得到候选块；`< min_chunk_ms` 的块并入前块。
5. 对任何 `> max_chunk_ms` 的块：先找块内最长的合法静音孔切；若无，则按 `max_chunk_ms` 均分硬切。
6. 首尾的纯静音帧并入首/末块（避免产出纯静音空块）。

> 校验性质（单测断言）：相邻块首尾相接、覆盖 `[0, total_ms)`；单块时长 ∈ `[min_chunk_ms, max_chunk_ms]`（除末块允许更短）；`start_sample/end_sample` 落在合法样本区间；空/整静音/极短输入不 panic。

### 4.2 crate：`asplayer-transcribe/src/whisper.rs`（拆分）

关键重构：**模型加载与逐块解码分离**。

```rust
pub struct Whisper { ctx: WhisperContext }

impl Whisper {
    /// 载模型一次（含 setup_log_filter）。context 构建昂贵，整个任务只调一次。
    pub fn load(model_path: &str) -> Result<Self> { ... }

    /// 对单块 samples 解码，返回“相对该块起点”的段时间戳（毫秒）。
    /// 复用同一个 ctx（内部一个 WhisperState，whisper_full 每次调用自带 reset）。
    pub fn transcribe(&mut self, language: Option<&str>, samples: &[f32]) -> Result<Vec<Segment>> { ... }
}
```

- 保留原自由函数 `transcribe(model_path, lang, samples)` 作为便捷封装（`Whisper::load(...)? .transcribe(...)`），CLI 与既有调用不破坏。
- `transcribe` 方法沿用现行参数：`Greedy{b1}`、`set_print_* false`（progress/special/realtime/timestamps）、`set_language`。`setup_log_filter()` 已在 `load` 里调用。
- 块内段时间戳为相对量（0 基于块起点）；**绝对偏移由 app 层统一做**，crate 不关心全局时间轴。

### 4.3 app：`transcriber.rs`（编排 + 断点 + 事件）

`run_transcription` 流程改为：

```
1. register（防重）→ 读 media_path
2. 抽音轨 → read_samples_f32 →（两次既有 check_canceled 保留）
3. 判断 resume：
   · resume=false（重新生成）：clear_subtitles + set_transcribe_next_ms(0)
   · resume=true（从断点继续）：保留已写字幕，读取 transcribe_next_ms 作为起点
4. chunks = vad::split_samples(&samples, &cfg)
   · 若 resume 且 next_ms>0：跳过 all chunk.end_ms <= next_ms 的块，从首个跨越断点的块继续
5. model = resolve_model_path(db)；whisper = Whisper::load(&model)   // 只载一次
6. 循环剩余块（idx 映射回全局 chunks）：
   a. check_canceled(db, media_id)?        // 块间检查点
   b. 锁外 chunk_samples = &samples[c.start_sample..c.end_sample]
      segs = whisper.transcribe(lang_opt, chunk_samples)?
   c. 锁内：逐段 seg 加 c.start_ms 偏移 → save_subtitle（幂等 upsert）
      set_transcribe_next_ms(media_id, c.end_ms)
   d. emit_progress(progress = 15 + 65*(已处理块数/总块数), message = "转写 x/y 块")
7. 全部完成：set_subtitle_status(done) + set_transcribe_next_ms(0)
8. Err(Canceled)：若 next_ms>0 → set_subtitle_status(partial)（保留断点）；否则归 none
```

- `transcribe_media` 命令签名新增 `resume: bool`（见 §4.6）。
- `check_canceled` 复用在块循环内，语义不变，但「取消生效粒度」从整次推理降到**一个块**（≤ max_chunk_ms）。
- 每块一次锁内写库 + 一次短锁断点更新，锁外只做 whisper 推理，符合既有「短锁」原则。

### 4.4 app：`db.rs`（迁移 + 断点读写）

**新增列**（幂等迁移，进 `migrate`）：
```sql
ALTER TABLE media_files ADD COLUMN transcribe_next_ms INTEGER NOT NULL DEFAULT 0;
```

**新增 UNIQUE 约束**（幂等内容写的前提）：
```sql
-- 先按 (media_id, start_ms) 去重（保留最小 id），
DELETE FROM subtitles WHERE id NOT IN (
  SELECT MIN(id) FROM subtitles GROUP BY media_id, start_ms
);
-- 再建唯一索引
CREATE UNIQUE INDEX IF NOT EXISTS idx_sub_unique ON subtitles(media_id, start_ms);
```

> **为什么要 UNIQUE**：现行 `save_subtitle` 用了 `ON CONFLICT DO UPDATE`（[db.rs:227](app/src-tauri/src/db.rs#L227)），但 `subtitles` 没有唯一约束，当前靠「先 `clear_subtitles` 再插」才不重复。续跑要**增量写但不重复**，必须让 upsert 真正生效。加 UNIQUE 后 `ON CONFLICT DO UPDATE` 才会触发，`translation` 保留逻辑（`CASE WHEN excluded.translation='' THEN old`）同时保证续跑不覆盖已有译文。

**断点读写**：
```rust
pub fn get_transcribe_next_ms(&self, id: i64) -> rusqlite::Result<i64>; // 缺省 0
pub fn set_transcribe_next_ms(&self, id: i64, next_ms: i64) -> rusqlite::Result<()>;
```

### 4.5 状态机（subtitle_status）

新增 `partial`：

| 状态 | 含义 |
|---|---|
| `none` | 无字幕 |
| `transcribing` | 推理中 |
| `partial` | 转写被取消/中断，已有部分块入库，**可继续**（断点=transcribe_next_ms） |
| `done` | 完整字幕 |
| `translating` / `error` | 不变 |

- 取消落地：`next_ms>0` → `partial`；`next_ms==0` → `none`。
- `run_translation` 仍只允许 `done` 才翻译（`partial` 不被误当完整）。
- 崩溃恢复（`cancel_transcribe` 非运行分支，见 [lib.rs:164](app/src-tauri/src/lib.rs#L164)）：status==`transcribing` 且 `next_ms>0` → 修正为 `partial`（可续跑），而非现在的 `rollback_after_cancel` 回退成 done/none。

### 4.6 事件与命令变更

**事件**（沿用 `transcribe://*` 命名空间）：

| 事件 | 载荷 | 时机 |
|---|---|---|
| `transcribe://progress` | `{media_id, stage, progress, message}` | stage=transcribe 时 progress=15+65*已处理/总，message=「转写 x/y 块」 |
| `transcribe://canceled` | `media_id` | 取消落地（=partial，附带可续跑断点） |
| `transcribe://done` / `error` | 不变 | —— |

**命令**：
| 命令 | 变更 | 说明 |
|---|---|---|
| `transcribe_media(id, lang, resume: bool)` | 签名新增 `resume` | `resume=true` 从断点继续；`false` 清空重转。前端「从断点继续」传 true、「重新生成字幕」传 false |
| `cancel_transcribe(id)` | 行为变化，签名不变 | 取消置 partial（保留断点） |

---

## 5. 前端设计

### 5.1 设置项（SettingsPanel 新增，或沿用全局设置表）
- `vad_window_ms`、`vad_min_silence_ms`、`vad_min_chunk_ms`、`vad_max_chunk_ms`（写入 `settings` KV；默认见 §2）。普通用户不动，懂行可调，默认值即可工作。

### 5.2 媒体库行 / 转写按钮
- 当 `subtitle_status == 'partial'`：媒体行显示横幅「已转写 X%（可继续）」，主按钮变成「从断点继续」（调用 `transcribe_media(resume:true)`）；旁边保留「重新生成字幕」（`resume:false`，清空重转）。X% 由 `transcribe_next_ms / duration_ms` 计算，两者均已随 `list_media` 返回的 `MediaItem` 下发（不额外新增查询命令）。
- 否则按钮为「生成字幕」（`resume:false`）。
- 进度沿用现有渐进步，stage=transcribe 时显示「转写 x/y 块」。

### 5.3 订阅
- `onTranscribeProgress` 更新进度条；`onTranscribeCanceled` 把状态置为 partial 并刷新横幅；`onTranscribeDone` 清横幅。

### 5.4 边界展示
- 若 `duration_ms==0`（从未探测成功）或后端未给出总块数，横幅退化为「已转写，可继续」而不显示百分比。

---

## 6. 错误处理与边界

| 场景 | 处理 |
|---|---|
| 取消发生在首块完成前 | `next_ms==0` → 状态 `none`（无可保留，不提供继续） |
| 取消发生在块之间 | 保留已完成块，`partial` + 断点，提示可继续 |
| 中断/崩溃/断电 | 下次启动 `cancel_transcribe` 非运行分支把 `transcribing` 修正为 `partial`（若 `next_ms>0`） |
| 续跑时媒体文件被替换/时长变化 | 断点按 `end_ms` 比较，越界块自然重解码；VAD 重算后旧断点可能不命中 → 视为从头（保守） |
| 某块 decode 失败（非取消） | 作为整任务 error 处理，已写块保留 + `partial`（由错误路径置 partial），可后续继续 |
| 短于 `min_chunk_ms` 的残余块 | 并入前块，Upsert 幂等，重复解码无害 |
| `save_subtitle` 重复写 | UNIQUE + `ON CONFLICT` 幂等，`translation` 空值不覆盖已有译文 |
| whisper `-DWHISPER_DEBUG` 刷屏 | 已由 `setup_log_filter` 全局过滤 DEBUG 级，逐块解码不再刷屏 |

---

## 7. 测试与验收

### 7.1 Rust 单元测试（crate）
**vad.rs**（纯函数，无需模型）：
- 静音孔切块：构造 `[噪声...][静音...][噪声...]`，断言 3 段边界/时长。
- `min_silence_ms` 阈值：太短静音不切。
- `min_chunk_ms`：过小块并入前块。
- `max_chunk_ms`：超长无静音 → 按上限硬切，块长 ≤ max。
- 无静音整段：只按 max 硬切。
- 边界：空/整静音/极短输入 => 不 panic、结果覆盖正确。
- 拼接性质：相邻块 `start_sample==prev.end_sample`，覆盖 `[0, total)`。

**whisper.rs**：`setup_log_filter` 幂等（重复调用无害，回调仅装一次）。

### 7.2 MediaDb 测试
- 迁移：旧表补 `transcribe_next_ms` 列；`subtitles` 去重后建 UNIQUE（含重复数据构造幂等）。
- `save_subtitle` 幂等：同 `(media_id,start_ms)` 写两次 → 1 行；`translation` 非空时不被空值覆盖。
- `get/set_transcribe_next_ms` 往返；`open_in_memory` 全新库含新列。

### 7.3 命令级（mock 网络不可行，用内存库）
- `transcribe_media(id, resume:false)` 全量：结束后 `done` 且 `transcribe_next_ms==0`。
- 取消中断：模拟在首个「已写块」后取消，断言 `partial` 且断点>0；再 `resume:true` 从断点继续，成功后 `done`、断点清零。

### 7.4 手动验收清单
1. 长音频（≥5min）转写：进度条平滑推进「x/y 块」，字幕逐块出现（无需等全程）。
2. 转写中途点取消：一个块粒度内停止；媒体行出现「已转写 X%（可继续）」。
3. 从断点继续：只解码剩余块，提示「从断点继续」，完成后 `done`。
4. 杀掉进程再触发：状态修正为 `partial`，可继续（崩溃恢复）。
5. 断网/ffmpeg 失败：错误置 `error`，已写块保留为 `partial`。
6. 设置面板调 `max_chunk_ms` 更小：转写更细、可更频繁取消（验证配置生效）。
7. 重新生成字幕（resume:false）：字幕清空重转，翻译不残留（覆盖已有译文）。

### 7.5 与既有回归
- ASPLAYER_MODEL 环境变量优先路径不因本轮回归（沿用 `resolve_model_path`）。
- 翻译仅在 `done` 触发；`partial` 不被误判为完成。

---

## 8. 里程碑边界

**本设计属于里程碑 5（转写切片化）**，依赖已在位的 M4（模型下载器/路径解析）与本轮日志过滤。**未含**：cloud API 转写、实时/流式转写（排除项）、loudnorm 预处理、跨块上下文携带、增量翻译与切片联动（后续可独立立项）。

---

## 9. 风险与对策

| 风险 | 对策 |
|---|---|
| ASMR 长无静音导致 VAD 切不出块 | 最大时长硬切兜底（`max_chunk_ms`），保底取消响应性 |
| 逐块解码固定开销（mel+encode 每块重复） | `min_chunk_ms` 避免碎片化；块数通常几十，开销相对模型推理可忽略 |
| 静音阈值误判（环境噪声/音量差异） | 自适应 P90 参考电平 × 0.1；参数可配置；默认值经真实 ASMR 样本校准 |
| 边界处单词被切（若 max 硬切落在词中） | 静音 VAD 为主使边界对齐句/静音；仅在无静音时才硬切，接受小概率丢词尾 |
| 续跑后 VAD 参数变化导致断点漂移 | 断点按 `end_ms` 比较；不命中则保守从头，不产生错误字幕 |
| `subtitles` 去重迁移对有重复老库 | 保留最小 id + 排重；迁移幂等，测试覆盖 |

---

## 10. 验收后的下一阶段（非本里程碑）
主设计 §12 风险 #1（耳语音识别差）Plan B（loudnorm / faster-whisper / 更大模型）保持独立，切片化只解决「取消/续跑/进度」，不解决「识别质量」。
