<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { MediaItem } from "../types";

const props = defineProps<{ item: MediaItem | null }>();
const emit = defineEmits<{ back: [] }>();

const mediaEl = ref<HTMLVideoElement | HTMLAudioElement | null>(null);
const playing = ref(false);
const currentTime = ref(0);
const duration = ref(0);

const src = computed(() =>
  props.item ? convertFileSrc(props.item.path) : ""
);

function fmt(t: number): string {
  if (!isFinite(t)) return "0:00";
  const m = Math.floor(t / 60);
  const s = Math.floor(t % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

// 恢复上次播放位置（挂载与切换媒体时）
function restorePosition() {
  const el = mediaEl.value;
  if (el && props.item && props.item.playback_position > 0) {
    el.currentTime = props.item.playback_position / 1000;
  }
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

function seekBy(deltaSec: number) {
  const el = mediaEl.value;
  if (!el) return;
  el.currentTime = Math.max(0, Math.min(el.duration || 0, el.currentTime + deltaSec));
}

function onSeekInput(e: Event) {
  const el = mediaEl.value;
  if (!el) return;
  el.currentTime = Number((e.target as HTMLInputElement).value);
}

// 节流保存播放位置（每 3 秒）
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
  <div class="player">
    <button class="ghost back" @click="emit('back')">← 返回媒体库</button>

    <div v-if="item" class="stage">
      <!-- 封面区（仅音频显示） -->
      <div v-if="item.media_type !== 'video'" class="artwork">
        <span class="note">♪</span>
      </div>
      <h2 class="title">{{ item.title }}</h2>

      <!-- 控制区 -->
      <div class="controls">
        <div class="seek-row">
          <span class="time">{{ fmt(currentTime) }}</span>
          <input
            class="slider"
            type="range"
            min="0"
            :max="duration || 0"
            step="0.1"
            :value="currentTime"
            @input="onSeekInput"
          />
          <span class="time">{{ fmt(duration) }}</span>
        </div>

        <div class="buttons">
          <button class="ctl" title="后退 15 秒" @click="seekBy(-15)">⟲ 15s</button>
          <button class="ctl play" :title="playing ? '暂停' : '播放'" @click="togglePlay">
            {{ playing ? "❚❚" : "▶" }}
          </button>
          <button class="ctl" title="前进 15 秒" @click="seekBy(15)">15s ⟳</button>
        </div>
      </div>

      <!-- 实际媒体元素（视频可见带原生控件 / 音频隐藏） -->
      <video
        v-if="item.media_type === 'video'"
        ref="mediaEl"
        :src="src"
        controls
        @play="playing = true"
        @pause="playing = false"
        @timeupdate="onTimeUpdate"
        @loadedmetadata="duration = ($event.target as HTMLVideoElement).duration"
      ></video>
      <audio
        v-else
        ref="mediaEl"
        :src="src"
        @play="playing = true"
        @pause="playing = false"
        @timeupdate="onTimeUpdate"
        @loadedmetadata="duration = ($event.target as HTMLAudioElement).duration"
      ></audio>
    </div>
  </div>
</template>

<style scoped>
.player {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 16px 24px;
}

.back {
  align-self: flex-start;
  background: transparent;
  border: none;
  color: var(--fg-2);
}

.stage {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  min-height: 0;
}

.artwork {
  margin-top: 4vh;
  width: min(300px, 40vw);
  aspect-ratio: 1;
  border-radius: 24px;
  background: linear-gradient(145deg, var(--bg-2), var(--bg-1));
  border: 1px solid var(--line);
  box-shadow: var(--shadow-card);
  display: flex;
  align-items: center;
  justify-content: center;
}

.note {
  font-size: 72px;
  color: var(--accent);
  opacity: 0.85;
}

.title {
  font-size: 18px;
  font-weight: 600;
  text-align: center;
  max-width: 80%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.controls {
  width: min(560px, 90%);
  padding-bottom: 3vh;
  display: flex;
  flex-direction: column;
  gap: 12px;
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
  min-width: 38px;
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
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
}

.buttons {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 22px;
}

.ctl {
  background: transparent;
  border: 1px solid var(--line);
  color: var(--fg-2);
  padding: 8px 14px;
  font-size: 13px;
}

.play {
  width: 58px;
  height: 58px;
  padding: 0;
  border-radius: 50%;
  font-size: 18px;
  color: var(--accent);
  background: var(--accent-dim);
  border-color: var(--accent);
}

video {
  width: 100%;
  max-width: 760px;
  border-radius: var(--radius-card);
  background: #000;
}
</style>

