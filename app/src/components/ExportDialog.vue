<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { MediaItem } from "../types";
import {
  exportSubtitles,
  type ExportContent,
  type ExportFormat,
} from "../api/export";

const props = defineProps<{ item: MediaItem | null }>();
const emit = defineEmits<{ close: [] }>();

const FORMATS: { key: ExportFormat; label: string; ext: string; filter: string }[] = [
  { key: "srt", label: "SRT", ext: "srt", filter: "SRT 字幕" },
  { key: "vtt", label: "VTT", ext: "vtt", filter: "WebVTT 字幕" },
  { key: "txt", label: "TXT", ext: "txt", filter: "纯文本" },
];

const CONTENTS: { key: ExportContent; label: string }[] = [
  { key: "original", label: "原文" },
  { key: "translation", label: "译文" },
  { key: "bilingual", label: "双语" },
];

const format = ref<ExportFormat>("srt");
const content = ref<ExportContent>("bilingual");
const busy = ref(false);
const error = ref("");
const notice = ref("");
let closeTimer: number | null = null;
let runToken = 0;

function clearCloseTimer() {
  if (closeTimer !== null) {
    window.clearTimeout(closeTimer);
    closeTimer = null;
  }
}

watch(
  () => props.item,
  (item) => {
    runToken++;
    clearCloseTimer();
    if (!item) return;
    format.value = "srt";
    content.value = "bilingual";
    busy.value = false;
    error.value = "";
    notice.value = "";
  }
);

function closeIfIdle() {
  if (busy.value) return;
  clearCloseTimer();
  emit("close");
}

function onKey(e: KeyboardEvent) {
  if (!props.item || e.key !== "Escape") return;
  e.stopImmediatePropagation();
  closeIfIdle();
}

onMounted(() => window.addEventListener("keydown", onKey, true));
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey, true);
  clearCloseTimer();
});

function defaultPath(item: MediaItem, ext: string) {
  if (!item.path) return `${item.title}.${ext}`;
  const sep = Math.max(item.path.lastIndexOf("/"), item.path.lastIndexOf("\\"));
  const dot = item.path.lastIndexOf(".");
  const stem = dot > sep ? item.path.slice(0, dot) : item.path;
  return `${stem}.${ext}`;
}

async function doExport() {
  const item = props.item;
  if (!item || busy.value || notice.value) return;
  error.value = "";
  notice.value = "";
  const meta = FORMATS.find((f) => f.key === format.value);
  if (!meta) return;

  const token = ++runToken;
  const mediaId = item.id;
  const fmt = format.value;
  const ct = content.value;
  busy.value = true;
  try {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({
      defaultPath: defaultPath(item, meta.ext),
      filters: [{ name: meta.filter, extensions: [meta.ext] }],
    });
    if (!path || token !== runToken) return;
    const count = await exportSubtitles(mediaId, path, fmt, ct);
    if (token !== runToken) return;
    notice.value = `已导出 ${count} 句`;
    closeTimer = window.setTimeout(() => {
      closeTimer = null;
      emit("close");
    }, 800);
  } catch (e) {
    if (token === runToken) {
      error.value = e instanceof Error ? e.message : String(e);
    }
  } finally {
    if (token === runToken) busy.value = false;
  }
}
</script>

<template>
  <Teleport to="body">
    <div v-if="item" class="ex-overlay" @click.self="closeIfIdle">
      <div class="ex-dlg" role="dialog" aria-modal="true" aria-label="导出字幕">
        <div class="ex-title">导出字幕</div>
        <p class="ex-media">{{ item.title }}</p>

        <div class="ex-label">格式</div>
        <div class="ex-seg">
          <button
            v-for="f in FORMATS"
            :key="f.key"
            :class="{ on: format === f.key }"
            :aria-pressed="format === f.key"
            @click="format = f.key"
          >
            {{ f.label }}
          </button>
        </div>

        <div class="ex-label">内容</div>
        <div class="ex-seg">
          <button
            v-for="c in CONTENTS"
            :key="c.key"
            :class="{ on: content === c.key }"
            :aria-pressed="content === c.key"
            @click="content = c.key"
          >
            {{ c.label }}
          </button>
        </div>

        <p v-if="notice" class="ex-notice" role="status">{{ notice }}</p>
        <p v-else-if="error" class="ex-error" role="alert">{{ error }}</p>

        <div class="ex-actions">
          <button class="ex-btn" :disabled="busy" @click="closeIfIdle">取消</button>
          <button class="ex-btn primary" :disabled="busy || !!notice" @click="doExport">
            {{ busy ? "导出中…" : "导出" }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.ex-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
}

.ex-dlg {
  width: min(360px, 90vw);
  padding: 20px 22px;
  border: 1px solid var(--panel-border);
  border-radius: 14px;
  background: var(--bg-1);
  box-shadow: var(--shadow-panel);
  color: var(--fg-1);
}

.ex-title {
  font-size: 16px;
  font-weight: 600;
}

.ex-media {
  margin-top: 4px;
  font-size: 12px;
  color: var(--fg-3);
  word-break: break-all;
}

.ex-label {
  margin-top: 16px;
  margin-bottom: 6px;
  font-size: 11.5px;
  color: var(--fg-3);
}

.ex-seg {
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: 9px;
  background: var(--accent-dim);
}

.ex-seg button {
  flex: 1;
  padding: 6px 0;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-2);
  font-size: 12.5px;
}

.ex-seg button.on {
  background: var(--bg-2);
  color: var(--fg-1);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
}

.ex-notice {
  margin-top: 14px;
  font-size: 12.5px;
  color: var(--accent-soft);
}

.ex-error {
  margin-top: 14px;
  font-size: 12.5px;
  color: var(--danger);
  word-break: break-all;
}

.ex-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 20px;
}

.ex-btn {
  padding: 8px 16px;
  border: none;
  border-radius: 8px;
  background: var(--bg-2);
  color: var(--fg-1);
  font-size: 13px;
}

.ex-btn.primary {
  background: var(--accent);
  color: var(--on-accent);
}

.ex-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
