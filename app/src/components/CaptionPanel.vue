<script setup lang="ts">
import { computed } from "vue";
import type { Subtitle } from "../types";
import { useCaptionStyle } from "../stores/captionStyle";

const props = defineProps<{
  subtitles: Subtitle[];
  currentTime: number; // 秒
  status: string;
}>();

const current = computed<Subtitle | null>(() => {
  const ms = props.currentTime * 1000;
  return props.subtitles.find((s) => ms >= s.start_ms && ms < s.end_ms) ?? null;
});

const cap = useCaptionStyle();
const capVars = computed(() => ({
  "--cap-font": String(cap.captionStyle.fontScale),
  "--cap-color": cap.captionStyle.color,
  "--cap-bg": `rgba(0, 0, 0, ${cap.captionStyle.bgOpacity})`,
}));
</script>

<template>
  <div class="caption" :class="`pos-${cap.captionStyle.position}`" :style="capVars">
    <div v-if="status === 'transcribing' || status === 'translating'" class="caption-pending">
      <span class="spinner"></span>
      <span>{{ status === "translating" ? "正在翻译字幕…" : "正在转写字幕…" }}</span>
    </div>

    <div v-else-if="status === 'error'" class="caption-empty">
      <p>字幕生成失败</p>
      <p class="sub">请检查模型/API 配置后重试</p>
    </div>

    <div v-else-if="subtitles.length === 0" class="caption-empty">
      <p>暂无字幕</p>
      <p class="sub">可点击「转写」生成字幕</p>
    </div>

    <template v-else>
      <p v-if="current" class="caption-line">
        <span class="original">{{ current.text }}</span>
        <span v-if="current.translation" class="translated">{{ current.translation }}</span>
      </p>
      <p v-else class="caption-idle">…</p>
    </template>
  </div>
</template>

<style scoped>
.caption {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  max-width: 82%;
  text-align: center;
  pointer-events: none;
  z-index: 20;
}

.caption.pos-bottom {
  bottom: 22px;
}

.caption.pos-top {
  top: 22px;
}

.caption.pos-center {
  top: 50%;
  transform: translate(-50%, -50%);
}

.caption-line {
  display: flex;
  flex-direction: column;
  gap: 4px;
  align-items: center;
  background: var(--cap-bg, rgba(0, 0, 0, 0.35));
  padding: 8px 14px;
  border-radius: 10px;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.25);
}

.original {
  color: var(--cap-color, #fff);
  font-size: calc(19px * var(--cap-font, 1));
  font-weight: 500;
  text-shadow: 0 2px 10px rgba(0, 0, 0, 0.9);
  max-width: 100%;
  line-height: 1.35;
  word-break: break-word;
}

.translated {
  color: var(--cap-color, #cdd3db);
  opacity: 0.85;
  font-size: calc(15px * var(--cap-font, 1));
  text-shadow: 0 2px 8px rgba(0, 0, 0, 0.9);
  max-width: 100%;
  line-height: 1.35;
  word-break: break-word;
}

.caption-idle {
  color: rgba(255, 255, 255, 0.35);
  font-size: 18px;
}

.caption-empty {
  color: var(--fg-3);
  font-size: 13px;
}

.caption-empty .sub {
  font-size: 12px;
  color: var(--fg-3);
  opacity: 0.8;
  margin-top: 2px;
}

.caption-pending {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--fg-2);
  font-size: 14px;
  background: rgba(0, 0, 0, 0.5);
  padding: 8px 16px;
  border-radius: 10px;
}

.spinner {
  width: 14px;
  height: 14px;
  border: 2px solid var(--fg-3);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
