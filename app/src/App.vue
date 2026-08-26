<script setup lang="ts">
function toggleTheme() {
  const next =
    document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  localStorage.setItem("theme", next);
}
</script>

<template>
  <div class="shell">
    <header class="topbar">
      <span class="brand">ASPlayer</span>
      <div class="topbar-actions">
        <button class="ghost" title="切换主题" @click="toggleTheme">◐</button>
        <button class="ghost" title="设置">⚙</button>
      </div>
    </header>
    <main class="content">
      <p class="placeholder-text">媒体库即将到来（M1 Task 4）…</p>
    </main>
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 20px;
  border-bottom: 1px solid var(--line);
}

.brand {
  font-size: 16px;
  font-weight: 600;
  letter-spacing: 0.02em;
}

.topbar-actions {
  display: flex;
  gap: 8px;
}

.ghost {
  background: transparent;
  border: none;
  padding: 4px 10px;
  color: var(--fg-2);
}

.content {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.placeholder-text {
  color: var(--fg-3);
}
</style>
