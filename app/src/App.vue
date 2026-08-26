<script setup lang="ts">
import { ref } from "vue";
import LibraryView from "./components/LibraryView.vue";
import PlayerView from "./components/PlayerView.vue";
import type { MediaItem } from "./types";

const current = ref<MediaItem | null>(null);
type NavKey = "library" | "recent";
const nav = ref<NavKey>("library");
const navItems: { key: NavKey; label: string; icon: string }[] = [
  { key: "library", label: "媒体库", icon: "♫" },
  { key: "recent", label: "最近添加", icon: "◷" },
];

const THEME_KEY = "asplayer-theme-v2";

function toggleTheme() {
  const next =
    document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem(THEME_KEY, next);
  } catch {}
}
</script>

<template>
  <div class="shell">
    <!-- 左侧导航 -->
    <nav class="sidebar">
      <div class="brand" @click="nav = 'library'">ASPlayer</div>
      <div class="nav-list">
        <div
          v-for="item in navItems"
          :key="item.key"
          class="nav-item"
          :class="{ active: nav === item.key }"
          @click="nav = item.key"
        >
          <span class="nav-icon">{{ item.icon }}</span>
          <span>{{ item.label }}</span>
        </div>
      </div>

      <div class="sidebar-bottom">
        <div class="nav-item" @click="toggleTheme">
          <span class="nav-icon">◐</span>
          <span>切换主题</span>
        </div>
      </div>
    </nav>

    <!-- 主内容区 -->
    <main class="content">
      <div class="content-scroll">
        <LibraryView v-if="nav === 'library'" @play="(m: MediaItem) => (current = m)" />
        <div v-else class="placeholder-page">最近添加（M2 实现）</div>
      </div>
    </main>

    <!-- 右侧播放器面板（有正在播放项时显示） -->
    <aside v-if="current" class="player-pane">
      <PlayerView :item="current" @close="current = null" />
    </aside>
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  height: 100vh;
}

.sidebar {
  width: 200px;
  flex-shrink: 0;
  border-right: 1px solid var(--line);
  background: var(--bg-1);
  display: flex;
  flex-direction: column;
  padding: 18px 12px;
}

.brand {
  font-size: 18px;
  font-weight: 700;
  letter-spacing: 0.03em;
  padding: 4px 12px 22px;
  cursor: pointer;
  background: linear-gradient(120deg, var(--accent), var(--fg-1));
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}

.nav-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.sidebar-bottom {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 12px;
  border-radius: 10px;
  color: var(--fg-2);
  cursor: pointer;
  font-size: 14px;
  transition: background 160ms ease, color 160ms ease;
}

.nav-item:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.nav-item.active {
  background: var(--accent-dim);
  color: var(--fg-1);
}

.nav-icon {
  width: 18px;
  font-size: 14px;
}

.content {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}

.content-scroll {
  padding: 20px 24px;
}

.placeholder-page {
  color: var(--fg-3);
  padding: 40px;
}

.player-pane {
  width: 380px;
  flex-shrink: 0;
  border-left: 1px solid var(--line);
  background: var(--bg-1);
  display: flex;
  flex-direction: column;
}
</style>

