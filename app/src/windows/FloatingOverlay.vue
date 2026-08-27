<script setup lang="ts">
/**
 * M3 悬浮字幕窗 · 桌面歌词化重写（设计文档 2026-08-27 §2/§3）
 * - Quiet Glass 玻璃条：原文大字白 + 译文预设色，整条可拖拽（锁定态除外）
 * - 悬停工具栏：⏮⏯⏭ / 显示模式三态 / ⚙就地设置 / 锁定 / 关闭
 * - 显示模式在本窗渲染取舍：主窗恒推 原文+译文 全量，切换零通信
 * - 字幕数据来源：后端中继 overlay://subtitle（由主窗 overlayFeed 推送）
 */
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  TRANSLATION_HEX,
  loadOverlayPrefs,
  overlayPrefs,
  patchOverlayPrefs,
  watchOverlayPrefs,
} from "../stores/overlayPrefs";
import { overlayPlayPause, stepOverlaySubtitle } from "../api/overlay";
import type { OverlayDisplayMode } from "../types";

interface SubtitlePayload {
  text: string;
  translation: string;
  start_ms: number;
}

const appWindow = getCurrentWindow();
const text = ref("");
const translation = ref("");
const startMs = ref(0);
const locked = ref(false);
const tbVisible = ref(false);   // 工具栏可见性（悬停 2s 延迟隐藏）
const panelOpen = ref(false);   // ⚙迷你面板
const chipVisible = ref(false); // 锁定态悬停时浮现的"解锁"按钮（Rust 悬停探测驱动）
const receivedFirst = ref(false); // 是否收到过第一条真句（决定提示条显隐）
const unlisteners: (() => void)[] = [];

let hideTimer: number | null = null;

/** 按显示模式取舍渲染行；缺译文回退原文，绝不空白 */
const lines = computed<{ cls: "orig" | "trans"; text: string }[]>(() => {
  const o = text.value.trim();
  const tr = translation.value.trim();
  if (overlayPrefs.display_mode === "original") return o ? [{ cls: "orig", text: o }] : [];
  if (overlayPrefs.display_mode === "translation") {
    const t = tr || o;
    return t ? [{ cls: "trans", text: t }] : [];
  }
  const out: { cls: "orig" | "trans"; text: string }[] = [];
  if (o) out.push({ cls: "orig", text: o });
  if (tr && tr !== o) out.push({ cls: "trans", text: tr });
  return out;
});

const transColor = computed(
  () => TRANSLATION_HEX[overlayPrefs.trans_color] ?? TRANSLATION_HEX["soft-white"]
);
const origSize = computed(() => `${22 * overlayPrefs.font_scale}px`);
const transSize = computed(() => `${16 * overlayPrefs.font_scale}px`);

// ---- 手势与工具栏 ----

/** 整条玻璃卡拖拽（点击跳转功能已按设计移除） */
function onDragStart(e: PointerEvent) {
  if (locked.value || e.button !== 0) return;
  appWindow.startDragging().catch(() => {});
}

function tbShow() {
  if (locked.value) return;
  tbVisible.value = true;
  if (hideTimer !== null) window.clearTimeout(hideTimer);
}

function tbHide() {
  if (hideTimer !== null) window.clearTimeout(hideTimer);
  hideTimer = window.setTimeout(() => {
    tbVisible.value = false;
    panelOpen.value = false;
  }, 2000);
}

function setMode(m: OverlayDisplayMode) {
  patchOverlayPrefs({ display_mode: m });
}

function lockOverlay() {
  invoke("set_overlay_locked", { locked: true }).catch(() => {});
}

/** 锁定态下点击悬停浮现的解锁钮 */
function unlockOverlay() {
  invoke("set_overlay_locked", { locked: false }).catch(() => {});
}

function closeOverlay() {
  invoke("set_overlay_visible", { visible: false }).catch(() => {});
}

const MODE_ITEMS: { key: OverlayDisplayMode; label: string }[] = [
  { key: "original", label: "原文" },
  { key: "bilingual", label: "双语" },
  { key: "translation", label: "译文" },
];

onMounted(async () => {
  try {
    unlisteners.push(
      await listen<SubtitlePayload>("overlay://subtitle", (e) => {
        const p = e.payload ?? { text: "", translation: "", start_ms: 0 };
        if (p.text || p.translation) receivedFirst.value = true;
        text.value = p.text ?? "";
        translation.value = p.translation ?? "";
        startMs.value = p.start_ms ?? 0;
      }),
      await listen<boolean>("overlay://lock-changed", (e) => {
        locked.value = !!e.payload;
        if (locked.value) {
          tbVisible.value = false;
          panelOpen.value = false;
          chipVisible.value = false;
        }
      }),
      await listen<boolean>("overlay://hover-unlock", (e) => {
        chipVisible.value = !!e.payload && locked.value;
      }),
      await watchOverlayPrefs()
    );
  } catch (err) {
    console.error("[overlay] 事件监听注册失败:", err);
  }
  await loadOverlayPrefs();
  try {
    locked.value = await invoke<boolean>("is_overlay_locked");
  } catch {
    /* 忽略 */
  }
});

onUnmounted(() => {
  unlisteners.forEach((u) => u());
  if (hideTimer !== null) window.clearTimeout(hideTimer);
});
</script>

<template>
  <div
    class="overlay-root"
    :class="{ locked, picked: locked && chipVisible }"
    @mouseenter="tbShow"
    @mouseleave="tbHide"
  >
    <!-- 从未收到过字幕时的引导提示 -->
    <div v-if="!receivedFirst" class="boot-hint">开始播放后此处显示字幕</div>

    <!-- 玻璃歌词条 -->
    <div
      v-else
      class="glass"
      :class="{ dragging: !locked }"
      @pointerdown="onDragStart"
    >
      <!-- 锁定态悬停解锁钮：穿透解除由 Rust 侧悬停探测临时开启 -->
      <button
        v-show="locked && chipVisible"
        class="unlock-chip"
        title="点击解锁悬浮字幕窗"
        @pointerdown.stop
        @click.stop="unlockOverlay"
      >🔓 解锁</button>
      <!-- 悬停工具栏：交互区阻断拖拽冒泡 -->
      <div
        v-show="tbVisible"
        class="tb"
        @pointerdown.stop
      >
        <button class="tbtn" title="上一句" @click="stepOverlaySubtitle(-1)">⏮</button>
        <button class="tbtn" title="播放/暂停" @click="overlayPlayPause">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M7 5l12 7-12 7z"/></svg>
        </button>
        <button class="tbtn" title="下一句" @click="stepOverlaySubtitle(1)">⏭</button>
        <span class="sep"></span>
        <div class="seg">
          <button
            v-for="m in MODE_ITEMS"
            :key="m.key"
            class="seg-btn"
            :class="{ on: overlayPrefs.display_mode === m.key }"
            @click="setMode(m.key)"
          >{{ m.label }}</button>
        </div>
        <span class="sep"></span>
        <div
          v-show="panelOpen"
          class="panel"
          @pointerdown.stop
        >
          <div class="panel-title">译文颜色</div>
          <div class="swatches">
            <button
              v-for="(hex, key) in TRANSLATION_HEX"
              :key="key"
              class="swatch"
              :class="{ on: overlayPrefs.trans_color === key }"
              :style="{ background: hex }"
              :title="key"
              @click="patchOverlayPrefs({ trans_color: key })"
            ></button>
          </div>
          <div class="panel-title">句间空隙</div>
          <select
            class="sel"
            :value="overlayPrefs.gap_behavior"
            @change="patchOverlayPrefs({ gap_behavior: ($event.target as HTMLSelectElement).value as 'keep-last' | 'fade-5s' })"
          >
            <option value="keep-last">保留上一句</option>
            <option value="fade-5s">5 秒后淡出</option>
          </select>
          <div class="panel-title">字号 {{ Math.round(overlayPrefs.font_scale * 100) }}%</div>
          <input
            class="font-range"
            type="range" min="0.8" max="2" step="0.05"
            :value="overlayPrefs.font_scale"
            @change="patchOverlayPrefs({ font_scale: Number(($event.target as HTMLInputElement).value) })"
          />
        </div>
        <button class="tbtn" title="设置" @click="panelOpen = !panelOpen">⚙</button>
        <button class="tbtn" title="锁定（鼠标穿透，Ctrl+Alt+L 解锁）" @click="lockOverlay">🔒</button>
        <button class="tbtn danger" title="关闭悬浮字幕窗" @click="closeOverlay">✕</button>
      </div>

      <Transition name="linefade" mode="out-in">
        <div v-if="lines.length" :key="startMs" class="lines">
          <p
            v-for="(l, i) in lines"
            :key="i"
            class="line"
            :class="l.cls"
            :style="l.cls === 'orig'
              ? { fontSize: origSize }
              : { fontSize: transSize, color: transColor }"
          >{{ l.text }}</p>
        </div>
        <div v-else :key="'clear'" class="cleared"></div>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.overlay-root {
  width: 100vw;
  height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  overflow: hidden;
  user-select: none;
}

/* 网易云式形态：平时纯文字悬浮于透明底，悬停才浮出毛玻璃条；锁定态不可 hover 自动全透明 */
.glass {
  width: calc(100vw - 28px);
  max-height: calc(100vh - 16px);
  border-radius: 18px;
  padding: 34px 20px 18px;
  cursor: grab;
  transition:
    background-color 0.16s ease,
    box-shadow 0.16s ease;
}
/* Quiet Glass 玻璃条：毛玻璃不生效时自然降级为半透明深底（观感相近） */
.overlay-root:not(.locked) .glass:hover {
  background: rgba(12, 14, 20, 0.55);
  backdrop-filter: blur(16px) saturate(130%);
  outline: 1px solid rgba(255, 255, 255, 0.09);
  box-shadow: 0 10px 36px rgba(0, 0, 0, 0.45);
}
.glass.dragging:active {
  cursor: grabbing;
}
.overlay-root.locked .glass {
  cursor: default;
}

/* ---- 悬停工具栏 ---- */
.tb {
  position: absolute;
  top: 8px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 6px;
  border-radius: 11px;
  background: rgba(255, 255, 255, 0.07);
  white-space: nowrap;
  z-index: 3;
}
.overlay-root.locked .tb {
  display: none;
}
.tbtn {
  width: 26px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: rgba(235, 235, 240, 0.85);
  font-size: 12px;
  cursor: pointer;
}
.tbtn:hover {
  background: rgba(255, 255, 255, 0.14);
  color: #fff;
}
.tbtn.danger:hover {
  background: #e5484d;
  color: #fff;
}
.tbtn svg {
  width: 12px;
  height: 12px;
}
.sep {
  width: 1px;
  height: 14px;
  background: rgba(255, 255, 255, 0.14);
}
.seg {
  display: flex;
  gap: 2px;
  padding: 2px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.08);
}
.seg-btn {
  border: none;
  border-radius: 6px;
  padding: 2px 8px;
  background: transparent;
  color: rgba(235, 235, 240, 0.55);
  font-size: 11px;
  cursor: pointer;
}
.seg-btn.on {
  background: rgba(255, 255, 255, 0.16);
  color: #fff;
  font-weight: 600;
}

/* ---- ⚙就地迷你面板 ---- */
.panel {
  position: absolute;
  top: calc(100% + 8px);
  left: 50%;
  transform: translateX(-50%);
  width: 208px;
  padding: 10px 12px 12px;
  border-radius: 13px;
  background: rgba(12, 14, 20, 0.82);
  outline: 1px solid rgba(255, 255, 255, 0.09);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  cursor: default;
}
.panel-title {
  margin: 7px 0 5px;
  font-size: 10px;
  letter-spacing: 0.06em;
  color: rgba(235, 235, 240, 0.45);
}
.swatches {
  display: flex;
  gap: 7px;
}
.swatch {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: 2px solid transparent;
  cursor: pointer;
  padding: 0;
}
.swatch.on {
  border-color: #fff;
}
.sel {
  width: 100%;
  border: none;
  border-radius: 7px;
  padding: 4px 6px;
  background: rgba(255, 255, 255, 0.08);
  color: rgba(235, 235, 240, 0.85);
  font-size: 11px;
  cursor: pointer;
}
.font-range {
  width: 100%;
  accent-color: var(--accent, #d98d5f);
}

/* ---- 文字行 ---- */
.lines {
  text-align: center;
  line-height: 1.55;
}
.line {
  margin: 0;
  word-break: break-word;
}
.line.orig {
  color: #fff;
  font-weight: 600;
  /* 无底衬时靠描边+投影保证游戏画面可读性 */
  text-shadow:
    0 0 5px rgba(0, 0, 0, 0.8),
    0 1px 4px rgba(0, 0, 0, 0.65);
}
.line.trans {
  margin-top: 5px;
  text-shadow:
    0 0 5px rgba(0, 0, 0, 0.8),
    0 1px 3px rgba(0, 0, 0, 0.6);
}
.cleared {
  height: 8px; /* 占位保住布局，视觉完全透明 */
}
.boot-hint {
  padding: 8px 14px;
  border-radius: 10px;
  background: rgba(8, 10, 14, 0.55);
  color: rgba(255, 255, 255, 0.55);
  font-size: 13px;
}

.linefade-enter-active,
.linefade-leave-active {
  transition: opacity 0.17s ease;
}
.linefade-enter-from,
.linefade-leave-to {
  opacity: 0;
}

/* ---- 锁定态悬停解锁钮（正上方居中） ---- */
.unlock-chip {
  position: absolute;
  top: 6px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 6;
  border: none;
  border-radius: 9px;
  padding: 3px 12px;
  background: rgba(12, 14, 20, 0.78);
  outline: 1px solid rgba(255, 255, 255, 0.14);
  color: #fff;
  font-size: 11px;
  cursor: pointer;
  animation: chip-in 0.18s ease;
}
.unlock-chip:hover {
  background: #e5484d;
}
@keyframes chip-in {
  from { opacity: 0; transform: translateX(-50%) translateY(-4px); }
  to   { opacity: 1; transform: translateX(-50%) translateY(0); }
}
/* 悬停期间整窗垫一层极淡底色：WebView2 透明像素处会丢鼠标事件，
   垫色后从文字移到按钮的路径上事件不断链，按钮不再半路消失 */
.overlay-root.picked {
  background: rgba(0, 0, 0, 0.01);
}
</style>
