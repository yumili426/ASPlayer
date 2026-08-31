export type PlaybackMode = "broadcast" | "intensive";
export type ProfileOverride = "broadcast" | "intensive" | null;

/** 有效模式 = 文件覆盖 ?? 全局模式（覆盖合法性内置防御：非法值回落全局） */
export function resolveMode(override: ProfileOverride, global: PlaybackMode): PlaybackMode {
  if (override === "broadcast" || override === "intensive") return override;
  return global;
}

/** AB 循环状态机：a/b 为秒或 null。三态循环 */
export interface AbState {
  a: number | null;
  b: number | null;
}
export const EMPTY_AB: AbState = { a: null, b: null };

export function abStep(s: AbState, current: number): AbState {
  if (s.a === null) return { a: current, b: null }; // 设 A
  if (s.b === null) return { a: s.a, b: current }; // 设 B（开始循环）
  return { a: null, b: null }; // 清除
}

export function abActive(s: AbState): boolean {
  return s.a !== null && s.b !== null;
}

export function abRange(s: AbState): [number, number] | null {
  if (!abActive(s)) return null;
  return [Math.min(s.a!, s.b!), Math.max(s.a!, s.b!)];
}

export function abContains(s: AbState, t: number): boolean {
  const r = abRange(s);
  if (!r) return false;
  return t >= r[0] && t <= r[1];
}

export interface IntensiveFlags {
  autoPause: boolean;
  sentenceLoop: boolean;
}

/** 是否应在句末自动暂停（精听且有字幕、自动暂停开、单句循环关、AB 未激活） */
export function shouldAutoPause(
  mode: PlaybackMode,
  flags: IntensiveFlags,
  abIsActive: boolean,
  hasSubtitle: boolean
): boolean {
  return mode === "intensive" && hasSubtitle && flags.autoPause && !flags.sentenceLoop && !abIsActive;
}

/** 是否应进入单句循环回放（精听且有字幕、单句循环开、AB 未激活） */
export function shouldSentenceLoop(
  mode: PlaybackMode,
  flags: IntensiveFlags,
  abIsActive: boolean,
  hasSubtitle: boolean
): boolean {
  return mode === "intensive" && hasSubtitle && flags.sentenceLoop && !abIsActive;
}
