<script setup lang="ts">
import { ref, watch } from "vue";
import { getSettings, saveSettings } from "../api/subtitle";

const props = defineProps<{ open: boolean; theme: string }>();
const emit = defineEmits<{ close: []; setTheme: [theme: "light" | "dark"] }>();

const apiBase = ref("");
const apiKey = ref("");
const apiModel = ref("deepseek-chat");
const saving = ref(false);

async function load() {
  try {
    const s = await getSettings();
    apiBase.value = s.api_base ?? "https://api.deepseek.com/v1";
    apiKey.value = s.api_key ?? "";
    apiModel.value = s.api_model ?? "deepseek-chat";
  } catch {
    /* ignore */
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) load();
  }
);

async function onSave() {
  saving.value = true;
  try {
    await saveSettings({
      api_base: apiBase.value,
      api_key: apiKey.value,
      api_model: apiModel.value,
    });
    emit("close");
  } finally {
    saving.value = false;
  }
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
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M6 6l12 12M18 6L6 18"/></svg>
        </button>
      </div>

      <div class="section">
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
          </div>
        </div>
      </div>

      <div class="section">
        <div class="section-label">翻译</div>

        <label class="field">
          <span>API 地址</span>
          <input v-model="apiBase" type="text" placeholder="https://api.deepseek.com/v1" />
        </label>

        <label class="field">
          <span>API Key</span>
          <input v-model="apiKey" type="password" placeholder="sk-..." />
        </label>

        <label class="field">
          <span>模型</span>
          <input v-model="apiModel" type="text" placeholder="deepseek-chat" />
        </label>

        <p class="hint">优先读取环境变量 ASPLAYER_API_BASE / ASPLAYER_API_KEY，其次用这里保存的值。</p>

        <button class="save-btn" :disabled="saving" @click="onSave">
          {{ saving ? "保存中…" : "保存" }}
        </button>
      </div>

      <div class="foot-hint">更多设置项将在后续里程碑加入（快捷键、字幕样式等）</div>
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

.foot-hint {
  margin-top: 16px;
  font-size: 12px;
  color: var(--fg-3);
  border-top: 1px solid var(--line);
  padding-top: 12px;
}
</style>
