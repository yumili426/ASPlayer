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

/** 迷你悬浮窗配置（后端 SQLite settings 单键持久化，修改即广播） */
export interface OverlayConfig {
  visible: boolean;
  fontSize: number; // 14 ~ 48
  maxLines: number; // 1 ~ 3（原文最大可见行数）
  backdrop: boolean; // 文字底衬（极淡黑色圆角条），默认关闭
  mode: CaptionMode; // 悬浮窗独立的字幕显示模式记忆
}

/** 当前句推送给悬浮窗的数据 */
export interface OverlayCurrent {
  ordinal: number;
  startMs: number;
  endMs: number;
  text: string;
  translation: string;
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
