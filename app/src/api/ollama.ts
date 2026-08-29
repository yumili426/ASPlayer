import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { OllamaProgress, OllamaStatus, PullState } from "../types";

export function ollamaStatus(): Promise<OllamaStatus> {
  return invoke<OllamaStatus>("ollama_status");
}

export function ollamaPull(model: string) {
  return invoke<void>("ollama_pull", { model });
}

export function ollamaPullCancel() {
  return invoke<boolean>("cancel_ollama_pull");
}

export function onOllamaStatus(cb: (s: PullState) => void): Promise<UnlistenFn> {
  return listen<PullState>("ollama://status", (ev) => cb(ev.payload));
}

export function onOllamaProgress(cb: (p: OllamaProgress) => void): Promise<UnlistenFn> {
  return listen<OllamaProgress>("ollama://progress", (ev) => cb(ev.payload));
}
