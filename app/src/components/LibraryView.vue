<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { MediaItem } from "../types";

const emit = defineEmits<{ play: [item: MediaItem] }>();
const items = ref<MediaItem[]>([]);
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
  const dir = await open({ directory: true });
  if (!dir) return;
  loading.value = true;
  try {
    items.value = await invoke<MediaItem[]>("import_folder", { path: dir });
  } finally {
    loading.value = false;
  }
}

function fmtDuration(ms: number): string {
  if (!ms) return "";
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

refresh();
</script>

<template>
  <div class="library">
    <div class="toolbar">
      <button class="primary" @click="importFolder">导入文件夹</button>
      <button class="ghost" @click="refresh">刷新</button>
      <span v-if="loading" class="hint">处理中…</span>
    </div>

    <p v-if="items.length === 0 && !loading" class="empty">
      媒体库还是空的。点击「导入文件夹」，把你的 ASMR / 学习视频加进来吧。
    </p>

    <div v-else class="grid">
      <div
        v-for="item in items"
        :key="item.id"
        class="card"
        @dblclick="emit('play', item)"
        @click="emit('play', item)"
      >
        <div class="thumb">{{ item.media_type === "video" ? "▶" : "♪" }}</div>
        <div class="meta">
          <div class="title">{{ item.title }}</div>
          <div class="sub">
            {{ item.media_type === "video" ? "视频" : "音频" }}
            <span v-if="item.duration_ms"> · {{ fmtDuration(item.duration_ms) }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.library {
  padding: 20px;
  height: 100%;
  overflow-y: auto;
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 18px;
}

.primary {
  background: var(--accent-dim);
  border-color: var(--accent);
  color: var(--fg-1);
}

.hint,
.empty {
  color: var(--fg-3);
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 14px;
}

.card {
  display: flex;
  gap: 12px;
  align-items: center;
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: var(--radius-card);
  padding: 12px;
  cursor: pointer;
  transition: transform 200ms cubic-bezier(0.32, 0.72, 0, 1),
    box-shadow 200ms ease;
}

.card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-card);
}

.thumb {
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 18px;
  color: var(--accent);
  background: var(--accent-dim);
  border-radius: 10px;
  flex-shrink: 0;
}

.meta {
  min-width: 0;
}

.title {
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sub {
  color: var(--fg-2);
  font-size: 12px;
}
</style>
