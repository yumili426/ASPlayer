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

/** 悬浮窗工具栏：上/下一句 */
export function stepOverlaySubtitle(delta: number) {
  return invoke<void>("step_overlay_subtitle", { delta });
}

/** 悬浮窗工具栏：播放/暂停（转发主窗） */
export function overlayPlayPause() {
  return invoke<void>("overlay_control", { action: "togglePlay" });
}

/** 读/写悬浮窗偏好（结构体整体写入，Rust 端 serde 缺省补齐） */
export function getOverlayPrefs() {
  return invoke<import("../types").OverlayPrefs>("get_overlay_prefs");
}

export function setOverlayPrefs(prefs: import("../types").OverlayPrefs) {
  return invoke<void>("set_overlay_prefs", { prefs });
}
