export type VideoScale = "fit" | "cover" | "fill" | "original";

/** 画面模式档位：适应 / 铺满 / 拉伸 / 原始 */
export const SCALE_MODES: { key: VideoScale; label: string }[] = [
  { key: "fit", label: "适应" },
  { key: "cover", label: "铺满" },
  { key: "fill", label: "拉伸" },
  { key: "original", label: "原始" },
];

/** 循环到下一档（尾部回绕到第一档） */
export function cycleScale(current: VideoScale): VideoScale {
  const i = SCALE_MODES.findIndex((m) => m.key === current);
  return SCALE_MODES[(i + 1) % SCALE_MODES.length].key;
}

export function scaleLabel(mode: VideoScale): string {
  return SCALE_MODES.find((m) => m.key === mode)?.label ?? SCALE_MODES[0].label;
}

export type ObjectFitValue = "contain" | "cover" | "fill" | "none";

/** 映射到 CSS object-fit；原始尺寸用 none（等比、天然像素、居中裁切溢出） */
export function scaleObjectFit(mode: VideoScale): ObjectFitValue {
  const map: Record<VideoScale, ObjectFitValue> = {
    fit: "contain",
    cover: "cover",
    fill: "fill",
    original: "none",
  };
  return map[mode] ?? "contain";
}
