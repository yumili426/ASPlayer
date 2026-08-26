<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import PlayerStage from "./components/PlayerStage.vue";
import PlaylistPanel from "./components/PlaylistPanel.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import type { MediaItem } from "./types";

const items = ref<MediaItem[]>([]);
const current = ref<MediaItem | null>(null);
const loading = ref(false);
const settingsOpen = ref(false);
const theme = ref<"light" | "dark">("dark");

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

function play(item: MediaItem) {
  current.value = item;
}

refresh();
</script>

<template>
  <div class="app-layout">
    <PlayerStage
      :item="current"
      :items="items"
      @import="importFolder"
      @play="play"
      @settings="settingsOpen = true"
    />
    <PlaylistPanel
      :items="items"
      :current-id="current?.id ?? null"
      :loading="loading"
      @play="play"
      @import="importFolder"
      @refresh="refresh"
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




