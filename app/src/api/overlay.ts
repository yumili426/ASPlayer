import { invoke } from "@tauri-apps/api/core";

/** M3 悬浮字幕窗相关命令的前端封装 */

export function setOverlayVisible(visible: boolean) {
  return invoke<void>("set_overlay_visible", { visible });
}

export function toggleOverlayVisible() {
  return invoke<void>("toggle_overlay_visible");
}

export function isOverlayVisible() {
  return invoke<boolean>("is_overlay_visible");
}

export function setOverlayLocked(locked: boolean) {
  return invoke<void>("set_overlay_locked", { locked });
}

export function isOverlayLocked() {
  return invoke<boolean>("is_overlay_locked");
}

/** 主窗推送当前句到悬浮窗 */
export function pushOverlaySubtitle(text: string, translation: string, startMs: number) {
  return invoke<void>("push_overlay_subtitle", { text, translation, startMs });
}
