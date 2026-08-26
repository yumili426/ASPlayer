import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ProgressEvent, Subtitle } from "../types";

export function transcribeMedia(id: number, lang?: string) {
  return invoke<void>("transcribe_media", { id, ...(lang ? { lang } : {}) });
}

export function translateMedia(id: number) {
  return invoke<void>("translate_media", { id });
}

export function getSubtitles(id: number): Promise<Subtitle[]> {
  return invoke<Subtitle[]>("get_subtitles", { id });
}

export function getSubtitleStatus(id: number): Promise<[string, string]> {
  return invoke<[string, string]>("get_subtitle_status", { id });
}

export function saveSettings(settings: Record<string, string>) {
  return invoke<void>("save_settings", { settings });
}

export function getSettings(): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("get_settings");
}

export function onTranscribeProgress(
  cb: (e: ProgressEvent) => void
): Promise<UnlistenFn> {
  return listen<ProgressEvent>("transcribe://progress", (ev) => cb(ev.payload));
}

export function onTranscribeDone(cb: (mediaId: number) => void): Promise<UnlistenFn> {
  return listen<number>("transcribe://done", (ev) => cb(ev.payload));
}

export function onTranscribeError(cb: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("transcribe://error", (ev) => cb(ev.payload));
}
