import { ref, watch } from "vue";

export const defaultAutoplayNext = true;
const STORAGE_KEY = "asplayer-playback-v1";

function load(): boolean {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw != null) return JSON.parse(raw) as boolean;
  } catch {
    /* ignore */
  }
  return defaultAutoplayNext;
}

// 自动播放：列表循环模式下当前集播完后是否自动连播下一集
// （关闭后即达到"播完即停、不循环"的效果）
export const autoplayNext = ref<boolean>(load());

watch(autoplayNext, (v) => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(v));
  } catch {
    /* ignore */
  }
});

export function usePlayback() {
  return { autoplayNext, defaultAutoplayNext };
}
