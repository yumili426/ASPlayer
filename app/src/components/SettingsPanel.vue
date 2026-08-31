<script setup lang="ts">
import { ref, watch, computed } from "vue";
import { getSettings, saveSettings, getEnvApiConfig } from "../api/subtitle";
import { dictStatus, dictDownload, dictCancel, onDictStatus, onDictProgress } from "../api/dict";
import { useCaptionStyle } from "../stores/captionStyle";
import { useShortcuts } from "../stores/shortcuts";
import { usePlayback } from "../stores/playback";
import type { ShortcutActionName } from "../types";
import type { DictStatus, DictProgress } from "../types";
import { ollamaStatus, ollamaPull, ollamaPullCancel, onOllamaStatus, onOllamaProgress } from "../api/ollama";
import type { OllamaStatus, PullState } from "../types";
import { useModels, MODEL_META } from "../stores/model";

type TabKey = "appearance" | "playback" | "subtitle" | "translate" | "model" | "dict" | "shortcuts";

const props = defineProps<{ open: boolean; theme: "light" | "dark" | "system" }>();
const emit = defineEmits<{ close: []; setTheme: [theme: "light" | "dark" | "system"] }>();

interface Provider {
  label: string;
  base: string;
  model: string;
}

// OpenAI 兼容服务商预设（选中后自动填 base + model，仍可手动改）
const providers: Provider[] = [
  { label: "DeepSeek", base: "https://api.deepseek.com/v1", model: "deepseek-chat" },
  { label: "OpenAI", base: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  { label: "通义千问 Qwen", base: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus" },
  { label: "智谱 GLM", base: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash" },
  { label: "月之暗面 Kimi", base: "https://api.moonshot.cn/v1", model: "moonshot-v1-8k" },
  { label: "本地 Ollama", base: "http://localhost:11434/v1", model: "llama3.1" },
];

const apiBase = ref("");
const apiKey = ref("");
const apiModel = ref("deepseek-chat");
const vadWindowMs = ref(30);
const vadMinSilenceMs = ref(300);
const vadMinChunkMs = ref(1000);
const vadMaxChunkMs = ref(30000);
const providerIdx = ref(-1); // -1 = 自定义，不匹配任何预设
const saving = ref(false);
const showKey = ref(false);
const envKey = ref("");
const showAdv = ref(false); // 「模型」页里折叠的转写高级参数
const vadSaved = ref(false);
let vadSaveTimer: ReturnType<typeof setTimeout> | null = null;
const dictUrlEn = ref("");
const dictUrlJa = ref("");
const dictUrlSaved = ref(false);
let dictUrlTimer: ReturnType<typeof setTimeout> | null = null;

// ---- 本地翻译引擎（Ollama）----
const ollamaBase = ref("http://localhost:11434");
const ollamaBaseSaved = ref(false);
let ollamaTimer: ReturnType<typeof setTimeout> | null = null;
const ollamaInfo = ref<OllamaStatus | null>(null);
const ollamaPullState = ref<PullState | null>(null);
let ollamaInit = false;
const OLLAMA_RECOMMENDED = [
  { model: "qwen2.5:3b", label: "小 · 约 1.9 GB" },
  { model: "qwen2.5:7b", label: "中 · 约 4.7 GB" },
] as const;

async function initOllama() {
  if (ollamaInit) return;
  await onOllamaStatus((s) => (ollamaPullState.value = s));
  await onOllamaProgress((p) => {
    ollamaPullState.value = {
      model: p.model,
      status: "downloading",
      bytes: p.bytes,
      total: p.total,
      error: null,
    };
  });
  ollamaInit = true;
}

async function loadOllama() {
  try {
    ollamaInfo.value = await ollamaStatus();
  } catch {
    /* ignore */
  }
}

async function onPullLocal(model: string) {
  try {
    await ollamaPull(model);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 拉取本地翻译模型失败:", e);
  }
}

async function onCancelPullLocal() {
  try {
    await ollamaPullCancel();
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 取消拉取本地翻译模型失败:", e);
  }
}

async function onSaveOllamaBase() {
  try {
    await saveSettings({ ollama_base: ollamaBase.value });
    ollamaBaseSaved.value = true;
    if (ollamaTimer) clearTimeout(ollamaTimer);
    ollamaTimer = setTimeout(() => (ollamaBaseSaved.value = false), 1500);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 保存 Ollama 地址失败:", e);
  }
}

// 一键接通：把翻译配置指向本地模型（api_base 用 base+"/v1"，api_key 留空）
async function onUseLocal(model: string) {
  try {
    await saveSettings({
      api_base: ollamaBase.value.replace(/\/+$/, "") + "/v1",
      api_model: model,
      api_key: "",
    });
    apiBase.value = ollamaBase.value.replace(/\/+$/, "") + "/v1";
    apiModel.value = model;
    apiKey.value = "";
    providerIdx.value = -1; // 不匹配任何云端预设，落回「自定义」
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 一键接通本地模型失败:", e);
  }
}

const ollamaConnected = computed(() => !!ollamaInfo.value?.connected);
const localPulling = computed(() => ollamaPullState.value?.status === "downloading");
const localPercent = computed(() => {
  const s = ollamaPullState.value;
  if (!s || !s.total) return 0;
  return Math.min(100, Math.round((s.bytes / s.total) * 100));
});

// 拉取完成后自动刷新模型列表
watch(
  () => ollamaPullState.value?.status,
  (status) => {
    if (status === "done") loadOllama();
  }
);

const cap = useCaptionStyle();
const captionStyle = cap.captionStyle;
const capColors = [
  "#ffffff",
  "#f0f0f5",
  "#000000",
  "#ffd60a",
  "#0a84ff",
  "#30d158",
  "#ff453a",
  "#ff9f0a",
];
function onCaptionColor(e: Event) {
  captionStyle.color = (e.target as HTMLInputElement).value;
}

const activeTab = ref<TabKey>("appearance");
const tabs: { key: TabKey; label: string }[] = [
  { key: "appearance", label: "外观" },
  { key: "playback", label: "播放" },
  { key: "subtitle", label: "字幕" },
  { key: "translate", label: "翻译" },
  { key: "model", label: "模型" },
  { key: "dict", label: "词典" },
  { key: "shortcuts", label: "快捷键" },
];

const sc = useShortcuts();
const pb = usePlayback();
const ms = useModels();
// ---- 内置词典：设置页下载管理 ----
const dictStatuses = ref<DictStatus[]>([]);
const dictProgress = ref<DictProgress[]>([]);
let dictInit = false; // 事件只订阅一次（与 useModels 同思路），状态每次打开面板刷新

async function initDict() {
  if (dictInit) return;
  await onDictStatus((s) => {
    const i = dictStatuses.value.findIndex((x) => x.lang === s.lang);
    if (i >= 0) dictStatuses.value[i] = s;
    else dictStatuses.value.push(s);
  });
  await onDictProgress((p) => {
    const i = dictProgress.value.findIndex((x) => x.lang === p.lang);
    if (i >= 0) dictProgress.value[i] = p;
    else dictProgress.value.push(p);
  });
  dictInit = true;
}

async function loadDict() {
  try {
    dictStatuses.value = await dictStatus();
  } catch {
    /* ignore */
  }
}

async function onDictDownload(lang: string) {
  try {
    await dictDownload(lang);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 下载词典失败:", e);
  }
}

async function onDictCancel(lang: string) {
  try {
    await dictCancel(lang);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 取消下载词典失败:", e);
  }
}

function dictStatusText(s: DictStatus): string {
  switch (s.status) {
    case "done": return "已就绪";
    case "downloading": return "下载中";
    case "failed": return "下载失败";
    case "canceled": return "已取消";
    default: return "未下载";
  }
}

function dictPercent(lang: string): number {
  const p = dictProgress.value.find((x) => x.lang === lang);
  return p ? Math.min(100, Math.round(p.percent)) : 0;
}

function fmtBytes(n: number): string {
  if (n <= 0) return "";
  const mb = n / (1024 * 1024);
  return mb >= 1024 ? (mb / 1024).toFixed(1) + " GB" : mb.toFixed(1) + " MB";
}

const downloadingLang = computed(
  () => dictStatuses.value.find((s) => s.status === "downloading")?.lang ?? null
);

const selectedFileExists = computed(() => {
  const s = ms.modelState.models.find((m) => m.selected);
  return !!s && s.file_exists;
});
const activePercent = computed(() => {
  const s = ms.modelState.models.find((m) => m.size === ms.modelState.activeSize);
  if (!s || !s.total_bytes) return 0;
  return Math.min(100, Math.round((s.bytes_downloaded / s.total_bytes) * 100));
});

function keysOf(action: ShortcutActionName): string {
  return sc.shortcuts.value.find((s) => s.action === action)?.keys ?? "";
}

function startRecord(action: ShortcutActionName) {
  sc.recording.value = action;
}

function cancelRecord() {
  sc.recording.value = null;
}

let recHandler: ((e: KeyboardEvent) => void) | null = null;
watch(sc.recording, (action) => {
  if (recHandler) {
    window.removeEventListener("keydown", recHandler, true);
    recHandler = null;
  }
  if (action) {
    recHandler = (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        cancelRecord();
        return;
      }
      sc.setShortcut(action, sc.normalizeKey(e));
      cancelRecord();
    };
    window.addEventListener("keydown", recHandler, true);
  }
});

// 根据 base 反推当前匹配的预设（用于回显下拉）
function matchProvider(base: string, model: string): number {
  return providers.findIndex((p) => p.base === base && p.model === model);
}

function applyProvider(i: number) {
  if (i < 0) return;
  apiBase.value = providers[i].base;
  apiModel.value = providers[i].model;
}

async function load() {
  try {
    const s = await getSettings();
    apiBase.value = s.api_base ?? "";
    apiKey.value = s.api_key ?? "";
    apiModel.value = s.api_model ?? "";
    vadWindowMs.value = Number(s.vad_window_ms ?? 30);
    vadMinSilenceMs.value = Number(s.vad_min_silence_ms ?? 300);
    vadMinChunkMs.value = Number(s.vad_min_chunk_ms ?? 1000);
    vadMaxChunkMs.value = Number(s.vad_max_chunk_ms ?? 30000);
    providerIdx.value = matchProvider(apiBase.value, apiModel.value);
    dictUrlEn.value = s.dict_url_en ?? "";
    dictUrlJa.value = s.dict_url_ja ?? "";
    ollamaBase.value = s.ollama_base ?? "http://localhost:11434";
  } catch {
    /* ignore */
  }
  try {
    const env = await getEnvApiConfig();
    envKey.value = env.key;
  } catch {
    /* ignore */
  }
}

watch(
  () => props.open,
  async (open) => {
    if (open) {
      await ms.initModel();
      await ms.loadModel();
      await initDict();
      await loadDict();
      await initOllama();
      await loadOllama();
      load(); // 既有：载入翻译/API 设置
    }
  }
);

async function onSave() {
  saving.value = true;
  try {
    await saveSettings({
      api_base: apiBase.value,
      api_key: apiKey.value,
      api_model: apiModel.value,
      vad_window_ms: String(vadWindowMs.value),
      vad_min_silence_ms: String(vadMinSilenceMs.value),
      vad_min_chunk_ms: String(vadMinChunkMs.value),
      vad_max_chunk_ms: String(vadMaxChunkMs.value),
    });
    emit("close");
  } finally {
    saving.value = false;
  }
}

// 「模型」页高级折叠内的转写参数保存：只存 4 个 vad key，不关闭面板
async function onSaveVad() {
  saving.value = true;
  try {
    await saveSettings({
      vad_window_ms: String(vadWindowMs.value),
      vad_min_silence_ms: String(vadMinSilenceMs.value),
      vad_min_chunk_ms: String(vadMinChunkMs.value),
      vad_max_chunk_ms: String(vadMaxChunkMs.value),
    });
    vadSaved.value = true;
    if (vadSaveTimer) clearTimeout(vadSaveTimer);
    vadSaveTimer = setTimeout(() => (vadSaved.value = false), 1500);
  } finally {
    saving.value = false;
  }
}

function resetVad() {
  vadWindowMs.value = 30;
  vadMinSilenceMs.value = 300;
  vadMinChunkMs.value = 1000;
  vadMaxChunkMs.value = 30000;
}

async function onSaveDictUrl() {
  try {
    await saveSettings({ dict_url_en: dictUrlEn.value, dict_url_ja: dictUrlJa.value });
    dictUrlSaved.value = true;
    if (dictUrlTimer) clearTimeout(dictUrlTimer);
    dictUrlTimer = setTimeout(() => (dictUrlSaved.value = false), 1500);
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 保存词典源地址失败:", e);
  }
}

function resetDictUrl() {
  dictUrlEn.value = "";
  dictUrlJa.value = "";
}

function onClick(e: MouseEvent) {
  // 点击遮罩关闭
  if (e.target === e.currentTarget) emit("close");
}
</script>

<template>
  <div v-if="open" class="overlay" @click="onClick">
    <div class="sheet">
      <div class="head">
        <span class="title">设置</span>
        <button class="close-btn" title="关闭" @click="emit('close')">
          <svg viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-1)" stroke-width="1.8"><path d="M6 6l12 12M18 6L6 18"/></svg>
        </button>
      </div>

      <div class="body">
        <nav class="nav">
          <button
            v-for="t in tabs"
            :key="t.key"
            class="tab"
            :class="{ active: activeTab === t.key }"
            @click="activeTab = t.key"
          >
            {{ t.label }}
          </button>
        </nav>
        <div class="content">

      <div class="section" v-show="activeTab === 'appearance'">
        <div class="section-label">外观</div>

        <div class="row">
          <span class="row-label">主题</span>
          <div class="segmented">
            <button
              class="segment"
              :class="{ active: theme === 'light' }"
              @click="emit('setTheme', 'light')"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>
              浅色
            </button>
            <button
              class="segment"
              :class="{ active: theme === 'dark' }"
              @click="emit('setTheme', 'dark')"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z"/></svg>
              深色
            </button>
            <button
              class="segment"
              :class="{ active: theme === 'system' }"
              @click="emit('setTheme', 'system')"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/></svg>
              跟随系统
            </button>
          </div>
        </div>
      </div>

      <div class="section" v-show="activeTab === 'playback'">
        <div class="section-label">播放</div>

        <div class="row">
          <span class="row-label">自动播放</span>
          <div class="segmented">
            <button
              class="segment"
              :class="{ active: pb.playback.autoplayNext }"
              @click="pb.playback.autoplayNext = true"
            >
              开
            </button>
            <button
              class="segment"
              :class="{ active: !pb.playback.autoplayNext }"
              @click="pb.playback.autoplayNext = false"
            >
              关
            </button>
          </div>
        </div>

        <p class="hint">开启后，列表循环模式下播完当前集会连播下一集；关闭则播完即停（取消循环）。</p>

        <div class="row">
          <span class="row-label">播放模式</span>
          <div class="segmented">
            <button
              class="segment"
              :class="{ active: pb.playback.playbackMode === 'broadcast' }"
              @click="pb.playback.playbackMode = 'broadcast'"
            >
              连播
            </button>
            <button
              class="segment"
              :class="{ active: pb.playback.playbackMode === 'intensive' }"
              @click="pb.playback.playbackMode = 'intensive'"
            >
              精听
            </button>
          </div>
        </div>
        <p class="hint">精听：每句结束自动暂停、可单句循环、可盲听（隐藏译文）。仅在有字幕时生效。</p>

        <div class="row">
          <span class="row-label">精听 · 自动暂停每句</span>
          <div class="segmented">
            <button
              class="segment"
              :class="{ active: pb.playback.intensiveAutoPause }"
              @click="pb.playback.intensiveAutoPause = true"
            >
              开
            </button>
            <button
              class="segment"
              :class="{ active: !pb.playback.intensiveAutoPause }"
              @click="pb.playback.intensiveAutoPause = false"
            >
              关
            </button>
          </div>
        </div>

        <div class="row">
          <span class="row-label">精听 · 单句循环</span>
          <div class="segmented">
            <button
              class="segment"
              :class="{ active: pb.playback.intensiveSentenceLoop }"
              @click="pb.playback.intensiveSentenceLoop = true"
            >
              开
            </button>
            <button
              class="segment"
              :class="{ active: !pb.playback.intensiveSentenceLoop }"
              @click="pb.playback.intensiveSentenceLoop = false"
            >
              关
            </button>
          </div>
        </div>

        <div class="row">
          <span class="row-label">精听 · 盲听</span>
          <div class="segmented">
            <button
              class="segment"
              :class="{ active: pb.playback.intensiveBlindListen }"
              @click="pb.playback.intensiveBlindListen = true"
            >
              开
            </button>
            <button
              class="segment"
              :class="{ active: !pb.playback.intensiveBlindListen }"
              @click="pb.playback.intensiveBlindListen = false"
            >
              关
            </button>
          </div>
        </div>
        <p class="hint">盲听开启后字幕浮层隐藏译文（临时原文态）；按住 H 键可临时显示译文。</p>
      </div>

      <div class="section" v-show="activeTab === 'subtitle'">
        <div class="section-label">字幕</div>

        <label class="field">
          <span>字号 {{ Math.round(captionStyle.fontScale * 100) }}%</span>
          <input
            type="range"
            min="0.8"
            max="1.6"
            step="0.1"
            v-model.number="captionStyle.fontScale"
          />
        </label>

        <label class="field">
          <span>颜色</span>
          <div class="cap-colors">
            <button
              v-for="c in capColors"
              :key="c"
              class="cap-chip"
              :class="{ active: captionStyle.color === c }"
              :style="{ background: c }"
              :title="c"
              @click="captionStyle.color = c"
            ></button>
            <label class="cap-chip custom">
              <input
                type="color"
                :value="captionStyle.color"
                @input="onCaptionColor"
                title="自定义颜色"
              />
            </label>
          </div>
        </label>

        <label class="field">
          <span>位置</span>
          <div class="segmented">
            <button
              class="segment"
              :class="{ active: captionStyle.position === 'top' }"
              @click="captionStyle.position = 'top'"
            >
              上
            </button>
            <button
              class="segment"
              :class="{ active: captionStyle.position === 'center' }"
              @click="captionStyle.position = 'center'"
            >
              中
            </button>
            <button
              class="segment"
              :class="{ active: captionStyle.position === 'bottom' }"
              @click="captionStyle.position = 'bottom'"
            >
              下
            </button>
          </div>
        </label>

        <label class="field">
          <span>背景不透明度 {{ Math.round(captionStyle.bgOpacity * 100) }}%</span>
          <input
            type="range"
            min="0"
            max="0.85"
            step="0.05"
            v-model.number="captionStyle.bgOpacity"
          />
        </label>

        <div
          class="cap-preview"
          :style="{
            color: captionStyle.color,
            fontSize: 19 * captionStyle.fontScale + 'px',
            background: `rgba(0, 0, 0, ${captionStyle.bgOpacity})`,
          }"
        >
          <span class="cap-preview-orig">这是一段字幕预览</span>
          <span class="cap-preview-trans">This is a subtitle preview</span>
        </div>

        <button class="rs-btn" @click="cap.resetCaptionStyle()">重置默认</button>
      </div>

      <div class="section" v-show="activeTab === 'shortcuts'">
        <div class="section-label">快捷键</div>
        <p class="hint">点击某项后按下新的组合键即可更换；按 Esc 取消录制。</p>
        <div class="sc-list">
          <div v-for="a in sc.shortcutActions" :key="a.name" class="sc-item">
            <span class="sc-label">{{ a.label }}</span>
            <span class="sc-controls">
              <button
                class="sc-key"
                :class="{ rec: sc.recording.value === a.name }"
                @click="startRecord(a.name)"
              >
                {{ sc.keysLabel(keysOf(a.name)) }}
              </button>
              <button class="sc-clear" title="清除" @click="sc.clearShortcut(a.name)">
                ×
              </button>
            </span>
          </div>
        </div>
        <button class="rs-btn" @click="sc.resetShortcuts()">重置默认快捷键</button>
      </div>

      <div class="section" v-show="activeTab === 'translate'">
        <div class="section-label">翻译</div>

        <label class="field">
          <span>服务商</span>
          <select
            :value="providerIdx"
            @change="applyProvider(Number(($event.target as HTMLSelectElement).value))"
          >
            <option :value="-1">自定义</option>
            <option v-for="(p, i) in providers" :key="i" :value="i">{{ p.label }}</option>
          </select>
        </label>

        <label class="field">
          <span>API 地址</span>
          <input v-model="apiBase" type="text" placeholder="https://api.deepseek.com/v1" />
        </label>

        <label class="field">
          <span>API Key</span>
          <div class="key-wrap">
  <input v-model="apiKey" :type="showKey ? 'text' : 'password'" placeholder="sk-..." />
  <button class="key-eye" title="显示/隐藏" @click="showKey = !showKey">
    <svg v-if="showKey" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.6"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></svg>
    <svg v-else viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.6"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
  </button>
</div>
<p v-if="envKey" class="env-hint">
  检测到系统环境变量 <code>ASPLAYER_API_KEY</code>，运行时将优先使用；此处可留空。
</p>
        </label>

        <label class="field">
          <span>模型</span>
          <input v-model="apiModel" type="text" placeholder="deepseek-chat" />
        </label>

        <p class="hint">选择服务商可自动填入地址与模型，Model 仍可手改。兼容所有 OpenAI 接口。</p>

        <button class="save-btn" :disabled="saving" @click="onSave">
          {{ saving ? "保存中…" : "保存" }}
        </button>

      <div class="section-divider"></div>
      <div class="section-label">本地翻译引擎（Ollama）</div>

      <label class="field">
        <span>Ollama 地址</span>
        <div class="key-wrap">
          <input v-model="ollamaBase" type="text" placeholder="http://localhost:11434" />
          <button class="key-eye" title="保存" @click="onSaveOllamaBase">保存</button>
        </div>
        <small class="field-desc">默认 http://localhost:11434，改端口后点「保存」。</small>
      </label>

      <div v-if="!ollamaConnected" class="local-warn">
        未检测到 Ollama 服务。请先<a href="https://ollama.com" target="_blank" rel="noopener">安装 Ollama</a> 并启动后点「重新检测」。
      </div>
      <div v-else class="local-ok">
        已连接 Ollama{{ ollamaInfo?.version ? `（v${ollamaInfo.version}）` : "" }}。
        <span v-if="ollamaInfo?.models.length">{{ ollamaInfo.models.length }} 个模型</span>
      </div>
      <div class="local-actions">
        <button class="rs-btn" @click="loadOllama">重新检测</button>
        <span v-if="ollamaBaseSaved" class="saved-hint">已保存</span>
      </div>

      <!-- 已拉取模型 -->
      <div v-if="ollamaInfo?.models.length" class="model-list">
        <div v-for="m in ollamaInfo.models" :key="m.name" class="sc-item">
          <span class="sc-label">{{ m.name }}</span>
          <span class="sc-controls">
            <button class="sc-key sel" :disabled="localPulling" @click="onUseLocal(m.name)">用这个翻译</button>
            <span class="dl-mid">{{ fmtBytes(m.size) }}</span>
          </span>
        </div>
      </div>

      <!-- 推荐模型下载 -->
      <div class="adv-item">推荐翻译模型</div>
      <div class="model-list">
        <div v-for="r in OLLAMA_RECOMMENDED" :key="r.model" class="sc-item">
          <span class="sc-label">{{ r.model }}</span>
          <span class="sc-controls">
            <template v-if="localPulling && ollamaPullState?.model === r.model">
              <span class="dl-mid">{{ localPercent }}%</span>
              <button class="sc-clear" title="取消" @click="onCancelPullLocal">×</button>
            </template>
            <template v-else>
              <span class="dl-mid">{{ r.label }}</span>
              <button class="sc-key" :disabled="localPulling || !ollamaConnected" @click="onPullLocal(r.model)">下载</button>
            </template>
          </span>
        </div>
      </div>
      <div v-if="localPulling && ollamaPullState?.model && !OLLAMA_RECOMMENDED.some((r) => r.model === ollamaPullState?.model)" class="model-list">
        <div class="sc-item">
          <span class="sc-label">{{ ollamaPullState.model }}</span>
          <span class="sc-controls">
            <span class="dl-mid">{{ localPercent }}%</span>
            <button class="sc-clear" title="取消" @click="onCancelPullLocal">×</button>
          </span>
        </div>
      </div>

      <div v-if="localPulling && ollamaPullState" class="model-progress">
        <div class="model-bar" :style="{ width: localPercent + '%' }"></div>
      </div>
      <p v-if="ollamaPullState?.status === 'failed' && ollamaPullState.error" class="dict-err">{{ ollamaPullState.error }}</p>
      </div>

      <div class="section" v-show="activeTab === 'model'">
        <div class="section-label">模型</div>

        <div v-if="ms.modelState.selected && !selectedFileExists" class="model-warn">
          尚未下载所选模型「ggml-{{ ms.modelState.selected }}.bin」，转写前请先下载，或在「翻译」页改用云端 API。
        </div>

        <div class="row model-current">
          <span class="row-label">当前模型</span>
          <span class="model-path">ggml-{{ ms.modelState.selected }}.bin</span>
        </div>

        <div class="model-list">
          <div v-for="m in ms.modelState.models" :key="m.size" class="sc-item">
            <span class="sc-label">
              {{ m.size }}（{{ MODEL_META[m.size] }}）
              <span v-if="m.selected" class="model-badge">已选</span>
            </span>

            <span v-if="m.status !== 'downloading'" class="sc-controls">
              <template v-if="m.file_exists">
                <button class="sc-key" :class="{ sel: m.selected }" @click="ms.select(m.size)">
                  {{ m.selected ? "当前" : "选为当前" }}
                </button>
                <button class="sc-clear" title="删除模型" @click="ms.remove(m.size)">×</button>
              </template>
              <button v-else class="sc-key" @click="ms.download(m.size)">下载</button>
            </span>
            <span v-else class="sc-controls">
              <span class="dl-mid">
                {{
                  m.total_bytes
                    ? Math.round((m.bytes_downloaded / m.total_bytes) * 100) + "%"
                    : "下载中"
                }}
              </span>
              <button class="sc-clear" title="取消" @click="ms.cancel(m.size)">×</button>
            </span>
          </div>
        </div>

        <div v-if="ms.modelState.activeSize" class="model-progress">
          <div class="model-bar" :style="{ width: activePercent + '%' }"></div>
        </div>

        <p class="hint">
          建议用 small（466MB）兼顾体积与 ASMR 识别率。无 N 卡可跳过本地模型，直接在「翻译」页配置云端 API。
        </p>

        <div class="adv-box">
          <button class="adv-toggle" @click="showAdv = !showAdv">
            <span>高级</span>
            <svg :class="{ open: showAdv }" viewBox="0 0 24 24" fill="none" :style="{ stroke: 'var(--fg-2)' }" stroke-width="1.8"><path d="M6 9l6 6 6-6"/></svg>
          </button>

          <div v-show="showAdv" class="adv-body">
            <div class="adv-item">转写分段参数</div>
            <label class="field">
              <span>单段最长时间</span>
              <input v-model.number="vadMaxChunkMs" type="number" min="1000" max="120000" />
              <small class="field-desc">一句话转写最长不超过它；越短，进度更新更勤、取消也越及时。</small>
            </label>
            <label class="field">
              <span>停顿判定</span>
              <input v-model.number="vadMinSilenceMs" type="number" min="100" max="5000" />
              <small class="field-desc">一句话停顿超过这段时间，就分成两段。</small>
            </label>
            <label class="field">
              <span>最小分段</span>
              <input v-model.number="vadMinChunkMs" type="number" min="200" max="10000" />
              <small class="field-desc">太短的内容会和前后并成一句，通常无需改动。</small>
            </label>
            <label class="field">
              <span>检测窗口</span>
              <input v-model.number="vadWindowMs" type="number" min="10" max="200" />
              <small class="field-desc">底层算法参数，保持默认即可。</small>
            </label>

            <div class="adv-actions">
              <button class="rs-btn" @click="resetVad">恢复默认</button>
              <button class="save-btn" :disabled="saving" @click="onSaveVad">
                {{ saving ? "保存中…" : "保存" }}
              </button>
            </div>
            <p class="saved-hint" v-if="vadSaved">已保存</p>
          </div>
        </div>
      </div>

      <div class="section" v-show="activeTab === 'dict'">
        <div class="section-label">词典</div>
        <p class="hint">右键字幕行中的单词即可在应用内查词。首次使用需下载对应语言词典，离线可用。</p>

        <div class="model-list">
          <div v-if="!dictStatuses.length" class="hint">加载中…</div>
          <div v-for="s in dictStatuses" :key="s.lang" class="dict-item">
            <div class="sc-item">
              <span class="sc-label">
                {{ s.lang === "en" ? "英文 ECDICT" : "日文 JMdict" }}
                <span v-if="s.status === 'done'" class="model-badge">已就绪</span>
              </span>

              <span v-if="s.status === 'downloading'" class="sc-controls">
                <span class="dl-mid">{{ dictPercent(s.lang) }}%</span>
                <button class="sc-clear" title="取消下载" @click="onDictCancel(s.lang)">×</button>
              </span>
              <span v-else-if="s.status === 'done'" class="sc-controls">
                <span class="dl-mid">{{ fmtBytes(s.db_bytes) }}</span>
              </span>
              <span v-else class="sc-controls">
                <button class="sc-key" @click="onDictDownload(s.lang)">{{ dictStatusText(s) }}</button>
              </span>
            </div>
            <p v-if="s.status === 'failed' && s.error" class="dict-err">{{ s.error }}</p>
          </div>
        </div>

        <div class="dict-source">
          <div class="dict-source-label">词典源地址（镜像）</div>
          <p class="hint">留空自动依次尝试官方源与内置镜像（国内可直接下载）；仅当内置源全部失效时，才需在此填写可用的镜像地址。</p>
          <label class="field">
            <span>英文 ECDICT</span>
            <input v-model="dictUrlEn" type="text" placeholder="https://..." />
          </label>
          <label class="field">
            <span>日文 JMdict</span>
            <input v-model="dictUrlJa" type="text" placeholder="http://..." />
          </label>
          <div class="dict-source-actions">
            <button class="rs-btn" @click="resetDictUrl">恢复默认</button>
            <button class="save-btn" :disabled="saving" @click="onSaveDictUrl">
              {{ saving ? "保存中…" : "保存" }}
            </button>
          </div>
          <p class="saved-hint" v-if="dictUrlSaved">已保存</p>
        </div>

        <div v-if="downloadingLang" class="model-progress">
          <div class="model-bar" :style="{ width: dictPercent(downloadingLang) + '%' }"></div>
        </div>

        <p class="hint">已下载原始文件与生成的词典体积见上文；任一下载中时，其余语言会稍后再下（一次只下载一种）。</p>
      </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: flex-start;
  justify-content: flex-end;
  z-index: 100;
}

.sheet {
  width: 360px;
  margin: 56px 16px 0 0;
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: 14px;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
  padding: 18px;
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
}

.title {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.close-btn {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--fg-2);
  border-radius: 8px;
  cursor: pointer;
}

.close-btn:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.close-btn svg {
  width: 16px;
  height: 16px;
}

.section {
  border-top: 1px solid var(--line);
  padding-top: 14px;
}

.section-label {
  font-size: 12px;
  color: var(--fg-3);
  margin-bottom: 12px;
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.row-label {
  font-size: 14px;
  color: var(--fg-1);
}

.segmented {
  display: flex;
  background: var(--bg-2);
  border-radius: 9px;
  padding: 3px;
  gap: 2px;
}

.segment {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--fg-2);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.segment svg {
  width: 14px;
  height: 14px;
}

.segment.active {
  background: var(--bg-1);
  color: var(--fg-1);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
  margin-bottom: 14px;
}

.field span {
  font-size: 12px;
  color: var(--fg-3);
}

.field input {
  width: 100%;
  font-size: 13px;
}

.field select {
  width: 100%;
  font-size: 13px;
  color: var(--fg-1);
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 6px 12px;
  outline: none;
  cursor: pointer;
  appearance: none;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23a1a1a6' stroke-width='2'><path d='M6 9l6 6 6-6'/></svg>");
  background-repeat: no-repeat;
  background-position: right 12px center;
}

.hint {
  font-size: 11px;
  color: var(--fg-3);
  margin-bottom: 14px;
  line-height: 1.5;
}

.save-btn {
  width: 100%;
  padding: 9px 0;
  border: none;
  border-radius: 9px;
  background: var(--accent);
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s ease;
}

.save-btn:hover:not(:disabled) {
  opacity: 0.9;
}

.save-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.cap-colors {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}

.cap-chip {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  border: 1px solid var(--line);
  padding: 0;
  cursor: pointer;
}

.cap-chip.active {
  box-shadow: 0 0 0 2px var(--bg-1), 0 0 0 4px var(--accent);
}

.cap-chip.custom {
  display: flex;
  align-items: center;
  justify-content: center;
  background: conic-gradient(
    #ff453a,
    #ff9f0a,
    #ffd60a,
    #30d158,
    #0a84ff,
    #bf5af2,
    #ff453a
  );
  overflow: hidden;
  position: relative;
}

.cap-chip.custom input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  opacity: 0;
  cursor: pointer;
  padding: 0;
  border: none;
}

.cap-preview {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 10px 14px;
  border-radius: 10px;
  margin-bottom: 14px;
  text-align: center;
  font-weight: 500;
}

.cap-preview-orig {
  line-height: 1.35;
}

.cap-preview-trans {
  opacity: 0.85;
  font-size: 0.78em;
  line-height: 1.35;
}

.rs-btn {
  width: 100%;
  padding: 8px 0;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: transparent;
  color: var(--fg-2);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.rs-btn:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.foot-hint {
  margin-top: 16px;
  font-size: 12px;
  color: var(--fg-3);
  border-top: 1px solid var(--line);
  padding-top: 12px;
}

.tabs {
  display: flex;
  gap: 4px;
  padding: 0 16px 10px;
  border-bottom: 1px solid var(--line);
}

.tab {
  flex: 1;
  padding: 8px 0;
  border: none;
  background: transparent;
  color: var(--fg-2);
  font-size: 13px;
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.tab:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.tab.active {
  background: var(--bg-2);
  color: var(--fg-1);
  font-weight: 600;
}

.sc-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.sc-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 12px;
  background: var(--bg-2);
  border-radius: 9px;
}

.sc-label {
  font-size: 13px;
  color: var(--fg-1);
}

.sc-controls {
  display: flex;
  align-items: center;
  gap: 6px;
}

.sc-key {
  min-width: 96px;
  padding: 6px 10px;
  border: 1px solid var(--line);
  border-radius: 7px;
  background: var(--bg-1);
  color: var(--fg-1);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease;
}

.sc-key:hover {
  border-color: var(--accent);
}

.sc-key.rec {
  border-color: var(--accent);
  background: var(--accent-dim);
  color: var(--accent);
  animation: sc-pulse 1s ease-in-out infinite;
}

.sc-clear {
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--fg-3);
  font-size: 14px;
  cursor: pointer;
}

.sc-clear:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

@keyframes sc-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.6;
  }
}

/* 弹窗居中覆盖 */
.overlay {
  align-items: center;
  justify-content: center;
}

.sheet {
  margin: 0;
  width: min(520px, calc(100vw - 48px));
  max-height: min(82vh, 760px);
  overflow-y: auto;
}

.body {
  display: flex;
  gap: 0;
}

.nav {
  width: 116px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  border-right: 1px solid var(--line);
  padding-right: 12px;
}

.nav .tab {
  flex: 0 0 auto;
  width: 100%;
  text-align: left;
  padding: 8px 10px;
  border-radius: 7px;
}

.content {
  flex: 1;
  min-width: 0;
  padding-left: 14px;
}

.section {
  border-top: none;
  padding-top: 0;
}

.close-btn {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: var(--bg-2);
  color: var(--fg-1);
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s ease, filter 0.15s ease;
}

.close-btn:hover {
  background: var(--bg-2);
  filter: brightness(1.2);
}

.close-btn svg {
  width: 15px;
  height: 15px;
  stroke-width: 2;
}

.sheet {
  height: min(480px, 82vh);
  padding: 16px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.body {
  flex: 1;
  min-height: 0;
}

.content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-right: 4px;
}

.field .key-wrap {
  position: relative;
}

.field .key-wrap input {
  padding-right: 36px;
}

.key-eye {
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  border-radius: 7px;
  cursor: pointer;
}

.key-eye:hover {
  background: var(--bg-2);
}

.key-eye svg {
  width: 16px;
  height: 16px;
}

.env-hint {
  font-size: 11px;
  color: var(--fg-3);
  line-height: 1.5;
  margin-top: -6px;
}

.env-hint code {
  font-family: inherit;
  color: var(--accent);
}

.model-current {
  margin-bottom: 12px;
}
.model-path {
  font-size: 13px;
  color: var(--fg-1);
  font-variant-numeric: tabular-nums;
}
.model-warn {
  background: var(--accent-dim);
  color: var(--accent);
  border: 1px solid var(--accent);
  border-radius: 9px;
  padding: 8px 12px;
  font-size: 12px;
  line-height: 1.5;
  margin-bottom: 12px;
}
.model-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 12px;
}
.model-badge {
  margin-left: 6px;
  font-size: 11px;
  color: var(--accent);
  background: var(--accent-dim);
  padding: 2px 6px;
  border-radius: 5px;
}
.sc-key.sel {
  border-color: var(--accent);
  color: var(--accent);
}
.dl-mid {
  font-size: 12px;
  color: var(--fg-2);
  font-variant-numeric: tabular-nums;
}
.model-progress {
  height: 6px;
  background: var(--bg-2);
  border-radius: 3px;
  overflow: hidden;
  margin-bottom: 12px;
}
.model-bar {
  height: 100%;
  background: var(--accent);
  transition: width 0.2s ease;
}

.adv-box {
  margin-top: 6px;
  border-top: 1px solid var(--line);
  padding-top: 10px;
}

.adv-toggle {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: transparent;
  color: var(--fg-2);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.adv-toggle:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.adv-toggle svg {
  width: 14px;
  height: 14px;
  transition: transform 0.2s ease;
}

.adv-toggle svg.open {
  transform: rotate(180deg);
}

.adv-body {
  margin-top: 6px;
  padding: 4px 2px 0;
}

.adv-item {
  font-size: 12px;
  font-weight: 600;
  color: var(--fg-2);
  padding: 8px 2px;
}

.field-desc {
  display: block;
  font-size: 11px;
  color: var(--fg-3);
  line-height: 1.5;
}

.adv-actions {
  display: flex;
  gap: 10px;
  margin-top: 4px;
}

.adv-actions .rs-btn {
  flex: 1;
}

.adv-actions .save-btn {
  flex: 2;
}

.saved-hint {
  font-size: 12px;
  color: var(--accent);
  margin-top: 8px;
  text-align: right;
}

.dict-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.dict-err {
  font-size: 11px;
  line-height: 1.5;
  color: #e5484d;
  padding: 0 2px;
  white-space: pre-wrap;
}
.section-divider {
  margin: 16px 0 12px;
  border-top: 1px solid var(--line);
  padding-top: 12px;
}
.local-warn {
  font-size: 12px;
  line-height: 1.5;
  color: var(--accent);
  background: var(--accent-dim);
  border: 1px solid var(--accent);
  border-radius: 9px;
  padding: 8px 12px;
  margin-bottom: 12px;
}
.local-warn a { color: var(--accent); }
.local-ok {
  font-size: 12px;
  color: var(--fg-2);
  margin-bottom: 10px;
}
.local-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
}
.local-actions .rs-btn { width: auto; padding: 6px 14px; flex: 0 0 auto; }
.local-actions .saved-hint { margin: 0; }

.dict-source {
  margin-top: 6px;
  border-top: 1px solid var(--line);
  padding-top: 10px;
  margin-bottom: 12px;
}

.dict-source-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--fg-2);
  padding: 8px 2px;
}

.dict-source-actions {
  display: flex;
  gap: 10px;
  margin-top: 4px;
}

.dict-source-actions .rs-btn {
  flex: 1;
}

.dict-source-actions .save-btn {
  flex: 2;
}

</style>
