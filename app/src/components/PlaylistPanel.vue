<script setup lang="ts">
import type { MediaItem } from "../types";

defineProps<{
  items: MediaItem[];
  currentId: number | null;
  loading: boolean;
}>();
const emit = defineEmits<{
  play: [item: MediaItem];
  import: [];
  refresh: [];
}>();

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
        <span class="pl-count">{{ loading ? "…" : `${items.length} 个` }}</span>
      </div>
      <div class="pl-actions">
        <button class="tool-btn" title="导入" @click="emit('import')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 5v14M5 12h14"/></svg>
          <span>导入</span>
        </button>
        <button class="tool-btn" title="刷新" @click="emit('refresh')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M20 12a8 8 0 1 1-2.34-5.66M20 4v4h-4"/></svg>
        </button>
      </div>
    </div>

    <!-- 空态 / 列表 -->
    <div v-if="items.length === 0" class="pl-empty">
      <p>媒体库还是空的</p>
      <p class="pl-empty-sub">点击右上角 ＋ 导入 ASMR 文件夹</p>
    </div>

    <div v-else class="pl-scroll">
      <div
        v-for="(item, i) in items"
        :key="item.id"
        class="pl-item"
        :class="{ active: item.id === currentId }"
        @click="emit('play', item)"
      >
        <span class="pl-num">{{ item.id === currentId ? "▶" : i + 1 }}</span>
        <span class="pl-name">{{ item.title }}</span>
        <span v-if="item.subtitle_count > 0" class="pl-sub" :title="`${item.subtitle_count} 段字幕`">字</span>
        <span class="pl-dur">{{ fmtDuration(item.duration_ms) }}</span>
      </div>
    </div>
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
  padding: 14px 16px;
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
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 12px;
  border: none;
  border-radius: 8px;
  background: var(--bg-2);
  color: var(--fg-1);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s ease, opacity 0.15s ease;
}

.tool-btn:first-child {
  background: var(--accent);
  color: #fff;
}

.tool-btn:hover {
  opacity: 0.9;
}

.tool-btn svg {
  width: 15px;
  height: 15px;
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
</style>
