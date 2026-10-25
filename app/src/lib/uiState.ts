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
  if (item.subtitle_status === "transcribing") {
    return { action: "cancel", label: "取消转写", disabled: false, progress };
  }
  if (item.subtitle_status === "translating") {
    return { action: "cancel", label: "取消翻译", disabled: false, progress };
  }
  if (item.subtitle_status === "error") {
    return { action: "retranscribe", label: "重新转写", disabled: false, progress: null };
  }
  if (item.subtitle_status === "partial") {
    return { action: "resume", label: "继续转写", disabled: false, progress: null };
  }
  if (subtitles.length > 0) {
    if (subtitles.some((s) => !s.translation.trim())) {
      return { action: "translate", label: "翻译", disabled: false, progress: null };
    }
    return { action: "ready", label: "已就绪", disabled: false, progress: null };
  }
  // 状态已非 none 但字幕为空，多半是读取失败；宁可显示就绪，也不重新转写覆盖已有字幕。
  if (item.subtitle_status === "none") {
    return { action: "transcribe", label: "转写并翻译", disabled: false, progress: null };
  }
  return { action: "ready", label: "已就绪", disabled: false, progress: null };
}

export function sortRecentMedia(items: MediaItem[]): MediaItem[] {
  // SQLite datetime('now') 输出定宽 YYYY-MM-DD HH:MM:SS（UTC），字典序即时间序。
  return items
    .filter(
      (m): m is MediaItem & { last_played_at: string } =>
        m.last_played_at != null && m.last_played_at !== ""
    )
    .sort((a, b) => {
      if (a.last_played_at === b.last_played_at) return b.id - a.id;
      return a.last_played_at < b.last_played_at ? 1 : -1;
    });
}
