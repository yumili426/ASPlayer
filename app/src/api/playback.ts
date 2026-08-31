import { invoke } from "@tauri-apps/api/core";
import type { ProfileOverride } from "../lib/intensive";

/** 设置某媒体的精听/连播覆盖（null=跟随全局） */
export function setMediaProfile(id: number, profile: ProfileOverride) {
  return invoke<void>("set_media_profile", { id, value: profile });
}
