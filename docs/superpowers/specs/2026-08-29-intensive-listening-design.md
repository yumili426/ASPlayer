# 精听模式（Intensive Listening）设计 — 已记录·暂不实现

> **状态：⏭️ 已记录设计，暂不实现。** 2026-08-29 与作者逐节确认，决定从一期 MVP 裁掉，作为 **MVP 后** 的学习功能。
> 本稿冻结在此，涉及主设计文档 `docs/specs/2026-08-26-asplayer-design.md` §5 与 §3.3/§4 的 MVP 标记业已同步降级。后续若要实现，直接据此进入 writing-plans 即可。

**目标：** 双播放模式（连播/精听）+ 四项学习交互（自动暂停每句、单句循环、AB 循环、盲听隐藏），全局切换 + 按文件覆盖。

---

## 范围决策（已与作者确认）

- **覆盖粒度：全局 + 按文件 覆盖**（不含按文件夹）。对应 brainstorming 的选项二。
  - 理由：文件夹级覆盖需「文件 → 文件夹 → 全局」优先级链 + 文件夹设置 UI，成本高；文件级已覆盖「某教学视频固定精听」的典型场景。
- **句末交互：句末显示按钮**（重听本句 / 下一句），空格/播放键亦可推进。对应 brainstorming 的「句末显示按钮」选项。

## 技术要点

- 架构不变：Rust 唯一事实来源 + 纯函数 crate；播放模式解析放前端，避免额外 IPC 往返。本次全部落在 `app/src`（播放引擎 + UI）与 DB 迁移（加列），无新 crate。
- 现有播放逻辑入口：`app/src/stores/playback.ts`、`app/src/components/PlayerStage.vue`、`app/src/components/CaptionPanel.vue`、`app/src/components/SubtitlePanel.vue`、`app/src/stores/shortcuts.ts`、`app/src-tauri/src/{media.rs,db.rs,lib.rs}`。

---

## §A 播放模式模型

- **三态模式**：`intensive`（精听）/ `broadcast`（连播）。
- **全局**：`playback.ts` 新增 `playbackMode: "broadcast" | "intensive"`，默认 `broadcast`，随既有 `asplayer-playback-v1` 持久化（`load()` 已与默认值合并，加字段即兼容）。
- **按文件覆盖**：`media_files.profile_override`（`NULL`=跟随全局 / `'intensive'` / `'broadcast'`）。DB 迁移加一列，可为空。
- **有效模式解析（纯前端）**：`resolveMode(item) = item.profile_override ?? playback.playbackMode`。每次选文件算一次即可。
- **数据结构**：`MediaItem` 前端类型 + Rust struct 同步加 `profile_override: Option<String>`；`list_media` 的 SELECT 补该列；新增 Rust command `set_media_profile(id, value: Option<String>)` 写回、前端同步当前 item。

## §B 四项学习交互

### (1) 自动暂停每句（精听核心）
- 依据字幕 `end_ms`。在 `PlayerStage.onTimeUpdate` 检测当前活动句到达 `end_ms`：若有效模式为 `intensive` 且「自动暂停」开 → `pause()` 并精确定位到该句末。
- 暂停后在 `CaptionPanel` 字幕浮层下方弹两个按钮：
  - **↺ 重听本句**：seek 回本句 `start_ms` 再 `play()`。
  - **→ 下一句**：seek 到下句 `start_ms` 再 `play()`。
- 空格/播放键在此情境 = 下一句；`R` = 重听本句。
- **注意**：`CaptionPanel` 根容器当前为 `pointer-events: none`，按钮需单独开 `pointer-events: auto`。

### (2) 单句循环（精听内可选开关）
- 开启后句末不暂停，而是 loop 回本句 `start_ms` 无限重复，直到按「下一句 / 空格」进入下句。
- 与「自动暂停」互斥：开单句循环时关闭自动暂停句末逻辑。

### (3) AB 循环（两种模式都手动可用）
- 控制条新增 AB 按钮，三段式：按①设 A（当前播放位置）→ 按②设 B（当前播放位置，开始 A↔B 循环）→ 按③清除。
- AB 生效时屏蔽自动暂停与单句循环（AB 优先级最高）。
- A/B 标记显示在进度条上；手动拖动进度不清除 AB；seek 到 A~B 区间外时自动停 AB。

### (4) 盲听隐藏（精听内可选开关）
- 语义（§5 + §9.3 折中）：一个「盲听」开关，默认**临时切原文态**，即隐藏**译文**（对应 §9.3「盲听=临时切原文态」）。可选改为隐藏原文。
- 按绑定键（默认 `KeyH`）临时揭示被隐藏的那种；松开恢复。作用于字幕浮层；字幕面板保持原样。

## §C UI 接线

- **控制条**（`PlayerStage` 底部）：加「连播/精听」分段切换（反映当前文件有效模式，点击=设全局模式）+ AB 循环按钮。
- **播放列表右键**：当前项加「播放模式」子菜单 → 跟随全局 / 精听 / 连播，写回 `profile_override`。
- **设置面板·播放页**：全局模式三态 + 精听下 3 个开关（自动暂停 / 单句循环 / 盲听），默认值见 §E。

## §D 快捷键（新增 action，均可改键）

- `togglePlaybackMode` 切连播/精听（默认 `Mod+Alt+KeyS`，对应设计 §8 的 Ctrl+Alt+S）
- `repeatSubtitle` 重听本句（默认 `KeyR`）
- `toggleSentenceLoop` 单句循环开关（默认 `Mod+Alt+KeyL`）
- `blindListen` 盲听开关（默认 `KeyH`）
- 复用现有 `nextSubtitle` / `prevSubtitle` 做句导航。

## §E 默认值 & 边界

- 全局默认：连播；精听三开关——自动暂停**开**、单句循环**关**、盲听**关**。
- 精听仅在「有字幕」时生效；无字幕退化为普通连播。
- 暂停拖动进度条、切文件、开关模式，都清掉句末按钮状态，避免残留。
- AB 循环、单句循环在切文件时清除（per-file 不记忆 AB，便于教学）。

---
*记录于 2026-08-29。* 主设计文档对应 MVP 标记已同步更新为「MVP 后」。
