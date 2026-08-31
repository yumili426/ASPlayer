import { describe, it, expect, beforeEach } from "vitest";
import { useSubtitle } from "./subtitle";

describe("subtitle store append/clear", () => {
  const r = (start_ms: number) =>
    ({ start_ms, end_ms: start_ms + 500, text: `t${start_ms}`, translation: "", ordinal: 0 });

  beforeEach(() => {
    const sub = useSubtitle();
    sub.clearSubtitles();
    sub.currentId.value = 0;
  });

  it("append：仅当 mediaId 为当前媒体", () => {
    const sub = useSubtitle();
    sub.currentId.value = 5;
    sub.appendSubtitles(5, [r(1000)]);
    sub.appendSubtitles(6, [r(2000)]); // 非当前媒体，忽略
    expect(sub.subtitles.value).toEqual([
      { start_ms: 1000, end_ms: 1500, text: "t1000", translation: "", ordinal: 0 },
    ]);
  });

  it("append：按 start_ms 去重", () => {
    const sub = useSubtitle();
    sub.currentId.value = 5;
    sub.appendSubtitles(5, [r(1000), r(2000)]);
    sub.appendSubtitles(5, [r(1000), r(3000)]); // 1000 重复，3000 新
    expect(sub.subtitles.value.map((s) => s.start_ms)).toEqual([1000, 2000, 3000]);
  });

  it("append：合并后保持 start_ms 升序", () => {
    const sub = useSubtitle();
    sub.currentId.value = 5;
    sub.appendSubtitles(5, [r(5000)]);
    sub.appendSubtitles(5, [r(1000), r(3000)]);
    expect(sub.subtitles.value.map((s) => s.start_ms)).toEqual([1000, 3000, 5000]);
  });

  it("clearSubtitles：清空列表且不改 status", () => {
    const sub = useSubtitle();
    sub.currentId.value = 5;
    sub.appendSubtitles(5, [r(1000)]);
    sub.clearSubtitles();
    expect(sub.subtitles.value).toEqual([]);
  });
});
