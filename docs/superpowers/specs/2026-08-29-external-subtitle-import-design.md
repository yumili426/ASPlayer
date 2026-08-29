# 外部字幕导入（SRT/VTT）设计文档

- **日期**：2026-08-29
- **状态**：设计评审中
- **对应主设计**：[2026-08-26-asplayer-design.md](../../specs/2026-08-26-asplayer-design.md) §3「外部字幕加载（SRT/VTT/ASS）」、§9.2「只显示字幕」
- **上游依赖**：现有 `subtitles` 表 + `save_subtitle`（按 `media_id+start_ms` 幂等 upsert）+ `subtitle_status` 状态机；`srt.rs` 的 `Segment`（`{start_ms, end_ms, text}`）；翻译管线已支持「按未翻译段增量回写」。

---

## 1. 背景与目标

设计 §3 承诺「外部字幕加载（SRT/VTT/ASS）——已有字幕直接用，无需转写」。但当前只有「转写产生字幕」一条路：已带内嵌字幕或旁挂 .srt 的视频，也被迫跑一遍 whisper，既慢又浪费。

### 目标
1. 用户可为任一媒体导入外部 `.srt` / `.vtt` 字幕文件，跳过转写。
2. 导入作为该媒体的**字幕真源**：替换其已有全部字幕，`subtitle_status` 置 `done`，从而解锁翻译。
3. 导入后字幕只有原文（`translation=""`），译文靠现有翻译管线补。
4. 提供三种触发：播放器工具栏按钮（选文件）+ 播放列表右键菜单（选文件）+ 同名 .srt/.vtt 自动检测。

### 非目标（YAGNI）
- 不做 ASS 解析（带 `{\\an}` 定位/样式标签，复杂度高，放下一轮）。
- 不做多个外部字幕「轨道」并存/切换；导入即单一真源替换。
- 不做外部字幕的 OCR（那是 🔜 实时硬字幕功能的范畴）。
- 不做导入后自动触发翻译（沿用现有手动流程）。

---

## 2. 产品决策（用户已确认）

| 决策点 | 结论 |
|---|---|
| 导入语义 | **替换 + 置 done**：清空该媒体已有字幕（含之前转写的），外部字幕为唯一真源；`subtitle_status=done`，解锁翻译 |
| 格式范围 | **SRT + VTT**（本轮）；ASS 延后 |
| 触发方式 | **工具栏「导入字幕」按钮 + 播放列表右键菜单 + 同名 .srt/.vtt 自动检测** |
| 字幕内容 | 只有原文 `text`；`translation=""` |
| 导入后断点 | `transcribe_next_ms` 归 0（防后续「从断点继续」把转写块拼进外部字幕） |

---

## 3. 架构：沿用「Rust 是唯一事实来源」+ 纯函数 crate

```
┌─ 播放器工具栏/列表右键 (Vue 3) ──────────────────────┐
│  «导入字幕» 按钮 / 右键菜单 → 文件选择或同名检测      │
└──────────────┬───────────────────────────────────────┘
               │ invoke import_external_subtitle(media_id, path?)
┌──────────────▼─────── Rust app (tauri) ───────────────┐
│ lib.rs       命令：读取/media_path 解析入口，分派      │
│ db.rs        replace_subtitles()（单事务：clear+upsert+置done+断点清零）│
└──────────────┬────────────────────────────────────────┘
               │ 纯函数 crate（无 tauri，可单测）
┌──────────────▼─────── asplayer-transcribe ────────────┐
│ subtitle_import.rs :: parse_srt / parse_vtt / parse_subtitle_file │
└───────────────────────────────────────────────────────┘
```

**为什么如此分层**：SRT/VTT 解析、字符集解码、段规整都是离散纯逻辑，不碰 DB/事件，延续 `vad.rs`/`srt.rs` 的 crate 定位——可单测、不依赖 tauri。app 层只做编排（读取路径、选文件、调用 db、回状态）。

---

## 4. 后端设计

### 4.1 crate：`asplayer-transcribe/src/subtitle_import.rs`（新增）

复用 `crate::srt::Segment`（`{ start_ms: u64, end_ms: u64, text: String }`），统一输出段序列。

```rust
/// 解析 SRT 文本 → 段序列（升序、滤无效）。HTML 行内标签剥离。
pub fn parse_srt(input: &str) -> Vec<Segment>;

/// 解析 VTT 文本 → 段序列（跳过 WEBVTT 头 / NOTE / STYLE 块；丢弃 cue settings）。
pub fn parse_vtt(input: &str) -> Vec<Segment>;

/// 按扩展名分派解析文件；含字符集处理（BOM → UTF-8 → GBK 回退）。
pub fn parse_subtitle_file(path: &Path) -> anyhow::Result<Vec<Segment>>;
```

**SRT 解析要点**
- cue 块：`序号` + `HH:MM:SS,mmm --> HH:MM:SS,mmm` + 一行或多行文本 + 空行分隔。
- 时间戳：小时位可省（有些 cue 只有 `MM:SS,mmm`）；毫秒分隔符兼容 `,` 与 `.`。
- 文本：跨行拼接成单段文本；移除 `\r`；剥离行内 `<i>/<b>/<u>/<font ...>` 等 HTML 标签（保留内部文字）。
- 跳过：序号/时间戳解析失败的空段、`start_ms/end_ms` 缺失或 `end<=start` 的无效段。

**VTT 解析要点**
- 文件头 `WEBVTT` 以及其后的可带注释的元数据行跳过。
- 跳过 `NOTE` 注释块、`STYLE`/`REGION` 声明块。
- cue 时间戳 `HH:MM:SS.mmm --> HH:MM:SS.mmm` 或 `MM:SS.mmm`；`-->` 行尾 `cue settings`（`align:...` / `position:...` 等）丢弃。
- 文本可多行；以 `-->` 行为界，其后到下一个空行之间的行为 cue 文本。
- 计相同规整：升序、滤 `end<=start`。

**字符集处理（`parse_subtitle_file`）**
- 读原始字节 → 识别 BOM：UTF-8 BOM（`EF BB BF`）/ UTF-16 LE（`FF FE`）/ UTF-16 BE（`FE FF`），按 BOM 解码并剥离。
- 无 BOM：先按 UTF-8 严格解码；若失败，回退 `encoding_rs::GBK`（中文 SRT 常见 GBK / 多字节）。
- 结尾统一去掉可能残留的 `\u{FEFF}`。

**通用规整**
- 按 `start_ms` 升序排序；丢弃 `end_ms <= start_ms` 段。
- 重复 `start_ms` 的多段由 DB 的 `UNIQUE(media_id, start_ms)` 幂等 upsert 兜底（同刻仅保留一条）。

### 4.2 app：`db.rs` 新增 `replace_subtitles`

单事务内完成，避免半途状态：

```rust
pub fn replace_subtitles(&self, media_id: i64, segs: &[asplayer_transcribe::srt::Segment]) -> rusqlite::Result<()>;
```

等价操作（一个事务包裹）：
1. `clear_subtitles(media_id)`；
2. 逐条 `save_subtitle(media_id, seg.start_ms as i64, seg.end_ms as i64, seg.text, "", ordinal)`（translation 置空，`ordinal`=遍历索引）；
3. `set_subtitle_status(media_id, "done", "")`；
4. `set_transcribe_next_ms(media_id, 0)`。

> 事务保证：任一步失败整体回滚，不产生「清空了一半」的脏状态。

### 4.3 app：`lib.rs` 新增命令

```rust
#[tauri::command]
fn import_external_subtitle(media_id: i64, path: Option<String>, state: State<AppState>) -> CmdResult<usize>
```

- 读 `media_path(media_id)`；`path=None` → 在同一目录枚举（同一路径、仅扩展名不同）的同名 `.srt` 或 `.vtt`；两者都有且都不同时……按 `.srt` 优先（约定），无同名文件报错提示手选。
- `path=Some(..)` → 用用户所选文件；校验扩展名属于 `{srt, vtt}`。
- 调 `parse_subtitle_file`；得到空 Vec（0 段）→ 报错「未解析到任何字幕」，不写入。
- 调用 `replace_subtitles`；返回段数。

**注册**进 lib.rs 的 generate_handler；前端照常 invoke `import_external_subtitle`。

---

## 5. 前端设计

### 5.1 API：[subtitle.ts](../app/src/api/subtitle.ts)
```ts
export function importExternalSubtitle(mediaId: number, path?: string): Promise<number>;
```
invoke `import_external_subtitle(media_id, path)` 返回段数（number）。

### 5.2 App.vue：`onImportSubtitle`
- 交给 PlayerStage/PlaylistPanel 的按钮拿到 `media_id`。
- 若该媒体已有字幕（`sub.status.value !== 'none'` 或 `sub.subtitles.length > 0`）→ 先 `confirm('将替换该媒体现有字幕，继续？')`。
- 确认后 invoke；成功：
  - `sub.setStatus("done", "done", 100, "")`；
  - `if (sub.currentId.value === mediaId) sub.load(mediaId);`
  - `refresh()`（刷新媒体行 `done` 徽标）。
- 失败（报错）→ `sub.setStatus("error", "", 0, msg)` 或 toast。

### 5.3 PlayerStage 工具栏
- 新增「导入字幕」按钮，与现有「转写 / 翻译」并列。点击：名称 `import_subtitle` → 打开文件选择器（`@tauri-apps/plugin-dialog` 的 `open()`，过滤 `srt`/`vtt`）→ 拿到 path 调 `onImportSubtitle(media_id, path)`。
- 若该媒体已有字幕，按钮文案可加「重新导入」仍是同一动作。

### 5.4 PlaylistPanel 右键菜单
- 现有右键菜单（播放/定位/移除/删除）增加「导入字幕」项。同样：选文件 → 若当前媒体则刷新，否则仅刷新列表徽标。

### 5.5 同名自动检测
- 「工具栏导入」与「右键导入」都走手动选文件路径；同名自动检测在**命令层**（`path=None` 时）兜底——即当用户在别处未显式选文件、想直接「查找同名」时可用。为简化，本轮工具栏/右键默认打开文件选择器；同名自动作为命令的 `path=None` 分支保留（为将来「扫描时自动挂载」埋点），UI 暂不额外暴露入口。

> 说明：用户已选「工具栏+右键+同名自动」。实现里同名自动放在命令层 `path=None` 分支（可被测试与将来自动挂载复用），UI 当前按钮走文件选择器。两者并存不冲突。

---

## 6. 错误处理与边界

| 场景 | 处理 |
|---|---|
| 文件不存在 / 扩展名不支持 | 报错，已有字幕与状态不变 |
| 解析失败（非法格式） | 报错，不写入 |
| 解析得到 0 段 | 报错「未解析到任何字幕」，不写入 |
| 媒体已有字幕 | 前端先确认再替换 |
| 导入后残留断点 | `transcribe_next_ms` 置 0（事务内） |
| 同名自动找不到 | 报错「未找到同名字幕，请手动选择」 |
| 重复 `start_ms` | UNIQUE 幂等，仅保留一条 |
| 中文 GBK / UTF-16 编码 | 字符集处理兜底，不误判 |

---

## 7. 测试与验收

### 7.1 crate 单测（`subtitle_import.rs`）
- `parse_srt`：基本三块序列；时间戳缺小时 `MM:SS,mmm`；毫秒用 `.`；多行文本拼接；剥离 `<i>` 标签；跳空 cue；滤 `end<=start`；`\r` 清理。
- `parse_vtt`：`WEBVTT` 头跳过；`NOTE`/`STYLE` 块跳过；`MM:SS.mmm`；cue settings 丢弃；多行文本。
- `parse_subtitle_file`：GBK 编码 SRT 正确解码；UTF-8 BOM；UTF-16 LE BOM；非法/不支持的扩展名报错；空文件 → 空 Vec。

### 7.2 MediaDb 测试
- `replace_subtitles`：事务内 clear+upsert；结束后 `subtitle_status=done`、`transcribe_next_ms=0`、段数=写入数；幂等重复调用段数不叠加。
- 回滚：传入 mock 失败场景（如注入多个段其中一条触发约束）时整体回滚，不残留半程。

### 7.3 命令级（内存库）
- `import_external_subtitle(id, Some(测试.srt))` → 返回段数、`done`、断点 0。
- `import_external_subtitle(id, None)` 在同目录有同名 .srt → 成功；无同名 → 报错。
- 0 段文件 → 报错、状态不变。

### 7.4 手动验收清单
1. 对一未转写视频：工具栏「导入字幕」选 .srt → 字幕面板出现原文、状态 `done`，进度条无（不转写）。
2. 同一媒体已有转写字幕时导入 → 弹确认 → 替换为外部字幕，旧字幕消失。
3. 文件名与媒体同名的 .srt：命令层 `path=None` 自动命中。
4. GBK 中文 .srt → 正常显示无乱码。
5. .vtt 导入 → 正常。
6. 导入后点「翻译」→ 只翻译未翻译段（全部），正常回写。
7. 导入后「从断点继续转写」不再污染（外部字幕保持不变）；「重新生成字幕」仍可正常重转。

### 7.5 与既有回归
- 转写/翻译路径不受影响（`replace_subtitles` 为新增，不触碰 `transcribe_inner`）。
- `save_subtitle` 的 `translation` 保留逻辑不变（导入时置空是刻意的：外部字幕无译文）。
- `subtitle_status` 状态机新增的仅是「导入后置 done」，无新枚举值。

---

## 8. 里程碑边界

**属于一期 MVP 收尾功能**，独立于转写/翻译管线。依赖：`tauri-plugin-dialog`（已有）、`srt.rs` 的 `Segment`、`save_subtitle` + UNIQUE 迁移（已落地）。**未含**：ASS、字幕轨道切换、自动挂载扫描、导入后自动翻译、OCR 硬字幕。

---

## 9. 风险与对策

| 风险 | 对策 |
|---|---|
| SRT 时间戳 / 格式变体多样 | 解析器对小时省略、毫秒 `,`/`.`、`\r\n` 兼容；无效段静默跳过；单测覆盖典型变体 |
| 中文编码（GBK/UTF-16）导致乱码 | BOM 检测 + UTF-8 严格解码 + GBK 回退；单测用真实编码样本 |
| 导入覆盖误删已转写字幕 | 前端有 confirm；DB 单事务保证要么全成功要么全回滚 |
| VTT cue settings / NOTE / STYLE 干扰 | 解析器显式跳过；单测覆盖 |
| 同名自动检测歧义 | 约定 `.srt` 优先；找不到报错提示手选 |

---

## 10. 验收后的下一阶段（非本里程碑）
ASS 字幕解析、字幕轨道（多字幕并存/切换）、扫描时自动挂载同名字幕、OCR 硬字幕（🔜）——均依赖本里程碑的数据埋点（导入即 `done` + 真源替换）作为地基。
