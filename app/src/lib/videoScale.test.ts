import { describe, it, expect } from "vitest";
import { cycleScale, scaleLabel, scaleObjectFit, SCALE_MODES } from "./videoScale";

describe("cycleScale", () => {
  it("按顺序循环并在尾部回绕", () => {
    expect(cycleScale("fit")).toBe("cover");
    expect(cycleScale("cover")).toBe("fill");
    expect(cycleScale("fill")).toBe("original");
    expect(cycleScale("original")).toBe("fit");
  });
});

describe("scaleLabel", () => {
  it("返回对应中文标签", () => {
    expect(scaleLabel("fit")).toBe("适应");
    expect(scaleLabel("cover")).toBe("铺满");
    expect(scaleLabel("fill")).toBe("拉伸");
    expect(scaleLabel("original")).toBe("原始");
  });

  it("未知档回退到第一档", () => {
    expect(scaleLabel("bogus" as never)).toBe("适应");
  });
});

describe("scaleObjectFit", () => {
  it("映射到 CSS object-fit", () => {
    expect(scaleObjectFit("fit")).toBe("contain");
    expect(scaleObjectFit("cover")).toBe("cover");
    expect(scaleObjectFit("fill")).toBe("fill");
    expect(scaleObjectFit("original")).toBe("none");
  });

  it("未知档回退到 contain", () => {
    expect(scaleObjectFit("bogus" as never)).toBe("contain");
  });
});

describe("SCALE_MODES", () => {
  it("共四档", () => {
    expect(SCALE_MODES.map((m) => m.key)).toEqual(["fit", "cover", "fill", "original"]);
  });
});
