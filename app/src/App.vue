<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import PlayerStage from "./components/PlayerStage.vue";
import PlaylistPanel from "./components/PlaylistPanel.vue";
import type { MediaItem } from "./types";

const items = ref<MediaItem[]>([]);
const current = ref<MediaItem | null>(null);
const loading = ref(false);

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
  } finally {
    loading.value = false;
  }
}

function play(item: MediaItem) {
  current.value = item;
}

const THEME_KEY = "asplayer-theme-v2";
function toggleTheme() {
  const next =
    document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem(THEME_KEY, next);
  } catch {}
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
    />
    <PlaylistPanel
      :items="items"
      :current-id="current?.id ?? null"
      :loading="loading"
      @play="play"
      @import="importFolder"
      @refresh="refresh"
    />
    <button class="theme-toggle" title="切换主题" @click="toggleTheme">◐</button>
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  height: 100vh;
  background: var(--bg-0);
}

.theme-toggle {
  position: fixed;
  top: 10px;
  right: 64px;
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--fg-3);
  border-radius: 8px;
  cursor: pointer;
  z-index: 10;
  transition: background 0.15s ease, color 0.15s ease;
}

.theme-toggle:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}
</style>


