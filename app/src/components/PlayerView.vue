<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { MediaItem } from "../types";

const props = defineProps<{ item: MediaItem | null }>();
const emit = defineEmits<{ back: [] }>();

const mediaEl = ref<HTMLVideoElement | HTMLAudioElement | null>(null);
const playing = ref(false);
const currentTime = ref(0);

const src = computed(() =>
  props.item ? convertFileSrc(props.item.path) : ""
);

// 挂载后跳到上次播放位置
onMounted(() => {
  const el = mediaEl.value;
  if (el && props.item && props.item.playback_position > 0) {
    el.currentTime = props.item.playback_position / 1000;
  }
});

watch(
  () => props.item?.id,
  () => {
    // 切换媒体：等元素刷新后恢复位置
    requestAnimationFrame(() => {
      const el = mediaEl.value;
      if (el && props.item && props.item.playback_position > 0) {
        el.currentTime = props.item.playback_position / 1000;
      }
    });
  }
);

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
    <button class="ghost back" @click="emit('back')">← 返回</button>

    <div v-if="item" class="stage">
      <video
        v-if="item.media_type === 'video'"
        ref="mediaEl"
        :src="src"
        controls
        @play="playing = true"
        @pause="playing = false"
        @timeupdate="onTimeUpdate"
      ></video>
      <audio
        v-else
        ref="mediaEl"
        :src="src"
        @play="playing = true"
        @pause="playing = false"
        @timeupdate="onTimeUpdate"
      ></audio>

      <h2 class="title">{{ item.title }}</h2>
      <p class="hint">
        双语字幕面板将在 M2 到来——先享受无干扰播放 ♪（当前 {{ Math.floor(currentTime) }}s）
      </p>
    </div>
  </div>
</template>

<style scoped>
.player {
  padding: 16px 20px;
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.back {
  align-self: flex-start;
  background: transparent;
  border: none;
  color: var(--fg-2);
}

.stage {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

video,
audio {
  width: 100%;
  border-radius: var(--radius-card);
  background: #000;
  outline: none;
}

.title {
  font-size: 18px;
  font-weight: 600;
}

.hint {
  color: var(--fg-3);
}
</style>
