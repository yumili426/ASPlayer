<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { MediaItem } from "../types";

const props = defineProps<{ item: MediaItem | null; items: MediaItem[] }>();
const emit = defineEmits<{ import: []; play: [item: MediaItem]; settings: [] }>();

const mediaEl = ref<HTMLVideoElement | HTMLAudioElement | null>(null);
const playing = ref(false);
const currentTime = ref(0);
const duration = ref(0);

const src = computed(() => (props.item ? convertFileSrc(props.item.path) : ""));

function fmt(t: number): string {
  if (!isFinite(t)) return "0:00";
  const total = Math.floor(t);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

function restorePosition() {
  const el = mediaEl.value;
  if (!el || !props.item) return;
  if (props.item.playback_position > 0) {
    el.currentTime = props.item.playback_position / 1000;
  }
  if (el.paused) el.play().catch(() => {});
}
onMounted(restorePosition);
watch(
  () => props.item?.id,
  () => requestAnimationFrame(restorePosition)
);

function togglePlay() {
  const el = mediaEl.value;
  if (!el) return;
  if (el.paused) el.play();
  else el.pause();
}

function seekBy(delta: number) {
  const el = mediaEl.value;
  if (!el) return;
  el.currentTime = Math.max(0, Math.min(el.duration || 0, el.currentTime + delta));
}

function onSeekInput(e: Event) {
  const el = mediaEl.value;
  if (!el) return;
  el.currentTime = Number((e.target as HTMLInputElement).value);
}

function next() {
  if (!props.item) return;
  const idx = props.items.findIndex((m) => m.id === props.item!.id);
  const nxt = props.items[(idx + 1) % props.items.length];
  if (nxt) emit("play", nxt);
}
function prev() {
  if (!props.item) return;
  const idx = props.items.findIndex((m) => m.id === props.item!.id);
  const prv = props.items[(idx - 1 + props.items.length) % props.items.length];
  if (prv) emit("play", prv);
}

let lastSave = 0;
function onTimeUpdate() {
  const el = mediaEl.value;
  if (!el || !props.item) return;
  currentTime.value = el.currentTime;
  const now = Date.now();
  if (now - lastSave > 3000) {
    lastSave = now;
    invoke("save_playback_position", {
      id: props.item.id,
      positionMs: Math.round(el.currentTime * 1000),
    }).catch(() => {});
  }
}
</script>

<template>
  <main class="stage">
    <div class="stage-topbar">
      <span class="stage-title">ASPlayer</span>
      <div class="toolbar">
        <button class="iconbtn" title="字幕"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 15h4M11 10h6"/></svg></button>
        <button class="iconbtn" title="翻译"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h8M8 3v2M6.5 5c.5 3 2.5 6 4.5 7M12 8c-1.5 3-4 5-7 6"/></svg></button>
        <button class="iconbtn" title="下载"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12m0 0 4-4m-4 4-4-4M4 21h16"/></svg></button>
        <button class="iconbtn" title="搜索"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m21 21-4-4"/></svg></button>
        <button class="iconbtn" title="设置" @click="emit('settings')"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 13.5a7 7 0 0 0 0-3l1.7-1.3-1.7-2.9-2 .8a7 7 0 0 0-2.6-1.5L14.6 3h-3.2l-.4 2.2a7 7 0 0 0-2.6 1.5l-2-.8L4.7 8.8l1.7 1.3a7 7 0 0 0 0 3l-1.7 1.3 1.7 2.9 2-.8a7 7 0 0 0 2.6 1.5l.4 2.2h3.2l.4-2.2a7 7 0 0 0 2.6-1.5l2 .8 1.7-2.9z"/></svg></button>
      </div>
    </div>

    <div class="canvas">
      <div v-if="!item" class="empty">
        <div class="empty-badge">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.2">
            <path d="M4 13a8 8 0 0 1 16 0M12 13v5"/>
            <rect x="3" y="13" width="4" height="6" rx="1.5"/>
            <rect x="17" y="13" width="4" height="6" rx="1.5"/>
          </svg>
        </div>
        <p class="empty-text">还没有在播放</p>
        <p class="empty-sub">从右侧播放列表选择，或导入你的 ASMR 资源</p>
        <button class="open-btn" @click="emit('import')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 7h4l2-2h4l2 2h4v11H4z"/></svg>
          打开文件
        </button>
      </div>

      <div v-else class="playing">
        <video
          v-if="item.media_type === 'video'"
          ref="mediaEl" :src="src" controls
          @play="playing = true" @pause="playing = false"
          @timeupdate="onTimeUpdate"
          @loadedmetadata="duration = ($event.target as HTMLVideoElement).duration"
        ></video>
        <div v-else class="artwork">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.1"><path d="M4 13a8 8 0 0 1 16 0M12 13v5"/><rect x="3" y="13" width="4" height="6" rx="1.5"/><rect x="17" y="13" width="4" height="6" rx="1.5"/></svg>
        </div>
        <audio
          v-if="item.media_type === 'audio'"
          ref="mediaEl" :src="src"
          @play="playing = true" @pause="playing = false"
          @timeupdate="onTimeUpdate"
          @loadedmetadata="duration = ($event.target as HTMLAudioElement).duration"
        ></audio>
        <p class="now-title">{{ item.title }}</p>
      </div>
    </div>

    <div class="controls">
      <div class="seek-row">
        <span class="time">{{ fmt(currentTime) }}</span>
        <input class="slider" type="range" min="0" :max="duration || 0" step="0.1" :value="currentTime" :disabled="!item" @input="onSeekInput" />
        <span class="time">{{ fmt(duration) }}</span>
      </div>
      <div class="btn-row">
        <div class="btn-group">
          <button class="ctl" title="上一首" :disabled="!item" @click="prev"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M6 5v14M19 6l-8 6 8 6z"/></svg></button>
          <button class="ctl play" :disabled="!item" :title="playing ? '暂停' : '播放'" @click="togglePlay">
            <svg v-if="playing" width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M8 5v14M16 5v14"/></svg>
            <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M7 5l12 7-12 7z"/></svg>
          </button>
          <button class="ctl" title="下一首" :disabled="!item" @click="next"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M18 5v14M5 6l8 6-8 6z"/></svg></button>
        </div>
        <div class="flex-spacer"></div>
        <div class="btn-group">
          <button class="ctl" title="后退 15 秒" :disabled="!item" @click="seekBy(-15)"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M3 8a9 9 0 1 0 2-5"/><path d="M3 3v5h5"/><text x="9" y="20" font-size="6" fill="currentColor" stroke="none">15</text></svg></button>
          <button class="ctl" title="前进 15 秒" :disabled="!item" @click="seekBy(15)"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8a9 9 0 1 1-2-5"/><path d="M21 3v5h-5"/><text x="9" y="20" font-size="6" fill="currentColor" stroke="none">15</text></svg></button>
        </div>
      </div>
    </div>
  </main>
</template>

<style scoped>
.stage {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: #000;
}

.stage-topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 16px;
  background: var(--bg-1);
  border-bottom: 1px solid var(--line);
}

.stage-title {
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.02em;
}

.toolbar {
  display: flex;
  gap: 2px;
}

.iconbtn {
  width: 32px;
  height: 32px;
  flex: 0 0 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--fg-1);
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.iconbtn:hover {
  background: var(--bg-2);
}

.iconbtn svg {
  width: 16px;
  height: 16px;
  flex: 0 0 16px;
  display: block;
  stroke: var(--fg-2);
}

.iconbtn:hover svg {
  stroke: var(--fg-1);
}

.canvas {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 0;
  overflow: hidden;
}

.empty {
  text-align: center;
  padding: 20px;
}

.empty-badge {
  width: 84px;
  height: 84px;
  margin: 0 auto 22px;
  border-radius: 20px;
  background: linear-gradient(160deg, var(--bg-2), var(--bg-1));
  border: 1px solid var(--line);
  display: flex;
  align-items: center;
  justify-content: center;
}

.empty-badge svg {
  width: 36px;
  height: 36px;
  color: var(--fg-3);
}

.empty-text {
  font-size: 20px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--fg-1);
}

.empty-sub {
  margin-top: 8px;
  font-size: 13px;
  color: var(--fg-3);
}

.open-btn {
  margin-top: 26px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 11px 24px;
  border: none;
  border-radius: 12px;
  background: var(--accent);
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s ease, transform 0.1s ease;
}

.open-btn:hover {
  opacity: 0.9;
}

.open-btn:active {
  transform: scale(0.97);
}

.open-btn svg {
  width: 16px;
  height: 16px;
}

.playing {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  width: 100%;
  padding: 16px;
}

.playing video {
  max-width: 100%;
  max-height: calc(100vh - 220px);
  border-radius: var(--radius-card);
  background: #000;
  outline: none;
}

.artwork {
  width: min(180px, 40vh);
  aspect-ratio: 1;
  border-radius: 18px;
  background: linear-gradient(160deg, var(--bg-2), var(--bg-1));
  border: 1px solid var(--line);
  display: flex;
  align-items: center;
  justify-content: center;
}

.artwork svg {
  width: 40%;
  height: 40%;
  color: var(--fg-3);
}

.now-title {
  color: var(--fg-1);
  font-size: 15px;
  font-weight: 500;
  text-align: center;
  max-width: 90%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.controls {
  padding: 10px 16px 14px;
  background: var(--bg-1);
  border-top: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.seek-row {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
}

.time {
  color: var(--fg-2);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  min-width: 40px;
  text-align: center;
}

.slider {
  flex: 1;
  appearance: none;
  height: 4px;
  border-radius: var(--radius-pill);
  background: var(--bg-2);
  border: none;
  outline: none;
  cursor: pointer;
}

.slider::-webkit-slider-thumb {
  appearance: none;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
}

.btn-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.btn-group {
  display: flex;
  align-items: center;
  gap: 6px;
}

.flex-spacer {
  flex: 1;
}

.ctl {
  width: 36px;
  height: 36px;
  flex: 0 0 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--fg-2);
  border-radius: 10px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.ctl:hover:not(:disabled) {
  background: var(--bg-2);
  color: var(--fg-1);
}

.ctl:disabled {
  opacity: 0.4;
  cursor: default;
}

.ctl svg {
  width: 18px;
  height: 18px;
  flex: 0 0 18px;
  display: block;
  stroke: var(--fg-2);
}

.ctl.play {
  width: 44px;
  height: 44px;
  border-radius: 50%;
  color: #fff;
  background: var(--accent);
}

.ctl.play:hover:not(:disabled) {
  background: var(--accent);
  opacity: 0.9;
}
</style>
