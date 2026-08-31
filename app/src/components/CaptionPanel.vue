<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { Subtitle } from "../types";
import { useCaptionStyle } from "../stores/captionStyle";

const props = defineProps<{
  subtitles: Subtitle[];
  currentTime: number; // 秒
  status: string;
  override?: Subtitle | null; // 句末暂停时显示的句子（覆盖当前活动句）
  showEndActions?: boolean;
  blind?: boolean; // 盲听中（隐藏译文）
  reveal?: boolean; // 按住揭示译文
}>();
const emit = defineEmits<{ replay: []; next: [] }>();

const current = computed<Subtitle | null>(() => {
  const ms = props.currentTime * 1000;
  return props.subtitles.find((s) => ms >= s.start_ms && ms < s.end_ms) ?? null;
});

const cap = useCaptionStyle();
const mode = computed(() => cap.captionStyle.mode);
// 句末暂停显示覆盖句；否则跟随当前播放句
const line = computed<Subtitle | null>(() => props.override ?? current.value);
// 盲听按 H 揭示：临时切回原文态隐藏译文
const effMode = computed(() => {
  if (props.blind && !props.reveal) return "original" as const;
  return mode.value;
});

// “暂无字幕”空态可手动关闭；一旦出现字幕或进入转写/翻译/出错状态则重新展示
const dismissed = ref(false);
watch(
  [() => props.subtitles.length, () => props.status],
  ([n, s]) => {
    if (n > 0 || s !== "none") dismissed.value = false;
  }
);
const capVars = computed(() => ({
  "--cap-font": String(cap.captionStyle.fontScale),
  "--cap-color": cap.captionStyle.color,
  "--cap-bg": `rgba(0, 0, 0, ${cap.captionStyle.bgOpacity})`,
}));
</script>

<template>
  <div class="caption" :class="`pos-${cap.captionStyle.position}`" :style="capVars">
    <div v-if="status === 'error'" class="caption-empty">
      <p>字幕生成失败</p>
      <p class="sub">请检查模型/API 配置后重试</p>
    </div>

    <div v-else-if="subtitles.length === 0 && !dismissed && status !== 'transcribing' && status !== 'translating'" class="caption-empty">
      <p>暂无字幕</p>
      <p class="sub">可点击「转写」生成字幕</p>
      <button class="caption-dismiss" title="关闭提示" @click.stop="dismissed = true">×</button>
    </div>

    <template v-else>
      <p v-if="line" class="caption-line">
        <span v-if="effMode !== 'translation'" class="original">{{ line.text }}</span>
        <span v-if="effMode === 'bilingual' && line.translation" class="translated">{{ line.translation }}</span>
        <span v-if="effMode === 'translation'" class="original">{{ line.translation || line.text }}</span>
      </p>
      <p v-else class="caption-idle">…</p>
      <div v-if="showEndActions" class="caption-actions">
        <button class="ca-btn" @click.stop="emit('replay')">↺ 重听本句</button>
        <button class="ca-btn" @click.stop="emit('next')">→ 下一句</button>
      </div>
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

.caption-actions {
  display: flex;
  gap: 10px;
  margin-top: 12px;
  pointer-events: auto;
  justify-content: center;
}

.ca-btn {
  pointer-events: auto;
  padding: 6px 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--bg-1);
  color: var(--fg-1);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.12s ease, filter 0.12s ease;
}

.ca-btn:hover {
  background: var(--bg-2);
}

.caption-empty {
  position: relative;
  display: inline-flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 11px 34px 11px 16px;
  background: rgba(0, 0, 0, 0.55);
  border-radius: 10px;
  color: var(--fg-2);
  font-size: 13px;
  pointer-events: auto;
}

.caption-empty .sub {
  font-size: 12px;
  color: var(--fg-3);
  opacity: 0.85;
  margin-top: 2px;
}

.caption-dismiss {
  position: absolute;
  top: 5px;
  right: 7px;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fg-3);
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  pointer-events: auto;
  transition: background 0.12s ease, color 0.12s ease;
}

.caption-dismiss:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

</style>
