import { ref } from "vue";
import type { ShortcutActionName, ShortcutBinding } from "../types";

export interface ShortcutMeta {
  name: ShortcutActionName;
  label: string;
}

export const shortcutActions: ShortcutMeta[] = [
  { name: "togglePlay", label: "播放 / 暂停" },
  { name: "seekBack", label: "后退 15 秒" },
  { name: "seekForward", label: "前进 15 秒" },
  { name: "volumeUp", label: "音量 +" },
  { name: "volumeDown", label: "音量 -" },
  { name: "mute", label: "静音" },
  { name: "fullscreen", label: "全屏" },
  { name: "nextSubtitle", label: "下一句字幕" },
  { name: "prevSubtitle", label: "上一句字幕" },
  { name: "togglePlaylist", label: "切换播放列表" },
  { name: "toggleSubtitle", label: "切换字幕面板" },
  { name: "openSettings", label: "打开设置" },
  { name: "togglePlaybackMode", label: "切换连播 / 精听" },
  { name: "repeatSubtitle", label: "重听本句" },
  { name: "toggleSentenceLoop", label: "单句循环开关" },
];

const STORAGE_KEY = "asplayer-shortcuts-v1";

const defaults: ShortcutBinding[] = [
  { action: "togglePlay", keys: "Space" },
  { action: "seekBack", keys: "ArrowLeft" },
  { action: "seekForward", keys: "ArrowRight" },
  { action: "volumeUp", keys: "ArrowUp" },
  { action: "volumeDown", keys: "ArrowDown" },
  { action: "mute", keys: "KeyM" },
  { action: "fullscreen", keys: "KeyF" },
  { action: "nextSubtitle", keys: "KeyJ" },
  { action: "prevSubtitle", keys: "KeyK" },
  { action: "togglePlaylist", keys: "Mod+KeyL" },
  { action: "toggleSubtitle", keys: "Mod+KeyT" },
  { action: "openSettings", keys: "Mod+Comma" },
  { action: "togglePlaybackMode", keys: "Mod+Alt+KeyS" },
  { action: "repeatSubtitle", keys: "KeyR" },
  { action: "toggleSentenceLoop", keys: "Mod+Alt+KeyL" },
];

function load(): ShortcutBinding[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const saved = JSON.parse(raw) as ShortcutBinding[];
      const map = new Map(saved.map((s) => [s.action, s.keys]));
      return defaults.map((d) => ({ ...d, keys: map.get(d.action) ?? d.keys }));
    }
  } catch {
    /* ignore */
  }
  return defaults.map((d) => ({ ...d }));
}

// 共享快捷键状态（App 全局监听 + 设置面板自定义共用）
const shortcuts = ref<ShortcutBinding[]>(load());
const recording = ref<ShortcutActionName | null>(null);

function persist() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(shortcuts.value));
  } catch {
    /* ignore */
  }
}

function setShortcut(action: ShortcutActionName, keys: string) {
  const b = shortcuts.value.find((s) => s.action === action);
  if (b) {
    b.keys = keys;
    persist();
  }
}

function clearShortcut(action: ShortcutActionName) {
  const b = shortcuts.value.find((s) => s.action === action);
  if (b) {
    b.keys = "";
    persist();
  }
}

function resetShortcuts() {
  shortcuts.value = defaults.map((d) => ({ ...d }));
  persist();
}

// 把 KeyboardEvent 归一化成可比较的键串（Mod 统一 Ctrl/Cmd）
function normalizeKey(e: KeyboardEvent): string {
  const mods: string[] = [];
  if (e.ctrlKey || e.metaKey) mods.push("Mod");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  const code = e.code;
  return mods.length ? `${mods.join("+")}+${code}` : code;
}

const CODE_LABELS: Record<string, string> = {
  Space: "空格",
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
  Enter: "Enter",
  Escape: "Esc",
  Tab: "Tab",
  Backspace: "⌫",
};

function codeLabel(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return CODE_LABELS[code] ?? code;
}

function keysLabel(keys: string): string {
  if (!keys) return "未绑定";
  return keys
    .split("+")
    .map((p) => (p === "Mod" ? "Ctrl/Cmd" : codeLabel(p)))
    .join(" + ");
}

export function useShortcuts() {
  return {
    shortcuts,
    shortcutActions,
    recording,
    setShortcut,
    clearShortcut,
    resetShortcuts,
    normalizeKey,
    keysLabel,
  };
}
