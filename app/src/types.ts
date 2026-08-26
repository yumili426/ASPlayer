export interface MediaItem {
  id: number;
  path: string;
  title: string;
  media_type: "video" | "audio";
  duration_ms: number;
  playback_position: number;
}
