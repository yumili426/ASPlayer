import { reactive, watch } from "vue";
import type { CaptionStyle } from "../types";

export const defaultCaptionStyle: CaptionStyle = {
  fontScale: 1, // 0.8 ~ 1.6
  color: "#ffffff",
  bgOpacity: 0.35, // 0 ~ 0.85
  position: "bottom", // top | center | bottom
};

const STORAGE_KEY = "asplayer-caption-style-v1";

function load(): CaptionStyle {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...defaultCaptionStyle, ...JSON.parse(raw) };
  } catch {
    /* ignore */
  }
  return { ...defaultCaptionStyle };
}

// 共享字幕样式（CaptionPanel 浮层 + SettingsPanel 配置共用）
export const captionStyle = reactive<CaptionStyle>(load());

export function updateCaptionStyle(patch: Partial<CaptionStyle>) {
  Object.assign(captionStyle, patch);
}

export function resetCaptionStyle() {
  Object.assign(captionStyle, defaultCaptionStyle);
}

// 持久化到 localStorage
watch(
  captionStyle,
  () => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(captionStyle));
    } catch {
      /* ignore */
    }
  },
  { deep: true }
);

export function useCaptionStyle() {
  return { captionStyle, updateCaptionStyle, resetCaptionStyle, defaultCaptionStyle };
}
