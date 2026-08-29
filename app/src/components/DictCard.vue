<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import type { DictLookup } from "../types";

const props = defineProps<{
  open: boolean;
  loading: boolean; // 正在等 dict_lookup 返回
  result: DictLookup | null; // 查询结果；null = 未就绪/未查到
  error: string | null; // 查询失败（区别于词典未安装）
  downloading: boolean; // 词典下载进行中（英/日任一）
}>();

const emit = defineEmits<{
  close: [];
  lookup: [term: string]; // 点击「是不是想找」相似词 → 重新查该词
  download: []; // 点击「下载词典」→ 让父组件触发下载
}>();

// Esc 关闭：仅当卡片打开时响应，避免误吞全局 Escape（如退出全屏）
function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape" && props.open) emit("close");
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <Teleport to="body">
    <template v-if="open">
      <!-- 透明全屏背景：点击关闭 -->
      <div class="dc-backdrop" aria-hidden="true" @click="emit('close')"></div>

      <aside class="dc-card" @click.stop>
        <button class="dc-close" title="关闭查词" @click="emit('close')">
          <svg viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8"><path d="M6 6l12 12M18 6L6 18"/></svg>
        </button>

        <!-- 查询中 -->
        <div v-if="loading" class="dc-loading">
          <span class="dc-spinner" aria-hidden="true"></span>
          <span>查询中…</span>
        </div>

        <!-- 查询失败：区别于词典未安装（不显示下载按钮） -->
        <div v-else-if="error" class="dc-empty">
          <p class="dc-empty-main">查询出错</p>
          <p class="dc-empty-sub">网络或接口异常，请重试</p>
        </div>

        <!-- 命中：term + 音标/假名 + 词性 + 释义 -->
        <template v-else-if="result && result.definitions.length > 0">
          <header class="dc-head">
            <span class="dc-term">{{ result.term }}</span>
            <span v-if="result.phonetic" class="dc-phonetic">{{ result.phonetic }}</span>
            <span v-if="result.reading" class="dc-phonetic">{{ result.reading }}</span>
            <span v-if="result.pos" class="dc-pos">{{ result.pos }}</span>
          </header>
          <ul class="dc-defs">
            <li v-for="(d, i) in result.definitions" :key="i" class="dc-def">{{ d }}</li>
          </ul>
        </template>

        <!-- 相似词建议 -->
        <template v-else-if="result && result.suggestions.length > 0">
          <p class="dc-suggest-title">未找到「{{ result.term }}」，是不是想找：</p>
          <div class="dc-chips">
            <button
              v-for="(s, i) in result.suggestions"
              :key="i"
              class="dc-chip"
              @click="emit('lookup', s)"
            >{{ s }}</button>
          </div>
        </template>

        <!-- 未就绪（result 为 null = 词典库还没建，dict_lookup 返回空数组） -->
        <div v-else-if="result === null" class="dc-empty">
          <p v-if="downloading" class="dc-empty-main">正在下载词典…</p>
          <template v-else>
            <p class="dc-empty-main">词典未就绪，请下载</p>
            <p class="dc-empty-sub">首次使用需先下载词典才能联网查词</p>
            <button class="dc-download" @click="emit('download')">下载词典</button>
          </template>
        </div>

        <!-- 已查到但无结果（result 非 null 但无释义也无相似词 = 真·未查到） -->
        <div v-else class="dc-empty">
          <p class="dc-empty-main">未找到「{{ result.term }}」</p>
          <p class="dc-empty-sub">换个词试试</p>
        </div>
      </aside>
    </template>
  </Teleport>
</template>

<style scoped>
.dc-backdrop {
  position: fixed;
  inset: 0;
  z-index: 890;
  background: transparent;
}

.dc-card {
  position: fixed;
  right: 16px;
  bottom: 80px;
  z-index: 900;
  width: 340px;
  max-height: 60vh;
  overflow-y: auto;
  padding: 14px 16px;
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: var(--radius-card);
  box-shadow: var(--shadow-card);
  color: var(--fg-1);
  font-size: 14px;
  line-height: 1.6;
  user-select: text;
  -webkit-user-select: text;
}

.dc-close {
  position: sticky;
  top: 0;
  float: right;
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: var(--bg-2);
  border-radius: 7px;
  cursor: pointer;
  transition: background 0.15s ease, filter 0.15s ease;
}
.dc-close:hover {
  filter: brightness(1.2);
}
.dc-close svg {
  width: 15px;
  height: 15px;
  stroke-width: 2;
}

/* 查询中 */
.dc-loading {
  display: flex;
  align-items: center;
  gap: 9px;
  color: var(--fg-2);
  font-size: 13px;
  padding-top: 18px;
}
.dc-spinner {
  width: 15px;
  height: 15px;
  border-radius: 50%;
  border: 2px solid var(--fg-3);
  border-top-color: var(--accent);
  animation: dc-spin 0.8s linear infinite;
  flex-shrink: 0;
}
@keyframes dc-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 命中 */
.dc-head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
  padding-top: 4px;
  min-height: 40px;
}
.dc-term {
  font-size: 19px;
  font-weight: 700;
  letter-spacing: -0.01em;
}
.dc-phonetic {
  font-size: 13px;
  color: var(--fg-2);
}
.dc-pos {
  font-size: 11px;
  font-weight: 600;
  color: var(--accent);
  background: var(--accent-dim);
  border-radius: var(--radius-pill);
  padding: 1px 8px;
  align-self: center;
}

.dc-defs {
  list-style: none;
  margin-top: 10px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.dc-def {
  font-size: 14px;
  color: var(--fg-1);
  line-height: 1.6;
  word-break: break-word;
}

/* 相似词 */
.dc-suggest-title {
  font-size: 13px;
  color: var(--fg-2);
  padding-top: 4px;
}
.dc-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 10px;
}
.dc-chip {
  padding: 5px 14px;
  font-size: 13px;
  line-height: 1.5;
  background: var(--bg-2);
  border: 1px solid var(--line);
  border-radius: var(--radius-pill);
  color: var(--fg-1);
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}
.dc-chip:hover {
  background: var(--accent-dim);
  color: var(--accent);
}

/* 未就绪 / 未查到 */
.dc-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  gap: 6px;
  min-height: 96px;
  padding: 8px 0;
}
.dc-empty-main {
  font-size: 14px;
  color: var(--fg-1);
}
.dc-empty-sub {
  font-size: 12px;
  color: var(--fg-3);
}
.dc-download {
  margin-top: 6px;
  padding: 6px 20px;
  font-size: 13px;
  font-weight: 600;
  background: var(--accent-dim);
  border: 1px solid var(--accent);
  border-radius: var(--radius-pill);
  color: var(--accent);
  cursor: pointer;
  transition: filter 0.15s ease, background 0.15s ease;
}
.dc-download:hover {
  filter: brightness(1.1);
  background: var(--accent-dim);
}
</style>
