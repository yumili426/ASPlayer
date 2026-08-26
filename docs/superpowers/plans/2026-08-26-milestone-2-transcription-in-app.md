# ASPlayer 里程碑 2：转写/翻译管线接入应用 实施计划

> **Goal:** 将 M0 验证的 ffmpeg→whisper→DeepSeek 转写管线作为**后台任务**接入 Tauri 应用：媒体项可触发转写，转写结果存 SQLite，播放时按时间轴显示双语字幕；翻译也作为可独立/接力触发的任务。
>
> **架构决策（关键）：**
> 1. **后台线程 + 事件推送**：whisper 推理耗时长（~2min/120s），不能阻塞 UI/主线程。用 std::thread 或 tauri async + `AppHandle.emit` 向前端推 `transcribe://progress` / `transcribe://done` / `transcribe://error`。
> 2. **数据库表**：新增 `subtitles`（media_id, start_ms, end_ms, text, translation）与 `transcribe_jobs`（media_id, status, progress, error）。media_files 增列 `subtitle_status`（none|transcribing|done|error）与 `subtitle_lang`。
> 3. **配置**：whisper 模型路径默认 `~/.asplayer/models/ggml-small.bin`（env `ASPLAYER_MODEL` 可覆盖）；翻译 API 用 `ASPLAYER_API_KEY` / `ASPLAYER_API_BASE`（设置面板可填入并持久化到 DB `settings` 表）。
> 4. **复用 M0 crate**：`app/src-tauri` 依赖 `asplayer-transcribe`，调用其 `audio::extract_wav / read_samples_f32`、`whisper::transcribe`、`translate::translate_segments`、`srt::Segment`。model 路径与目标语言由命令参数传入。
>
> **IPC（新增 Tauri commands）：**
> - `transcribe_media(id) -> ()`：后台启动转写（音频→whisper→存 subtitles），事件 `transcribe://progress`、`transcribe://done`
> - `translate_media(id) -> ()`：后台对未翻译的字幕段做翻译，事件 `transcribe://translated`
> - `get_subtitles(id) -> Vec<Subtitle>`：读取该媒体字幕（原文+译文）
> - `get_subtitle_status(id) -> SubtitleStatus`：查询转写/翻译状态
> - `save_settings(json) / get_settings() -> json`：持久化 API key/base/model/language

---

## Task 1: 转写 crate 暴露可复用 API
- [x] 确认 `asplayer-transcribe` 的 `lib.rs` 已 `pub mod audio/whisper/translate/srt`
- [x] 在 `app/src-tauri/Cargo.toml` 添加路径依赖 `asplayer-transcribe = { path = "../../crates/asplayer-transcribe" }`
- [x] 确认 libclang / /utf-8 编译环境（新建 `.cargo/config.toml` 固化 LIBCLANG_PATH + /utf-8）
- [x] `cargo check` 于 app 通过（验证依赖能被 app 复用）→ Commit

## Task 2: 数据库扩展（TDD）
- [x] db.rs 新增 `subtitles` 表 + `settings` 表 + media_files 增列
- [x] 方法：`save_subtitles`、`get_subtitles`、`set_subtitle`、`save_setting/get_setting`、`set_subtitle_status`
- [x] 单测：插入/读取字幕往返、settings 往返 → `cargo test` 通过 → Commit

## Task 3: 转写后台任务（lib.rs）
- [x] `AppState` 扩展：db 用 `Arc<Mutex<MediaDb>>` 便于后台线程共享
- [x] `transcribe_media(id)`：spawn 线程 → 抽音轨 → whisper → 写库 → emit 事件
- [x] `translate_media(id)`：spawn → 读未翻译段 → 批量翻译 → 回写 → emit
- [x] `get_subtitles(id)`、`get_subtitle_status`、`save_settings/get_settings`
- [x] 编译通过 → Commit

## Task 4: 前端字幕模型与类型
- [x] types.ts 新增 `Subtitle`、`ProgressEvent`、MediaItem 扩展
- [x] 前端 API 封装 `subtitle.ts`（invoke + 监听事件）

## Task 5: 字幕面板 UI
- [x] 新组件 `CaptionPanel.vue`：按 currentTime 高亮当前段、双语、空态/失败态
- [x] PlayerStage 接入：工具栏加 转写/转写+翻译/翻译 按钮，播放中驱动高亮
- [x] PlaylistPanel 显示"字"徽标（字幕段数）
- [x] SettingsPanel 加 API 配置（base/key/model）

## Task 6: 端到端验证
- [ ] `npm run tauri dev` 用真实样本：导入 → 播放 → 触发转写 → 事件进度 → 完成显示双语字幕 → 触发翻译
- [ ] 重启后字幕仍在（SQLite）
- [ ] `cargo test` 全绿 + `npm run build` 通过 → 打标签 `milestone-2`
