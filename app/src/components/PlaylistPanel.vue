<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { MediaItem } from "../types";

const props = defineProps<{
  items: MediaItem[];
  currentId: number | null;
  loading: boolean;
}>();
const emit = defineEmits<{
  play: [item: MediaItem];
  import: [];
  importFolder: [];
  refresh: [];
  close: [];
}>();

const ctxMenu = ref<{ x: number; y: number; item: MediaItem } | null>(null);
const confirmDelete = ref<MediaItem | null>(null);
const hover = ref<{ x: number; y: number; item: MediaItem } | null>(null);
const tipEl = ref<HTMLElement | null>(null);

async function onItemHover(e: MouseEvent, item: MediaItem) {
  const w = 232;
  const estH = 112;
  const t = e.currentTarget as HTMLElement | null;
  let x: number;
  let y: number;
  if (t) {
    const r = t.getBoundingClientRect();
    // 悬浮层置于播放列表左侧（视频区），垂直居中于当前曲目，避免遮挡曲目
    x = r.left - w - 14;
    y = r.top + r.height / 2 - estH / 2;
  } else {
    x = e.clientX;
    y = e.clientY;
  }
  x = Math.max(8, Math.min(x, window.innerWidth - w - 8));
  y = Math.max(8, Math.min(y, window.innerHeight - estH - 8));
  hover.value = { x, y, item };
  // 文件名完整显示时高度不定，渲染后按实际高度修正位置，保证完整显示且不超出窗口
  await nextTick();
  const el = tipEl.value;
  if (!el || !hover.value || hover.value.item !== item) return;
  const realH = el.offsetHeight;
  const top = Math.max(8, Math.min(hover.value.y, window.innerHeight - realH - 8));
  if (top !== hover.value.y) hover.value = { ...hover.value, y: top };
}

function onItemLeave() {
  hover.value = null;
}

function fmtSize(bytes: number): string {
  if (!bytes || bytes <= 0) return "未知";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let n = bytes;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i += 1;
  }
  const digits = i === 0 || n >= 100 ? 0 : 1;
  return `${n.toFixed(digits)} ${units[i]}`;
}

function onContextMenu(e: MouseEvent, item: MediaItem) {
  const w = 180;
  const h = 172;
  const x = Math.min(e.clientX, window.innerWidth - w - 8);
  const y = Math.min(e.clientY, window.innerHeight - h - 8);
  ctxMenu.value = { x, y, item };
}

function closeCtx() {
  ctxMenu.value = null;
}

function playItem(item: MediaItem) {
  emit("play", item);
  closeCtx();
}

async function revealInFolder(item: MediaItem) {
  closeCtx();
  try {
    await revealItemInDir(item.path);
  } catch {
    /* ignore */
  }
}

async function removeFromList(item: MediaItem) {
  closeCtx();
  try {
    await invoke<void>("remove_media", { id: item.id });
    emit("refresh");
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 从列表移除失败:", e);
  }
}

function deleteFile(item: MediaItem) {
  closeCtx();
  confirmDelete.value = item;
}

function cancelDelete() {
  confirmDelete.value = null;
}

async function doDelete() {
  const item = confirmDelete.value;
  if (!item) return;
  confirmDelete.value = null;
  try {
    await invoke<void>("delete_media_file", { id: item.id });
    emit("refresh");
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 删除文件失败:", e);
  }
}

onMounted(() => window.addEventListener("click", closeCtx));
onBeforeUnmount(() => window.removeEventListener("click", closeCtx));

const search = ref("");
const sortBy = ref<"added" | "title" | "duration" | "subtitle">("added");

const sortedItems = computed(() => {
  const q = search.value.trim().toLowerCase();
  let list = props.items;
  if (q) list = list.filter((m) => m.title.toLowerCase().includes(q));
  const arr = [...list];
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

function fmtDuration(ms: number): string {
  if (!ms) return "00:00";
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}
</script>

<template>
  <aside class="playlist">
    <!-- 面板头 -->
    <div class="pl-head">
      <div class="pl-head-left">
        <span class="pl-title">播放列表</span>
        <span class="pl-count">{{ loading ? "…" : `${sortedItems.length} 个` }}</span>
      </div>
      <div class="pl-actions">
        <button class="tool-btn" title="导入文件" @click="emit('import')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M12 5v14M5 12h14"/></svg>
        </button>
        <button class="tool-btn" title="导入文件夹" @click="emit('importFolder')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
        </button>
        <button class="tool-btn" title="刷新" @click="emit('refresh')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M20 12a8 8 0 1 1-2.34-5.66M20 4v4h-4"/></svg>
        </button>
        <button class="pl-close" title="关闭播放列表" @click="emit('close')">
          <svg viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-1)" stroke-width="1.8"><path d="M6 6l12 12M18 6L6 18"/></svg>
        </button>
      </div>
    </div>

    <!-- 搜索与排序 -->
    <div class="pl-tools">
      <input v-model="search" class="pl-search" type="text" placeholder="搜索标题…" />
      <select v-model="sortBy" class="pl-sort">
        <option value="added">默认</option>
        <option value="title">标题</option>
        <option value="duration">时长</option>
        <option value="subtitle">字幕数</option>
      </select>
    </div>

    <!-- 空态 / 列表 -->
    <div v-if="items.length === 0" class="pl-empty">
      <p>媒体库还是空的</p>
      <p class="pl-empty-sub">点击右上角导入文件或文件夹</p>
    </div>

    <div v-else-if="sortedItems.length === 0" class="pl-empty">
      <p>无匹配结果</p>
      <p class="pl-empty-sub">换个关键词试试</p>
    </div>

    <div v-else class="pl-scroll">
      <div
        v-for="(item, i) in sortedItems"
        :key="item.id"
        class="pl-item"
        :class="{ active: item.id === currentId }"
        @click="emit('play', item)"
        @contextmenu.prevent="onContextMenu($event, item)"
        @mousemove="onItemHover($event, item)"
        @mouseleave="onItemLeave"
      >
        <span class="pl-num">{{ item.id === currentId ? "▶" : i + 1 }}</span>
        <span class="pl-name">{{ item.title }}</span>
        <span v-if="item.subtitle_count > 0" class="pl-sub" :title="`${item.subtitle_count} 段字幕`">字</span>
        <span class="pl-dur">{{ fmtDuration(item.duration_ms) }}</span>
      </div>
    </div>

    <Teleport to="body">
      <div
        v-if="ctxMenu"
        class="pl-ctx"
        :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
        @click.stop
      >
        <button class="pl-ctx-item" @click="playItem(ctxMenu!.item)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M7 5l12 7-12 7z"/></svg>
          播放
        </button>
        <button class="pl-ctx-item" @click="revealInFolder(ctxMenu!.item)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
          在文件夹中显示
        </button>
        <button class="pl-ctx-item" @click="removeFromList(ctxMenu!.item)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/></svg>
          从列表移除
        </button>
        <button class="pl-ctx-item danger" @click="deleteFile(ctxMenu!.item)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/></svg>
          删除文件
        </button>
      </div>
    </Teleport>

    <Teleport to="body">
      <div v-if="confirmDelete" class="dlg-overlay" @click.self="cancelDelete">
        <div class="dlg">
          <div class="dlg-title">删除文件</div>
          <p class="dlg-body">
            此操作将<strong>永久删除磁盘上的文件</strong>并从媒体库移除，删除后<strong>无法恢复</strong>，请谨慎选择。<br />
            确认删除：<span class="dlg-name">{{ confirmDelete.title }}</span>
          </p>
          <div class="dlg-actions">
            <button class="dlg-btn" @click="cancelDelete">取消</button>
            <button class="dlg-btn danger" @click="doDelete">删除</button>
          </div>
        </div>
      </div>
    </Teleport>

    <Teleport to="body">
      <div v-if="hover" ref="tipEl" class="pl-tip" :style="{ left: hover.x + 'px', top: hover.y + 'px' }">
        <div class="pl-tip-title">{{ hover.item.title }}</div>
        <div class="pl-tip-row"><label>文件大小</label><span>{{ fmtSize(hover.item.file_size) }}</span></div>
        <div class="pl-tip-row"><label>文件时长</label><span>{{ fmtDuration(hover.item.duration_ms) }}</span></div>
      </div>
    </Teleport>
  </aside>
</template>

<style scoped>
.playlist {
  width: 300px;
  flex-shrink: 0;
  background: var(--bg-1);
  border-left: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  user-select: none;
}

.pl-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  min-height: 52px;
  border-bottom: 1px solid var(--line);
}

.pl-head-left {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.pl-title {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.pl-count {
  font-size: 12px;
  color: var(--fg-3);
}

.pl-actions {
  display: flex;
  gap: 6px;
}

.tool-btn {
  width: 30px;
  height: 30px;
  flex: 0 0 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 8px;
  background: var(--bg-2);
  color: var(--fg-1);
  cursor: pointer;
  transition: background 0.15s ease, filter 0.15s ease;
}

.tool-btn:hover {
  background: var(--bg-2);
  filter: brightness(1.25);
  color: var(--fg-1);
}

.tool-btn svg {
  width: 15px;
  height: 15px;
}

.pl-close {
  width: 30px;
  height: 30px;
  flex: 0 0 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: var(--bg-2);
  color: var(--fg-1);
  border-radius: 7px;
  cursor: pointer;
  transition: background 0.15s ease, filter 0.15s ease;
}

.pl-close:hover {
  background: var(--bg-2);
  filter: brightness(1.2);
}

.pl-close svg {
  width: 15px;
  height: 15px;
  stroke-width: 2;
}

.pl-tools {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--line);
}

.pl-search {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  padding: 6px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--bg-2);
  color: var(--fg-1);
  outline: none;
}

.pl-search::placeholder {
  color: var(--fg-3);
}

.pl-search:focus {
  border-color: var(--accent);
}

.pl-sort {
  flex-shrink: 0;
  font-size: 12px;
  padding: 6px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--bg-2);
  color: var(--fg-1);
  outline: none;
  cursor: pointer;
}

.pl-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0;
}

.pl-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  color: var(--fg-2);
  font-size: 14px;
}

.pl-empty-sub {
  font-size: 12px;
  color: var(--fg-3);
}

.pl-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 11px 16px;
  cursor: pointer;
  color: var(--fg-2);
  border-left: 2px solid transparent;
  transition: background 0.12s ease, color 0.12s ease;
}

.pl-item:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.pl-item.active {
  background: var(--accent-dim);
  color: var(--fg-1);
  border-left-color: var(--accent);
}

.pl-num {
  width: 18px;
  font-size: 11px;
  color: var(--fg-3);
  text-align: center;
  flex-shrink: 0;
}

.pl-item.active .pl-num {
  color: var(--accent);
}

.pl-name {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pl-dur {
  font-size: 12px;
  color: var(--fg-3);
  font-variant-numeric: tabular-nums;
}

.pl-sub {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--accent);
  background: var(--accent-dim);
  border-radius: 4px;
  padding: 1px 4px;
  line-height: 1.3;
  margin-left: auto;
  margin-right: 6px;
}

.pl-ctx {
  position: fixed;
  z-index: 999;
  min-width: 176px;
  padding: 6px;
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: 10px;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.3);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.pl-ctx-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 8px 10px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-1);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
  transition: background 0.12s ease, color 0.12s ease;
}

.pl-ctx-item:hover {
  background: var(--accent-dim);
}

.pl-ctx-item svg {
  width: 15px;
  height: 15px;
  flex: 0 0 15px;
}

.pl-ctx-item.danger {
  color: #ff453a;
}

.pl-ctx-item.danger:hover {
  background: rgba(255, 69, 58, 0.12);
}

.dlg-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
}

.dlg {
  width: min(360px, 90vw);
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: 14px;
  padding: 22px;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
}

.dlg-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--fg-1);
  margin-bottom: 10px;
}

.dlg-body {
  font-size: 13px;
  color: var(--fg-2);
  line-height: 1.7;
  margin-bottom: 18px;
}

.dlg-body strong {
  color: #ff453a;
}

.dlg-name {
  color: var(--fg-1);
  font-weight: 500;
  word-break: break-all;
}

.dlg-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.dlg-btn {
  padding: 8px 16px;
  border: none;
  border-radius: 8px;
  background: var(--bg-2);
  color: var(--fg-1);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s ease, filter 0.15s ease;
}

.dlg-btn:hover {
  filter: brightness(1.15);
}

.dlg-btn.danger {
  background: #ff453a;
  color: #fff;
}

.pl-tip {
  position: fixed;
  z-index: 1000;
  width: 232px;
  padding: 11px 14px;
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: 12px;
  box-shadow: 0 16px 44px rgba(0, 0, 0, 0.4);
  pointer-events: none;
}

.pl-tip-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--fg-1);
  margin-bottom: 8px;
  word-break: break-all;
  line-height: 1.35;
}

.pl-tip-row {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  font-size: 12px;
  line-height: 1.75;
}

.pl-tip-row label {
  color: var(--fg-3);
  flex: 0 0 auto;
}

.pl-tip-row span {
  color: var(--fg-1);
  font-variant-numeric: tabular-nums;
  text-align: right;
  word-break: break-all;
}
</style>
