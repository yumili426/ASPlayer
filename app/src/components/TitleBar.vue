<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

const props = defineProps<{
  title?: string;
  meta?: string;
  ctaLabel?: string;
  ctaDisabled?: boolean;
  ctaProgress?: number | null;
}>();

const emit = defineEmits<{ menu: []; primary: []; more: [] }>();

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
    <button class="tb-icon" title="素材轨" @click="emit('menu')">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
        <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />
      </svg>
    </button>

    <div class="tb-drag" data-tauri-drag-region>
      <div class="tb-doc">
        <h1>{{ props.title || "ASPlayer" }}</h1>
        <p v-if="props.meta">{{ props.meta }}</p>
      </div>
    </div>

    <div class="tb-right">
      <button
        v-if="props.ctaLabel"
        class="cta"
        :disabled="props.ctaDisabled"
        @click="emit('primary')"
      >
        <span
          v-if="props.ctaProgress != null"
          class="cta-ring"
          :style="{ '--p': props.ctaProgress + '%' }"
        />
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 3v10" /><path d="m8 9 4 4 4-4" /><path d="M4 17v2h16v-2" />
        </svg>
        {{ props.ctaLabel }}
      </button>

      <button class="tb-icon" title="更多" @click="emit('more')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
          <circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" />
        </svg>
      </button>

      <div class="tb-btns">
        <button class="tb-btn" title="最小化" @click="minimize">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M5 12h14" /></svg>
        </button>
        <button class="tb-btn" :title="maximized ? '还原' : '最大化'" @click="toggleMaximize">
          <svg v-if="!maximized" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4">
            <rect x="6" y="6" width="12" height="12" rx="1.5" />
          </svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4">
            <rect x="5" y="9" width="10" height="10" rx="1.5" />
            <path d="M9 9V6.5A1.5 1.5 0 0 1 10.5 5H18a1.5 1.5 0 0 1 1.5 1.5V14a1.5 1.5 0 0 1-1.5 1.5H15" />
          </svg>
        </button>
        <button class="tb-btn tb-close" title="关闭" @click="close">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </div>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: var(--topbar-h);
  z-index: 30;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px 0 12px;
  background: var(--topbar-scrim);
  user-select: none;
}

.tb-icon {
  width: 34px;
  height: 34px;
  flex: none;
  display: grid;
  place-items: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 9px;
  background: transparent;
  color: var(--fg-2);
}

.tb-icon:hover {
  background: var(--panel-bg);
  border-color: var(--panel-border);
  color: var(--fg-1);
}

.tb-icon svg {
  width: 17px;
  height: 17px;
}

.tb-drag {
  flex: 1;
  min-width: 0;
  height: 100%;
  display: flex;
  align-items: center;
  padding: 0 8px;
}

.tb-doc {
  min-width: 0;
  pointer-events: none;
}

.tb-doc h1 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: -0.01em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tb-doc p {
  margin: 1px 0 0;
  font-size: 11.5px;
  color: var(--fg-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tb-right {
  display: flex;
  align-items: center;
  gap: 6px;
}

.cta {
  height: 32px;
  padding: 0 14px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: none;
  border-radius: var(--radius-pill);
  background: var(--accent);
  color: var(--on-accent);
  font-size: 12.5px;
  font-weight: 600;
  white-space: nowrap;
}

.cta:disabled {
  opacity: 0.35;
  cursor: default;
}

.cta svg {
  width: 15px;
  height: 15px;
}

.cta-ring {
  width: 15px;
  height: 15px;
  border-radius: 50%;
  background: conic-gradient(
    var(--on-accent) var(--p),
    color-mix(in srgb, var(--on-accent) 22%, transparent) 0
  );
  mask: radial-gradient(circle, transparent 52%, #000 54%);
  -webkit-mask: radial-gradient(circle, transparent 52%, #000 54%);
}

.tb-btns {
  display: flex;
  align-items: stretch;
  margin-left: 4px;
  border-left: 1px solid var(--line);
  padding-left: 4px;
}

.tb-btn {
  width: 40px;
  height: 34px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  background: transparent;
  border: none;
  border-radius: 8px;
  color: var(--fg-2);
  transition: background 150ms ease, color 150ms ease;
}

.tb-btn:hover {
  background: var(--panel-bg);
  color: var(--fg-1);
  filter: none;
}

.tb-btn:active {
  transform: none;
}

.tb-btn svg {
  width: 15px;
  height: 15px;
}

.tb-close:hover {
  background: var(--danger);
  color: var(--danger-fg);
}
</style>
