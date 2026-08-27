/**
 * 悬浮窗字幕推送引擎（设计 §4，Bug #4 根治）。
 * 唯一职责：根据媒体元素时钟计算当前句 → 内容变化才推给悬浮窗。
 * 三重时间源：timeupdate（常规）/ seeked（跳转后强制）/ 播放中 500ms 轮询（兜底）；
 * 内容级去重：与上次实际推送的 文本+start_ms 全等比较，序号类状态彻底退场。
 */
import { pushOverlaySubtitle } from "./api/overlay";
import { useSubtitle } from "./stores/subtitle";
import { overlayPrefs } from "./stores/overlayPrefs";
import type { Subtitle } from "./types";

const GAP_FADE_MS = 5000;
const POLL_MS = 500;
/** 已清屏哨兵：区别于"从未推送"（null） */
const CLEAR = { key: "\0clear\0" };

const sub = useSubtitle();

let media: HTMLMediaElement | null = null;
let enabled = false;
let pollTimer: number | null = null;
let abortCtrl: AbortController | null = null;
let lastKey: string | null = null; // 上次推送的内容指纹；CLEAR.key 或 `${start}${text}${tr}`
let gapSince = -1;                // 进入句间空隙的时间戳；-1 = 不在空隙

function sentenceAt(tMs: number): Subtitle | null {
  return (
    sub.subtitles.value.find((s) => tMs >= s.start_ms && tMs < s.end_ms) ?? null
  );
}

function pushClear(): void {
  gapSince = -1;
  lastKey = CLEAR.key;
  pushOverlaySubtitle("", "", 0).catch(() => {});
}

/** 句间空隙行为（设计 §4.5）：keep-last 保留上句不动；fade-5s 满 5s 清屏 */
function tickGap(inGap: boolean): void {
  if (!inGap) {
    gapSince = -1;
    return;
  }
  if (lastKey === CLEAR.key) return;
  if (overlayPrefs.gap_behavior !== "fade-5s") return;
  if (gapSince < 0) {
    gapSince = Date.now();
    return;
  }
  if (Date.now() - gapSince >= GAP_FADE_MS) pushClear();
}

/** 核心：算当前句并与上次推送比对。force 忽略去重缓存（seek 场景） */
export function sync(force = false): void {
  if (!enabled || !media) return;
  const tMs = Math.round(media.currentTime * 1000);
  const sentence = sentenceAt(tMs);
  tickGap(sentence === null);
  if (!sentence) return;
  const key = `${sentence.start_ms}${sentence.text}${sentence.translation}`;
  if (!force && lastKey === key) return;
  lastKey = key;
  pushOverlaySubtitle(sentence.text, sentence.translation, sentence.start_ms).catch(() => {});
}

function startPolling(): void {
  if (pollTimer !== null) return;
  pollTimer = window.setInterval(() => sync(false), POLL_MS);
}

function stopPolling(): void {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer);
    pollTimer = null;
  }
}

/**
 * PlayerStage 把 mediaEl ref 变化喂进来（video/audio 切换或置 null）。
 * 所有 seek 入口（面板点击/进度条/±15s/上下一句/悬浮窗请求）都作用于同一元素，
 * 因此只需在此监听 seeking/seeked 即覆盖全部来源，无需改造各个入口。
 */
export function attachMedia(el: HTMLMediaElement | null): void {
  abortCtrl?.abort();
  stopPolling();
  media = el;
  lastKey = null; // 元素更换视为内容全变，下次必推
  gapSince = -1;
  if (!el) return;
  abortCtrl = new AbortController();
  const opt = { signal: abortCtrl.signal };
  el.addEventListener("timeupdate", () => sync(false), opt);
  el.addEventListener("seeking", () => { gapSince = -1; }, opt);
  el.addEventListener("seeked", () => sync(true), opt);
  el.addEventListener("loadedmetadata", () => { lastKey = null; }, opt);
  el.addEventListener(
    "play",
    () => {
      startPolling();
      sync(true);
    },
    opt
  );
  el.addEventListener("pause", () => sync(false), opt);
  if (!el.paused) startPolling();
}

/** 悬浮窗隐藏时挂起推送；重新可见时强制补推当前句（避免停留在陈旧去重缓存） */
export function setEnabled(v: boolean): void {
  enabled = v;
  if (v) sync(true);
}

/** 切换文件 / 字幕列表变化后调用：清掉悬浮窗残留内容 */
export function resetFeed(): void {
  lastKey = null;
  gapSince = -1;
  if (enabled) pushClear();
}
