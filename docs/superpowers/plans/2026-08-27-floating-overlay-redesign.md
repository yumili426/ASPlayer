# 悬浮字幕窗桌面歌词化重设计 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按[设计文档](../specs/2026-08-27-floating-overlay-redesign-design.md)重建迷你悬浮字幕窗：网易云式双行玻璃歌词条 + 悬停工具栏、整条拖拽、显示模式三态独立记忆、偏好持久化，并以"三重时间源 + 内容级去重"根治 seek 后字幕冻结的 Bug #4。

**Architecture:** 后端仍是唯一事实来源——新增 `OverlayPrefs` 存 settings KV 单键 JSON 并经 `overlay://prefs-changed` 双向广播；主窗新增 `overlayFeed.ts` 作为唯一"算当前句并推送"模块（timeupdate + seeked + 500ms 轮询三重时间源，文本全等去重）；悬浮窗本地按 display_mode 渲染取舍，控制动作经后端中继转发主窗执行。

**Tech Stack:** Tauri 2 (Rust) · Vue 3 `<script setup>` TS · SQLite(settings 表) · Tauri event 中继。

**实施注意（对规格的两处落地差异）：**
1. 规格 §5 提到更新 `capabilities/overlay.json` ACL —— 经验证**不需要**：现悬浮窗 capabilities 仅 `core:default` 就已能调用 `set_overlay_visible` 等全部应用自定义命令（Tauri v2 自有命令不经 ACL 网关），本计划不改该文件。
2. 空 payload（gap 清屏）期间窗口完全透明。此时拖拽不可用（无可视面），属已知权衡；用户仍可全局快捷键 Ctrl+Alt+O 隐藏或等下一句浮现。

---

## File Structure（本计划锁定）

```
app/
├─ src-tauri/src/floating.rs        # 改：增 OverlayPrefs 持久化/广播、两个控制转发命令；删 overlay_request_seek
├─ src-tauri/src/lib.rs             # 改：invoke_handler 注册表增删条目
├─ src/types.ts                     # 改：删未使用的 OverlayConfig/OverlayCurrent，增 OverlayPrefs 相关类型
├─ src/api/overlay.ts               # 改：删点击跳转残链，增 step/play-pause 转发封装
├─ src/stores/overlayPrefs.ts       # 新：偏好响应式仓库（读/写/跨窗订阅）
├─ src/overlayFeed.ts               # 新：主窗侧推送引擎（三重时间源+内容去重+gap 行为）
├─ src/App.vue                      # 改：拆掉旧推送段与 do-seek 监听，接 overlayFeed
├─ src/components/PlayerStage.vue   # 改：mediaEl 挂载到 overlayFeed（一处 watch）
└─ src/windows/FloatingOverlay.vue  # 重写：玻璃条 UI/工具栏/⚙面板/整条拖拽/模式渲染
```

各文件单一职责：`overlayFeed` 只管"何时推什么"；`overlayPrefs` 只管"偏好读写同步"；`FloatingOverlay` 只管"展示与手势"；Rust 只做存储、中继与穿透切换。

---

### Task 1: Bug #4 根因复现诊断（动手前取证）

**Files:** 临时修改（诊断后还原）：`app/src/App.vue`、`app/src/windows/FloatingOverlay.vue`

- [ ] **Step 1: 在旧推送链路加临时日志**

`app/src/App.vue` 的 `pushCurrentSubtitle()` 函数体开头插入：

```ts
  const t = sub.currentTime.value * 1000;
  const list = sub.subtitles.value;
  const idx = list.findIndex((s) => t >= s.start_ms && t < s.end_ms);
  // eslint-disable-next-line no-console
  console.debug("[ovdbg-main]", { tMs: Math.round(t), idx, lastSentOrdinal, vis: overlayVisible.value });
  if (idx < 0 || idx === lastSentOrdinal) return;
```

（`t/list/idx` 若已有同名声明则合并，避免重复 declare）

`app/src/windows/FloatingOverlay.vue` 的 `overlay://subtitle` 监听回调首行加：

```ts
        console.debug("[ovdbg-ov] recv", e.payload); // eslint-disable-line no-console
```

- [ ] **Step 2: 运行并复现**

用户在 PowerShell 执行：

```powershell
. .\scripts\m0-env.ps1
cd app
npm run tauri dev
```

操作序列：打开悬浮窗 → 正常播放观察 `[ovdbg-*]` 成对滚动 → 在字幕面板点击某句跳转 → 反复进度条拖动、±15s 快进。右键悬浮窗区域可打开 DevTools 看两个窗口各自日志（悬浮窗 DevTools 用 F12 或在 DevTools 控制台过滤 `ovdbg`）。

- [ ] **Step 3: 记录根因结论**

<!-- 结论 (2026-08-27, CDP 双窗控制台捕获两轮取证)：
     首轮会话复现卡死指纹 = 播放全程 pushCurrentSubtitle() 零调用（含去重跳过帧也无），
     仅在用户主动 seek 后 ~0.5s 内出现 2-3 帧瞬态推送随后再度静默；同期悬浮窗只显示开窗瞬间的那一句。
     主窗内嵌字幕同期表现正常（用户目测），指向"currentTime watch 订阅静默失效"而非元素时钟死亡；
     但应用重载后第二会话完全无法复现（tick 连续、三层链路同步、seek 后存活），无法进一步收敛更深层指纹。
     定性：单时间源链路脆弱性——currentTime watch 一旦失效无任何自愈路径，旧实现无轮询/seeked 兜底。
     三重时间源(timeupdate+seeked+500ms 轮询直读元素)+内容级去重对两类候选(事件断流/响应式断裂)均结构性免疫，
     与设计文档 §7 风险预案一致。此结论并入最终提交信息。 -->

把观察到的证据链写进本任务上方备注行（例：`<!-- 结论：xxx -->`）。此结论将并入最终提交信息。已知三种候选指纹：
- main 有 tick 但 `idx` 恒 `-1` → 时间值异常 / 字幕区间边界问题
- main 有 tick 且 idx 有效但无 recv → IPC/事件投递问题
- main 彻底无 tick → timeupdate 链路中断（恢复依赖本计划的 seeked/轮询双保险）

- [ ] **Step 4: 还原两处临时日志**

精确删除 Step 1 插入的两行（不提交诊断代码）：

```bash
git checkout -- app/src/App.vue app/src/windows/FloatingOverlay.vue
git status   # 确认工作区干净
```

---

### Task 2: Rust —— OverlayPrefs 结构、默认值与读写命令（单测先行）

**Files:**
- Modify: `app/src-tauri/src/floating.rs`
- Modify: `app/src-tauri/src/lib.rs`（注册命令）

- [ ] **Step 1: 写失败的单测**

在 `floating.rs` 文件末尾追加（`err_s` 已存在该文件底部）：

```rust
#[cfg(test)]
mod prefs_tests {
    use super::*;

    #[test]
    fn prefs_partial_json_fills_defaults() {
        let p: OverlayPrefs = serde_json::from_str(r#"{"display_mode":"original"}"#).unwrap();
        assert_eq!(p.display_mode, "original");
        assert_eq!(p.trans_color, "soft-white");
        assert_eq!(p.gap_behavior, "keep-last");
        assert_eq!(p.font_scale, 1.0);
    }

    #[test]
    fn prefs_roundtrip_keeps_fields() {
        let p = OverlayPrefs {
            trans_color: "amber".into(),
            font_scale: 1.4,
            ..Default::default()
        };
        let back: OverlayPrefs =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.trans_color, "amber");
        assert_eq!(back.font_scale, 1.4);
    }

    #[test]
    fn prefs_invalid_json_rejected_not_default() {
        // 坏数据必须显式报错而非静默回落（上层决定如何兜底）
        assert!(serde_json::from_str::<OverlayPrefs>("not-json").is_err());
    }
}
```

- [ ] **Step 2: 运行确认编译失败**

Run: `cargo test -p app floating`
Expected: 编译错误 `cannot find type OverlayPrefs in this scope`。

- [ ] **Step 3: 实现结构体与读写**

`floating.rs` 顶部 use 区改为（保留原有行，新增三行）：

```rust
use serde::{Deserialize, Serialize};
use crate::db::MediaDb;
use crate::AppState;
```

`OverlayState` 定义之后插入：

```rust
// ---------- 偏好持久化（设计 §3）：settings KV 单键 JSON ----------

pub const OVERLAY_PREFS_KEY: &str = "asplayer.overlay.prefs.v1";

/// 悬浮窗偏好。容器级 #[serde(default)]：JSON 缺字段自动补 Default。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayPrefs {
    /// original | bilingual | translation
    pub display_mode: String,
    /// soft-white | amber | rose | mist-blue | mint | lavender
    pub trans_color: String,
    /// keep-last | fade-5s
    pub gap_behavior: String,
    /// 0.8 ~ 2.0
    pub font_scale: f64,
}

impl Default for OverlayPrefs {
    fn default() -> Self {
        Self {
            display_mode: "bilingual".into(),
            trans_color: "soft-white".into(),
            gap_behavior: "keep-last".into(),
            font_scale: 1.0,
        }
    }
}

/// 从 DB 读偏好；键不存在返回默认，坏 JSON 显式报错
fn load_prefs(db: &MediaDb) -> Result<OverlayPrefs, String> {
    match db.get_setting(OVERLAY_PREFS_KEY) {
        Ok(Some(raw)) => serde_json::from_str(&raw).map_err(|e| format!("解析悬浮窗偏好失败: {e}")),
        Ok(None) => Ok(OverlayPrefs::default()),
        Err(e) => Err(format!("{e}")),
    }
}

#[tauri::command]
pub fn get_overlay_prefs(state: State<'_, AppState>) -> Result<OverlayPrefs, String> {
    let db = state.db.lock().map_err(err_s)?;
    load_prefs(&db)
}

/// 写库成功后向悬浮窗与主窗双向广播新值（两窗 reactive 共享同一事实）
#[tauri::command]
pub fn set_overlay_prefs(
    app: AppHandle,
    state: State<'_, AppState>,
    prefs: OverlayPrefs,
) -> Result<(), String> {
    {
        let db = state.db.lock().map_err(err_s)?;
        let raw = serde_json::to_string(&prefs).map_err(err_s)?;
        db.save_setting(OVERLAY_PREFS_KEY, &raw).map_err(err_s)?;
    }
    let _ = app.emit_to(OVERLAY_LABEL, "overlay://prefs-changed", prefs.clone());
    if let Some(main) = primary_label(&app) {
        let _ = app.emit_to(main, "overlay://prefs-changed", prefs);
    }
    Ok(())
}
```

- [ ] **Step 4: 注册两条命令**

`lib.rs` 的 `invoke_handler` 列表中，紧跟 `floating::is_overlay_locked,` 之后加入：

```rust
            floating::get_overlay_prefs,
            floating::set_overlay_prefs,
```

Run: `cargo test -p app floating`
Expected: 3 个 prefs 测试 PASS。

Run: `cargo check`
Expected: 通过（暂有未使用警告 `step_overlay_subtitle` 不存在故无警告；若报 `dead_code` 仅针对后续任务的函数则忽略至对应任务消除）。

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/floating.rs app/src-tauri/src/lib.rs
git commit -m "feat(overlay): 悬浮窗偏好 OverlayPrefs 持久化 + 读写命令 + 双窗广播"
```

---

### Task 3: Rust —— 工具栏控制转发命令 + 移除点击跳转链路

**Files:** 同 Task 2 两文件。

- [ ] **Step 1: 新增两个转发命令**

`floating.rs` 中删除整个 `overlay_request_seek` 函数及其文档注释，原地替换为：

```rust
/// 悬浮窗工具栏 ⏮⏭ → 主窗：上/下一句（±1），复用快捷键既有通道
#[tauri::command]
pub fn step_overlay_subtitle(app: AppHandle, delta: i64) {
    if let Some(main) = primary_label(&app) {
        let _ = app.emit_to(main, "overlay://step-subtitle", delta);
    }
}

/// 悬浮窗工具栏 ⏯ → 主窗：播放/暂停
#[tauri::command]
pub fn overlay_control(app: AppHandle, action: String) {
    if action != "togglePlay" {
        return;
    }
    if let Some(main) = primary_label(&app) {
        let _ = app.emit_to(main, "overlay://global-action", action);
    }
}
```

- [ ] **Step 2: 更新注册表**

`lib.rs` 注册表：删除 `floating::overlay_request_seek,` 一行，`set_overlay_prefs` 之后补两行：

```rust
            floating::step_overlay_subtitle,
            floating::overlay_control,
```

- [ ] **Step 3: 回归**

Run: `cargo test && cargo check`
Expected: workspace 全部测试 PASS（含既有 db/transcribe 套件）、无错误。

- [ ] **Step 4: Commit**

```bash
git add app/src-tauri/src/floating.rs app/src-tauri/src/lib.rs
git commit -m "feat(overlay): 工具栏控制转发命令；移除点击歌词跳转链路"
```

---

### Task 4: 前端 API 封装 + 类型清理

**Files:**
- Modify: `app/src/api/overlay.ts`
- Modify: `app/src/types.ts`

- [ ] **Step 1: 清理 types.ts 死类型**

先确认确无引用：

Run（Grep 工具，pattern `\bOverlayConfig\b|\bOverlayCurrent\b`，path `app/src`）
Expected: 仅命中 `types.ts` 自身两处定义。

`types.ts` 删除以下两个接口整块：

```ts
/** 迷你悬浮窗配置（后端 SQLite settings 单键持久化，修改即广播） */
export interface OverlayConfig { /* …现有整块… */ }

/** 当前句推送给悬浮窗的数据 */
export interface OverlayCurrent { /* …现有整块… */ }
```

原位新增：

```ts
export type OverlayDisplayMode = "original" | "bilingual" | "translation";
export type OverlayGapBehavior = "keep-last" | "fade-5s";
export const OVERLAY_PRESET_COLORS = [
  "soft-white", "amber", "rose", "mist-blue", "mint", "lavender",
] as const;
export type OverlayPresetColor = (typeof OVERLAY_PRESET_COLORS)[number];

/** 悬浮窗偏好（后端 settings KV 单键持久化，修改即双窗广播） */
export interface OverlayPrefs {
  display_mode: OverlayDisplayMode;
  trans_color: OverlayPresetColor;
  gap_behavior: OverlayGapBehavior;
  font_scale: number; // 0.8 ~ 2.0
}
```

- [ ] **Step 2: 扩展 api/overlay.ts**

文件末尾追加：

```ts
/** 悬浮窗工具栏：上/下一句 */
export function stepOverlaySubtitle(delta: number) {
  return invoke<void>("step_overlay_subtitle", { delta });
}

/** 悬浮窗工具栏：播放/暂停（转发主窗） */
export function overlayPlayPause() {
  return invoke<void>("overlay_control", { action: "togglePlay" });
}

/** 读/写悬浮窗偏好（结构体整体写入，Rust 端 serde 缺省补齐） */
export function getOverlayPrefs() {
  return invoke<import("../types").OverlayPrefs>("get_overlay_prefs");
}

export function setOverlayPrefs(prefs: import("../types").OverlayPrefs) {
  return invoke<void>("set_overlay_prefs", { prefs });
}
```

- [ ] **Step 3: 类型检查**

Run: `cd app && npx vue-tsc --noEmit`
Expected: 通过。

- [ ] **Step 4: Commit**

```bash
git add app/src/types.ts app/src/api/overlay.ts
git commit -m "feat(overlay): 偏好/控制 API 封装；清理未用的 OverlayConfig 旧类型"
```

---

### Task 5: 前端偏好仓库 stores/overlayPrefs.ts

**Files:**
- Create: `app/src/stores/overlayPrefs.ts`

- [ ] **Step 1: 新建文件（完整内容）**

```ts
import { reactive } from "vue";
import { listen } from "@tauri-apps/api/event";
import { getOverlayPrefs, setOverlayPrefs } from "../api/overlay";
import type { OverlayPrefs } from "../types";

/** 预设色枚举 → 实际渲染色值（低饱和方向，配 Quiet Glass） */
export const TRANSLATION_HEX = {
  "soft-white": "rgba(255,255,255,0.72)",
  amber: "#e5b389",
  rose: "#ff9fb2",
  "mist-blue": "#9dc4f0",
  mint: "#9fe0c6",
  lavender: "#c9aef0",
} as const;

export const overlayPrefs = reactive<OverlayPrefs>({
  display_mode: "bilingual",
  trans_color: "soft-white",
  gap_behavior: "keep-last",
  font_scale: 1,
});

/** 启动时调用一次：读后端持久化值（失败保持默认） */
export async function loadOverlayPrefs(): Promise<void> {
  try {
    Object.assign(overlayPrefs, await getOverlayPrefs());
  } catch {
    /* 后端不可达时静默用默认值，不打断窗口加载 */
  }
}

/** 本地乐观更新 + 后端持久化；后端回声事件与本地位收敛为同值，无竞态风险 */
export function patchOverlayPrefs(patch: Partial<OverlayPrefs>): void {
  Object.assign(overlayPrefs, patch);
  setOverlayPrefs({ ...overlayPrefs }).catch(() => {});
}

/** 订阅另一窗口发起的变更（由 Rust 统一广播） */
export async function watchOverlayPrefs(): Promise<() => void> {
  const un = await listen<Partial<OverlayPrefs>>("overlay://prefs-changed", (e) => {
    if (e.payload) Object.assign(overlayPrefs, e.payload);
  });
  return un;
}
```

- [ ] **Step 2: 类型检查并提交**

Run: `cd app && npx vue-tsc --noEmit`
Expected: 通过。

```bash
git add app/src/stores/overlayPrefs.ts
git commit -m "feat(overlay): 跨窗共享的偏好响应式仓库"
```

---

### Task 6: 主窗推送引擎 overlayFeed.ts

**Files:**
- Create: `app/src/overlayFeed.ts`

- [ ] **Step 1: 新建文件（完整内容）**

```ts
/**
 * 悬浮窗字幕推送引擎（设计 §4，Bug #4 根治）。
 * 唯一职责：根据媒体元素时钟计算当前句 → 内容变化才推给悬浮窗。
 * 三重时间源：timeupdate（常规）/ seeked（跳转后强制）/ 播放中 500ms 轮询（兜底）；
 * 内容级去重：与上次实际推送的 文本+start_ms 全等比较，序号类状态彻底退场。
 */
import { pushOverlaySubtitle } from "./api/overlay";
import { useSubtitle } from "./stores/subtitle";
import { overlayPrefs } from "./stores/overlayPrefs";
import type { Subtitle } from "./types";

const GAP_FADE_MS = 5000;
const POLL_MS = 500;
/** 已清屏哨兵：区别于"从未推送"（null） */
const CLEAR = { key: "\0clear\0" };

const sub = useSubtitle();

let media: HTMLMediaElement | null = null;
let enabled = false;
let pollTimer: number | null = null;
let abortCtrl: AbortController | null = null;
let lastKey: string | null = null; // 上次推送的内容指纹；CLEAR.key 或 `${start}${text}${tr}`
let gapSince = -1;                // 进入句间空隙的时间戳；-1 = 不在空隙

function sentenceAt(tMs: number): Subtitle | null {
  return (
    sub.subtitles.value.find((s) => tMs >= s.start_ms && tMs < s.end_ms) ?? null
  );
}

function pushClear(): void {
  gapSince = -1;
  lastKey = CLEAR.key;
  pushOverlaySubtitle("", "", 0).catch(() => {});
}

/** 句间空隙行为（设计 §4.5）：keep-last 保留上句不动；fade-5s 满 5s 清屏 */
function tickGap(inGap: boolean): void {
  if (!inGap) {
    gapSince = -1;
    return;
  }
  if (lastKey === CLEAR.key) return;
  if (overlayPrefs.gap_behavior !== "fade-5s") return;
  if (gapSince < 0) {
    gapSince = Date.now();
    return;
  }
  if (Date.now() - gapSince >= GAP_FADE_MS) pushClear();
}

/** 核心：算当前句并与上次推送比对。force 忽略去重缓存（seek 场景） */
export function sync(force = false): void {
  if (!enabled || !media) return;
  const tMs = Math.round(media.currentTime * 1000);
  const sentence = sentenceAt(tMs);
  tickGap(sentence === null);
  if (!sentence) return;
  const key = `${sentence.start_ms}${sentence.text}${sentence.translation}`;
  if (!force && lastKey === key) return;
  lastKey = key;
  pushOverlaySubtitle(sentence.text, sentence.translation, sentence.start_ms).catch(() => {});
}

function startPolling(): void {
  if (pollTimer !== null) return;
  pollTimer = window.setInterval(() => sync(false), POLL_MS);
}

function stopPolling(): void {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer);
    pollTimer = null;
  }
}

/**
 * PlayerStage 把 mediaEl ref 变化喂进来（video/audio 切换或置 null）。
 * 所有 seek 入口（面板点击/进度条/±15s/上下一句/悬浮窗请求）都作用于同一元素，
 * 因此只需在此监听 seeking/seeked 即覆盖全部来源，无需改造各个入口。
 */
export function attachMedia(el: HTMLMediaElement | null): void {
  abortCtrl?.abort();
  stopPolling();
  media = el;
  lastKey = null; // 元素更换视为内容全变，下次必推
  gapSince = -1;
  if (!el) return;
  abortCtrl = new AbortController();
  const opt = { signal: abortCtrl.signal };
  el.addEventListener("timeupdate", () => sync(false), opt);
  el.addEventListener("seeking", () => { gapSince = -1; }, opt);
  el.addEventListener("seeked", () => sync(true), opt);
  el.addEventListener("loadedmetadata", () => { lastKey = null; }, opt);
  el.addEventListener(
    "play",
    () => {
      startPolling();
      sync(true);
    },
    opt
  );
  el.addEventListener("pause", () => sync(false), opt);
  if (!el.paused) startPolling();
}

/** 悬浮窗隐藏时挂起推送；重新可见时强制补推当前句（避免停留在陈旧去重缓存） */
export function setEnabled(v: boolean): void {
  enabled = v;
  if (v) sync(true);
}

/** 切换文件 / 字幕列表变化后调用：清掉悬浮窗残留内容 */
export function resetFeed(): void {
  lastKey = null;
  gapSince = -1;
  if (enabled) pushClear();
}
```

- [ ] **Step 2: 类型检查并提交**

Run: `cd app && npx vue-tsc --noEmit`
Expected: 通过（`attachMedia/sync/setEnabled/resetFeed` 暂未被引用会有 lint 提示但 vue-tsc 不报未使用导出，正常）。

```bash
git add app/src/overlayFeed.ts
git commit -m "feat(overlay): 推送引擎——三重时间源 + 内容级去重 + 句间空隙行为"
```

---

### Task 7: App.vue 接线替换

**Files:**
- Modify: `app/src/App.vue`

- [ ] **Step 1: 删除旧推送实现**

删除以下整块（约位于"M3 迷你悬浮字幕窗"注释区）：

```ts
/** 最近一次推送到悬浮窗的句序号；-2 表示未知（换文件/换列表后强制重发） */
let lastSentOrdinal = -2;
```

```ts
/**
 * 歌词式推送当前句到悬浮窗（后端中继）。
 * ……原 pushCurrentSubtitle 及其三条 watch、overlayVisible 补推 watch……
 */
function pushCurrentSubtitle() { /* 整个函数 */ }

watch(() => sub.currentTime.value, pushCurrentSubtitle);
watch(
  () => sub.subtitles.value,
  () => {
    lastSentOrdinal = -2;
  }
);
watch(
  () => current.value?.id,
  () => {
    lastSentOrdinal = -2;
  }
);
watch(overlayVisible, (v) => {
  if (!v) return;
  lastSentOrdinal = -2;
  pushCurrentSubtitle();
});
```

同时删除 onMounted 内的 do-seek 监听（原 u7 整段）：

```ts
  const u7 = await listen<number>("overlay://do-seek", (e) => {
    seekTo((e.payload ?? 0) / 1000);
  });
```

并把 `unlisteners.push(u5, u6, u7, u8, u9);` 改为 `unlisteners.push(u5, u6, u8, u9);`，其后的重编号注释同步去掉 u7。

`api/overlay` 的 import 列表中去掉 `pushOverlaySubtitle`。

- [ ] **Step 2: 接入 overlayFeed**

script 顶部新增：

```ts
import { setEnabled as overlaySetEnabled, resetFeed as overlayResetFeed } from "./overlayFeed";
```

M3 注释区原位置改为：

```ts
// 悬浮窗显隐开关直接驱动推送引擎挂起/恢复
watch(overlayVisible, overlaySetEnabled);
// 换文件或字幕数据刷新后：清空悬浮窗残留，等待下一次真实句子
watch(
  () => sub.currentId.value,
  () => overlayResetFeed()
);
watch(sub.subtitles, () => overlayResetFeed());
```

onMounted 尾部读取初始状态的代码块中，`isOverlayVisible().then(...)` 之后追加一行启用引擎：

```ts
  isOverlayVisible()
    .then((v) => {
      overlayVisible.value = v;
      overlaySetEnabled(v);
    })
    .catch(() => {});
```

- [ ] **Step 3: 类型检查**

Run: `cd app && npx vue-tsc --noEmit`
Expected: 通过。

- [ ] **Step 4: Commit**

```bash
git add app/src/App.vue
git commit -m "refactor(overlay): 主窗接入推送引擎，拆除序号去重旧链路与 do-seek 监听"
```

---

### Task 8: PlayerStage 挂载媒体元素

**Files:**
- Modify: `app/src/components/PlayerStage.vue`（现有 `watch(mediaEl, …)` 位于约 123 行）

- [ ] **Step 1: 引擎接管元素监听**

script 区新增 import：

```ts
import { attachMedia as attachOverlayMedia } from "../overlayFeed";
```

把现有：

```ts
watch(mediaEl, (el) => {
  if (el) applyVolume();
});
```

改为：

```ts
watch(mediaEl, (el) => {
  if (el) applyVolume();
  // video/audio 切换、卸载皆经此点同步给推送引擎（null 亦传递以解绑）
  attachOverlayMedia(el);
});
```

- [ ] **Step 2: 类型检查并提交**

Run: `cd app && npx vue-tsc --noEmit`
Expected: 通过。

```bash
git add app/src/components/PlayerStage.vue
git commit -m "feat(overlay): mediaEl 生命周期移交推送引擎托管"
```

---

### Task 9: FloatingOverlay.vue 全面重写

**Files:**
- Rewrite: `app/src/windows/FloatingOverlay.vue`

- [ ] **Step 1: 用以下完整内容替换整个文件**

```vue
<script setup lang="ts">
/**
 * M3 悬浮字幕窗 · 桌面歌词化重写（设计文档 2026-08-27 §2/§3）
 * - Quiet Glass 玻璃条：原文大字白 + 译文预设色，整条可拖拽（锁定态除外）
 * - 悬停工具栏：⏮⏯⏭ / 显示模式三态 / ⚙就地设置 / 锁定 / 关闭
 * - 显示模式在本窗渲染取舍：主窗恒推 原文+译文 全量，切换零通信
 * - 字幕数据来源：后端中继 overlay://subtitle（由主窗 overlayFeed 推送）
 */
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  TRANSLATION_HEX,
  loadOverlayPrefs,
  overlayPrefs,
  patchOverlayPrefs,
  watchOverlayPrefs,
} from "../stores/overlayPrefs";
import { overlayPlayPause, stepOverlaySubtitle } from "../api/overlay";
import type { OverlayDisplayMode } from "../types";

interface SubtitlePayload {
  text: string;
  translation: string;
  start_ms: number;
}

const appWindow = getCurrentWindow();
const text = ref("");
const translation = ref("");
const startMs = ref(0);
const locked = ref(false);
const tbVisible = ref(false);   // 工具栏可见性（悬停 2s 延迟隐藏）
const panelOpen = ref(false);   // ⚙迷你面板
const receivedFirst = ref(false); // 是否收到过第一条真句（决定提示条显隐）
const unlisteners: (() => void)[] = [];

let hideTimer: number | null = null;

/** 按显示模式取舍渲染行；缺译文回退原文，绝不空白 */
const lines = computed<{ cls: "orig" | "trans"; text: string }[]>(() => {
  const o = text.value.trim();
  const tr = translation.value.trim();
  if (overlayPrefs.display_mode === "original") return o ? [{ cls: "orig", text: o }] : [];
  if (overlayPrefs.display_mode === "translation") {
    const t = tr || o;
    return t ? [{ cls: "trans", text: t }] : [];
  }
  const out: { cls: "orig" | "trans"; text: string }[] = [];
  if (o) out.push({ cls: "orig", text: o });
  if (tr && tr !== o) out.push({ cls: "trans", text: tr });
  return out;
});

const transColor = computed(
  () => TRANSLATION_HEX[overlayPrefs.trans_color] ?? TRANSLATION_HEX["soft-white"]
);
const origSize = computed(() => `${22 * overlayPrefs.font_scale}px`);
const transSize = computed(() => `${16 * overlayPrefs.font_scale}px`);

// ---- 手势与工具栏 ----

/** 整条玻璃卡拖拽（点击跳转功能已按设计移除） */
function onDragStart(e: PointerEvent) {
  if (locked.value || e.button !== 0) return;
  appWindow.startDragging().catch(() => {});
}

function tbShow() {
  if (locked.value) return;
  tbVisible.value = true;
  if (hideTimer !== null) window.clearTimeout(hideTimer);
}

function tbHide() {
  if (hideTimer !== null) window.clearTimeout(hideTimer);
  hideTimer = window.setTimeout(() => {
    tbVisible.value = false;
    panelOpen.value = false;
  }, 2000);
}

function setMode(m: OverlayDisplayMode) {
  patchOverlayPrefs({ display_mode: m });
}

function lockOverlay() {
  invoke("set_overlay_locked", { locked: true }).catch(() => {});
}

function closeOverlay() {
  invoke("set_overlay_visible", { visible: false }).catch(() => {});
}

const MODE_ITEMS: { key: OverlayDisplayMode; label: string }[] = [
  { key: "original", label: "原文" },
  { key: "bilingual", label: "双语" },
  { key: "translation", label: "译文" },
];

const COLOR_KEYS = Object.keys(TRANSLATION_HEX) as (keyof typeof TRANSLATION_HEX)[];

onMounted(async () => {
  try {
    unlisteners.push(
      await listen<SubtitlePayload>("overlay://subtitle", (e) => {
        const p = e.payload ?? { text: "", translation: "", start_ms: 0 };
        if (p.text || p.translation) receivedFirst.value = true;
        text.value = p.text ?? "";
        translation.value = p.translation ?? "";
        startMs.value = p.start_ms ?? 0;
      }),
      await listen<boolean>("overlay://lock-changed", (e) => {
        locked.value = !!e.payload;
        if (locked.value) {
          tbVisible.value = false;
          panelOpen.value = false;
        }
      }),
      await watchOverlayPrefs()
    );
  } catch (err) {
    console.error("[overlay] 事件监听注册失败:", err);
  }
  await loadOverlayPrefs();
  try {
    locked.value = await invoke<boolean>("is_overlay_locked");
  } catch {
    /* 忽略 */
  }
});

onUnmounted(() => {
  unlisteners.forEach((u) => u());
  if (hideTimer !== null) window.clearTimeout(hideTimer);
});
</script>

<template>
  <div
    class="overlay-root"
    :class="{ locked }"
    @mouseenter="tbShow"
    @mouseleave="tbHide"
  >
    <!-- 从未收到过字幕时的引导提示 -->
    <div v-if="!receivedFirst" class="boot-hint">开始播放后此处显示字幕</div>

    <!-- 玻璃歌词条 -->
    <div
      v-else
      class="glass"
      :class="{ dragging: !locked }"
      @pointerdown="onDragStart"
    >
      <!-- 悬停工具栏：交互区阻断拖拽冒泡 -->
      <div
        v-show="tbVisible"
        class="tb"
        @pointerdown.stop
      >
        <button class="tbtn" title="上一句" @click="stepOverlaySubtitle(-1)">⏮</button>
        <button class="tbtn" title="播放/暂停" @click="overlayPlayPause">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M7 5l12 7-12 7z"/></svg>
        </button>
        <button class="tbtn" title="下一句" @click="stepOverlaySubtitle(1)">⏭</button>
        <span class="sep"></span>
        <div class="seg">
          <button
            v-for="m in MODE_ITEMS"
            :key="m.key"
            class="seg-btn"
            :class="{ on: overlayPrefs.display_mode === m.key }"
            @click="setMode(m.key)"
          >{{ m.label }}</button>
        </div>
        <span class="sep"></span>
        <div
          v-show="panelOpen"
          class="panel"
          @pointerdown.stop
        >
          <div class="panel-title">译文颜色</div>
          <div class="swatches">
            <button
              v-for="(hex, key) in TRANSLATION_HEX"
              :key="key"
              class="swatch"
              :class="{ on: overlayPrefs.trans_color === key }"
              :style="{ background: hex }"
              :title="key"
              @click="patchOverlayPrefs({ trans_color: key })"
            ></button>
          </div>
          <div class="panel-title">句间空隙</div>
          <select
            class="sel"
            :value="overlayPrefs.gap_behavior"
            @change="patchOverlayPrefs({ gap_behavior: ($event.target as HTMLSelectElement).value as 'keep-last' | 'fade-5s' })"
          >
            <option value="keep-last">保留上一句</option>
            <option value="fade-5s">5 秒后淡出</option>
          </select>
          <div class="panel-title">字号 {{ Math.round(overlayPrefs.font_scale * 100) }}%</div>
          <input
            class="font-range"
            type="range" min="0.8" max="2" step="0.05"
            :value="overlayPrefs.font_scale"
            @change="patchOverlayPrefs({ font_scale: Number(($event.target as HTMLInputElement).value) })"
          />
        </div>
        <button class="tbtn" title="设置" @click="panelOpen = !panelOpen">⚙</button>
        <button class="tbtn" title="锁定（鼠标穿透，Ctrl+Alt+L 解锁）" @click="lockOverlay">🔒</button>
        <button class="tbtn danger" title="关闭悬浮字幕窗" @click="closeOverlay">✕</button>
      </div>

      <Transition name="linefade" mode="out-in">
        <div v-if="lines.length" :key="startMs" class="lines">
          <p
            v-for="(l, i) in lines"
            :key="i"
            class="line"
            :class="l.cls"
            :style="l.cls === 'orig'
              ? { fontSize: origSize }
              : { fontSize: transSize, color: transColor }"
          >{{ l.text }}</p>
        </div>
        <div v-else :key="'clear'" class="cleared"></div>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.overlay-root {
  width: 100vw;
  height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  overflow: hidden;
  user-select: none;
}

/* Quiet Glass 玻璃条：毛玻璃不生效时自然降级为半透明深底（观感相近） */
.glass {
  width: calc(100vw - 28px);
  max-height: calc(100vh - 16px);
  border-radius: 18px;
  background: rgba(12, 14, 20, 0.55);
  backdrop-filter: blur(16px) saturate(130%);
  outline: 1px solid rgba(255, 255, 255, 0.09);
  box-shadow: 0 10px 36px rgba(0, 0, 0, 0.45);
  padding: 34px 20px 18px;
  cursor: grab;
}
.glass.dragging:active {
  cursor: grabbing;
}
.overlay-root.locked .glass {
  cursor: default;
}

/* ---- 悬停工具栏 ---- */
.tb {
  position: absolute;
  top: 8px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 6px;
  border-radius: 11px;
  background: rgba(255, 255, 255, 0.07);
  white-space: nowrap;
  z-index: 3;
}
.overlay-root.locked .tb {
  display: none;
}
.tbtn {
  width: 26px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: rgba(235, 235, 240, 0.85);
  font-size: 12px;
  cursor: pointer;
}
.tbtn:hover {
  background: rgba(255, 255, 255, 0.14);
  color: #fff;
}
.tbtn.danger:hover {
  background: #e5484d;
  color: #fff;
}
.tbtn svg {
  width: 12px;
  height: 12px;
}
.sep {
  width: 1px;
  height: 14px;
  background: rgba(255, 255, 255, 0.14);
}
.seg {
  display: flex;
  gap: 2px;
  padding: 2px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.08);
}
.seg-btn {
  border: none;
  border-radius: 6px;
  padding: 2px 8px;
  background: transparent;
  color: rgba(235, 235, 240, 0.55);
  font-size: 11px;
  cursor: pointer;
}
.seg-btn.on {
  background: rgba(255, 255, 255, 0.16);
  color: #fff;
  font-weight: 600;
}

/* ---- ⚙就地迷你面板 ---- */
.panel {
  position: absolute;
  top: calc(100% + 8px);
  left: 50%;
  transform: translateX(-50%);
  width: 208px;
  padding: 10px 12px 12px;
  border-radius: 13px;
  background: rgba(12, 14, 20, 0.82);
  outline: 1px solid rgba(255, 255, 255, 0.09);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  cursor: default;
}
.panel-title {
  margin: 7px 0 5px;
  font-size: 10px;
  letter-spacing: 0.06em;
  color: rgba(235, 235, 240, 0.45);
}
.swatches {
  display: flex;
  gap: 7px;
}
.swatch {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: 2px solid transparent;
  cursor: pointer;
  padding: 0;
}
.swatch.on {
  border-color: #fff;
}
.sel {
  width: 100%;
  border: none;
  border-radius: 7px;
  padding: 4px 6px;
  background: rgba(255, 255, 255, 0.08);
  color: rgba(235, 235, 240, 0.85);
  font-size: 11px;
  cursor: pointer;
}
.font-range {
  width: 100%;
  accent-color: var(--accent, #d98d5f);
}

/* ---- 文字行 ---- */
.lines {
  text-align: center;
  line-height: 1.55;
}
.line {
  margin: 0;
  word-break: break-word;
}
.line.orig {
  color: #fff;
  font-weight: 600;
  text-shadow: 0 1px 4px rgba(0, 0, 0, 0.55);
}
.line.trans {
  margin-top: 5px;
  text-shadow: 0 1px 3px rgba(0, 0, 0, 0.5);
}
.cleared {
  height: 8px; /* 占位保住布局，视觉完全透明 */
}
.boot-hint {
  padding: 8px 14px;
  border-radius: 10px;
  background: rgba(8, 10, 14, 0.55);
  color: rgba(255, 255, 255, 0.55);
  font-size: 13px;
}

.linefade-enter-active,
.linefade-leave-active {
  transition: opacity 0.17s ease;
}
.linefade-enter-from,
.linefade-leave-to {
  opacity: 0;
}
</style>
```

- [ ] **Step 2: 类型检查**

Run: `cd app && npx vue-tsc --noEmit`
Expected: 通过。（若 `COLOR_KEYS` 报未使用告警则从脚本中删除该常量——模板直接遍历 `TRANSLATION_HEX`，它确属多余。）

- [ ] **Step 3: Commit**

```bash
git add app/src/windows/FloatingOverlay.vue
git commit -m "feat(overlay): 桌面歌词化玻璃条重写——整条拖拽/悬停工具栏/就地设置/模式三态"
```

---

### Task 10: 全量回归验收（含 Bug #4 验证闭环）

**Files:** 无代码变更（问题的话回到对应任务修复）。

- [ ] **Step 1: 自动化回归**

```powershell
. .\scripts\m0-env.ps1
cargo test              # workspace 全绿
cargo check             # 无 error
cd app; npx vue-tsc --noEmit   # 通过
```

- [ ] **Step 2: 启动应用跑手动验收清单**

`npm run tauri dev`，对照设计文档 §6 逐项打勾（结果记到本 checkbox 下备注）：

1. 整条玻璃卡任意位置按住即可拖动；锁定后点击穿透、Ctrl+Alt+L 解锁生效
2. 原文/双语/译文即时切换且与主窗字幕面板的模式互不影响；重启后两窗各自记忆保持
3. **seek 四连测（核心验收，每项之后悬浮窗必须立即显示目标句）**：字幕面板点句跳转 / 进度条拖动 / ±15s 按钮 / 全局快捷键上一句下一句
4. 设置 gap=fade-5s 后进入长空隙约 5 秒淡出、下一句浮现恢复；切回 keep-last 空隙保持上句；切换文件悬浮窗清空
5. ⚙面板：色板切换译文颜色即时生效、字号滑杆缩放、重启持久；⏯⏮⏭在主窗最小化/失焦时依然生效
6. 回归：悬浮窗显隐按钮与快捷键、锁定图标状态同步、转写/翻译进行中的进度推送正常、普通播放流转句正常

- [ ] **Step 3: 写入根因结论并收尾提交（若有遗留小修）**

把 Task 1 记录的根因结论作为最终修复相关提交的信息正文（或新建一条 `docs:` 提交记录于 `docs/milestone-notes` 的习惯位置不存在则放提交正文）：

```bash
git status   # 应仅有预期文件；确认干净则跳过此步
```

---

## Self-Review 记录

1. **规格覆盖**：§2 玻璃条/工具栏/拖拽/锁定（Task 9）、§3 偏好四字段+KV+广播（Task 2/5）、§4 三重时间源+内容去重+gap+本地模式渲染（Task 6/9）、§5 文件清单全覆盖（ACL 差异已在头部说明）、§6 验收清单（Task 10）✅
2. **占位符扫描**：无 TBD/TODO；Task 1/10 的运行观察步骤属于必要的运行时人工动作，非代码占位 ✅
3. **类型一致性**：`OverlayPrefs` 四字段前后端同名 snake_case 对齐；`step_overlay_subtitle(delta:i64)↔stepOverlaySubtitle({delta:number})`、`overlay_control(action)↔overlayPlayPause()`、payload `overlay://prefs-changed` 广播↔订阅名一致；`attachMedia(el|null)` 与 PlayerStage watch 的 `el` 类型（HTMLVideoElement|HTMLAudioElement|null → HTMLMediaElement|null）兼容 ✅
