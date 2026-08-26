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
- [ ] 确认 `asplayer-transcribe` 的 `lib.rs` 已 `pub mod audio/whisper/translate/srt`
- [ ] 在 `app/src-tauri/Cargo.toml` 添加路径依赖 `asplayer-transcribe = { path = "../../crates/asplayer-transcribe" }`
- [ ] 确认 libclang / /utf-8 编译环境（根 `.cargo/config.toml` 或在 m0-env.ps1 中已被 `/utf-8` 覆盖）
- [ ] `cargo check` 于 app 通过（验证依赖能被 app 复用）→ Commit

## Task 2: 数据库扩展（TDD）
- [ ] db.rs 新增 `subtitles` 表 + `settings` 表 + media_files 增列
- [ ] 方法：`save_subtitles(media_id, &[Segment])`、`get_subtitles(media_id)`、`set_subtitle(media_id, seg)`（单段 upsert，便于进度续写）、`save_setting/get_setting`、`set_subtitle_status(media_id, status)`
- [ ] 单测：插入/读取字幕往返、settings 往返 → `cargo test` 通过 → Commit

## Task 3: 转写后台任务（lib.rs）
- [ ] `AppState` 扩展：持有 model 路径等配置
- [ ] `transcribe_media(id, state, app: AppHandle)`：spawn 线程 → 调 `audio::extract_wav + read_samples_f32 + whisper::transcribe` → 写库 → emit `transcribe://done`；每批进度 emit `transcribe://progress`
- [ ] `translate_media(id, state, app)`：spawn → 读未翻译段 → `translate::translate_segments` → 更新库 → emit
- [ ] `get_subtitles(id) -> Vec<Subtitle>`、`get_subtitle_status(id)`
- [ ] `save_settings/get_settings`
- [ ] 编译通过 → Commit

## Task 4: 前端字幕模型与类型
- [ ] types.ts 新增 `Subtitle { start_ms, end_ms, text, translation }`、`SubtitleStatus`
- [ ] 前端 API 封装 `subtitle.ts`（invoke + 监听事件）

## Task 5: 字幕面板 UI
- [ ] 新组件 `CaptionPanel.vue`：按 `currentTime` 高亮当前段，显示双语，失败/无字幕态
- [ ] PlayerStage 接入：播放中 `@timeupdate` 驱动高亮；工具栏"字幕"按钮切换显示
- [ ] 主界面：选中媒体后出现"转写/翻译"操作（根据 subtitle_status）

## Task 6: 端到端验证
- [ ] `npm run tauri dev` 用真实样本：导入 → 播放 → 触发转写 → 事件进度 → 完成显示双语字幕 → 触发翻译
- [ ] 重启后字幕仍在（SQLite）
- [ ] `cargo test` 全绿 + `npm run build` 通过 → 打标签 `milestone-2`
