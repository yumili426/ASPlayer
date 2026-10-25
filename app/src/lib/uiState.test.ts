import { describe, expect, it } from "vitest";
import type { MediaItem } from "../types";
import { resolvePrimaryAction, sortRecentMedia } from "./uiState";

function item(over: Partial<MediaItem>): MediaItem {
  return {
    id: 1,
    path: "D:/m/a.mp4",
    title: "a",
    media_type: "video",
    duration_ms: 60_000,
    playback_position: 0,
    file_size: 0,
    subtitle_status: "none",
    subtitle_lang: "",
    subtitle_count: 0,
    transcribe_next_ms: 0,
    speed: 1,
    volume: 1,
    profile_override: null,
    last_played_at: null,
    ...over,
  };
}

describe("resolvePrimaryAction", () => {
  it("无文件时禁用", () => {
    expect(resolvePrimaryAction(null, [], null).action).toBe("none");
    expect(resolvePrimaryAction(null, [], null).disabled).toBe(true);
  });

  it("无字幕时转写", () => {
    const s = resolvePrimaryAction(item({}), [], null);
    expect(s.action).toBe("transcribe");
    expect(s.label).toBe("转写并翻译");
  });

  it("断点续传", () => {
    expect(resolvePrimaryAction(item({ subtitle_status: "partial" }), [], null).action).toBe("resume");
  });

  it("运行中显示取消与进度", () => {
    const s = resolvePrimaryAction(item({ subtitle_status: "transcribing" }), [], 42);
    expect(s.action).toBe("cancel");
    expect(s.progress).toBe(42);
  });

  it("错误时重新转写", () => {
    expect(resolvePrimaryAction(item({ subtitle_status: "error" }), [], null).action).toBe("retranscribe");
  });

  it("存在空译文时翻译", () => {
    const s = resolvePrimaryAction(
      item({ subtitle_status: "done" }),
      [{ translation: "你好" }, { translation: "  " }],
      null
    );
    expect(s.action).toBe("translate");
  });

  it("全部有译文时就绪", () => {
    const s = resolvePrimaryAction(
      item({ subtitle_status: "done" }),
      [{ translation: "你好" }],
      null
    );
    expect(s.action).toBe("ready");
  });
});

describe("sortRecentMedia", () => {
  it("只保留播放过的并按时间倒序", () => {
    const list = [
      item({ id: 1, last_played_at: "2026-10-20 10:00:00" }),
      item({ id: 2, last_played_at: null }),
      item({ id: 3, last_played_at: "2026-10-24 10:00:00" }),
    ];
    expect(sortRecentMedia(list).map((m) => m.id)).toEqual([3, 1]);
  });

  it("同一时间按 id 倒序", () => {
    const list = [
      item({ id: 5, last_played_at: "2026-10-24 10:00:00" }),
      item({ id: 9, last_played_at: "2026-10-24 10:00:00" }),
    ];
    expect(sortRecentMedia(list).map((m) => m.id)).toEqual([9, 5]);
  });
});
