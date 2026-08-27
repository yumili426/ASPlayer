export interface MediaItem {
  id: number;
  path: string;
  title: string;
  media_type: "video" | "audio";
  duration_ms: number;
  playback_position: number;
  file_size: number;
  subtitle_status: "none" | "transcribing" | "done" | "error" | "translating";
  subtitle_lang: string;
  subtitle_count: number;
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
