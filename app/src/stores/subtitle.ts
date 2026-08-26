import { ref } from "vue";
import { getSubtitles } from "../api/subtitle";
import type { Subtitle } from "../types";

// 共享字幕状态（PlayerStage / CaptionPanel / SubtitlePanel 共用）
const subtitles = ref<Subtitle[]>([]);
const status = ref<string>("none"); // none|transcribing|translating|done|error
const stage = ref<string>(""); // extract|transcribe|translate|done
const progress = ref<number>(0); // 0-100
const message = ref<string>("");
const currentTime = ref<number>(0); // 秒
const currentId = ref<number | null>(null);
let autoTranslateId: number | null = null;

function requestAutoTranslate(mediaId: number | null) {
  autoTranslateId = mediaId;
}

function consumeAutoTranslate(): number | null {
  const id = autoTranslateId;
  autoTranslateId = null;
  return id;
}

async function load(id: number) {
  currentId.value = id;
  try {
    subtitles.value = await getSubtitles(id);
  } catch {
    subtitles.value = [];
  }
}

function reset() {
  subtitles.value = [];
  status.value = "none";
  stage.value = "";
  progress.value = 0;
  message.value = "";
}

function setStatus(s: string, st = "", p = 0, msg = "") {
  status.value = s;
  stage.value = st;
  progress.value = p;
  message.value = msg;
}

function setTime(t: number) {
  currentTime.value = t;
}

export function useSubtitle() {
  return {
    subtitles,
    status,
    stage,
    progress,
    message,
    currentTime,
    currentId,
    load,
    reset,
    setStatus,
    setTime,
    requestAutoTranslate,
    consumeAutoTranslate,
  };
}
