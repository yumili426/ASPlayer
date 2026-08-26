<script setup lang="ts">
import { computed } from "vue";
import type { Subtitle } from "../types";

const props = defineProps<{
  subtitles: Subtitle[];
  currentTime: number; // 秒
  status: string;
  stage: string;
  progress: number;
  message: string;
}>();

const emit = defineEmits<{ close: []; seek: [t: number] }>();

const currentMs = computed(() => Math.floor(props.currentTime * 1000));

function isActive(s: Subtitle) {
  return currentMs.value >= s.start_ms && currentMs.value < s.end_ms;
}

function fmt(t: number): string {
  const ms = Math.max(0, Math.floor(t));
  const m = Math.floor(ms / 60000);
  const s = Math.floor((ms % 60000) / 1000);
  return `${m}:${String(s).padStart(2, "0")}`;
}

function stageLabel() {
  if (props.status === "translating") return "正在翻译字幕";
  if (props.status === "transcribing") {
    switch (props.stage) {
      case "extract":
        return "正在抽取音轨";
      case "transcribe":
        return "正在转写字幕";
      default:
        return "正在转写字幕";
    }
  }
  return "";
}
</script>

<template>
  <aside class="subpanel">
    <div class="sp-head">
      <span class="sp-title">字幕</span>
      <div class="sp-actions">
        <span v-if="subtitles.length" class="sp-count">{{ subtitles.length }} 段</span>
        <button class="sp-close" title="关闭字幕面板" @click="emit('close')">
          <svg viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-1)" stroke-width="1.8"><path d="M6 6l12 12M18 6L6 18"/></svg>
        </button>
      </div>
    </div>

    <!-- 实时转写/翻译进度 -->
    <div v-if="status === 'transcribing' || status === 'translating'" class="sp-progress">
      <div class="sp-progress-label">
        <span>{{ stageLabel() }}</span>
        <span class="sp-progress-pct">{{ progress }}%</span>
      </div>
      <div class="sp-progress-bar">
        <div class="sp-progress-fill" :style="{ width: progress + '%' }"></div>
      </div>
      <p v-if="message" class="sp-progress-msg">{{ message }}</p>
    </div>

    <!-- 错误 -->
    <div v-else-if="status === 'error'" class="sp-empty">
      <p>字幕生成失败</p>
      <p class="sp-empty-sub">请检查模型 / API 配置后重试</p>
    </div>

    <!-- 无字幕 -->
    <div v-else-if="subtitles.length === 0" class="sp-empty">
      <p>暂无字幕</p>
      <p class="sp-empty-sub">点击工具栏「转写」生成双语字幕</p>
    </div>

    <!-- 字幕列表 -->
    <div v-else class="sp-scroll">
      <div
        v-for="(s, i) in subtitles"
        :key="i"
        class="sp-line"
        :class="{ active: isActive(s) }"
        @click="emit('seek', s.start_ms / 1000)"
      >
        <span class="sp-line-time">{{ fmt(s.start_ms) }}</span>
        <div class="sp-line-body">
          <span class="sp-line-orig">{{ s.text }}</span>
          <span v-if="s.translation" class="sp-line-trans">{{ s.translation }}</span>
        </div>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.subpanel {
  width: 300px;
  flex-shrink: 0;
  background: var(--bg-1);
  border-left: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  user-select: none;
}

.sp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  min-height: 52px;
  border-bottom: 1px solid var(--line);
}

.sp-title {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.sp-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.sp-count {
  font-size: 12px;
  color: var(--fg-3);
}

.sp-close {
  width: 28px;
  height: 28px;
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

.sp-close:hover {
  background: var(--bg-2);
  filter: brightness(1.2);
}

.sp-close svg {
  width: 15px;
  height: 15px;
  stroke-width: 2;
}

/* 进度 */
.sp-progress {
  padding: 14px 16px;
  border-bottom: 1px solid var(--line);
}

.sp-progress-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
  color: var(--fg-1);
  margin-bottom: 8px;
}

.sp-progress-pct {
  color: var(--accent);
  font-variant-numeric: tabular-nums;
  font-weight: 600;
}

.sp-progress-bar {
  height: 5px;
  border-radius: 3px;
  background: var(--bg-2);
  overflow: hidden;
}

.sp-progress-fill {
  height: 100%;
  border-radius: 3px;
  background: var(--accent);
  transition: width 0.2s ease;
}

.sp-progress-msg {
  margin-top: 8px;
  font-size: 12px;
  color: var(--fg-3);
}

/* 空态 */
.sp-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  color: var(--fg-2);
  font-size: 14px;
}

.sp-empty-sub {
  font-size: 12px;
  color: var(--fg-3);
}

/* 列表 */
.sp-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0;
}

.sp-line {
  display: flex;
  gap: 10px;
  padding: 10px 16px;
  cursor: pointer;
  border-left: 2px solid transparent;
  transition: background 0.12s ease;
}

.sp-line:hover {
  background: var(--bg-2);
}

.sp-line.active {
  background: var(--accent-dim);
  border-left-color: var(--accent);
}

.sp-line-time {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--fg-3);
  font-variant-numeric: tabular-nums;
  padding-top: 1px;
  width: 36px;
}

.sp-line-body {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.sp-line-orig {
  font-size: 13px;
  color: var(--fg-1);
  line-height: 1.4;
  word-break: break-word;
}

.sp-line.active .sp-line-orig {
  color: var(--fg-1);
}

.sp-line-trans {
  font-size: 12px;
  color: var(--fg-2);
  line-height: 1.4;
  word-break: break-word;
}
</style>
