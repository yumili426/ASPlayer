import { reactive } from "vue";
import { listen } from "@tauri-apps/api/event";
import { getOverlayPrefs, setOverlayPrefs } from "../api/overlay";
import type { OverlayPrefs } from "../types";

/** 预设色枚举 → 实际渲染色值（低饱和方向，配 Quiet Glass） */
export const TRANSLATION_HEX = {
  "soft-white": "rgba(255,255,255,0.72)",
  amber: "#e5b389",
  rose: "#ff9fb2",
  "mist-blue": "#9dc4f0",
  mint: "#9fe0c6",
  lavender: "#c9aef0",
} as const;

export const overlayPrefs = reactive<OverlayPrefs>({
  display_mode: "bilingual",
  trans_color: "soft-white",
  gap_behavior: "keep-last",
  font_scale: 1,
});

/** 启动时调用一次：读后端持久化值（失败保持默认） */
export async function loadOverlayPrefs(): Promise<void> {
  try {
    Object.assign(overlayPrefs, await getOverlayPrefs());
  } catch {
    /* 后端不可达时静默用默认值，不打断窗口加载 */
  }
}

/** 本地乐观更新 + 后端持久化；后端回声事件与本地位收敛为同值，无竞态风险 */
export function patchOverlayPrefs(patch: Partial<OverlayPrefs>): void {
  Object.assign(overlayPrefs, patch);
  setOverlayPrefs({ ...overlayPrefs }).catch(() => {});
}

/** 订阅另一窗口发起的变更（由 Rust 统一广播） */
export async function watchOverlayPrefs(): Promise<() => void> {
  const un = await listen<Partial<OverlayPrefs>>("overlay://prefs-changed", (e) => {
    if (e.payload) Object.assign(overlayPrefs, e.payload);
  });
  return un;
}
