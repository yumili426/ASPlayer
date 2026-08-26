<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import CaptionPanel from "./CaptionPanel.vue";
import type { MediaItem } from "../types";
import { useSubtitle } from "../stores/subtitle";
import { usePlayback } from "../stores/playback";
import { transcribeMedia, translateMedia } from "../api/subtitle";

const sub = useSubtitle();
const pb = usePlayback();

const props = defineProps<{ item: MediaItem | null; items: MediaItem[] }>();
const emit = defineEmits<{
  import: [];
  play: [item: MediaItem];
  settings: [];
  togglePlaylist: [];
  toggleSubtitle: [];
}>();

const mediaEl = ref<HTMLVideoElement | HTMLAudioElement | null>(null);
const playing = ref(false);
const duration = ref(0);
const captionOn = ref(true);
const rate = ref(1);
const volume = ref(1);
const muted = ref(false);
const appWindow = getCurrentWindow();
const isFullscreen = ref(false);
const volTrack = ref<HTMLDivElement | null>(null);
const showVolOsd = ref(false);
const showVolPop = ref(false);
let volOsdTimer: number | null = null;
let volDragging = false;
const rateSteps = [0.5, 0.75, 1, 1.25, 1.5, 2];
const rateText = computed(() => `${rate.value}x`);
const volumePct = computed(() => Math.round(volume.value * 100));
const volFillPct = computed(() => Math.min(volume.value, 1) * 100);
const volTicks = [0, 25, 50, 75, 100];
const showRateMenu = ref(false);
const rateMenuEl = ref<HTMLDivElement | null>(null);

const src = computed(() => (props.item ? convertFileSrc(props.item.path) : ""));

function fmt(t: number): string {
  if (!isFinite(t)) return "0:00";
  const total = Math.floor(t);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

function restorePosition() {
  const el = mediaEl.value;
  if (!el || !props.item) return;
  el.playbackRate = rate.value;
  el.loop = pb.playback.loopMode === "single";
  if (props.item.playback_position > 0) {
    el.currentTime = props.item.playback_position / 1000;
  }
  if (el.paused) el.play().catch(() => {});
}
onMounted(restorePosition);
onMounted(() => document.addEventListener("click", onRateDocClick));
onMounted(() => {
  appWindow
    .isFullscreen()
    .then((v) => (isFullscreen.value = v))
    .catch(() => {});
});
onBeforeUnmount(() => document.removeEventListener("click", onRateDocClick));
watch(
  () => props.item?.id,
  (id) => {
    requestAnimationFrame(restorePosition);
    if (id) sub.load(id);
    else sub.reset();
  }
);

watch(mediaEl, (el) => {
  if (el) applyVolume();
});

async function doTranscribe(withTranslate: boolean) {
  if (!props.item) return;
  sub.setStatus("transcribing", "transcribe", 0, "正在转写…");
  if (withTranslate) sub.requestAutoTranslate(props.item.id);
  await transcribeMedia(props.item.id);
}

async function doTranslate() {
  if (!props.item) return;
  sub.setStatus("translating", "translate", 0, "正在翻译…");
  await translateMedia(props.item.id);
}

function togglePlay() {
  const el = mediaEl.value;
  if (!el) return;
  if (el.paused) el.play();
  else el.pause();
}

function seekBy(delta: number) {
  const el = mediaEl.value;
  if (!el) return;
  el.currentTime = Math.max(0, Math.min(el.duration || 0, el.currentTime + delta));
}

function onSeekInput(e: Event) {
  const el = mediaEl.value;
  if (!el) return;
  el.currentTime = Number((e.target as HTMLInputElement).value);
}

function next() {
  if (!props.item) return;
  const idx = props.items.findIndex((m) => m.id === props.item!.id);
  const nxt = props.items[(idx + 1) % props.items.length];
  if (nxt) emit("play", nxt);
}
function prev() {
  if (!props.item) return;
  const idx = props.items.findIndex((m) => m.id === props.item!.id);
  const prv = props.items[(idx - 1 + props.items.length) % props.items.length];
  if (prv) emit("play", prv);
}

function applyVolume() {
  const el = mediaEl.value;
  if (!el) return;
  // 原生音量上限即 100%。Web Audio 增益在 Tauri WebView 下会造成整条静音且不可逆，
  // 故移除增益方案，音量均走原生路径以保证始终发声。
  el.volume = volume.value;
}

function flashVolumeOsd() {
  showVolOsd.value = true;
  if (volOsdTimer) clearTimeout(volOsdTimer);
  volOsdTimer = window.setTimeout(() => (showVolOsd.value = false), 900);
}

function setVolume(v: number) {
  volume.value = Math.max(0, Math.min(1, v));
  const el = mediaEl.value;
  if (el) {
    el.volume = volume.value;
    if (volume.value > 0 && el.muted) el.muted = false;
  }
  flashVolumeOsd();
}

function toggleMute() {
  const el = mediaEl.value;
  if (!el) return;
  el.muted = !el.muted;
}

function adjustVolume(delta: number) {
  setVolume(volume.value + delta);
}

function onVolumeChange() {
  const el = mediaEl.value;
  if (!el) return;
  muted.value = el.muted;
}

function onPlay() {
  playing.value = true;
}

function onVolWheel(e: WheelEvent) {
  const delta = e.deltaY > 0 ? -0.1 : 0.1;
  setVolume(volume.value + delta);
}

function updateVolFromPointer(e: PointerEvent) {
  const track = volTrack.value;
  if (!track) return;
  const rect = track.getBoundingClientRect();
  const ratio = 1 - (e.clientY - rect.top) / rect.height;
  setVolume(ratio * 2);
}

function onVolPointerDown(e: PointerEvent) {
  volDragging = true;
  volTrack.value?.setPointerCapture?.(e.pointerId);
  updateVolFromPointer(e);
}

function onVolPointerMove(e: PointerEvent) {
  if (volDragging) updateVolFromPointer(e);
}

function onVolPointerUp() {
  volDragging = false;
}

async function toggleFullscreen() {
  try {
    const next = !isFullscreen.value;
    await appWindow.setFullscreen(next);
    isFullscreen.value = next;
  } catch (e) {
    console.error("[ASPlayer] 切换全屏失败:", e);
  }
}

function applyRate(r: number) {
  rate.value = r;
  const el = mediaEl.value;
  if (el) el.playbackRate = r;
}

function toggleRateMenu() {
  showRateMenu.value = !showRateMenu.value;
}

function selectRate(r: number) {
  applyRate(r);
  showRateMenu.value = false;
}

function onRateDocClick(e: MouseEvent) {
  if (!showRateMenu.value) return;
  const target = e.target as Node;
  if (rateMenuEl.value && !rateMenuEl.value.contains(target)) {
    showRateMenu.value = false;
  }
}

function toggleLoop() {
  pb.playback.loopMode = pb.playback.loopMode === "single" ? "list" : "single";
  const el = mediaEl.value;
  if (el) el.loop = pb.playback.loopMode === "single";
}

function onEnded() {
  // 单曲循环由 HTML loop（el.loop=true）自动无限重播，通常不会触发 ended
  if (pb.playback.loopMode === "single") return;
  // 列表循环：关闭自动播放则播完即停
  if (!pb.playback.autoplayNext) return;
  next();
}

let lastSave = 0;
function onTimeUpdate() {
  const el = mediaEl.value;
  if (!el || !props.item) return;
  sub.setTime(el.currentTime);
  const now = Date.now();
  if (now - lastSave > 3000) {
    lastSave = now;
    import("@tauri-apps/api/core")
      .then(({ invoke }) =>
        invoke("save_playback_position", {
          id: props.item!.id,
          positionMs: Math.round(el.currentTime * 1000),
        })
      )
      .catch(() => {});
  }
}

defineExpose({ togglePlay, seekBy, next, prev, toggleMute, adjustVolume, toggleFullscreen });
</script>


<template>
  <main class="stage">
    <div class="stage-topbar">
      <span class="stage-title">ASPlayer</span>
      <div class="toolbar">
        <button class="iconbtn" title="播放列表面板" @click="emit('togglePlaylist')"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/></svg></button>
        <button class="iconbtn" title="字幕面板" @click="emit('toggleSubtitle')" :class="{ active: captionOn }"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 15h4M11 10h6"/></svg></button>
        <button class="iconbtn" title="转写" @click="doTranscribe(false)" :disabled="!item"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v10"/><path d="m8 9 4 4 4-4"/><path d="M4 17v2h16v-2"/></svg></button>
        <button class="iconbtn" title="转写并翻译" @click="doTranscribe(true)" :disabled="!item"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 6 6"/><path d="m4 14 6-6 2-3"/><path d="M2 5h12"/><path d="M7 2h1"/><path d="m22 22-5-10-5 10"/><path d="M14 18h6"/></svg></button>
        <button class="iconbtn" title="翻译" @click="doTranslate" :disabled="!item"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12m0 0 4-4m-4 4-4-4M4 21h16"/></svg></button>
        <button class="iconbtn" title="设置" @click="emit('settings')"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></svg></button>
      </div>
    </div>

    <div class="canvas">
      <CaptionPanel
        v-if="captionOn && item"
        :subtitles="sub.subtitles.value"
        :current-time="sub.currentTime.value"
        :status="sub.status.value"
      />
      <div v-if="!item" class="empty">
        <div class="empty-badge">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.2">
            <path d="M4 13a8 8 0 0 1 16 0M12 13v5"/>
            <rect x="3" y="13" width="4" height="6" rx="1.5"/>
            <rect x="17" y="13" width="4" height="6" rx="1.5"/>
          </svg>
        </div>
        <p class="empty-text">还没有在播放</p>
        <p class="empty-sub">从右侧播放列表选择，或导入你的媒体文件</p>
        <button class="open-btn" @click="emit('import')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 7h4l2-2h4l2 2h4v11H4z"/></svg>
          打开文件
        </button>
      </div>

      <div v-else class="playing">
        <video
          v-if="item.media_type === 'video'"
          ref="mediaEl" :src="src" controls
          @play="onPlay" @pause="playing = false"
          @timeupdate="onTimeUpdate"
          @loadedmetadata="duration = ($event.target as HTMLVideoElement).duration"
          @ended="onEnded"
          @volumechange="onVolumeChange"
        ></video>
        <div v-else class="artwork">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.1"><path d="M4 13a8 8 0 0 1 16 0M12 13v5"/><rect x="3" y="13" width="4" height="6" rx="1.5"/><rect x="17" y="13" width="4" height="6" rx="1.5"/></svg>
        </div>
        <audio
          v-if="item.media_type === 'audio'"
          ref="mediaEl" :src="src"
          @play="onPlay" @pause="playing = false"
          @timeupdate="onTimeUpdate"
          @loadedmetadata="duration = ($event.target as HTMLAudioElement).duration"
          @ended="onEnded"
          @volumechange="onVolumeChange"
        ></audio>
        <p class="now-title">{{ item.title }}</p>

        <Transition name="fade">
          <div v-if="showVolOsd" class="vol-osd">
            <svg class="vol-osd-icon" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4.7a1 1 0 0 0-1.7-.7L5 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2l4.3 4a1 1 0 0 0 1.7-.7z"/><path d="M15 9.3a4 4 0 0 1 0 5.4"/></svg>
            <span class="vol-osd-num">{{ volumePct }}%</span>
          </div>
        </Transition>
      </div>
    </div>

    <div class="controls">
      <div class="seek-row">
        <span class="time">{{ fmt(sub.currentTime.value) }}</span>
        <input class="slider" type="range" min="0" :max="duration || 0" step="0.1" :value="sub.currentTime.value" :disabled="!item" @input="onSeekInput" />
        <span class="time">{{ fmt(duration) }}</span>
      </div>
      <div class="btn-row">
        <div class="btn-group">
          <button class="ctl" title="上一首" :disabled="!item" @click="prev"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M6 5v14M19 6l-8 6 8 6z"/></svg></button>
          <button class="ctl play" :disabled="!item" :title="playing ? '暂停' : '播放'" @click="togglePlay">
            <svg v-if="playing" width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M8 5v14M16 5v14"/></svg>
            <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M7 5l12 7-12 7z"/></svg>
          </button>
          <button class="ctl" title="下一首" :disabled="!item" @click="next"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M18 5v14M5 6l8 6-8 6z"/></svg></button>
        </div>
        <div class="flex-spacer"></div>
            <div class="btn-group">
              <div class="rate-wrap" ref="rateMenuEl">
                <button class="ctl rate-btn" :disabled="!item" title="倍速" @click.stop="toggleRateMenu">{{ rateText }}</button>
                <Transition name="rate-pop">
                  <div v-if="showRateMenu && item" class="rate-menu">
                    <div class="rate-menu-title">播放速度</div>
                    <div class="rate-list">
                      <div
                        v-for="r in rateSteps"
                        :key="r"
                        class="rate-item"
                        :class="{ active: r === rate }"
                        @click.stop="selectRate(r)"
                      >
                        <span>{{ r }}x</span>
                        <svg v-if="r === rate" width="14" height="14" viewBox="0 0 24 24" fill="none" style="stroke:var(--accent)" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                      </div>
                    </div>
                  </div>
                </Transition>
              </div>
              <button class="ctl" :disabled="!item" :title="pb.playback.loopMode === 'single' ? '单曲循环' : '列表循环'" @click="toggleLoop">
                <svg v-if="pb.playback.loopMode === 'single'" fill="none" viewBox="0 0 24 24" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M17 2l4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="M7 22l-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/><path d="M11 10h1v4"/></svg>
                <svg v-else fill="none" viewBox="0 0 24 24" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M17 2l4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="M7 22l-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/></svg>
              </button>
            </div>
        <div class="btn-group">
          <button class="ctl seek" title="后退 15 秒" :disabled="!item" @click="seekBy(-15)">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
            <span class="seek-num">15</span>
          </button>
          <button class="ctl seek" title="前进 15 秒" :disabled="!item" @click="seekBy(15)">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/></svg>
            <span class="seek-num">15</span>
          </button>
        </div>
        <div class="btn-group">
          <div class="vol-wrap" @mouseenter="showVolPop = true" @mouseleave="showVolPop = false" @wheel.prevent="onVolWheel">
            <button class="ctl" :disabled="!item" :title="muted || volume === 0 ? '取消静音' : '静音'" @click="toggleMute">
              <svg v-if="muted || volume === 0" width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4.7a1 1 0 0 0-1.7-.7L5 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2l4.3 4a1 1 0 0 0 1.7-.7z"/><path d="m16 9 6 6"/><path d="m22 9-6 6"/></svg>
              <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4.7a1 1 0 0 0-1.7-.7L5 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2l4.3 4a1 1 0 0 0 1.7-.7z"/><path d="M15 9.3a4 4 0 0 1 0 5.4"/></svg>
            </button>
            <Transition name="vol-pop">
              <div v-if="showVolPop" class="vol-pop">
                <div class="vol-ctl">
                  <span class="vol-pct">{{ volumePct }}%</span>
                  <div
                    class="vol-track"
                    ref="volTrack"
                    title="音量"
                    @pointerdown="onVolPointerDown"
                    @pointermove="onVolPointerMove"
                    @pointerup="onVolPointerUp"
                    @pointercancel="onVolPointerUp"
                  >
                    <div class="vol-fill" :style="{ height: volFillPct + '%' }"></div>
                    <span v-for="t in volTicks" :key="t" class="vol-tick" :style="{ bottom: (t / 100 * 100) + '%' }"></span>
                  </div>
                </div>
              </div>
            </Transition>
          </div>
          <button class="ctl" :disabled="!item" :title="isFullscreen ? '退出全屏' : '全屏'" @click="toggleFullscreen">
            <svg v-if="isFullscreen" width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3v3a2 2 0 0 1-2 2H3"/><path d="M21 8h-3a2 2 0 0 1-2-2V3"/><path d="M3 16h3a2 2 0 0 1 2 2v3"/><path d="M16 21v-3a2 2 0 0 1 2-2h3"/></svg>
            <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M21 8V5a2 2 0 0 0-2-2h-3"/><path d="M3 16v3a2 2 0 0 0 2 2h3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/></svg>
          </button>
        </div>
      </div>
    </div>
  </main>
</template>

<style scoped>
.stage {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: #000;
}

.stage-topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  min-height: 52px;
  background: var(--bg-1);
  border-bottom: 1px solid var(--line);
}

.stage-title {
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.02em;
}

.toolbar {
  display: flex;
  gap: 2px;
}

.iconbtn {
  width: 32px;
  height: 32px;
  flex: 0 0 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--fg-1);
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.iconbtn:hover {
  background: var(--bg-2);
}

.iconbtn svg {
  width: 16px;
  height: 16px;
  flex: 0 0 16px;
  display: block;
  stroke: var(--fg-2);
}

.iconbtn:hover svg {
  stroke: var(--fg-1);
}

.iconbtn.active svg {
  stroke: var(--accent);
}

.iconbtn:disabled {
  opacity: 0.35;
  cursor: default;
}

.iconbtn:disabled:hover {
  background: transparent;
}

.canvas {
  position: relative;
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 0;
  overflow: hidden;
}

.empty {
  text-align: center;
  padding: 20px;
}

.empty-badge {
  width: 84px;
  height: 84px;
  margin: 0 auto 22px;
  border-radius: 20px;
  background: linear-gradient(160deg, var(--bg-2), var(--bg-1));
  border: 1px solid var(--line);
  display: flex;
  align-items: center;
  justify-content: center;
}

.empty-badge svg {
  width: 36px;
  height: 36px;
  color: var(--fg-3);
}

.empty-text {
  font-size: 20px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--fg-1);
}

.empty-sub {
  margin-top: 8px;
  font-size: 13px;
  color: var(--fg-3);
}

.open-btn {
  margin-top: 26px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 11px 24px;
  border: none;
  border-radius: 12px;
  background: var(--accent);
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s ease, transform 0.1s ease;
}

.open-btn:hover {
  opacity: 0.9;
}

.open-btn:active {
  transform: scale(0.97);
}

.open-btn svg {
  width: 16px;
  height: 16px;
}

.playing {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  width: 100%;
  padding: 16px;
}

.playing video {
  max-width: 100%;
  max-height: calc(100vh - 220px);
  border-radius: var(--radius-card);
  background: #000;
  outline: none;
}

.artwork {
  width: min(180px, 40vh);
  aspect-ratio: 1;
  border-radius: 18px;
  background: linear-gradient(160deg, var(--bg-2), var(--bg-1));
  border: 1px solid var(--line);
  display: flex;
  align-items: center;
  justify-content: center;
}

.artwork svg {
  width: 40%;
  height: 40%;
  color: var(--fg-3);
}

.now-title {
  color: var(--fg-1);
  font-size: 15px;
  font-weight: 500;
  text-align: center;
  max-width: 90%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.controls {
  padding: 10px 16px 14px;
  background: var(--bg-1);
  border-top: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.seek-row {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
}

.time {
  color: var(--fg-2);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  min-width: 40px;
  text-align: center;
}

.slider {
  flex: 1;
  appearance: none;
  height: 4px;
  border-radius: var(--radius-pill);
  background: var(--bg-2);
  border: none;
  outline: none;
  cursor: pointer;
}

.slider::-webkit-slider-thumb {
  appearance: none;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
}

.btn-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.btn-group {
  display: flex;
  align-items: center;
  gap: 6px;
}

.flex-spacer {
  flex: 1;
}

.ctl {
  width: 36px;
  height: 36px;
  flex: 0 0 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--fg-2);
  border-radius: 10px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.ctl:hover:not(:disabled) {
  background: var(--bg-2);
  color: var(--fg-1);
}

.ctl:disabled {
  opacity: 0.4;
  cursor: default;
}

.ctl svg {
  width: 18px;
  height: 18px;
  flex: 0 0 18px;
  display: block;
  stroke: var(--fg-2);
}

.vol-wrap {
  position: relative;
}

.vol-pop {
  position: absolute;
  bottom: calc(100% + 8px);
  left: 50%;
  transform: translateX(-50%);
  padding: 8px 4px;
  background: var(--bg-1);
  border: 1px solid var(--line);
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
}

/* 透明连接区：占住按钮与浮层之间的空隙，避免鼠标移动时误触发 mouseleave */
.vol-pop::before {
  content: "";
  position: absolute;
  left: 0;
  right: 0;
  top: 100%;
  height: 8px;
}

.vol-pop-enter-active,
.vol-pop-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}

.vol-pop-enter-from,
.vol-pop-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(4px);
}

.vol-ctl {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
  width: 42px;
  height: 140px;
  justify-content: flex-end;
}

.vol-pct {
  width: 100%;
  text-align: center;
  font-size: 11px;
  color: var(--fg-1);
  font-variant-numeric: tabular-nums;
  line-height: 1;
  user-select: none;
}

.vol-track {
  position: relative;
  width: 4px;
  flex: 1;
  min-height: 0;
  border-radius: 2px;
  background: var(--bg-2);
  cursor: pointer;
  overflow: hidden;
}

.vol-fill {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--accent);
  border-radius: 3px;
  transition: height 0.06s linear;
}

.vol-tick {
  position: absolute;
  left: 0;
  right: 0;
  height: 1px;
  background: var(--bg-1);
  opacity: 0.65;
  pointer-events: none;
}

.vol-osd {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  z-index: 30;
  display: flex;
  align-items: center;
  gap: 7px;
  background: rgba(0, 0, 0, 0.5);
  border-radius: 9px;
  padding: 7px 14px;
  pointer-events: none;
}

.vol-osd-icon {
  width: 16px;
  height: 16px;
}

.vol-osd-num {
  color: #fff;
  font-size: 18px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.01em;
  line-height: 1;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.18s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.ctl.seek {
  position: relative;
}

.ctl.seek .seek-num {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  font-size: 9px;
  font-weight: 600;
  line-height: 1;
  color: var(--fg-2);
  pointer-events: none;
}

.ctl.seek:hover .seek-num {
  color: var(--fg-1);
}

.ctl.play {
  width: 44px;
  height: 44px;
  border-radius: 50%;
  color: #fff;
  background: var(--accent);
}

.ctl.play:hover:not(:disabled) {
  background: var(--accent);
  opacity: 0.9;
}

.rate-btn {
  width: auto;
  padding: 0 9px;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  font-weight: 600;
}

.rate-wrap {
  position: relative;
  display: flex;
}

.rate-menu {
  position: absolute;
  bottom: calc(100% + 10px);
  right: 0;
  min-width: 132px;
  padding: 6px;
  background: var(--bg-glass);
  border: 1px solid var(--line);
  border-radius: 14px;
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(16px);
  z-index: 40;
}

.rate-menu-title {
  padding: 4px 10px 6px;
  font-size: 11px;
  color: var(--fg-3);
}

.rate-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.rate-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 7px 10px;
  border-radius: 9px;
  font-size: 13px;
  color: var(--fg-2);
  cursor: pointer;
  transition: background 0.12s ease, color 0.12s ease;
}

.rate-item:hover {
  background: var(--bg-2);
  color: var(--fg-1);
}

.rate-item.active {
  color: var(--accent);
  font-weight: 600;
}

.rate-item svg {
  flex: 0 0 14px;
}

.rate-pop-enter-active,
.rate-pop-leave-active {
  transition: opacity 0.12s ease, transform 0.12s ease;
}

.rate-pop-enter-from,
.rate-pop-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

</style>
