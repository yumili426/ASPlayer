import { reactive } from "vue";
import {
  cancelModelDownload,
  downloadModel,
  getModelsStatus,
  onModelCanceled,
  onModelDone,
  onModelError,
  onModelProgress,
  onModelSelected,
  removeModel,
  setModel,
} from "../api/model";
import type { ModelStatus } from "../types";

/** 五档体积估算（仅用于展示） */
export const MODEL_META: Record<string, string> = {
  tiny: "75MB",
  base: "142MB",
  small: "466MB",
  medium: "1.5GB",
  "large-v3": "3.1GB",
};

export const modelState = reactive<{
  models: ModelStatus[];
  selected: string;
  loading: boolean;
  activeSize: string | null;
}>({
  models: [],
  selected: "small",
  loading: false,
  activeSize: null,
});

let initialized = false;

/** 首次打开面板时调用一次：订阅后台事件并拉取一次状态 */
export async function initModel() {
  if (initialized) return;
  initialized = true;
  await onModelProgress((p) => {
    const m = modelState.models.find((x) => x.size === p.size);
    if (m) {
      m.bytes_downloaded = p.bytes_downloaded;
      m.total_bytes = p.total_bytes;
    }
  });
  await onModelDone((size) => settle(size, "done", null));
  await onModelError((e) => settle(e.size, "failed", e.error));
  await onModelCanceled((size) => settle(size, "canceled", null));
  await onModelSelected((size) => {
    modelState.selected = size;
  });
}

function settle(size: string, status: ModelStatus["status"], error: string | null) {
  const m = modelState.models.find((x) => x.size === size);
  if (m) {
    m.status = status;
    m.error = error;
  }
  if (status === "done" || status === "canceled" || status === "failed") {
    modelState.activeSize = null;
  }
  if (status === "done") {
    void loadModel();
  }
}

export async function loadModel() {
  modelState.loading = true;
  try {
    modelState.models = await getModelsStatus();
    const sel = modelState.models.find((m) => m.selected);
    modelState.selected = sel?.size ?? "small";
    const active = modelState.models.find((m) => m.status === "downloading");
    modelState.activeSize = active ? active.size : null;
  } finally {
    modelState.loading = false;
  }
}

export async function download(size: string) {
  modelState.activeSize = size;
  try {
    await downloadModel(size);
  } catch {
    modelState.activeSize = null;
  }
}

export async function cancel(size: string) {
  await cancelModelDownload(size);
}

export async function select(size: string) {
  await setModel(size);
  modelState.selected = size;
  await loadModel();
}

export async function remove(size: string) {
  await removeModel(size);
  await loadModel();
}

export function useModels() {
  return { modelState, initModel, loadModel, download, cancel, select, remove };
}
