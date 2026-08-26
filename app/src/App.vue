<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import PlayerStage from "./components/PlayerStage.vue";
import PlaylistPanel from "./components/PlaylistPanel.vue";
import SubtitlePanel from "./components/SubtitlePanel.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import type { MediaItem } from "./types";
import { useSubtitle } from "./stores/subtitle";
import {
  onTranscribeProgress,
  onTranscribeDone,
  onTranscribeError,
  translateMedia,
} from "./api/subtitle";

const sub = useSubtitle();

const items = ref<MediaItem[]>([]);
const current = ref<MediaItem | null>(null);
const loading = ref(false);
const settingsOpen = ref(false);
const theme = ref<"light" | "dark">("dark");
const showPlaylist = ref(true);
const showSubtitle = ref(true);
const unlisteners: (() => void)[] = [];

const THEME_KEY = "asplayer-theme-v2";
const saved = (() => {
  try {
    return localStorage.getItem(THEME_KEY) as "light" | "dark" | null;
  } catch {
    return null;
  }
})();
theme.value = saved ?? "dark";
document.documentElement.dataset.theme = theme.value;

function setTheme(t: "light" | "dark") {
  theme.value = t;
  document.documentElement.dataset.theme = t;
  try {
    localStorage.setItem(THEME_KEY, t);
  } catch {}
}

async function refresh() {
  loading.value = true;
  try {
    items.value = await invoke<MediaItem[]>("list_media");
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
    // eslint-disable-next-line no-console
    console.log("[ASPlayer] 导入文件:", paths, "→", items.value.length, "个");
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 导入文件失败:", e);
  } finally {
    loading.value = false;
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
  unlisteners.push(u1, u2, u3);
  refresh();
});

onUnmounted(() => {
  unlisteners.forEach((u) => u());
});
</script>

<template>
  <div class="app-layout">
    <PlayerStage
      :item="current"
      :items="items"
      @import="importFiles"
      @play="play"
      @settings="settingsOpen = true"
      @toggle-playlist="togglePlaylist"
      @toggle-subtitle="toggleSubtitle"
    />
    <SubtitlePanel
      v-if="showSubtitle"
      :subtitles="sub.subtitles.value"
      :current-time="sub.currentTime.value"
      :status="sub.status.value"
      :stage="sub.stage.value"
      :progress="sub.progress.value"
      :message="sub.message.value"
      @close="showSubtitle = false"
      @seek="seekTo"
    />
    <PlaylistPanel
      v-if="showPlaylist"
      :items="items"
      :current-id="current?.id ?? null"
      :loading="loading"
      @play="play"
      @import="importFolder"
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




