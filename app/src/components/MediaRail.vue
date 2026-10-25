<script setup lang="ts">
import { computed, ref } from "vue";
import type { MediaItem } from "../types";
import type { ProfileOverride } from "../lib/intensive";
import { sortRecentMedia } from "../lib/uiState";
import PlaylistPanel from "./PlaylistPanel.vue";

const props = defineProps<{
  items: MediaItem[];
  currentId: number | null;
  loading: boolean;
  pinned: boolean;
}>();

const emit = defineEmits<{
  play: [item: MediaItem];
  import: [];
  importFolder: [];
  refresh: [];
  importSubtitle: [item: MediaItem];
  setProfile: [item: MediaItem, value: ProfileOverride];
  settings: [];
}>();

const tab = ref<"recent" | "all">("recent");
const hover = ref(false);
let enterTimer: number | null = null;
let leaveTimer: number | null = null;

const open = computed(() => props.pinned || hover.value);

const covers = computed(() => {
  const recent = sortRecentMedia(props.items);
  return (recent.length ? recent : props.items).slice(0, 4);
});

function onEnter() {
  if (leaveTimer !== null) {
    window.clearTimeout(leaveTimer);
    leaveTimer = null;
  }
  enterTimer = window.setTimeout(() => {
    hover.value = true;
    enterTimer = null;
  }, 180);
}

function onLeave() {
  if (enterTimer !== null) {
    window.clearTimeout(enterTimer);
    enterTimer = null;
  }
  leaveTimer = window.setTimeout(() => {
    hover.value = false;
    leaveTimer = null;
  }, 180);
}
</script>

<template>
  <aside class="rail" :class="{ open }" @mouseenter="onEnter" @mouseleave="onLeave">
    <div class="rail-collapsed">
      <div class="rail-mark" aria-hidden="true"></div>
      <button
        v-for="m in covers"
        :key="m.id"
        class="cp"
        :class="{ on: m.id === currentId }"
        :title="m.title"
        @click="emit('play', m)"
      ></button>
      <div class="gap"></div>
      <button class="rail-icon" title="设置" @click="emit('settings')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
          <circle cx="12" cy="12" r="3" />
          <path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M19.1 4.9 17 7M7 17l-2.1 2.1" />
        </svg>
      </button>
    </div>

    <div class="rail-expanded">
      <div class="rx-head">
        <b>素材</b>
        <div class="rx-tools">
          <button class="rail-icon" title="导入文件" @click="emit('import')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
              <path d="M12 5v14M5 12h14" />
            </svg>
          </button>
          <button class="rail-icon" title="导入文件夹" @click="emit('importFolder')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
            </svg>
          </button>
        </div>
      </div>

      <div class="rx-tabs">
        <button :class="{ on: tab === 'recent' }" @click="tab = 'recent'">最近播放</button>
        <button :class="{ on: tab === 'all' }" @click="tab = 'all'">全部素材</button>
      </div>

      <PlaylistPanel
        embedded
        :recent-only="tab === 'recent'"
        :items="items"
        :current-id="currentId"
        :loading="loading"
        @play="(m) => emit('play', m)"
        @import="emit('import')"
        @import-folder="emit('importFolder')"
        @refresh="emit('refresh')"
        @import-subtitle="(m) => emit('importSubtitle', m)"
        @set-profile="(m, v) => emit('setProfile', m, v)"
      />
    </div>
  </aside>
</template>

<style scoped>
.rail {
  position: absolute;
  left: var(--gutter);
  top: 64px;
  bottom: 96px;
  width: var(--rail-w);
  z-index: 22;
  overflow: hidden;
  border-radius: var(--radius-card);
  border: 1px solid var(--panel-border);
  background: var(--panel-bg);
  backdrop-filter: blur(20px);
  box-shadow: var(--shadow-card);
  transition: width 300ms cubic-bezier(0.22, 0.61, 0.36, 1), background 300ms ease;
}

.rail.open {
  width: var(--rail-w-open);
}

.rail-collapsed {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 12px 0;
  transition: opacity 140ms ease;
}

.rail.open .rail-collapsed {
  opacity: 0;
  pointer-events: none;
}

.rail-mark {
  width: 22px;
  height: 22px;
  border-radius: 6px;
  background: linear-gradient(150deg, #6f9dc6, #12212e);
}

.cp {
  width: 28px;
  height: 28px;
  flex: none;
  padding: 0;
  border: none;
  border-radius: 6px;
  background: linear-gradient(150deg, #35506a, #0d151c 72%);
  opacity: 0.55;
}

.cp:nth-of-type(2) { background: linear-gradient(150deg, #5b4730, #191108 72%); }
.cp:nth-of-type(3) { background: linear-gradient(150deg, #40365c, #120e1c 72%); }
.cp:nth-of-type(4) { background: linear-gradient(150deg, #2e5145, #0b1712 72%); }

.cp.on {
  opacity: 1;
  box-shadow: 0 0 0 2px var(--accent-soft);
}

.gap {
  flex: 1;
}

.rail-icon {
  width: 28px;
  height: 28px;
  flex: none;
  display: grid;
  place-items: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-2);
}

.rail-icon:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.rail-icon svg {
  width: 15px;
  height: 15px;
}

.rail-expanded {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  padding: 12px 10px 10px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 180ms ease 40ms;
}

.rail.open .rail-expanded {
  opacity: 1;
  pointer-events: auto;
}

.rx-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 2px;
  margin-bottom: 8px;
}

.rx-head b {
  font-size: 12.5px;
  font-weight: 650;
}

.rx-tools {
  display: flex;
  gap: 2px;
}

.rx-tabs {
  display: flex;
  gap: 2px;
  padding: 2px;
  margin-bottom: 8px;
  border-radius: 8px;
  background: var(--accent-dim);
}

.rx-tabs button {
  flex: 1;
  padding: 4px 0;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fg-3);
  font-size: 11px;
}

.rx-tabs button.on {
  background: var(--bg-2);
  color: var(--fg-1);
}
</style>
