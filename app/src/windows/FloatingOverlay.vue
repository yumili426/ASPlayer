<script setup lang="ts">
/**
 * M3 迷你悬浮字幕窗（设计 §9.2）
 * - 置顶透明窗口内的纯展示组件，不持有播放器状态
 * - 数据来源：后端转发的 overlay://subtitle（由主窗逐句推送）
 * - 点击原文 → 请求跳转到该句句首（经后端转发回主窗执行）
 * - 锁定态仅影响样式提示；真实鼠标穿透由 Rust 侧 set_ignore_cursor_events 控制
 */
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCaptionStyle } from "../stores/captionStyle";

interface SubtitlePayload {
  text: string;
  translation: string;
  start_ms: number;
}

const cap = useCaptionStyle();
const text = ref("");
const translation = ref("");
const startMs = ref(0);
const locked = ref(false);
const unlisteners: (() => void)[] = [];

const hasContent = computed(() => text.value.length > 0 || translation.value.length > 0);

// 悬浮窗字号随主窗字幕设置的缩放倍率（两窗共享 localStorage，同一 origin）
const boxStyle = computed(() => ({
  color: cap.captionStyle.color,
  background: `rgba(8, 10, 14, ${cap.captionStyle.bgOpacity})`,
}));
const originalStyle = computed(() => ({ fontSize: `${18 * cap.captionStyle.fontScale}px` }));
const translationStyle = computed(() => ({ fontSize: `${15 * cap.captionStyle.fontScale}px` }));

const appWindow = getCurrentWindow();

/** 按住 HUD 把手拖动窗口（start-dragging 权限含于 core:default） */
function startDrag() {
  if (locked.value) return;
  appWindow.startDragging().catch(() => {});
}

/** 关闭悬浮窗：走后端命令，Rust 是显隐事实来源（会反推 visibility 到主窗） */
function closeOverlay() {
  invoke("set_overlay_visible", { visible: false }).catch(() => {});
}

function seekToSentence() {
  if (!hasContent.value) return;
  invoke("overlay_request_seek", { ms: startMs.value }).catch(() => {});
}

onMounted(async () => {
  // 监听注册失败 = 收不到任何推送（ACL/平台异常），显式打日志便于排查
  try {
    unlisteners.push(
      await listen<SubtitlePayload>("overlay://subtitle", (e) => {
        const p = e.payload ?? { text: "", translation: "", start_ms: 0 };
        text.value = p.text ?? "";
        translation.value = p.translation ?? "";
        startMs.value = p.start_ms ?? 0;
      })
    );
    unlisteners.push(
      await listen<boolean>("overlay://lock-changed", (e) => {
        locked.value = !!e.payload;
      })
    );
  } catch (err) {
    console.error("[overlay] 事件监听注册失败:", err);
  }
  // 初始锁定状态探测（启动时后端可能已处于锁定态）
  try {
    locked.value = await invoke<boolean>("is_overlay_locked");
  } catch {
    /* 忽略 */
  }
});

onUnmounted(() => unlisteners.forEach((u) => u()));
</script>

<template>
  <div class="overlay-root" :class="{ locked }">
    <!-- HUD：仅解锁态出现；左侧把手拖动，右侧关闭 -->
    <div v-if="!locked" class="overlay-hud">
      <span class="hud-grip" title="按住拖动悬浮窗" @mousedown.prevent="startDrag"></span>
      <button class="hud-close" title="关闭悬浮字幕窗" @click.stop="closeOverlay">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </div>
    <Transition name="fade">
      <div v-if="hasContent" class="overlay-box" :style="boxStyle">
        <p class="line original" :style="originalStyle" title="点击回到本句开头" @click.stop="seekToSentence">
          {{ text }}
        </p>
        <p v-if="translation" class="line translation" :style="translationStyle">{{ translation }}</p>
      </div>
      <div v-else class="overlay-hint">开始播放后此处显示字幕</div>
    </Transition>
  </div>
</template>

<style scoped>
.overlay-root {
  width: 100vw;
  height: 100vh;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  overflow: hidden;
  user-select: none;
  border-radius: 12px;
}

.overlay-root.locked .overlay-box {
  opacity: 0.92;
}

.overlay-box {
  max-width: calc(100vw - 24px);
  padding: 10px 16px;
  border-radius: 12px;
  text-align: center;
  box-shadow: 0 4px 24px rgba(0, 0, 0, 0.35);
  transition: opacity 0.15s ease;
}

.line {
  margin: 2px 0;
  line-height: 1.45;
  text-shadow: 0 1px 3px rgba(0, 0, 0, 0.6);
  word-break: break-word;
}

.original {
  font-weight: 600;
  cursor: pointer;
}

.original:hover {
  text-decoration: underline;
}

.translation {
  opacity: 0.88;
}

.overlay-hint {
  padding: 8px 14px;
  border-radius: 10px;
  background: rgba(8, 10, 14, 0.55);
  color: rgba(255, 255, 255, 0.55);
  font-size: 13px;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.18s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

/* ---- HUD：拖动把手 + 关闭（悬停窗体或空态时浮现） ---- */
.overlay-hud {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 5px;
  background: linear-gradient(to bottom, rgba(0, 0, 0, 0.35), rgba(0, 0, 0, 0));
  opacity: 0;
  transition: opacity 0.15s ease;
  pointer-events: none;
}

.overlay-root:hover .overlay-hud,
.overlay-root:not(:has(.overlay-box)) .overlay-hud {
  opacity: 1;
}

/* HUD 可见时可交互；隐藏时不挡点击 */
.overlay-root:hover .overlay-hud > *,
.overlay-root:not(:has(.overlay-box)) .overlay-hud > * {
  pointer-events: auto;
}

.hud-grip {
  width: 36px;
  height: 13px;
  cursor: grab;
  background-image: radial-gradient(circle, rgba(255, 255, 255, 0.55) 1px, transparent 1.4px);
  background-size: 6px 6px;
}

.hud-grip:active {
  cursor: grabbing;
}

.hud-close {
  width: 17px;
  height: 17px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.16);
  color: rgba(255, 255, 255, 0.78);
}

.hud-close:hover {
  background: #e5484d;
  color: #fff;
}

.hud-close svg {
  width: 9px;
  height: 9px;
}
</style>
