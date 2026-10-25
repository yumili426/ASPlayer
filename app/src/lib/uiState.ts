import type { MediaItem, Subtitle } from "../types";

export type PrimaryAction =
  | "none"
  | "transcribe"
  | "retranscribe"
  | "resume"
  | "cancel"
  | "translate"
  | "ready";

export interface PrimaryActionState {
  action: PrimaryAction;
  label: string;
  disabled: boolean;
  progress: number | null;
}

export function resolvePrimaryAction(
  item: MediaItem | null,
  subtitles: Pick<Subtitle, "translation">[],
  progress: number | null
): PrimaryActionState {
  if (!item) {
    return { action: "none", label: "转写并翻译", disabled: true, progress: null };
  }
  if (item.subtitle_status === "transcribing" || item.subtitle_status === "translating") {
    return { action: "cancel", label: "取消转写", disabled: false, progress };
  }
  if (item.subtitle_status === "error") {
    return { action: "retranscribe", label: "重新转写", disabled: false, progress: null };
  }
  if (item.subtitle_status === "partial") {
    return { action: "resume", label: "继续转写", disabled: false, progress: null };
  }
  if (subtitles.length === 0) {
    return { action: "transcribe", label: "转写并翻译", disabled: false, progress: null };
  }
  if (subtitles.some((s) => !s.translation.trim())) {
    return { action: "translate", label: "翻译", disabled: false, progress: null };
  }
  return { action: "ready", label: "已就绪", disabled: false, progress: null };
}

export function sortRecentMedia(items: MediaItem[]): MediaItem[] {
  return items
    .filter((m) => m.last_played_at)
    .slice()
    .sort((a, b) => {
      const at = a.last_played_at ?? "";
      const bt = b.last_played_at ?? "";
      if (at === bt) return b.id - a.id;
      return at < bt ? 1 : -1;
    });
}
