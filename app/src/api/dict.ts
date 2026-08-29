import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DictProgress, DictStatus, DictLookup } from "../types";

export function dictStatus(): Promise<DictStatus[]> {
  return invoke<DictStatus[]>("dict_status");
}

export function dictDownload(lang: string) {
  return invoke<void>("dict_download", { lang });
}

export function dictCancel(lang: string) {
  return invoke<boolean>("cancel_dict_download", { lang });
}

export function dictLookup(term: string): Promise<DictLookup[]> {
  return invoke<DictLookup[]>("dict_lookup", { term });
}

export function onDictStatus(cb: (s: DictStatus) => void): Promise<UnlistenFn> {
  return listen<DictStatus>("dict://status", (ev) => cb(ev.payload));
}

export function onDictProgress(cb: (p: DictProgress) => void): Promise<UnlistenFn> {
  return listen<DictProgress>("dict://progress", (ev) => cb(ev.payload));
}
