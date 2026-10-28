<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { Subtitle } from "../types";
import { useCaptionStyle } from "../stores/captionStyle";

const props = defineProps<{
  subtitles: Subtitle[];
  currentTime: number; // 秒
  status: string;
  stage: string;
  progress: number;
  message: string;
}>();

const emit = defineEmits<{
  close: [];
  seek: [t: number];
  cancel: [];
  lookup: [text: string];
  transcribe: [];
  export: [];
}>();

const cap = useCaptionStyle();
const mode = computed(() => cap.captionStyle.mode);

// 字幕搜索：边打边过滤，命中原文或译文，点击匹配句即 seek（承接既有点击跳转）
const query = ref("");
const searchOpen = ref(false);
const hasQuery = computed(() => query.value.trim().length > 0);
const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return props.subtitles;
  return props.subtitles.filter(
    (s) => s.text.toLowerCase().includes(q) || s.translation.toLowerCase().includes(q)
  );
});

// ---- 查词联动：右键字幕行（行内选中词则优先查选中词）----
const ctxMenu = ref<{ x: number; y: number; text: string } | null>(null);
function closeCtx() {
  ctxMenu.value = null;
}
onMounted(() => window.addEventListener("click", closeCtx));
onBeforeUnmount(() => window.removeEventListener("click", closeCtx));

/** 取当前点击元素内的选中文本；无选中或选中跨越元素边界则返回空串 */
function selectionWithinEl(el: HTMLElement): string {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed) return "";
  const text = sel.toString().trim();
  if (!text) return "";
  const range = sel.getRangeAt(0);
  if (!el.contains(range.commonAncestorContainer)) return "";
  return text;
}

function onLineContextMenu(e: MouseEvent, s: Subtitle) {
  e.preventDefault();
  const el = e.currentTarget as HTMLElement;
  const text = selectionWithinEl(el) || s.text;
  const w = 200;
  const h = 60;
  const x = Math.min(e.clientX, window.innerWidth - w - 8);
  const y = Math.min(e.clientY, window.innerHeight - h - 8);
  ctxMenu.value = { x, y, text };
}

/** 点击字幕行默认跳转；本行内正在选中词时不触发，避免选词瞬间误跳播放 */
function onLineClick(e: MouseEvent, s: Subtitle) {
  const el = e.currentTarget as HTMLElement;
  if (selectionWithinEl(el)) return;
  emit("seek", s.start_ms / 1000);
}

function doLookup() {
  const text = ctxMenu.value?.text;
  closeCtx();
  if (text) emit("lookup", text);
}

const currentMs = computed(() => Math.floor(props.currentTime * 1000));

function isActive(s: Subtitle) {
  return currentMs.value >= s.start_ms && currentMs.value < s.end_ms;
}

const activeSub = computed(() => props.subtitles.find(isActive) ?? null);

const scrollEl = ref<HTMLElement | null>(null);

// activeSub 只在句边界变化（不要 watch currentMs，否则每帧都触发）
watch(
  activeSub,
  async (s) => {
    if (!s) return;
    await nextTick();
    scrollEl.value
      ?.querySelector(".sp-line.active")
      ?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  },
  { immediate: true }
);

// 句间空隙保留上一句，避免当前句卡片闪烁/布局抖动；切换媒体时清除
const lastSub = ref<Subtitle | null>(null);
watch(activeSub, (s) => {
  if (s) lastSub.value = s;
});
watch(
  () => props.subtitles,
  () => {
    if (lastSub.value && !props.subtitles.includes(lastSub.value)) lastSub.value = null;
  }
);
const nowSub = computed(() => activeSub.value ?? lastSub.value);

function seekNext() {
  const list = props.subtitles;
  const i = activeSub.value ? list.indexOf(activeSub.value) : -1;
  const next = i >= 0 ? list[i + 1] : list.find((s) => s.start_ms > currentMs.value);
  if (next) emit("seek", next.start_ms / 1000);
}

function toggleSearch() {
  if (searchOpen.value || hasQuery.value) {
    searchOpen.value = false;
    query.value = "";
  } else {
    searchOpen.value = true;
  }
}
function clearSearch() {
  query.value = "";
  searchOpen.value = false;
}

/** Esc 优先收起搜索：有展开的搜索条或输入内容时返回 true */
function closeSearch(): boolean {
  if (!searchOpen.value && !query.value) return false;
  clearSearch();
  return true;
}

defineExpose({ closeSearch });

function fmt(t: number): string {
  const ms = Math.max(0, Math.floor(t));
  const m = Math.floor(ms / 60000);
  const s = Math.floor((ms % 60000) / 1000);
  return `${m}:${String(s).padStart(2, "0")}`;
}

function stageLabel() {
  if (props.status === "translating") return "正在翻译字幕";
  if (props.status === "transcribing") {
    switch (props.stage) {
      case "extract":
        return "正在抽取音轨";
      case "transcribe":
        return "正在转写字幕";
      default:
        return "正在转写字幕";
    }
  }
  return "";
}
</script>

<template>
  <aside class="subpanel">
    <div class="sp-head">
      <span class="sp-title">字幕</span>
      <div class="sp-mode">
        <button class="sp-modeseg" :class="{ active: mode === 'original' }" @click="cap.captionStyle.mode = 'original'">原文</button>
        <button class="sp-modeseg" :class="{ active: mode === 'bilingual' }" @click="cap.captionStyle.mode = 'bilingual'">双语</button>
        <button class="sp-modeseg" :class="{ active: mode === 'translation' }" @click="cap.captionStyle.mode = 'translation'">译文</button>
      </div>
      <div class="sp-actions">
        <button
          v-if="subtitles.length > 0"
          class="sp-icon"
          title="导出字幕"
          @click="emit('export')"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 3v12M7 10l5 5 5-5M4 21h16" />
          </svg>
        </button>
        <button
          class="sp-icon"
          title="搜索字幕"
          :class="{ active: searchOpen || hasQuery }"
          :aria-expanded="searchOpen || hasQuery"
          @click="toggleSearch"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
            <circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" />
          </svg>
        </button>
        <button class="sp-icon" title="收起字幕" @click="emit('close')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="m9 6 6 6-6 6" />
          </svg>
        </button>
      </div>
    </div>

    <!-- 实时转写/翻译进度（横幅，置于列表上方） -->
    <div v-if="status === 'transcribing' || status === 'translating'" class="sp-progress">
      <div class="sp-progress-label">
        <span>{{ stageLabel() }}</span>
        <span class="sp-progress-pct">{{ progress }}%</span>
      </div>
      <div class="sp-progress-bar">
        <div class="sp-progress-fill" :style="{ width: progress + '%' }"></div>
      </div>
      <p v-if="message" class="sp-progress-msg">{{ message }}</p>
      <!-- 仅转写阶段可取消（翻译暂不支持中断）；whisper 推理不可中断，最迟在推理结束后生效 -->
      <button v-if="status === 'transcribing'" class="sp-cancel" @click="emit('cancel')">取消转写</button>
    </div>

    <div v-if="nowSub && status !== 'error'" class="sp-now">
      <div v-if="mode !== 'translation'" class="sp-now-en">{{ nowSub.text }}</div>
      <div v-if="mode === 'bilingual' && nowSub.translation" class="sp-now-zh">{{ nowSub.translation }}</div>
      <div v-if="mode === 'translation'" class="sp-now-zh sp-now-only">{{ nowSub.translation || nowSub.text }}</div>
      <div class="sp-now-actions">
        <button @click="emit('seek', nowSub.start_ms / 1000)">↺ 重听</button>
        <button @click="seekNext">下一句</button>
      </div>
    </div>

    <!-- 错误 -->
    <div v-if="status === 'error'" class="sp-empty">
      <p>字幕生成失败</p>
      <p v-if="message" class="sp-empty-sub">{{ message }}</p>
      <p v-else class="sp-empty-sub">请检查模型 / API 配置后重试</p>
    </div>

    <!-- 字幕列表（转写中也会逐句累积显示；空态文案按状态区分） -->
    <div v-else class="sp-list-wrap">
      <div v-if="searchOpen || hasQuery" class="sp-search">
        <svg class="sp-search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
        <div class="sp-search-field">
          <input v-model="query" class="sp-search-input" type="text" placeholder="搜索字幕…" spellcheck="false" />
          <button v-if="query" class="sp-search-clear" title="清空" @click="clearSearch">×</button>
        </div>
      </div>

      <div v-if="filtered.length === 0" class="sp-empty">
        <p v-if="hasQuery">无匹配「{{ query.trim() }}」</p>
        <p v-else>{{ status === 'transcribing' ? '正在转写…' : status === 'translating' ? '正在翻译…' : '暂无字幕' }}</p>
        <p v-if="status === 'none'" class="sp-empty-sub">点击顶部「转写并翻译」生成双语字幕</p>
        <button v-if="status === 'none'" class="sp-start" @click="emit('transcribe')">开始转写</button>
      </div>
      <div v-else ref="scrollEl" class="sp-scroll">
        <div
          v-for="s in filtered"
          :key="s.start_ms"
          class="sp-line"
          :class="{ active: isActive(s) }"
          @click="onLineClick($event, s)"
          @contextmenu.prevent="onLineContextMenu($event, s)"
        >
          <span class="sp-line-time">{{ fmt(s.start_ms) }}</span>
          <div class="sp-line-body">
            <span v-if="mode !== 'translation'" class="sp-line-orig">{{ s.text }}</span>
            <span v-if="mode === 'bilingual' && s.translation" class="sp-line-trans">{{ s.translation }}</span>
            <span v-if="mode === 'translation'" class="sp-line-orig">{{ s.translation || s.text }}</span>
          </div>
        </div>
      </div>
    </div>

    <Teleport to="body">
      <div
        v-if="ctxMenu"
        class="sp-ctx"
        :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
        @click.stop
      >
        <button class="sp-ctx-item" @click="doLookup">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
          查词
        </button>
        <div class="sp-ctx-target" :title="ctxMenu.text">{{ ctxMenu.text }}</div>
      </div>
    </Teleport>
  </aside>
</template>

<style scoped>
.subpanel {
  width: var(--drawer-w);
  flex: none;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  user-select: none;
  border-radius: var(--radius-card);
  border: 1px solid var(--panel-border);
  background: var(--panel-bg);
  backdrop-filter: blur(24px);
  box-shadow: var(--shadow-panel);
}

.sp-head {
  height: 48px;
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 0 14px;
  border-bottom: 1px solid var(--line);
}

.sp-title {
  font-size: 13px;
  font-weight: 650;
  margin-right: auto;
}

.sp-mode {
  display: flex;
  gap: 1px;
  padding: 2px;
  border-radius: var(--radius-pill);
  background: var(--accent-dim);
}

.sp-modeseg {
  padding: 3px 9px;
  border: none;
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--fg-3);
  font-size: 11px;
}

.sp-modeseg.active {
  background: var(--bg-2);
  color: var(--fg-1);
}

.sp-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.sp-icon {
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-2);
}

.sp-icon:hover,
.sp-icon.active {
  background: var(--bg-2);
  color: var(--fg-1);
}

.sp-icon svg {
  width: 14px;
  height: 14px;
}

.sp-now {
  flex: none;
  margin: 10px 10px 4px;
  padding: 12px 14px;
  border-radius: 10px;
  background: var(--accent-dim);
  border-left: 2px solid var(--accent-soft);
}

.sp-now-en {
  font-size: 14.5px;
  font-weight: 600;
  line-height: 1.4;
}

.sp-now-zh {
  margin-top: 4px;
  font-size: 13px;
  line-height: 1.45;
  color: var(--fg-2);
}

.sp-now-only {
  margin-top: 0;
  font-size: 14px;
  color: var(--fg-1);
}

.sp-now-actions {
  display: flex;
  gap: 6px;
  margin-top: 9px;
}

.sp-now-actions button {
  padding: 3px 10px;
  font-size: 11px;
  border: 1px solid var(--panel-border);
  background: transparent;
  color: var(--fg-2);
}

.sp-now-actions button:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

/* 进度 */
.sp-progress {
  padding: 14px 16px;
  border-bottom: 1px solid var(--line);
}

.sp-progress-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
  color: var(--fg-1);
  margin-bottom: 8px;
}

.sp-progress-pct {
  color: var(--accent);
  font-variant-numeric: tabular-nums;
  font-weight: 600;
}

.sp-progress-bar {
  height: 5px;
  border-radius: 3px;
  background: var(--bg-2);
  overflow: hidden;
}

.sp-progress-fill {
  height: 100%;
  border-radius: 3px;
  background: var(--accent);
  transition: width 0.2s ease;
}

.sp-progress-msg {
  margin-top: 8px;
  font-size: 12px;
  color: var(--fg-3);
}

/* 取消转写按钮 */
.sp-cancel {
  margin-top: 10px;
  align-self: flex-start;
  padding: 4px 12px;
  font-size: 12px;
  line-height: 1.5;
  border-radius: 6px;
  border: 1px solid rgba(229, 72, 77, 0.45);
  background: transparent;
  color: var(--danger);
  cursor: pointer;
  transition: background 0.15s ease, border-color 0.15s ease;
}
.sp-cancel:hover {
  background: rgba(229, 72, 77, 0.1);
  border-color: var(--danger);
}

/* 空态 */
.sp-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  color: var(--fg-2);
  font-size: 14px;
}

.sp-empty-sub {
  font-size: 12px;
  color: var(--fg-3);
}

.sp-start {
  margin-top: 10px;
  padding: 7px 14px;
  border: none;
  border-radius: var(--radius-pill);
  background: var(--accent);
  color: var(--on-accent);
  font-size: 12.5px;
  font-weight: 600;
}

.sp-start:hover {
  filter: brightness(1.06);
}

/* 列表（含搜索条） */
.sp-list-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.sp-search {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--line);
}

.sp-search-icon {
  width: 14px;
  height: 14px;
  color: var(--fg-3);
  flex-shrink: 0;
}

.sp-search-field {
  position: relative;
  flex: 1;
  min-width: 0;
}

.sp-search-input {
  width: 100%;
  font-size: 12px;
  color: var(--fg-1);
  background: var(--bg-2);
  border: 1px solid var(--line);
  border-radius: 7px;
  padding: 5px 26px 5px 8px;
  outline: none;
  transition: border-color 0.15s ease;
}

.sp-search-input:focus {
  border-color: var(--accent);
}

.sp-search-clear {
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
  width: 18px;
  height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--fg-3);
  font-size: 14px;
  line-height: 1;
  cursor: pointer;
}

.sp-search-clear:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

/* 列表 */
.sp-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0;
}

.sp-line {
  display: flex;
  gap: 10px;
  padding: 10px 16px;
  cursor: pointer;
  border-left: 2px solid transparent;
  transition: background 0.12s ease;
}

.sp-line:hover {
  background: var(--bg-2);
}

.sp-line.active {
  background: var(--accent-dim);
  border-left-color: var(--accent-soft);
}

.sp-line-time {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--fg-3);
  font-variant-numeric: tabular-nums;
  padding-top: 1px;
  width: 36px;
}

.sp-line-body {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  user-select: text; /* 允许拖选词，配合右键查词 */
}

.sp-line-orig {
  font-size: 13px;
  color: var(--fg-1);
  line-height: 1.4;
  word-break: break-word;
}

.sp-line-trans {
  font-size: 12px;
  color: var(--fg-2);
  line-height: 1.4;
  word-break: break-word;
}

/* 查词右键菜单 */
.sp-ctx {
  position: fixed;
  z-index: 1000;
  min-width: 200px;
  max-width: 320px;
  padding: 6px;
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: 10px;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.3);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.sp-ctx-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 8px 10px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-1);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
  transition: background 0.12s ease;
}

.sp-ctx-item:hover {
  background: var(--accent-dim);
}

.sp-ctx-item svg {
  width: 15px;
  height: 15px;
  flex: 0 0 15px;
}

.sp-ctx-target {
  font-size: 11px;
  color: var(--fg-3);
  padding: 6px 10px 4px;
  border-top: 1px solid var(--line);
  margin-top: 2px;
  max-height: 60px;
  overflow: hidden;
  word-break: break-word;
}
</style>
