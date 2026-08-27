import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ModelProgress, ModelStatus } from "../types";

export function getModelsStatus(): Promise<ModelStatus[]> {
  return invoke<ModelStatus[]>("get_models_status");
}

export function downloadModel(size: string) {
  return invoke<void>("download_model", { size });
}

export function cancelModelDownload(size: string) {
  return invoke<boolean>("cancel_model_download", { size });
}

export function setModel(size: string) {
  return invoke<void>("set_model", { size });
}

export function removeModel(size: string) {
  return invoke<void>("remove_model", { size });
}

export function onModelProgress(cb: (e: ModelProgress) => void): Promise<UnlistenFn> {
  return listen<ModelProgress>("model://progress", (ev) => cb(ev.payload));
}

export function onModelDone(cb: (size: string) => void): Promise<UnlistenFn> {
  return listen<string>("model://done", (ev) => cb(ev.payload));
}

export function onModelError(
  cb: (e: { size: string; error: string }) => void
): Promise<UnlistenFn> {
  return listen<{ size: string; error: string }>("model://error", (ev) => cb(ev.payload));
}

export function onModelCanceled(cb: (size: string) => void): Promise<UnlistenFn> {
  return listen<string>("model://canceled", (ev) => cb(ev.payload));
}

export function onModelSelected(cb: (size: string) => void): Promise<UnlistenFn> {
  return listen<string>("model://selected", (ev) => cb(ev.payload));
}
