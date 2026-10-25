# Cinema First UI 改版 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 ASPlayer 主窗改成「整窗即舞台」的浮层布局：视频铺满、字幕抽屉展开时画面缩进安全区、播放时控件自动隐藏，并保留全部现有功能。

**Architecture:** `App.vue` 从三栏 flex 改为单层舞台容器，所有面板绝对定位为浮层。`PlayerStage` 继续持有播放逻辑，只重排视频区与控制条；`PlaylistPanel` 增加内嵌模式，由新的 `MediaRail` 包裹；`SubtitlePanel` 原地改成玻璃抽屉。播放器状态与最近播放排序抽成纯函数便于单测。

**Tech Stack:** Vue 3 `<script setup>` + TypeScript、Tauri 2、Rust + rusqlite、Vitest、CSS 变量与 `backdrop-filter`。

---

## 文件结构

```
app/src/
  App.vue                        # 单层舞台、浮层编排、自动隐藏、抽屉状态
  components/
    TitleBar.vue                 # 透明顶栏浮层 + 窗口控制
    MediaRail.vue                # 新增：收起封面列 + 悬停展开 + 双标签
    PlaylistPanel.vue            # 增加 embedded / recentOnly 模式
    SubtitlePanel.vue            # 原地改为玻璃抽屉
    PlayerStage.vue              # 安全区、底栏胶囊、暴露转写动作
    SettingsPanel.vue            # 外观页加自动隐藏开关
  lib/
    uiState.ts                   # 新增：主按钮状态 + 最近播放排序
    uiState.test.ts              # 新增
  styles/tokens.css              # 重写令牌
  stores/playback.ts             # 增加 chromeAutoHide
  types.ts                       # MediaItem 增加 last_played_at
app/src-tauri/src/
  media.rs                       # MediaItem 增加 last_played_at
  db.rs                          # 迁移、查询、写入
```

---

### Task 1: 后端记录 `last_played_at`

**Files:**
- Modify: `app/src-tauri/src/media.rs:17-32`
- Modify: `app/src-tauri/src/db.rs:25-63, 133-169`
- Test: `app/src-tauri/src/db.rs:444+`（现有 `mod tests`）
- Modify: `app/src/types.ts:1-16`

- [ ] **Step 1: 写失败测试**

在 `app/src-tauri/src/db.rs` 的 `mod tests` 末尾加入：

```rust
#[test]
fn save_position_stamps_last_played_at() -> rusqlite::Result<()> {
    let db = MediaDb::open_in_memory()?;
    let id = db.upsert_media("D:/m/a.mp3", "a", "audio", 0)?;
    assert_eq!(db.list_media()?[0].last_played_at, None);

    db.save_playback_position(id, 42_000)?;

    let item = &db.list_media()?[0];
    assert_eq!(item.playback_position, 42_000);
    assert!(item.last_played_at.is_some());
    Ok(())
}

#[test]
fn migrate_adds_last_played_column() -> rusqlite::Result<()> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TABLE media_files (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            path              TEXT NOT NULL UNIQUE,
            title             TEXT NOT NULL,
            media_type        TEXT NOT NULL,
            duration_ms       INTEGER NOT NULL DEFAULT 0,
            playback_position INTEGER NOT NULL DEFAULT 0,
            speed             REAL NOT NULL DEFAULT 1.0,
            added_at          TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    MediaDb::migrate_last_played(&conn)?;
    MediaDb::migrate_last_played(&conn)?;

    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(media_files)")?
        .query_map([], |r| r.get(1))?
        .collect::<Result<_, _>>()?;
    assert!(cols.iter().any(|c| c == "last_played_at"));
    Ok(())
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml last_played`
Expected: 编译失败，提示 `MediaItem` 没有 `last_played_at`、`migrate_last_played` 不存在。

- [ ] **Step 3: 增加 Rust 字段**

`app/src-tauri/src/media.rs`，在 `profile_override` 后加入：

```rust
    pub last_played_at: Option<String>, // 最近播放时间；从未播放为 None
```

- [ ] **Step 4: 迁移与建表**

`app/src-tauri/src/db.rs`：

1. `init` 的 `CREATE TABLE media_files` 在 `added_at` 前加入：

```sql
                last_played_at    TEXT,
```

2. `init` 末尾（`migrate_intensive` 后）加入：

```rust
        Self::migrate_last_played(conn)
```

3. 在 `migrate_intensive` 后新增函数：

```rust
    /// 幂等迁移（v4）：记录最近播放时间。
    pub fn migrate_last_played(conn: &Connection) -> rusqlite::Result<()> {
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(media_files)")?
            .query_map([], |r| r.get(1))?
            .collect::<Result<_, _>>()?;
        if !cols.iter().any(|c| c == "last_played_at") {
            conn.execute_batch("ALTER TABLE media_files ADD COLUMN last_played_at TEXT;")?;
        }
        Ok(())
    }
```

- [ ] **Step 5: 查询与写入**

`db.rs::list_media` 的 SELECT 末尾加 `m.last_played_at`：

```rust
            "SELECT m.id, m.path, m.title, m.media_type, m.duration_ms, m.playback_position,
                    m.file_size, m.subtitle_status, m.subtitle_lang,
                    (SELECT COUNT(*) FROM subtitles s WHERE s.media_id = m.id),
                    COALESCE(m.speed, 1.0), COALESCE(m.volume, 1.0),
                    m.transcribe_next_ms, m.profile_override, m.last_played_at
             FROM media_files m ORDER BY m.added_at DESC, m.id DESC",
```

结构体映射加入：

```rust
                last_played_at: r.get(14)?,
```

`save_playback_position` 改为：

```rust
    pub fn save_playback_position(&self, id: i64, position_ms: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_files
                SET playback_position = ?1, last_played_at = datetime('now')
              WHERE id = ?2",
            rusqlite::params![position_ms, id],
        )?;
        Ok(())
    }
```

- [ ] **Step 6: 前端类型**

`app/src/types.ts` 的 `MediaItem` 末尾加入：

```ts
  last_played_at: string | null;
```

- [ ] **Step 7: 运行测试**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml`
Expected: 全部 PASS。

- [ ] **Step 8: 提交**

```bash
git add app/src-tauri/src/media.rs app/src-tauri/src/db.rs app/src/types.ts
git commit -m "feat(db): 记录最近播放时间"
```

---

### Task 2: 纯函数与单测

**Files:**
- Create: `app/src/lib/uiState.ts`
- Test: `app/src/lib/uiState.test.ts`

- [ ] **Step 1: 写失败测试**

创建 `app/src/lib/uiState.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import type { MediaItem } from "../types";
import { resolvePrimaryAction, sortRecentMedia } from "./uiState";

function item(over: Partial<MediaItem>): MediaItem {
  return {
    id: 1,
    path: "D:/m/a.mp4",
    title: "a",
    media_type: "video",
    duration_ms: 60_000,
    playback_position: 0,
    file_size: 0,
    subtitle_status: "none",
    subtitle_lang: "",
    subtitle_count: 0,
    transcribe_next_ms: 0,
    speed: 1,
    volume: 1,
    profile_override: null,
    last_played_at: null,
    ...over,
  };
}

describe("resolvePrimaryAction", () => {
  it("无文件时禁用", () => {
    expect(resolvePrimaryAction(null, [], null).action).toBe("none");
    expect(resolvePrimaryAction(null, [], null).disabled).toBe(true);
  });

  it("无字幕时转写", () => {
    const s = resolvePrimaryAction(item({}), [], null);
    expect(s.action).toBe("transcribe");
    expect(s.label).toBe("转写并翻译");
  });

  it("断点续传", () => {
    expect(resolvePrimaryAction(item({ subtitle_status: "partial" }), [], null).action).toBe("resume");
  });

  it("运行中显示取消与进度", () => {
    const s = resolvePrimaryAction(item({ subtitle_status: "transcribing" }), [], 42);
    expect(s.action).toBe("cancel");
    expect(s.progress).toBe(42);
  });

  it("错误时重新转写", () => {
    expect(resolvePrimaryAction(item({ subtitle_status: "error" }), [], null).action).toBe("retranscribe");
  });

  it("存在空译文时翻译", () => {
    const s = resolvePrimaryAction(
      item({ subtitle_status: "done" }),
      [{ translation: "你好" }, { translation: "  " }],
      null
    );
    expect(s.action).toBe("translate");
  });

  it("全部有译文时就绪", () => {
    const s = resolvePrimaryAction(
      item({ subtitle_status: "done" }),
      [{ translation: "你好" }],
      null
    );
    expect(s.action).toBe("ready");
  });
});

describe("sortRecentMedia", () => {
  it("只保留播放过的并按时间倒序", () => {
    const list = [
      item({ id: 1, last_played_at: "2026-10-20 10:00:00" }),
      item({ id: 2, last_played_at: null }),
      item({ id: 3, last_played_at: "2026-10-24 10:00:00" }),
    ];
    expect(sortRecentMedia(list).map((m) => m.id)).toEqual([3, 1]);
  });

  it("同一时间按 id 倒序", () => {
    const list = [
      item({ id: 5, last_played_at: "2026-10-24 10:00:00" }),
      item({ id: 9, last_played_at: "2026-10-24 10:00:00" }),
    ];
    expect(sortRecentMedia(list).map((m) => m.id)).toEqual([9, 5]);
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm --prefix app run test -- src/lib/uiState.test.ts`
Expected: FAIL，提示无法解析 `./uiState`。

- [ ] **Step 3: 实现**

创建 `app/src/lib/uiState.ts`：

```ts
import type { MediaItem, Subtitle } from "../types";

export type PrimaryAction =
  | "none"
  | "transcribe"
  | "retranscribe"
  | "resume"
  | "cancel"
  | "translate"
  | "ready";

export interface PrimaryActionState {
  action: PrimaryAction;
  label: string;
  disabled: boolean;
  progress: number | null;
}

export function resolvePrimaryAction(
  item: MediaItem | null,
  subtitles: Pick<Subtitle, "translation">[],
  progress: number | null
): PrimaryActionState {
  if (!item) {
    return { action: "none", label: "转写并翻译", disabled: true, progress: null };
  }
  if (item.subtitle_status === "transcribing" || item.subtitle_status === "translating") {
    return { action: "cancel", label: "取消转写", disabled: false, progress };
  }
  if (item.subtitle_status === "error") {
    return { action: "retranscribe", label: "重新转写", disabled: false, progress: null };
  }
  if (item.subtitle_status === "partial") {
    return { action: "resume", label: "继续转写", disabled: false, progress: null };
  }
  if (subtitles.length === 0) {
    return { action: "transcribe", label: "转写并翻译", disabled: false, progress: null };
  }
  if (subtitles.some((s) => !s.translation.trim())) {
    return { action: "translate", label: "翻译", disabled: false, progress: null };
  }
  return { action: "ready", label: "已就绪", disabled: false, progress: null };
}

export function sortRecentMedia(items: MediaItem[]): MediaItem[] {
  return items
    .filter((m) => m.last_played_at)
    .slice()
    .sort((a, b) => {
      const at = a.last_played_at ?? "";
      const bt = b.last_played_at ?? "";
      if (at === bt) return b.id - a.id;
      return at < bt ? 1 : -1;
    });
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `npm --prefix app run test -- src/lib/uiState.test.ts`
Expected: 9 个用例 PASS。

- [ ] **Step 5: 提交**

```bash
git add app/src/lib/uiState.ts app/src/lib/uiState.test.ts
git commit -m "feat(ui): 增加界面状态纯函数"
```

---

### Task 3: 重写设计令牌

**Files:**
- Modify: `app/src/styles/tokens.css`（整文件替换）

- [ ] **Step 1: 替换文件内容**

```css
/*
 * ASPlayer · Cinema First 设计令牌
 * 全部颜色经由 CSS 变量，禁止在组件中写死色值。
 * data-theme="dark" | "light" 挂载在 <html> 上。
 */

:root,
:root[data-theme="dark"] {
  color-scheme: dark;

  /* 背景阶梯 */
  --bg-0: #08090a;
  --bg-1: #0e1012;
  --bg-2: #16191c;
  --bg-glass: rgba(16, 19, 22, 0.72);

  /* 前景阶梯 */
  --fg-1: #f5f6f7;
  --fg-2: #a8b0b8;
  --fg-3: #6b7178;

  /* 强调色 */
  --accent: #dfe9f2;
  --accent-soft: #a9c8ff;
  --accent-dim: rgba(223, 233, 242, 0.14);
  --on-accent: #0a1118;

  /* 危险色 */
  --danger: #e5484d;
  --danger-fg: #ffffff;

  /* 边框与分割 */
  --line: rgba(255, 255, 255, 0.08);
  --panel-border: rgba(255, 255, 255, 0.09);
  --panel-bg: rgba(16, 19, 22, 0.72);

  /* 圆角与阴影 */
  --radius-card: 14px;
  --radius-pill: 999px;
  --shadow-card: 0 8px 24px rgba(0, 0, 0, 0.35);
  --shadow-panel: 0 24px 64px rgba(0, 0, 0, 0.45);

  /* 顶栏/底栏渐变 */
  --topbar-scrim: linear-gradient(180deg, rgba(0, 0, 0, 0.55), transparent);
  --bottombar-scrim: linear-gradient(0deg, rgba(0, 0, 0, 0.62), transparent);

  /* 布局尺寸 */
  --topbar-h: 52px;
  --rail-w: 44px;
  --rail-w-open: 260px;
  --drawer-w: 320px;
  --gutter: 16px;
  --transport-h: 88px;
}

:root[data-theme="light"] {
  color-scheme: light;

  --bg-0: #eef0f3;
  --bg-1: #f7f8fa;
  --bg-2: #ffffff;
  --bg-glass: rgba(255, 255, 255, 0.82);

  --fg-1: #17191c;
  --fg-2: #5c6268;
  --fg-3: #9299a0;

  --accent: #1c2024;
  --accent-soft: #3b6ea5;
  --accent-dim: rgba(28, 32, 36, 0.1);
  --on-accent: #ffffff;

  --danger: #d13438;
  --danger-fg: #ffffff;

  --line: rgba(0, 0, 0, 0.08);
  --panel-border: rgba(0, 0, 0, 0.08);
  --panel-bg: rgba(255, 255, 255, 0.82);

  --shadow-card: 0 8px 24px rgba(0, 0, 0, 0.08);
  --shadow-panel: 0 18px 48px rgba(20, 24, 30, 0.16);

  --topbar-scrim: linear-gradient(180deg, rgba(255, 255, 255, 0.72), transparent);
  --bottombar-scrim: linear-gradient(0deg, rgba(255, 255, 255, 0.78), transparent);
}

/* ---- 全局基础 ---- */

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html,
body {
  height: 100%;
  background: var(--bg-0);
  color: var(--fg-1);
  font-family:
    "Segoe UI Variable Text", "Segoe UI", "HarmonyOS Sans SC", "MiSans",
    "Microsoft YaHei", system-ui, sans-serif;
  font-size: 13px;
  line-height: 1.5;
  -webkit-font-smoothing: antialiased;
  user-select: none;
  overflow: hidden;
}

button {
  font-family: inherit;
  color: inherit;
  background: var(--bg-2);
  border: 1px solid var(--line);
  border-radius: var(--radius-pill);
  padding: 6px 16px;
  cursor: pointer;
  transition: background 150ms ease, color 150ms ease, transform 150ms ease;
}

button:hover {
  filter: brightness(1.08);
}

button:active {
  transform: scale(0.98);
}

input {
  font-family: inherit;
  color: var(--fg-1);
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 6px 12px;
  outline: none;
}

input:focus-visible,
button:focus-visible {
  outline: 2px solid var(--accent-soft);
  outline-offset: 2px;
}

@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    transition-duration: 0.01ms !important;
    animation-duration: 0.01ms !important;
  }
}
```

- [ ] **Step 2: 构建检查**

Run: `npm --prefix app run build`
Expected: 构建成功。此时视觉会暂时变差（旧组件用旧令牌），属正常现象。

- [ ] **Step 3: 提交**

```bash
git add app/src/styles/tokens.css
git commit -m "feat(ui): 更新 Cinema First 设计令牌"
```

---

### Task 4: 顶栏浮层

**Files:**
- Modify: `app/src/components/TitleBar.vue`（整文件替换）

- [ ] **Step 1: 替换组件**

```vue
<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

const props = defineProps<{
  title?: string;
  meta?: string;
  ctaLabel?: string;
  ctaDisabled?: boolean;
  ctaProgress?: number | null;
}>();

const emit = defineEmits<{ menu: []; primary: []; more: [] }>();

const appWindow = getCurrentWindow();
const maximized = ref(false);

let unlistenResize: (() => void) | null = null;

async function minimize() {
  await appWindow.minimize();
}

async function toggleMaximize() {
  await appWindow.toggleMaximize();
}

async function close() {
  await appWindow.close();
}

onMounted(async () => {
  maximized.value = await appWindow.isMaximized();
  unlistenResize = await appWindow.onResized(async () => {
    maximized.value = await appWindow.isMaximized();
  });
});

onUnmounted(() => {
  unlistenResize?.();
});
</script>

<template>
  <header class="titlebar">
    <button class="tb-icon" title="素材轨" @click="emit('menu')">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
        <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />
      </svg>
    </button>

    <div class="tb-drag" data-tauri-drag-region @dblclick="toggleMaximize">
      <div class="tb-doc">
        <h1>{{ props.title || "ASPlayer" }}</h1>
        <p v-if="props.meta">{{ props.meta }}</p>
      </div>
    </div>

    <div class="tb-right">
      <button
        v-if="props.ctaLabel"
        class="cta"
        :disabled="props.ctaDisabled"
        @click="emit('primary')"
      >
        <span
          v-if="props.ctaProgress != null"
          class="cta-ring"
          :style="{ '--p': props.ctaProgress + '%' }"
        />
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 3v10" /><path d="m8 9 4 4 4-4" /><path d="M4 17v2h16v-2" />
        </svg>
        {{ props.ctaLabel }}
      </button>

      <button class="tb-icon" title="更多" @click="emit('more')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
          <circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" />
        </svg>
      </button>

      <div class="tb-btns">
        <button class="tb-btn" title="最小化" @click="minimize">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M5 12h14" /></svg>
        </button>
        <button class="tb-btn" :title="maximized ? '还原' : '最大化'" @click="toggleMaximize">
          <svg v-if="!maximized" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4">
            <rect x="6" y="6" width="12" height="12" rx="1.5" />
          </svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4">
            <rect x="5" y="9" width="10" height="10" rx="1.5" />
            <path d="M9 9V6.5A1.5 1.5 0 0 1 10.5 5H18a1.5 1.5 0 0 1 1.5 1.5V14a1.5 1.5 0 0 1-1.5 1.5H15" />
          </svg>
        </button>
        <button class="tb-btn tb-close" title="关闭" @click="close">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </div>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: var(--topbar-h);
  z-index: 30;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px 0 12px;
  background: var(--topbar-scrim);
  user-select: none;
}

.tb-icon {
  width: 34px;
  height: 34px;
  flex: none;
  display: grid;
  place-items: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 9px;
  background: transparent;
  color: var(--fg-2);
}

.tb-icon:hover {
  background: var(--panel-bg);
  border-color: var(--panel-border);
  color: var(--fg-1);
}

.tb-icon svg {
  width: 17px;
  height: 17px;
}

.tb-drag {
  flex: 1;
  min-width: 0;
  height: 100%;
  display: flex;
  align-items: center;
  padding: 0 8px;
}

.tb-doc {
  min-width: 0;
  pointer-events: none;
}

.tb-doc h1 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: -0.01em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tb-doc p {
  margin: 1px 0 0;
  font-size: 11.5px;
  color: var(--fg-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tb-right {
  display: flex;
  align-items: center;
  gap: 6px;
}

.cta {
  height: 32px;
  padding: 0 14px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: none;
  border-radius: var(--radius-pill);
  background: var(--accent);
  color: var(--on-accent);
  font-size: 12.5px;
  font-weight: 600;
  white-space: nowrap;
}

.cta:disabled {
  opacity: 0.35;
  cursor: default;
}

.cta svg {
  width: 15px;
  height: 15px;
}

.cta-ring {
  width: 15px;
  height: 15px;
  border-radius: 50%;
  background: conic-gradient(var(--on-accent) var(--p), rgba(0, 0, 0, 0.18) 0);
  mask: radial-gradient(circle, transparent 52%, #000 54%);
  -webkit-mask: radial-gradient(circle, transparent 52%, #000 54%);
}

.tb-btns {
  display: flex;
  align-items: stretch;
  margin-left: 4px;
  border-left: 1px solid var(--line);
  padding-left: 4px;
}

.tb-btn {
  width: 40px;
  height: 34px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  background: transparent;
  border: none;
  border-radius: 8px;
  color: var(--fg-2);
  transition: background 150ms ease, color 150ms ease;
}

.tb-btn:hover {
  background: var(--panel-bg);
  color: var(--fg-1);
  filter: none;
}

.tb-btn:active {
  transform: none;
}

.tb-btn svg {
  width: 15px;
  height: 15px;
}

.tb-close:hover {
  background: var(--danger);
  color: var(--danger-fg);
}
</style>
```

- [ ] **Step 2: 构建检查**

Run: `npm --prefix app run build`
Expected: 成功。`App.vue` 还没传新 props，但组件 props 全部可选，不会报错。

- [ ] **Step 3: 提交**

```bash
git add app/src/components/TitleBar.vue
git commit -m "feat(ui): 顶栏改为浮层"
```

---

### Task 5: 字幕面板改为抽屉

**Files:**
- Modify: `app/src/components/SubtitlePanel.vue`

- [ ] **Step 1: 增加脚本状态**

在 `const query = ref("");` 后加入：

```ts
const searchOpen = ref(false);
```

在 `isActive` 函数后加入：

```ts
const activeSub = computed(() => props.subtitles.find(isActive) ?? null);

function seekNext() {
  const list = props.subtitles;
  const i = activeSub.value ? list.indexOf(activeSub.value) : -1;
  const next = list[i + 1];
  if (next) emit("seek", next.start_ms / 1000);
}
```

- [ ] **Step 2: 改头部**

把现有 `<div class="sp-head">...</div>` 替换为：

```html
    <div class="sp-head">
      <span class="sp-title">字幕</span>
      <div class="sp-mode">
        <button class="sp-modeseg" :class="{ active: mode === 'original' }" @click="cap.captionStyle.mode = 'original'">原文</button>
        <button class="sp-modeseg" :class="{ active: mode === 'bilingual' }" @click="cap.captionStyle.mode = 'bilingual'">双语</button>
        <button class="sp-modeseg" :class="{ active: mode === 'translation' }" @click="cap.captionStyle.mode = 'translation'">译文</button>
      </div>
      <div class="sp-actions">
        <button class="sp-icon" title="搜索字幕" :class="{ active: searchOpen }" @click="searchOpen = !searchOpen">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
            <circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" />
          </svg>
        </button>
        <button class="sp-icon" title="收起字幕" @click="emit('close')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="m9 6 6 6-6 6" />
          </svg>
        </button>
      </div>
    </div>
```

- [ ] **Step 3: 插入当前句卡片，搜索改为折叠**

在进度横幅之后、错误块之前插入：

```html
    <div v-if="activeSub && status !== 'error'" class="sp-now">
      <div class="sp-now-en">{{ activeSub.text }}</div>
      <div v-if="activeSub.translation" class="sp-now-zh">{{ activeSub.translation }}</div>
      <div class="sp-now-actions">
        <button @click="emit('seek', activeSub.start_ms / 1000)">↺ 重听</button>
        <button @click="seekNext">下一句</button>
      </div>
    </div>
```

把搜索块改为条件渲染：

```html
      <div v-if="searchOpen || hasQuery" class="sp-search">
```

- [ ] **Step 4: 替换抽屉样式**

把 `.subpanel`、`.sp-head`、`.sp-title`、`.sp-mode`、`.sp-modeseg`、`.sp-actions`、`.sp-close` 这几段样式替换为：

```css
.subpanel {
  width: var(--drawer-w);
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  user-select: none;
  border-radius: var(--radius-card);
  border: 1px solid var(--panel-border);
  background: var(--panel-bg);
  backdrop-filter: blur(24px);
  box-shadow: var(--shadow-panel);
}

.sp-head {
  height: 48px;
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 0 14px;
  border-bottom: 1px solid var(--line);
}

.sp-title {
  font-size: 13px;
  font-weight: 650;
  margin-right: auto;
}

.sp-mode {
  display: flex;
  gap: 1px;
  padding: 2px;
  border-radius: var(--radius-pill);
  background: var(--accent-dim);
}

.sp-modeseg {
  padding: 3px 9px;
  border: none;
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--fg-3);
  font-size: 11px;
}

.sp-modeseg.active {
  background: var(--bg-2);
  color: var(--fg-1);
}

.sp-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.sp-icon {
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-2);
}

.sp-icon:hover,
.sp-icon.active {
  background: var(--bg-2);
  color: var(--fg-1);
}

.sp-icon svg {
  width: 14px;
  height: 14px;
}

.sp-now {
  flex: none;
  margin: 10px 10px 4px;
  padding: 12px 14px;
  border-radius: 10px;
  background: var(--accent-dim);
  border-left: 2px solid var(--accent-soft);
}

.sp-now-en {
  font-size: 14.5px;
  font-weight: 600;
  line-height: 1.4;
}

.sp-now-zh {
  margin-top: 4px;
  font-size: 13px;
  line-height: 1.45;
  color: var(--fg-2);
}

.sp-now-actions {
  display: flex;
  gap: 6px;
  margin-top: 9px;
}

.sp-now-actions button {
  padding: 3px 10px;
  font-size: 11px;
  border: 1px solid var(--panel-border);
  background: transparent;
  color: var(--fg-2);
}

.sp-now-actions button:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}
```

同时在 `.sp-line.active` 中把背景改为 `var(--accent-dim)`、左侧边框改为 `var(--accent-soft)`（若原样式使用别的变量）。

- [ ] **Step 5: 构建检查**

Run: `npm --prefix app run build`
Expected: 成功。

- [ ] **Step 6: 提交**

```bash
git add app/src/components/SubtitlePanel.vue
git commit -m "feat(ui): 字幕面板改为抽屉"
```

---

### Task 6: 素材轨与最近播放

**Files:**
- Modify: `app/src/components/PlaylistPanel.vue`
- Create: `app/src/components/MediaRail.vue`

- [ ] **Step 1: PlaylistPanel 增加内嵌模式**

脚本顶部导入改为：

```ts
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { MediaItem } from "../types";
import type { ProfileOverride } from "../lib/intensive";
import { sortRecentMedia } from "../lib/uiState";
```

(props 定义保持不变，加入两个可选字段：)

```ts
const props = defineProps<{
  items: MediaItem[];
  currentId: number | null;
  loading: boolean;
  embedded?: boolean;
  recentOnly?: boolean;
}>();
```

`sortedItems` 替换为：

```ts
const sortedItems = computed(() => {
  const q = search.value.trim().toLowerCase();
  let list = props.recentOnly ? sortRecentMedia(props.items) : props.items;
  if (q) list = list.filter((m) => m.title.toLowerCase().includes(q));
  const arr = [...list];
  if (props.recentOnly) return arr;
  switch (sortBy.value) {
    case "title":
      arr.sort((a, b) => a.title.localeCompare(b.title, "zh-CN"));
      break;
    case "duration":
      arr.sort((a, b) => b.duration_ms - a.duration_ms);
      break;
    case "subtitle":
      arr.sort((a, b) => b.subtitle_count - a.subtitle_count);
      break;
    default:
      arr.sort((a, b) => a.id - b.id);
  }
  return arr;
});
```

- [ ] **Step 2: 模板适配**

1. 根节点改为：

```html
  <aside class="playlist" :class="{ embedded: props.embedded }">
```

2. 面板头改为仅非内嵌时显示：

```html
    <div v-if="!props.embedded" class="pl-head">
```

3. 搜索/排序行在内嵌模式下保留，但隐藏关闭按钮（该按钮只存在于头部，已随头部隐藏）。

4. 空状态补充最近播放文案：

```html
      <p v-if="props.recentOnly">还没有播放记录</p>
      <p v-else>媒体库还是空的</p>
```

（保留原有导入按钮与说明。）

- [ ] **Step 3: 列表项加进度细线**

在列表项根节点内、`.pl-sub` 附近加入：

```html
        <span
          v-if="item.duration_ms > 0 && item.playback_position > 0"
          class="pl-progress"
          :style="{ width: Math.min(100, (item.playback_position / item.duration_ms) * 100) + '%' }"
        />
```

该项根节点需要 `position: relative`，并加入样式：

```css
.pl-progress {
  position: absolute;
  left: 0;
  bottom: 0;
  height: 2px;
  border-radius: 999px;
  background: var(--accent-soft);
  opacity: 0.7;
}
```

- [ ] **Step 4: 内嵌样式**

在样式末尾加入：

```css
.playlist.embedded {
  width: 100%;
  min-height: 0;
  flex: 1;
  background: transparent;
  border: none;
  box-shadow: none;
  backdrop-filter: none;
}

.playlist.embedded .pl-search-row {
  padding: 0 10px 8px;
}
```

- [ ] **Step 5: 创建 MediaRail**

新建 `app/src/components/MediaRail.vue`：

```vue
<script setup lang="ts">
import { computed, ref } from "vue";
import type { MediaItem } from "../types";
import type { ProfileOverride } from "../lib/intensive";
import PlaylistPanel from "./PlaylistPanel.vue";

const props = defineProps<{
  items: MediaItem[];
  currentId: number | null;
  loading: boolean;
  pinned: boolean;
}>();

const emit = defineEmits<{
  play: [item: MediaItem];
  import: [];
  importFolder: [];
  refresh: [];
  importSubtitle: [item: MediaItem];
  setProfile: [item: MediaItem, value: ProfileOverride];
  settings: [];
  pinToggle: [];
}>();

const tab = ref<"recent" | "all">("recent");
const hover = ref(false);
let enterTimer: number | null = null;
let leaveTimer: number | null = null;

const open = computed(() => props.pinned || hover.value);

const covers = computed(() => props.items.slice(0, 4));

function onEnter() {
  if (leaveTimer !== null) {
    window.clearTimeout(leaveTimer);
    leaveTimer = null;
  }
  enterTimer = window.setTimeout(() => {
    hover.value = true;
    enterTimer = null;
  }, 180);
}

function onLeave() {
  if (enterTimer !== null) {
    window.clearTimeout(enterTimer);
    enterTimer = null;
  }
  leaveTimer = window.setTimeout(() => {
    hover.value = false;
    leaveTimer = null;
  }, 180);
}
</script>

<template>
  <aside class="rail" :class="{ open }" @mouseenter="onEnter" @mouseleave="onLeave">
    <div class="rail-collapsed">
      <div class="rail-mark" aria-hidden="true"></div>
      <button
        v-for="m in covers"
        :key="m.id"
        class="cp"
        :class="{ on: m.id === currentId }"
        :title="m.title"
        @click="emit('play', m)"
      ></button>
      <div class="gap"></div>
      <button class="rail-icon" title="设置" @click="emit('settings')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
          <circle cx="12" cy="12" r="3" />
          <path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M19.1 4.9 17 7M7 17l-2.1 2.1" />
        </svg>
      </button>
    </div>

    <div class="rail-expanded">
      <div class="rx-head">
        <b>素材</b>
        <div class="rx-tools">
          <button class="rail-icon" title="导入文件" @click="emit('import')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
              <path d="M12 5v14M5 12h14" />
            </svg>
          </button>
          <button class="rail-icon" title="导入文件夹" @click="emit('importFolder')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
            </svg>
          </button>
        </div>
      </div>

      <div class="rx-tabs">
        <button :class="{ on: tab === 'recent' }" @click="tab = 'recent'">最近播放</button>
        <button :class="{ on: tab === 'all' }" @click="tab = 'all'">全部素材</button>
      </div>

      <PlaylistPanel
        embedded
        :recent-only="tab === 'recent'"
        :items="items"
        :current-id="currentId"
        :loading="loading"
        @play="(m) => emit('play', m)"
        @import="emit('import')"
        @import-folder="emit('importFolder')"
        @refresh="emit('refresh')"
        @import-subtitle="(m) => emit('importSubtitle', m)"
        @set-profile="(m, v) => emit('setProfile', m, v)"
      />
    </div>
  </aside>
</template>

<style scoped>
.rail {
  position: absolute;
  left: var(--gutter);
  top: 64px;
  bottom: 96px;
  width: var(--rail-w);
  z-index: 22;
  overflow: hidden;
  border-radius: var(--radius-card);
  border: 1px solid var(--panel-border);
  background: var(--panel-bg);
  backdrop-filter: blur(20px);
  box-shadow: var(--shadow-card);
  transition: width 300ms cubic-bezier(0.22, 0.61, 0.36, 1), background 300ms ease;
}

.rail.open {
  width: var(--rail-w-open);
}

.rail-collapsed {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 12px 0;
  transition: opacity 140ms ease;
}

.rail.open .rail-collapsed {
  opacity: 0;
  pointer-events: none;
}

.rail-mark {
  width: 22px;
  height: 22px;
  border-radius: 6px;
  background: linear-gradient(150deg, #6f9dc6, #12212e);
}

.cp {
  width: 28px;
  height: 28px;
  flex: none;
  padding: 0;
  border: none;
  border-radius: 6px;
  background: linear-gradient(150deg, #35506a, #0d151c 72%);
  opacity: 0.55;
}

.cp:nth-of-type(2) { background: linear-gradient(150deg, #5b4730, #191108 72%); }
.cp:nth-of-type(3) { background: linear-gradient(150deg, #40365c, #120e1c 72%); }
.cp:nth-of-type(4) { background: linear-gradient(150deg, #2e5145, #0b1712 72%); }

.cp.on {
  opacity: 1;
  box-shadow: 0 0 0 2px var(--accent-soft);
}

.gap {
  flex: 1;
}

.rail-icon {
  width: 28px;
  height: 28px;
  flex: none;
  display: grid;
  place-items: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-2);
}

.rail-icon:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.rail-icon svg {
  width: 15px;
  height: 15px;
}

.rail-expanded {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  padding: 12px 10px 10px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 180ms ease 40ms;
}

.rail.open .rail-expanded {
  opacity: 1;
  pointer-events: auto;
}

.rx-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 2px;
  margin-bottom: 8px;
}

.rx-head b {
  font-size: 12.5px;
  font-weight: 650;
}

.rx-tools {
  display: flex;
  gap: 2px;
}

.rx-tabs {
  display: flex;
  gap: 2px;
  padding: 2px;
  margin-bottom: 8px;
  border-radius: 8px;
  background: var(--accent-dim);
}

.rx-tabs button {
  flex: 1;
  padding: 4px 0;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fg-3);
  font-size: 11px;
}

.rx-tabs button.on {
  background: var(--bg-2);
  color: var(--fg-1);
}
</style>
```

- [ ] **Step 6: 构建检查**

Run: `npm --prefix app run build`
Expected: 成功。`MediaRail` 尚未被使用，但必须通过类型检查。

- [ ] **Step 7: 提交**

```bash
git add app/src/components/PlaylistPanel.vue app/src/components/MediaRail.vue
git commit -m "feat(ui): 素材轨支持最近播放与全部素材"
```

---

### Task 7: 播放器安全区与底栏胶囊

**Files:**
- Modify: `app/src/components/PlayerStage.vue`

- [ ] **Step 1: props / emits / 暴露**

`defineProps` 改为：

```ts
const props = defineProps<{
  item: MediaItem | null;
  items: MediaItem[];
  overlayOn?: boolean;
  drawerOpen?: boolean;
  chromeVisible?: boolean;
}>();
```

`defineEmits` 增加两个事件：

```ts
  playingChange: [playing: boolean];
  importFolder: [];
```

在 `watch(playing, ...)` 附近加入：

```ts
watch(playing, (v) => emit("playingChange", v));
```

`defineExpose` 改为：

```ts
defineExpose({
  togglePlay, seekBy, seekToSeconds, next, prev, toggleMute, adjustVolume,
  toggleFullscreen, repeatSubtitle, clearIntensiveState,
  doTranscribe, doTranslate, doImportSubtitle, doCancelTranscribe,
});
```

- [ ] **Step 2: 删除顶部工具栏**

删除模板中整个 `<div class="stage-topbar" ...> ... </div>` 块（原 602–614 行）。

- [ ] **Step 3: 根节点与空状态**

根节点 class 改为：

```html
  <main
    class="stage"
    :class="{ 'fs-hide-cursor': isFullscreen && !controlsVisible, 'has-drawer': drawerOpen }"
    @mousemove="onStageMouseMove"
  >
```

空状态按钮改为两个：

```html
        <div class="empty-actions">
          <button class="open-btn" @click="emit('import')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 7h4l2-2h4l2 2h4v11H4z" /></svg>
            导入文件
          </button>
          <button class="open-btn ghost" @click="emit('importFolder')">导入文件夹</button>
        </div>
```

- [ ] **Step 4: 控制条显示条件**

把：

```html
        v-show="!isFullscreen || controlsVisible"
```

改为：

```html
        v-show="props.chromeVisible !== false && (!isFullscreen || controlsVisible)"
```

- [ ] **Step 5: 替换布局与控制条样式**

把 `.stage`、`.stage-topbar`、`.toolbar`、`.iconbtn*`、`.canvas`、`.controls`、`.controls.fs-overlay` 这些样式替换为：

```css
.stage {
  position: absolute;
  inset: 0;
  overflow: hidden;
  background: var(--stage-bg, #000);
}

.stage::before {
  content: "";
  position: absolute;
  inset: 0;
  background:
    radial-gradient(60% 55% at 62% 34%, rgba(74, 96, 112, 0.28), transparent 70%),
    radial-gradient(40% 40% at 18% 80%, rgba(40, 58, 62, 0.22), transparent 72%),
    var(--bg-0);
  opacity: 0;
  transition: opacity 320ms ease;
  pointer-events: none;
}

.stage.has-drawer::before {
  opacity: 1;
}

.canvas {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  transition:
    top 320ms cubic-bezier(0.22, 0.61, 0.36, 1),
    left 320ms cubic-bezier(0.22, 0.61, 0.36, 1),
    right 320ms cubic-bezier(0.22, 0.61, 0.36, 1),
    bottom 320ms cubic-bezier(0.22, 0.61, 0.36, 1),
    border-radius 320ms ease;
}

.stage.has-drawer .canvas {
  top: 16px;
  left: 16px;
  right: 336px;
  bottom: 88px;
  border-radius: var(--radius-card);
  overflow: hidden;
  box-shadow: 0 22px 60px rgba(0, 0, 0, 0.55);
}

.playing video {
  width: 100%;
  height: 100%;
  background: #000;
  outline: none;
  cursor: pointer;
}

.controls {
  position: absolute;
  left: 16px;
  right: 16px;
  bottom: 12px;
  z-index: 24;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: transparent;
  border: none;
  transition:
    right 320ms cubic-bezier(0.22, 0.61, 0.36, 1),
    opacity 320ms ease,
    transform 320ms ease;
}

.stage.has-drawer .controls {
  right: 336px;
}

.stage.chrome-hidden .controls {
  opacity: 0;
  transform: translateY(12px);
  pointer-events: none;
}

.seek-row {
  padding: 0 8px;
}

.btn-row {
  height: 52px;
  padding: 0 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border-radius: var(--radius-pill);
  border: 1px solid var(--panel-border);
  background: var(--panel-bg);
  backdrop-filter: blur(24px);
  box-shadow: var(--shadow-panel);
}

.btn-row .flex-spacer {
  display: none;
}

.btn-group {
  display: flex;
  align-items: center;
  gap: 2px;
}

.btn-group + .btn-group {
  padding-left: 10px;
  border-left: 1px solid var(--line);
}

.ctl {
  width: 32px;
  height: 32px;
  flex: 0 0 32px;
  border-radius: 50%;
}

.ctl.play {
  width: 42px;
  height: 42px;
  flex-basis: 42px;
  margin: 0 4px;
  background: var(--accent);
  color: var(--on-accent);
}

.ctl.play:hover {
  background: var(--accent);
  filter: brightness(1.08);
}

.seek-num {
  font-size: 9px;
}

.rate-btn,
.seek {
  width: auto;
  padding: 0 10px;
  border-radius: var(--radius-pill);
  border: 1px solid var(--panel-border);
  font-size: 11px;
}
```

并在媒体查询中补充：

```css
@media (max-width: 900px) {
  .stage.has-drawer .canvas {
    right: 296px;
  }
  .stage.has-drawer .controls {
    right: 296px;
  }
}
```

- [ ] **Step 6: 添加 `chrome-hidden` 类**

根节点 class 已包含 `has-drawer`，再加入：

```html
    :class="{ 'fs-hide-cursor': isFullscreen && !controlsVisible, 'has-drawer': drawerOpen, 'chrome-hidden': props.chromeVisible === false }"
```

- [ ] **Step 7: 构建检查**

Run: `npm --prefix app run build`
Expected: 成功。

- [ ] **Step 8: 提交**

```bash
git add app/src/components/PlayerStage.vue
git commit -m "feat(ui): 播放器支持字幕安全区与浮层控制"
```

---

### Task 8: App 单层舞台编排与自动隐藏

**Files:**
- Modify: `app/src/App.vue`

- [ ] **Step 1: 调整导入**

把 `import PlaylistPanel from "./components/PlaylistPanel.vue";` 替换为：

```ts
import MediaRail from "./components/MediaRail.vue";
```

并加入：

```ts
import { resolvePrimaryAction } from "./lib/uiState";
```

- [ ] **Step 2: 新增状态与计算属性**

在 `const showPlaylist = ref(true);` 一带替换为：

```ts
const railPinned = ref(false);
const drawerOpen = ref(false);
const chromeVisible = ref(true);
const playing = ref(false);
const topMenuOpen = ref(false);
const winWidth = ref(window.innerWidth);

const DRAWER_KEY = "asplayer-subtitle-drawer-v1";

function defaultDrawerOpen() {
  return pb.playback.playbackMode === "intensive";
}

(function initDrawer() {
  try {
    const saved = localStorage.getItem(DRAWER_KEY);
    drawerOpen.value = saved ? saved === "open" : defaultDrawerOpen();
  } catch {
    drawerOpen.value = defaultDrawerOpen();
  }
})();

const safeDrawer = computed(() => drawerOpen.value && winWidth.value >= 720);

const primary = computed(() =>
  resolvePrimaryAction(
    current.value,
    sub.subtitles.value,
    sub.status.value === "transcribing" ? sub.progress.value : null
  )
);

const meta = computed(() => {
  const it = current.value;
  if (!it) return "";
  const parts = [it.media_type === "video" ? "视频" : "音频"];
  if (it.duration_ms > 0) {
    const total = Math.floor(it.duration_ms / 1000);
    const m = Math.floor(total / 60);
    const s = total % 60;
    parts.push(`${m}:${String(s).padStart(2, "0")}`);
  }
  const mode = it.profile_override ?? pb.playback.playbackMode;
  parts.push(mode === "intensive" ? "精听" : "连播");
  if (it.subtitle_count > 0) parts.push(`${it.subtitle_count} 段字幕`);
  return parts.join(" · ");
});

let idleTimer: number | null = null;

function canAutoHide() {
  return (
    pb.playback.chromeAutoHide &&
    playing.value &&
    !settingsOpen.value &&
    !dictOpen.value &&
    !topMenuOpen.value
  );
}

function wakeChrome() {
  chromeVisible.value = true;
  if (idleTimer !== null) window.clearTimeout(idleTimer);
  idleTimer = window.setTimeout(() => {
    if (canAutoHide()) chromeVisible.value = false;
  }, 2500);
}

function onWindowResize() {
  winWidth.value = window.innerWidth;
}
```

需要把 `import { onMounted, onUnmounted, ref, watch } from "vue";` 改为包含 `computed`。

- [ ] **Step 3: 改造开关函数**

替换 `togglePlaylist` 与 `toggleSubtitle`：

```ts
function togglePlaylist() {
  railPinned.value = !railPinned.value;
  wakeChrome();
}

function toggleSubtitle() {
  drawerOpen.value = !drawerOpen.value;
  try {
    localStorage.setItem(DRAWER_KEY, drawerOpen.value ? "open" : "closed");
  } catch {}
  wakeChrome();
}
```

- [ ] **Step 4: 主按钮与菜单动作**

加入：

```ts
function onPrimaryAction() {
  const a = primary.value.action;
  if (a === "cancel") stageRef.value?.doCancelTranscribe();
  else if (a === "translate") stageRef.value?.doTranslate(false);
  else if (a === "resume") stageRef.value?.doTranscribe(true);
  else if (a === "transcribe" || a === "retranscribe") stageRef.value?.doTranscribe(false);
  else if (a === "ready") toggleSubtitle();
  topMenuOpen.value = false;
}

function onPlayingChange(v: boolean) {
  playing.value = v;
  if (v) wakeChrome();
  else {
    chromeVisible.value = true;
    if (idleTimer !== null) window.clearTimeout(idleTimer);
  }
}
```

- [ ] **Step 5: 生命周期接线**

`onMounted` 中追加：

```ts
  window.addEventListener("resize", onWindowResize);
  window.addEventListener("mousemove", wakeChrome);
  window.addEventListener("keydown", wakeChrome);
  wakeChrome();
```

`onUnmounted` 中追加：

```ts
  window.removeEventListener("resize", onWindowResize);
  window.removeEventListener("mousemove", wakeChrome);
  window.removeEventListener("keydown", wakeChrome);
  if (idleTimer !== null) window.clearTimeout(idleTimer);
```

- [ ] **Step 6: 抽屉模式联动**

加入 watch：

```ts
watch(
  () => pb.playback.playbackMode,
  () => {
    try {
      if (!localStorage.getItem(DRAWER_KEY)) drawerOpen.value = defaultDrawerOpen();
    } catch {}
  }
);

watch(
  () => pb.playback.chromeAutoHide,
  (on) => {
    if (!on) {
      chromeVisible.value = true;
      if (idleTimer !== null) window.clearTimeout(idleTimer);
    } else {
      wakeChrome();
    }
  }
);
```

- [ ] **Step 7: 模板整体替换**

把 `<template>...</template>` 替换为：

```html
<template>
  <div class="app-shell" @mousemove="wakeChrome">
    <PlayerStage
      ref="stageRef"
      :item="current"
      :items="items"
      :overlay-on="overlayVisible"
      :drawer-open="safeDrawer"
      :chrome-visible="chromeVisible"
      @import="importFiles"
      @import-folder="importFolder"
      @play="play"
      @settings="settingsOpen = true"
      @toggle-playlist="togglePlaylist"
      @toggle-subtitle="toggleSubtitle"
      @fullscreen-change="onFullscreenChange"
      @overlay-toggle="onOverlayToggle"
      @import-subtitle="onImportSubtitle"
      @playing-change="onPlayingChange"
    />

    <TitleBar
      class="chrome-top"
      :class="{ 'is-hidden': !chromeVisible }"
      :title="current?.title"
      :meta="meta"
      :cta-label="primary.label"
      :cta-disabled="primary.disabled"
      :cta-progress="primary.progress"
      @menu="togglePlaylist"
      @primary="onPrimaryAction"
      @more="topMenuOpen = !topMenuOpen"
    />

    <MediaRail
      class="chrome-rail"
      :class="{ 'is-hidden': !chromeVisible }"
      :items="items"
      :current-id="current?.id ?? null"
      :loading="loading"
      :pinned="railPinned"
      @play="play"
      @import="importFiles"
      @import-folder="importFolder"
      @refresh="refresh"
      @import-subtitle="(item) => onImportSubtitle(item.id)"
      @set-profile="onSetProfile"
      @settings="settingsOpen = true"
      @pin-toggle="togglePlaylist"
    />

    <SubtitlePanel
      v-if="drawerOpen"
      class="drawer"
      :subtitles="sub.subtitles.value"
      :current-time="sub.currentTime.value"
      :status="sub.status.value"
      :stage="sub.stage.value"
      :progress="sub.progress.value"
      :message="sub.message.value"
      @close="toggleSubtitle"
      @seek="seekTo"
      @cancel="onCancelTranscribe"
      @lookup="onDictLookup"
    />

    <button v-else class="drawer-handle" title="展开字幕" @click="toggleSubtitle">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <path d="m9 6 6 6-6 6" />
      </svg>
    </button>

    <div v-if="topMenuOpen" class="top-menu" @click.stop>
      <button :disabled="!current" @click="onImportSubtitle(current!.id); topMenuOpen = false">导入字幕</button>
      <button :disabled="!current" @click="stageRef?.doTranslate(true); topMenuOpen = false">重新翻译</button>
      <button :disabled="!current" @click="onOverlayToggle(); topMenuOpen = false">悬浮字幕窗</button>
      <button @click="togglePlaybackMode(); topMenuOpen = false">切换播放模式</button>
      <button @click="settingsOpen = true; topMenuOpen = false">设置</button>
    </div>

    <DictCard
      :open="dictOpen"
      :loading="dictLoading"
      :result="dictResult"
      :error="dictError"
      :downloading="dictDownloading"
      @close="dictOpen = false"
      @lookup="onDictLookup"
      @download="onDictDownload"
    />

    <SettingsPanel
      :open="settingsOpen"
      :theme="theme"
      @close="settingsOpen = false"
      @set-theme="setTheme"
    />
  </div>
</template>
```

- [ ] **Step 8: 替换布局样式**

```css
.app-shell {
  position: relative;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: var(--bg-0);
}

.chrome-top,
.chrome-rail {
  transition: opacity 320ms ease, transform 320ms ease;
}

.chrome-top.is-hidden {
  opacity: 0;
  transform: translateY(-10px);
  pointer-events: none;
}

.chrome-rail.is-hidden {
  opacity: 0;
  transform: translateX(-10px);
  pointer-events: none;
}

.drawer {
  position: absolute;
  top: 64px;
  right: var(--gutter);
  bottom: var(--gutter);
  z-index: 26;
}

.drawer-handle {
  position: absolute;
  right: var(--gutter);
  top: 50%;
  z-index: 26;
  width: 24px;
  height: 48px;
  transform: translate(50%, -50%);
  display: grid;
  place-items: center;
  padding: 0;
  border-radius: var(--radius-pill);
  border: 1px solid var(--panel-border);
  background: var(--panel-bg);
  backdrop-filter: blur(16px);
  color: var(--fg-2);
}

.drawer-handle:hover {
  color: var(--fg-1);
}

.drawer-handle svg {
  width: 14px;
  height: 14px;
}

.top-menu {
  position: absolute;
  top: 54px;
  right: 130px;
  z-index: 40;
  min-width: 160px;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  border-radius: 10px;
  border: 1px solid var(--panel-border);
  background: var(--panel-bg);
  backdrop-filter: blur(24px);
  box-shadow: var(--shadow-panel);
}

.top-menu button {
  padding: 8px 10px;
  text-align: left;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-1);
  font-size: 12.5px;
}

.top-menu button:hover {
  background: var(--bg-2);
}

.top-menu button:disabled {
  opacity: 0.35;
}

@media (max-width: 900px) {
  .drawer {
    width: 280px;
  }
}

@media (max-width: 720px) {
  .drawer {
    width: min(92vw, 340px);
  }
}
```

- [ ] **Step 9: 构建检查**

Run: `npm --prefix app run build`
Expected: 成功。若出现 `pb` 未定义，确认 `const pb = usePlayback()` 位于新增 computed 之前（原文件在 273 行，需要上移到 `current` 定义附近）。

- [ ] **Step 10: 提交**

```bash
git add app/src/App.vue
git commit -m "feat(ui): 单层舞台与控件自动隐藏"
```

---

### Task 9: 自动隐藏设置开关

**Files:**
- Modify: `app/src/stores/playback.ts`
- Modify: `app/src/components/SettingsPanel.vue:427-459`

- [ ] **Step 1: 存储字段**

`PlaybackSettings` 加入：

```ts
  chromeAutoHide: boolean; // 播放时自动隐藏顶栏与底部控件
```

`defaultPlayback` 加入：

```ts
  chromeAutoHide: true,
```

- [ ] **Step 2: 设置面板加行**

在外观页「主题」行之后加入：

```html
        <div class="row">
          <span class="row-label">播放时自动隐藏控件</span>
          <div class="segmented">
            <button
              class="segment"
              :class="{ active: pb.playback.chromeAutoHide }"
              @click="pb.playback.chromeAutoHide = true"
            >
              开
            </button>
            <button
              class="segment"
              :class="{ active: !pb.playback.chromeAutoHide }"
              @click="pb.playback.chromeAutoHide = false"
            >
              关
            </button>
          </div>
        </div>
```

- [ ] **Step 3: 构建检查**

Run: `npm --prefix app run build`
Expected: 成功。

- [ ] **Step 4: 提交**

```bash
git add app/src/stores/playback.ts app/src/components/SettingsPanel.vue
git commit -m "feat(ui): 增加自动隐藏开关"
```

---

### Task 10: 浅色主题、断点与回归

**Files:**
- Modify: `app/src/components/*.vue`（仅补齐浅色覆盖，不新增功能）

- [ ] **Step 1: 浅色覆盖检查**

在深色下硬编码颜色的位置补上浅色回退。重点：

1. `PlayerStage.vue` 的 `.stage::before` 在浅色下降低不透明度：

```css
:root[data-theme="light"] .stage::before {
  opacity: 0.35;
}
```

2. `PlayerStage.vue` 的 `.empty-badge`、`.artwork` 使用令牌后应已适配；若仍有 `#000`，改为 `var(--bg-1)`。

3. `MediaRail.vue` 的封面渐变保持不变（内容预览，不属于主题）。

4. `SubtitlePanel.vue` 的 `sp-now` 背景已用 `--accent-dim`，无需改。

- [ ] **Step 2: 断点验证**

手动缩放窗口，确认：

- > 900px：抽屉 320px，画面右边距 336px。
- 720–900px：抽屉 280px，画面右边距 296px。
- < 720px：抽屉覆盖，画面铺满（`safeDrawer` 为 false）。

- [ ] **Step 3: 自动测试**

Run:

```bash
cargo test --manifest-path app/src-tauri/Cargo.toml
npm --prefix app run test
npm --prefix app run build
```

Expected: Rust 与 Vitest 全部 PASS，构建成功。

- [ ] **Step 4: 手动验收**

按顺序执行：

1. `npm --prefix app run tauri dev` 启动。
2. 导入视频并播放 → 2.5 秒后顶栏、左轨、底栏淡出；移动鼠标恢复；暂停常驻。
3. 展开字幕抽屉 → 画面缩进左侧安全区，底部控件同步让位；收起后恢复铺满。
4. 精听模式默认展开抽屉；连播默认收起；手动切换后重启保持。
5. `Ctrl+L` 固定/取消素材轨；双标签切换；最近播放只显示播放过的素材。
6. 主按钮六种状态逐一触发：无字幕、断点、转写中、翻译中、错误、已就绪。
7. 空库、无字幕、转写失败显示正常。
8. 浅色主题下顶栏、抽屉、菜单、字幕可读。
9. 悬浮字幕窗、词典、设置、快捷键无回归。

- [ ] **Step 5: 提交**

```bash
git add -A app/src
git commit -m "chore(ui): 浅色主题与断点收尾"
```

---

## 自查

- **Spec 覆盖**：布局、安全区、自动隐藏、素材轨双标签、主按钮状态机、深浅令牌、`last_played_at`、断点、验收均有对应任务。
- **类型一致**：`last_played_at: string | null` 在 Rust 为 `Option<String>`、前端为 `string | null`；`chromeAutoHide` 在 store 与设置面板同名；`resolvePrimaryAction` 返回的 `action` 字符串与 `onPrimaryAction` 分支一致。
- **无占位符**：所有新增文件给出完整代码；修改文件给出替换块与插入位置。
- **已知取舍**：不拆 `TransportBar`，控制条留在 `PlayerStage` 内重构；素材轨拆为 `MediaRail` 包裹 `PlaylistPanel`，避免复制右键菜单逻辑。
