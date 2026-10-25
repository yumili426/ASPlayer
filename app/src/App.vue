<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import PlayerStage from "./components/PlayerStage.vue";
import TitleBar from "./components/TitleBar.vue";
import MediaRail from "./components/MediaRail.vue";
import SubtitlePanel from "./components/SubtitlePanel.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import DictCard from "./components/DictCard.vue";
import type { MediaItem, DictLookup } from "./types";
import type { ProfileOverride } from "./lib/intensive";
import { resolvePrimaryAction } from "./lib/uiState";
import { dictDownload, dictLookup, onDictStatus } from "./api/dict";
import { useSubtitle } from "./stores/subtitle";
import { useShortcuts } from "./stores/shortcuts";
import { usePlayback } from "./stores/playback";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
  onTranscribeSegments,
  cancelTranscribe,
  getSubtitleStatus,
  importExternalSubtitle,
} from "./api/subtitle";

const sub = useSubtitle();
const sc = useShortcuts();
const pb = usePlayback();

const items = ref<MediaItem[]>([]);
const current = ref<MediaItem | null>(null);
const loading = ref(false);
const settingsOpen = ref(false);
const theme = ref<"light" | "dark" | "system">("system");
const stageFullscreen = ref(false);
const unlisteners: (() => void)[] = [];
const stageRef = ref<any>(null);

// ---- 单层舞台：左轨 / 字幕抽屉 / 控件自动隐藏 ----
const railPinned = ref(false);
const drawerOpen = ref(false);
const chromeVisible = ref(true);
const playing = ref(false);
const topMenuOpen = ref(false);
const winWidth = ref(window.innerWidth);
const drawerRef = ref<any>(null);

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

const safeDrawer = computed(() => drawerOpen.value && winWidth.value > 720);

// ---- 顶栏 CTA ----
type SubStatus = MediaItem["subtitle_status"];

/** 列表里的 subtitle_status 是加载时的快照；转写/翻译结束后同步，避免 CTA 停在旧状态 */
function setItemStatus(id: number, status: SubStatus) {
  const hit = items.value.find((m) => m.id === id);
  if (hit) hit.subtitle_status = status;
  if (current.value?.id === id) current.value.subtitle_status = status;
}

const liveStatus = computed<SubStatus>(() => {
  const s = sub.status.value;
  if (s === "transcribing" || s === "translating") return s;
  return current.value?.subtitle_status ?? "none";
});

const primary = computed(() => {
  const item = current.value;
  const liveItem = item ? { ...item, subtitle_status: liveStatus.value } : null;
  const progress =
    sub.status.value === "transcribing" || sub.status.value === "translating"
      ? sub.progress.value
      : null;
  // 切换媒体时 sub 仍持有上一部的字幕；不能用它决定新媒体的 CTA
  const subs = sub.currentId.value === current.value?.id ? sub.subtitles.value : [];
  return resolvePrimaryAction(liveItem, subs, progress);
});

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

// ---- 播放中自动隐藏顶栏 / 左轨 ----
let idleTimer: number | null = null;

function isEditableFocus() {
  const el = document.activeElement as HTMLElement | null;
  if (!el) return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable;
}

function canAutoHide() {
  return (
    pb.playback.chromeAutoHide &&
    playing.value &&
    !settingsOpen.value &&
    !dictOpen.value &&
    !topMenuOpen.value &&
    !isEditableFocus()
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

function onPlayingChange(v: boolean) {
  playing.value = v;
  if (v) wakeChrome();
  else {
    chromeVisible.value = true;
    if (idleTimer !== null) window.clearTimeout(idleTimer);
  }
}

// ---- 内置词典查词 ----
const dictOpen = ref(false);
const dictLoading = ref(false);
const dictResult = ref<DictLookup | null>(null);
const dictError = ref<string | null>(null); // 查询失败（区别于词典未安装）
const dictDownloading = ref(false);
let dictTerm = ""; // 最近一次查询的词，用于「下载词典」判定语言
let dictSeq = 0; // 查询序号：丢弃过期响应，防止慢的旧查询覆盖新结果

/** `dict_lookup` 未命中时的占位条：definitions 与 suggestions 皆空即"真未查到" */
async function onDictLookup(term: string) {
  const seq = ++dictSeq;
  dictTerm = term;
  dictOpen.value = true;
  dictLoading.value = true;
  dictResult.value = null;
  dictError.value = null;
  try {
    const res = await dictLookup(term);
    if (seq !== dictSeq) return; // 已有更新的查询，丢弃过期结果
    dictResult.value = res[0] ?? null;
  } catch (e) {
    if (seq !== dictSeq) return;
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 查词失败:", e);
    dictError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (seq === dictSeq) dictLoading.value = false;
  }
}

/** 推断查词语言：含假名（平/片假名，含长音与叠字）→ ja，否则 → en（与 Rust detect_lang 一致） */
function dictLangOf(term: string): "en" | "ja" {
  return /[ぁ-ゖゝゞーァ-ヺｦ-ﾟ]/.test(term) ? "ja" : "en";
}

async function onDictDownload() {
  const lang = dictLangOf(dictTerm);
  dictDownloading.value = true;
  try {
    await dictDownload(lang);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 下载词典失败:", e);
    dictDownloading.value = false;
  }
}

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
  // 原生标题栏跟随应用主题：手动 light/dark 时固定该主题；system 时传 null 让原生窗口
  // 跟随系统（内容走 data-theme + 标题栏走系统，二者随系统同步变化，不会互相漂移）
  getCurrentWindow().setTheme(theme.value === "system" ? null : resolved).catch(() => {});
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

// 导入外部字幕：选文件 → 有旧字幕先确认 → 替换 → 刷新当前媒体
async function onImportSubtitle(mediaId: number) {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const sel = await open({
    multiple: false,
    filters: [{ name: "字幕文件", extensions: ["srt", "vtt"] }],
  });
  if (!sel) return;
  const path = Array.isArray(sel) ? sel[0] : sel;
  const [st] = await getSubtitleStatus(mediaId).catch(() => ["unknown" as string, "" as string]);
  if (st !== "none") {
    const ok = window.confirm("导入将替换该媒体现有的字幕，继续？");
    if (!ok) return;
  }
  try {
    const count = await importExternalSubtitle(mediaId, path);
    // eslint-disable-next-line no-console
    console.log("[ASPlayer] 导入字幕:", count, "段");
    setItemStatus(mediaId, "done");
    if (sub.currentId.value === mediaId) {
      sub.setStatus("done", "done", 100, "");
      sub.load(mediaId);
    }
    refresh().catch(() => {});
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 导入字幕失败:", e);
    // 仅失败的是当前媒体时才改写字幕面板状态，避免非当前媒体的导入错误污染正在播放的媒体
    if (sub.currentId.value === mediaId) {
      sub.setStatus("error", "", 0, String(e));
    }
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
  if (current.value?.id !== item.id) sub.clearSubtitles();
  current.value = item;
}

async function onSetProfile(item: MediaItem, value: ProfileOverride) {
  const { setMediaProfile } = await import("./api/playback");
  try {
    await setMediaProfile(item.id, value);
    const hit = items.value.find((m) => m.id === item.id);
    if (hit) hit.profile_override = value;
    if (current.value?.id === item.id) current.value.profile_override = value;
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 设置播放模式失败:", e);
  }
}

// 字幕面板点击某行 → 跳转到对应时间（路由到 PlayerStage 暴露的 seekToSeconds，统一清句末锚/越界 AB）
function seekTo(t: number) {
  stageRef.value?.seekToSeconds(t);
}

// 快捷键：播放控制 / 面板开关 / 字幕跳转
function isEditableTarget(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  const tag = t.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable;
}

function togglePlaybackMode() {
  pb.playback.playbackMode = pb.playback.playbackMode === "intensive" ? "broadcast" : "intensive";
}

function toggleSentenceLoop() {
  pb.playback.intensiveSentenceLoop = !pb.playback.intensiveSentenceLoop;
}

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
    // DictCard 自己处理 Esc，这里不要再顺带关掉下层控件
    if (dictOpen.value) return;
    if (topMenuOpen.value) {
      topMenuOpen.value = false;
      return;
    }
    if (settingsOpen.value) {
      settingsOpen.value = false;
      return;
    }
    if (drawerRef.value?.closeSearch?.()) return;
    if (drawerOpen.value) {
      toggleSubtitle();
      return;
    }
    if (stageFullscreen.value) {
      stageRef.value?.toggleFullscreen();
      return;
    }
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
    case "togglePlaybackMode":
      togglePlaybackMode();
      break;
    case "repeatSubtitle":
      stageRef.value?.repeatSubtitle();
      break;
    case "toggleSentenceLoop":
      toggleSentenceLoop();
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
// 换媒体后旧词典卡片已过期，复位查词状态
watch(() => sub.currentId.value, () => {
  dictOpen.value = false;
  dictResult.value = null;
});

// 播放模式切换时：用户尚未手动开合过抽屉，则跟随模式默认值
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

let primaryLockUntil = 0;
let activeJobId: number | null = null;
let autoTranslateAfterTranscribe = false;

function onPrimaryAction() {
  // 双击保护：第一次点击启动任务，第二次点击会在状态返回前面误判为“取消”
  if (Date.now() < primaryLockUntil) return;
  primaryLockUntil = Date.now() + 400;
  const a = primary.value.action;
  if (a === "cancel") {
    // 翻译任务不在后端可取消集合内；只有转写阶段才真正可以取消
    if (liveStatus.value === "transcribing") stageRef.value?.doCancelTranscribe();
  } else if (a === "translate") stageRef.value?.doTranslate(false);
  else if (a === "resume") {
    autoTranslateAfterTranscribe = true;
    stageRef.value?.doTranscribe(true);
  } else if (a === "transcribe" || a === "retranscribe") {
    autoTranslateAfterTranscribe = true;
    stageRef.value?.doTranscribe(false);
  } else if (a === "ready" && !drawerOpen.value) toggleSubtitle();
  topMenuOpen.value = false;
}

onMounted(async () => {
  const u1 = await onTranscribeProgress((e) => {
    activeJobId = e.mediaId;
    sub.setStatus(
      e.stage === "done" ? "done" : e.stage === "translate" ? "translating" : "transcribing",
      e.stage,
      e.progress,
      e.message
    );
  });
  const u2 = await onTranscribeDone(async (mediaId) => {
    sub.setStatus("done", "done", 100, "");
    setItemStatus(mediaId, "done");
    if (sub.currentId.value === mediaId) await sub.load(mediaId);
    const n = sub.subtitles.value.length;
    const hit = items.value.find((m) => m.id === mediaId);
    if (hit) hit.subtitle_count = n;
    if (current.value?.id === mediaId) current.value.subtitle_count = n;
    activeJobId = null;
    // 转写完成后自动补翻译（一次性；翻译自身的 done 事件不会再触发）
    if (autoTranslateAfterTranscribe && current.value?.id === mediaId) {
      autoTranslateAfterTranscribe = false;
      if (sub.subtitles.value.some((s) => !s.translation.trim())) {
        stageRef.value?.doTranslate(false);
      }
    }
  });
  const u3 = await onTranscribeError((msg) => {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 转写/翻译错误:", msg);
    // setStatus 会清空 stage，先记下失败阶段
    const failedStage = sub.stage.value;
    sub.setStatus("error", "", 0, msg);
    const id = activeJobId ?? sub.currentId.value;
    // 翻译失败时字幕仍在（多为 done）；标记 error 会让 CTA 变成“重新转写”，故只标记转写类失败
    if (failedStage !== "translate" && id != null) setItemStatus(id, "error");
    activeJobId = null;
    autoTranslateAfterTranscribe = false;
  });
  // 用户取消转写：复位状态、丢弃挂起的自动翻译待办、刷新列表
  const u4 = await onTranscribeCanceled((mediaId) => {
    // eslint-disable-next-line no-console
    console.log("[ASPlayer] 转写已取消:", mediaId);
    sub.setStatus("partial", "canceled", sub.progress.value, "已取消，可继续转写");
    setItemStatus(mediaId, "partial");
    activeJobId = null;
    autoTranslateAfterTranscribe = false;
    // 取消后重载已写库的切片字幕，否则字幕面板仍是空（Bug：partial 不显示）
    if (sub.currentId.value === mediaId) sub.load(mediaId);
    refresh().catch(() => {});
  });
  const uSeg = await onTranscribeSegments(({ mediaId, rows }) => {
    sub.appendSubtitles(mediaId, rows);
  });
  unlisteners.push(u1, u2, u3, u4, uSeg);
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
  // 词典下载状态（一次只下载一种语言，后端 ACTIVE 互斥；终端状态即下载结束）
  const u10 = await onDictStatus((s) => {
    if (s.status === "downloading") dictDownloading.value = true;
    else if (s.status === "done" || s.status === "failed" || s.status === "canceled") dictDownloading.value = false;
  });
  unlisteners.push(u10);
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("resize", onWindowResize);
  window.addEventListener("mousemove", wakeChrome);
  window.addEventListener("keydown", wakeChrome);
  window.addEventListener("focus", wakeChrome);
  wakeChrome();
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
  window.removeEventListener("resize", onWindowResize);
  window.removeEventListener("mousemove", wakeChrome);
  window.removeEventListener("keydown", wakeChrome);
  window.removeEventListener("focus", wakeChrome);
  if (idleTimer !== null) window.clearTimeout(idleTimer);
  prefersDark.removeEventListener("change", onSystemThemeChange);
});
</script>

<template>
  <div class="app-shell">
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
    />

    <SubtitlePanel
      v-if="drawerOpen"
      ref="drawerRef"
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
      @transcribe="stageRef?.doTranscribe(false)"
    />

    <button v-else class="drawer-handle" title="展开字幕" @click="toggleSubtitle">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <path d="m9 6 6 6-6 6" />
      </svg>
    </button>

    <div v-if="topMenuOpen" class="menu-scrim" @click="topMenuOpen = false" />
    <div v-if="topMenuOpen" class="top-menu">
      <button :disabled="!current" @click="stageRef?.doImportSubtitle(); topMenuOpen = false">导入字幕</button>
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

<style scoped>
.app-shell {
  position: relative;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: var(--bg-0);
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

.menu-scrim {
  position: absolute;
  inset: 0;
  z-index: 39;
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
    --drawer-w: 280px;
  }
}

@media (max-width: 720px) {
  .drawer {
    --drawer-w: min(92vw, 340px);
    --panel-bg: var(--panel-bg-strong);
  }
}
</style>




