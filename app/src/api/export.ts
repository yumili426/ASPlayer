import { invoke } from "@tauri-apps/api/core";

export type ExportFormat = "srt" | "vtt" | "txt";
export type ExportContent = "original" | "translation" | "bilingual";

export function exportSubtitles(
  id: number,
  path: string,
  format: ExportFormat,
  content: ExportContent
): Promise<number> {
  return invoke<number>("export_subtitles", { id, path, format, content });
}
