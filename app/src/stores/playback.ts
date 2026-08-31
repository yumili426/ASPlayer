import { reactive, watch } from "vue";

export interface PlaybackSettings {
  autoplayNext: boolean; // 列表循环模式下播完自动连播下一集
  loopMode: "list" | "single"; // 循环模式：列表循环 / 单曲循环
  playbackMode: "broadcast" | "intensive"; // 全局播放模式（精听/连播）
  intensiveAutoPause: boolean; // 精听：每句结束自动暂停（默认开）
  intensiveSentenceLoop: boolean; // 精听：单句循环（默认关）
  intensiveBlindListen: boolean; // 精听：盲听（隐藏译文，默认关）
}

export const defaultPlayback: PlaybackSettings = {
  autoplayNext: true,
  loopMode: "list",
  playbackMode: "broadcast",
  intensiveAutoPause: true,
  intensiveSentenceLoop: false,
  intensiveBlindListen: false,
};

const STORAGE_KEY = "asplayer-playback-v1";

function load(): PlaybackSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      // 兼容旧版本：曾单独把 autoplayNext 存为布尔值
      if (typeof parsed === "boolean") {
        return { ...defaultPlayback, autoplayNext: parsed };
      }
      if (parsed && typeof parsed === "object") {
        return { ...defaultPlayback, ...(parsed as Partial<PlaybackSettings>) };
      }
    }
  } catch {
    /* ignore */
  }
  return { ...defaultPlayback };
}

// 共享播放设置（PlayerStage 控制栏 + SettingsPanel 配置共用），持久化到 localStorage
export const playback = reactive<PlaybackSettings>(load());

watch(
  playback,
  () => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(playback));
    } catch {
      /* ignore */
    }
  },
  { deep: true }
);

export function usePlayback() {
  return { playback, defaultPlayback };
}
