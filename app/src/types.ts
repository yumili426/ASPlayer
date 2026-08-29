export interface MediaItem {
  id: number;
  path: string;
  title: string;
  media_type: "video" | "audio";
  duration_ms: number;
  playback_position: number;
  file_size: number;
  subtitle_status: "none" | "transcribing" | "done" | "error" | "translating" | "partial";
  subtitle_lang: string;
  subtitle_count: number;
  transcribe_next_ms: number;
  speed: number;
  volume: number;
}

export interface Subtitle {
  start_ms: number;
  end_ms: number;
  text: string;
  translation: string;
  ordinal: number;
}

export interface ProgressEvent {
  mediaId: number;
  stage: string;
  progress: number;
  message: string;
}

export type CaptionPosition = "top" | "center" | "bottom";
export type CaptionMode = "original" | "bilingual" | "translation";

export type OverlayDisplayMode = "original" | "bilingual" | "translation";
export type OverlayGapBehavior = "keep-last" | "fade-5s";
export const OVERLAY_PRESET_COLORS = [
  "soft-white", "amber", "rose", "mist-blue", "mint", "lavender",
] as const;
export type OverlayPresetColor = (typeof OVERLAY_PRESET_COLORS)[number];

/** 悬浮窗偏好（后端 settings KV 单键持久化，修改即双窗广播） */
export interface OverlayPrefs {
  display_mode: OverlayDisplayMode;
  trans_color: OverlayPresetColor;
  gap_behavior: OverlayGapBehavior;
  font_scale: number; // 0.8 ~ 2.0
}

export interface CaptionStyle {
  fontScale: number; // 0.8 ~ 1.6
  color: string; // 主文本色（十六进制）
  bgOpacity: number; // 0 ~ 0.85 背景不透明度
  position: CaptionPosition; // 字幕位置
  mode: CaptionMode; // 字幕显示模式：原文 / 双语 / 译文
}

export type ShortcutActionName =
  | "togglePlay"
  | "seekBack"
  | "seekForward"
  | "volumeUp"
  | "volumeDown"
  | "mute"
  | "fullscreen"
  | "nextSubtitle"
  | "prevSubtitle"
  | "togglePlaylist"
  | "toggleSubtitle"
  | "openSettings";

export interface ShortcutBinding {
  action: ShortcutActionName;
  keys: string; // normalize 后的组合，如 "Space"、"ArrowLeft"、"Mod+KeyL"、"KeyJ"
}

export interface ModelStatus {
  size: string;
  file_exists: boolean;
  file_bytes: number;
  selected: boolean;
  status: "downloading" | "done" | "failed" | "canceled" | "idle";
  bytes_downloaded: number;
  total_bytes: number;
  error: string | null;
}

export interface ModelProgress {
  size: string;
  bytes_downloaded: number;
  total_bytes: number;
  percent: number;
}

export interface DictStatus {
  lang: string;
  raw_exists: boolean;
  raw_bytes: number;
  db_exists: boolean;
  db_bytes: number;
  status: "idle" | "downloading" | "done" | "failed" | "canceled";
  error: string | null;
}

export interface DictLookup {
  term: string;
  lang: string;
  phonetic: string | null;
  reading: string | null;
  pos: string | null;
  definitions: string[];
  suggestions: string[];
}

export interface DictProgress {
  lang: string;
  bytes_downloaded: number;
  total_bytes: number;
  percent: number;
}
