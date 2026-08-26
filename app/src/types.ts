export interface MediaItem {
  id: number;
  path: string;
  title: string;
  media_type: "video" | "audio";
  duration_ms: number;
  playback_position: number;
  subtitle_status: "none" | "transcribing" | "done" | "error" | "translating";
  subtitle_lang: string;
  subtitle_count: number;
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
