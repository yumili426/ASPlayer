import { describe, it, expect } from "vitest";
import {
  resolveMode, EMPTY_AB, abStep, abActive, abRange, abContains,
  shouldAutoPause, shouldSentenceLoop,
} from "./intensive";

const F = { autoPause: true, sentenceLoop: false };
const FLoop = { autoPause: false, sentenceLoop: true };

describe("resolveMode", () => {
  it("文件覆盖优先于全局", () => {
    expect(resolveMode("intensive", "broadcast")).toBe("intensive");
    expect(resolveMode("broadcast", "intensive")).toBe("broadcast");
  });
  it("null 或非法值回落全局", () => {
    expect(resolveMode(null, "intensive")).toBe("intensive");
    expect(resolveMode(null, "broadcast")).toBe("broadcast");
  });
  it("非法值回落全局", () => {
    // 测试运行时防御：用 as any 强制非法字面量越过联合类型
    expect(resolveMode("bogus" as any, "broadcast")).toBe("broadcast");
    expect(resolveMode("bogus" as any, "intensive")).toBe("intensive");
  });
});

describe("AB 状态机", () => {
  it("三态循环：设A→设B→清除→设A", () => {
    const s1 = abStep(EMPTY_AB, 3);
    expect(s1).toEqual({ a: 3, b: null });
    const s2 = abStep(s1, 10);
    expect(s2).toEqual({ a: 3, b: 10 });
    const s3 = abStep(s2, 20);
    expect(s3).toEqual({ a: null, b: null });
  });
  it("active/range/contains", () => {
    const s = { a: 10, b: 3 };
    expect(abActive(s)).toBe(true);
    expect(abRange(s)).toEqual([3, 10]);
    expect(abContains(s, 5)).toBe(true);
    expect(abContains(s, 2)).toBe(false);
    expect(abContains(EMPTY_AB, 5)).toBe(false);
  });
});

describe("shouldAutoPause", () => {
  it("仅精听且有字幕且无覆盖时暂停", () => {
    expect(shouldAutoPause("intensive", F, false, true)).toBe(true);
  });
  it("单句循环开或 AB 激活或无字幕或非精听时不暂停", () => {
    expect(shouldAutoPause("intensive", FLoop, false, true)).toBe(false);
    expect(shouldAutoPause("intensive", F, true, true)).toBe(false);
    expect(shouldAutoPause("intensive", F, false, false)).toBe(false);
    expect(shouldAutoPause("broadcast", F, false, true)).toBe(false);
  });
});

describe("shouldSentenceLoop", () => {
  it("仅精听且有字幕且单句循环开且 AB 未激活", () => {
    expect(shouldSentenceLoop("intensive", FLoop, false, true)).toBe(true);
  });
  it("AB 激活或非精听时关闭", () => {
    expect(shouldSentenceLoop("intensive", FLoop, true, true)).toBe(false);
    expect(shouldSentenceLoop("broadcast", FLoop, false, true)).toBe(false);
  });
});
