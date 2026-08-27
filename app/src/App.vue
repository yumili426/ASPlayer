<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import PlayerStage from "./components/PlayerStage.vue";
import PlaylistPanel from "./components/PlaylistPanel.vue";
import SubtitlePanel from "./components/SubtitlePanel.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import type { MediaItem } from "./types";
import { useSubtitle } from "./stores/subtitle";
import { useShortcuts } from "./stores/shortcuts";
import { listen } from "@tauri-apps/api/event";
import {
  isOverlayLocked,
  isOverlayVisible,
  toggleOverlayVisible,
} from "./api/overlay";
import { resetFeed as overlayResetFeed } from "./overlayFeed";
import {
  onTranscribeProgress,
  onTranscribeDone,
  onTranscribeError,
  onTranscribeCanceled,
  cancelTranscribe,
  translateMedia,
} from "./api/subtitle";

const sub = useSubtitle();

const items = ref<MediaItem[]>([]);
const current = ref<MediaItem | null>(null);
const loading = ref(false);
const settingsOpen = ref(false);
const theme = ref<"light" | "dark" | "system">("system");
const showPlaylist = ref(true);
const showSubtitle = ref(true);
const stageFullscreen = ref(false);
const unlisteners: (() => void)[] = [];
const stageRef = ref<any>(null);

const THEME_KEY = "asplayer-theme-v2";
const saved = (() => {
  try {
    return localStorage.getItem(THEME_KEY) as "light" | "dark" | "system" | null;
  } catch {
    return null;
  }
})();
const prefersDark = window.matchMedia("(prefers-color-scheme: dark)");

theme.value = saved ?? "system";

function applyTheme() {
  const resolved =
    theme.value === "system" ? (prefersDark.matches ? "dark" : "light") : theme.value;
  document.documentElement.dataset.theme = resolved;
}
applyTheme();

function setTheme(t: "light" | "dark" | "system") {
  theme.value = t;
  applyTheme();
  try {
    localStorage.setItem(THEME_KEY, t);
  } catch {}
}

function onSystemThemeChange() {
  if (theme.value === "system") applyTheme();
}
prefersDark.addEventListener("change", onSystemThemeChange);

async function refresh() {
  loading.value = true;
  try {
    items.value = await invoke<MediaItem[]>("list_media");
    probeDurations();
  } finally {
    loading.value = false;
  }
}

async function importFolder() {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const dir = await open({ directory: true });
  if (!dir) return;
  loading.value = true;
  try {
    items.value = await invoke<MediaItem[]>("import_folder", { path: dir });
    probeDurations();
    // eslint-disable-next-line no-console
    console.log("[ASPlayer] 导入完成:", dir, "→", items.value.length, "个文件");
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 导入失败:", e);
  } finally {
    loading.value = false;
  }
}

async function importFiles() {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: true,
    filters: [
      {
        name: "媒体文件",
        extensions: [
          "mp3", "m4a", "wav", "flac", "ogg", "oga", "opus", "aac", "m4b",
          "wma", "aiff", "aif", "ape", "mka", "mp2", "amr", "ac3",
          "mp4", "m4v", "webm", "mkv", "mov", "avi", "wmv", "flv", "ts",
        ],
      },
    ],
  });
  if (!selected || selected.length === 0) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  loading.value = true;
  try {
    items.value = await invoke<MediaItem[]>("import_files", { paths });
    probeDurations();
    // eslint-disable-next-line no-console
    console.log("[ASPlayer] 导入文件:", paths, "→", items.value.length, "个");
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 导入文件失败:", e);
  } finally {
    loading.value = false;
  }
}

function probeDurations() {
  // 导入时未探测时长（全部为 0）。用隐藏的 video/audio 元素读取元数据，回写 DB 并即时更新列表显示。
  for (const m of items.value) {
    if (m.duration_ms > 0) continue;
    const el = document.createElement(m.media_type === "video" ? "video" : "audio");
    el.preload = "metadata";
    el.muted = true;
    el.src = convertFileSrc(m.path);
    el.onloadedmetadata = () => {
      const ms = Math.round(el.duration * 1000);
      if (isFinite(ms) && ms > 0) {
        m.duration_ms = ms;
        invoke("update_media_duration", { id: m.id, durationMs: ms }).catch(() => {});
      }
      el.remove();
    };
    el.onerror = () => el.remove();
  }
}

function play(item: MediaItem) {
  current.value = item;
}

// 字幕面板点击某行 → 跳转到对应时间
function seekTo(t: number) {
  const mediaEl = document.querySelector<HTMLMediaElement>(".canvas video, .canvas audio");
  if (mediaEl) mediaEl.currentTime = t;
}

// 快捷键：播放控制 / 面板开关 / 字幕跳转
function isEditableTarget(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  const tag = t.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable;
}

const sc = useShortcuts();

function seekToSubtitle(dir: 1 | -1) {
  const list = sub.subtitles.value;
  if (!list.length) return;
  const t = sub.currentTime.value * 1000;
  let idx = list.findIndex((s) => t >= s.start_ms && t < s.end_ms);
  if (idx === -1) idx = list.findIndex((s) => s.start_ms > t);
  if (idx === -1) idx = list.length - 1;
  const nxt = idx + dir;
  if (nxt < 0 || nxt >= list.length) return;
  seekTo(list[nxt].start_ms / 1000);
}

function onKeydown(e: KeyboardEvent) {
  // 正在录制快捷键时，忽略全局快捷键
  if (sc.recording.value) return;
  if (e.key === "Escape") {
    if (stageFullscreen.value) {
      stageRef.value?.toggleFullscreen();
      return;
    }
    if (settingsOpen.value) settingsOpen.value = false;
    return;
  }
  if (isEditableTarget(e)) return;
  const keys = sc.normalizeKey(e);
  const binding = sc.shortcuts.value.find((s) => s.keys === keys);
  if (!binding) return;
  e.preventDefault();
  switch (binding.action) {
    case "togglePlay":
      stageRef.value?.togglePlay();
      break;
    case "seekBack":
      stageRef.value?.seekBy(-15);
      break;
    case "seekForward":
      stageRef.value?.seekBy(15);
      break;
    case "volumeUp":
      stageRef.value?.adjustVolume(0.1);
      break;
    case "volumeDown":
      stageRef.value?.adjustVolume(-0.1);
      break;
    case "mute":
      stageRef.value?.toggleMute();
      break;
    case "fullscreen":
      stageRef.value?.toggleFullscreen();
      break;
    case "nextSubtitle":
      seekToSubtitle(1);
      break;
    case "prevSubtitle":
      seekToSubtitle(-1);
      break;
    case "togglePlaylist":
      togglePlaylist();
      break;
    case "toggleSubtitle":
      toggleSubtitle();
      break;
    case "openSettings":
      settingsOpen.value = true;
      break;
  }
}

function onFullscreenChange(v: boolean) {
  stageFullscreen.value = v;
}

// ---- M3 迷你悬浮字幕窗 ----

const overlayVisible = ref(false);
const overlayLocked = ref(false);

async function onOverlayToggle() {
  try {
    // 不做本地乐观取反：Rust 是可见性唯一事实来源，成功后经
    // overlay://visibility 广播回推本 ref，避免双写漂移（Bug #4 真凶之一）
    await toggleOverlayVisible();
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 悬浮窗显隐切换失败:", e);
  }
}

// 悬浮窗显隐只做图标状态展示；推送引擎恒开，不依赖此状态
// 换文件或字幕数据刷新后：清空悬浮窗残留，等待下一次真实句子
watch(
  () => sub.currentId.value,
  () => overlayResetFeed()
);
watch(sub.subtitles, () => overlayResetFeed());

/** 全局快捷键转发来的动作（主窗口最小化/失焦时依然生效） */
function onGlobalAction(action: string) {
  if (action === "togglePlay") stageRef.value?.togglePlay();
}

function onStepSubtitle(delta: number) {
  seekToSubtitle(delta >= 0 ? 1 : -1);
}

// 字幕面板/工具栏的"取消转写"：受理取消请求（whisper 推理不可中断，
// 最迟在本轮推理结束后退出并广播 transcribe://canceled）
async function onCancelTranscribe() {
  if (!current.value) return;
  sub.setStatus("transcribing", "cancel", sub.progress.value, "已请求取消，等待当前步骤结束…");
  try {
    await cancelTranscribe(current.value.id);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 取消转写失败:", e);
  }
}

function togglePlaylist() {
  showPlaylist.value = !showPlaylist.value;
}

function toggleSubtitle() {
  showSubtitle.value = !showSubtitle.value;
}

onMounted(async () => {
  const u1 = await onTranscribeProgress((e) => {
    sub.setStatus(
      e.stage === "done" ? "done" : e.stage === "translate" ? "translating" : "transcribing",
      e.stage,
      e.progress,
      e.message
    );
  });
  const u2 = await onTranscribeDone(async (mediaId) => {
    sub.setStatus("done", "done", 100, "");
    if (sub.currentId.value === mediaId) sub.load(mediaId);
    // 若之前点了"转写并翻译"，转写完成后自动触发翻译
    const auto = sub.consumeAutoTranslate();
    if (auto != null && auto === mediaId) {
      sub.setStatus("translating", "translate", 0, "正在翻译…");
      await translateMedia(mediaId);
    }
  });
  const u3 = await onTranscribeError((msg) => {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 转写/翻译错误:", msg);
    sub.setStatus("error", "", 0, msg);
  });
  // 用户取消转写：复位状态、丢弃挂起的自动翻译待办、刷新列表
  const u4 = await onTranscribeCanceled((mediaId) => {
    // eslint-disable-next-line no-console
    console.log("[ASPlayer] 转写已取消:", mediaId);
    sub.consumeAutoTranslate();
    sub.setStatus("partial", "canceled", sub.progress.value, "已取消，可继续转写");
    // 取消后重载已写库的切片字幕，否则字幕面板仍是空（Bug：partial 不显示）
    if (sub.currentId.value === mediaId) sub.load(mediaId);
    refresh().catch(() => {});
  });
  unlisteners.push(u1, u2, u3, u4);
  // M3 悬浮窗事件接线（全部走后端中继）
  const u5 = await listen<boolean>("overlay://visibility", (e) => {
    overlayVisible.value = !!e.payload;
  });
  const u6 = await listen<boolean>("overlay://lock-changed", (e) => {
    overlayLocked.value = !!e.payload;
  });
  const u8 = await listen<number>("overlay://step-subtitle", (e) => {
    onStepSubtitle(e.payload ?? 0);
  });
  const u9 = await listen<string>("overlay://global-action", (e) => {
    onGlobalAction(e.payload ?? "");
  });
  unlisteners.push(u5, u6, u8, u9);
  window.addEventListener("keydown", onKeydown);
  refresh();
  // 同步悬浮窗初始状态（Rust 侧是事实来源）
  isOverlayVisible()
    .then((v) => (overlayVisible.value = v))
    .catch(() => {});
  isOverlayLocked()
    .then((v) => (overlayLocked.value = v))
    .catch(() => {});
});

onUnmounted(() => {
  unlisteners.forEach((u) => u());
  window.removeEventListener("keydown", onKeydown);
  prefersDark.removeEventListener("change", onSystemThemeChange);
});
</script>

<template>
  <div class="app-layout">
    <PlayerStage
      ref="stageRef"
      :item="current"
      :items="items"
      :overlay-on="overlayVisible"
      @import="importFiles"
      @play="play"
      @settings="settingsOpen = true"
      @toggle-playlist="togglePlaylist"
      @toggle-subtitle="toggleSubtitle"
      @fullscreen-change="onFullscreenChange"
      @overlay-toggle="onOverlayToggle"
    />
    <SubtitlePanel
      v-if="showSubtitle && !stageFullscreen"
      :subtitles="sub.subtitles.value"
      :current-time="sub.currentTime.value"
      :status="sub.status.value"
      :stage="sub.stage.value"
      :progress="sub.progress.value"
      :message="sub.message.value"
      @close="showSubtitle = false"
      @seek="seekTo"
      @cancel="onCancelTranscribe"
    />
    <PlaylistPanel
      v-if="showPlaylist && !stageFullscreen"
      :items="items"
      :current-id="current?.id ?? null"
      :loading="loading"
      @play="play"
      @import="importFiles"
      @import-folder="importFolder"
      @refresh="refresh"
      @close="showPlaylist = false"
    />
    <SettingsPanel
      :open="settingsOpen"
      :theme="theme"
      @close="settingsOpen = false"
      @set-theme="setTheme"
    />
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  height: 100vh;
  background: var(--bg-0);
}
</style>




