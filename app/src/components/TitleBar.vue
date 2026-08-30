<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

const appWindow = getCurrentWindow();
const maximized = ref(false);

let unlistenResize: (() => void) | null = null;

async function minimize() {
  await appWindow.minimize();
}

async function toggleMaximize() {
  await appWindow.toggleMaximize();
}

async function close() {
  await appWindow.close();
}

onMounted(async () => {
  maximized.value = await appWindow.isMaximized();
  // 最大化状态可能由拖拽贴靠/双击标题栏改变，resize 时同步一次
  unlistenResize = await appWindow.onResized(async () => {
    maximized.value = await appWindow.isMaximized();
  });
});

onUnmounted(() => {
  unlistenResize?.();
});
</script>

<template>
  <header class="titlebar">
    <!-- 拖拽区：文本/图标 pointer-events:none，事件穿透到容器触发拖动 -->
    <div class="tb-drag" data-tauri-drag-region @dblclick="toggleMaximize">
      <span class="tb-logo" aria-hidden="true">
        <!-- 应用图标：上浅青/下深海军蓝双层字幕条 + 白色播放三角 -->
        <svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
          <path d="M5.9 3.4 H18.1 A3.7 3.7 0 0 1 21.8 7.1 V12 H2.2 V7.1 A3.7 3.7 0 0 1 5.9 3.4 Z" fill="#0EC4DE"/>
          <path d="M2.2 12 H21.8 V16.9 A3.7 3.7 0 0 1 18.1 20.6 H5.9 A3.7 3.7 0 0 1 2.2 16.9 Z" fill="#0A2255"/>
          <path d="M10.2 8.6 L15.3 11.9 L10.2 15.2 Z" fill="#FEFCFA" stroke="#FEFCFA" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round"/>
        </svg>
      </span>
      <span class="tb-title">ASPlayer</span>
    </div>
    <div class="tb-btns">
      <button class="tb-btn" title="最小化" @click="minimize">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4">
          <path d="M5 12h14" />
        </svg>
      </button>
      <button
        class="tb-btn"
        :title="maximized ? '还原' : '最大化'"
        @click="toggleMaximize"
      >
        <svg
          v-if="!maximized"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
        >
          <rect x="6" y="6" width="12" height="12" rx="1.5" />
        </svg>
        <svg
          v-else
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
        >
          <rect x="5" y="9" width="10" height="10" rx="1.5" />
          <path d="M9 9V6.5A1.5 1.5 0 0 1 10.5 5H18a1.5 1.5 0 0 1 1.5 1.5V14a1.5 1.5 0 0 1-1.5 1.5H15" />
        </svg>
      </button>
      <button class="tb-btn tb-close" title="关闭" @click="close">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4">
          <path d="M6 6l12 12M18 6L6 18" />
        </svg>
      </button>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  display: flex;
  align-items: stretch;
  height: 36px;
  flex: none;
  background: var(--bg-0);
  border-bottom: 1px solid var(--line);
  user-select: none;
}

.tb-drag {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 12px;
  min-width: 0;
}

.tb-logo {
  display: inline-flex;
  width: 16px;
  height: 16px;
  pointer-events: none;
}

.tb-logo svg {
  width: 100%;
  height: 100%;
}

.tb-title {
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.04em;
  color: var(--fg-2);
  pointer-events: none;
}

.tb-btns {
  display: flex;
  align-items: stretch;
}

.tb-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 46px;
  padding: 0;
  background: transparent;
  border: none;
  border-radius: 0;
  color: var(--fg-2);
  transition: background 150ms ease, color 150ms ease;
}

.tb-btn:hover {
  background: var(--bg-2);
  color: var(--fg-1);
  filter: none;
}

.tb-btn:active {
  transform: none;
}

.tb-btn svg {
  width: 16px;
  height: 16px;
}

.tb-close:hover {
  background: var(--danger);
  color: var(--danger-fg);
}
</style>
