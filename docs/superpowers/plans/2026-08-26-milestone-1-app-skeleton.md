# ASPlayer 里程碑 1：Tauri 应用骨架与媒体库 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox syntax.

**Goal:** 建立 Tauri 2 + Vue 3 应用骨架，实现本地媒体库（文件夹导入/扫描、SQLite 元数据、播放 HTML5 音视频），并以 Quiet Glass 设计语言完成静态 UI 原型。

**Architecture:** 前端 Vue3+TS+Pinia；Rust 层复用/迁移 crates/asplayer-transcribe 的 audio 模块做格式探测，新增 media 库模块（目录扫描+SQLite）。IPC 用 Tauri command + event。

**Tech Stack:** Tauri 2、Vue 3、TypeScript、Pinia、Vite、rusqlite(bundled)、tauri-plugin-shell(调用ffmpeg探测)。

---

## File Structure

```
app/                          # Tauri 应用根（create-tauri-app 生成）
├─ src/                       # Vue 前端
│  ├─ main.ts / App.vue
│  ├─ styles/tokens.css       # Quiet Glass 主题令牌（浅/深两套 CSS 变量）
│  ├─ stores/library.ts       # Pinia 媒体库 store
│  ├─ views/LibraryView.vue   # 媒体库视图
│  ├─ views/PlayerView.vue    # 播放器视图（video/audio + 字幕面板占位）
│  └─ components/             # 顶栏/侧栏/播放条组件
└─ src-tauri/
   ├─ tauri.conf.json         # 窗口配置（无边框、深色、尺寸）
   └─ src/
      ├─ lib.rs               # Tauri commands: scan_folder, list_media, get_media
      ├─ media.rs             # 目录扫描 + 格式过滤 + ffmpeg 时长探测
      └─ db.rs                # rusqlite: media_files 表 CRUD
```

## Tasks

### Task 1: 脚手架
- [ ] `npm create tauri-app@latest app -- --template vue-ts --manager npm --yes`
- [ ] `cd app && npm install && npm run tauri info` 确认环境
- [ ] 验证 `cargo check` 于 src-tauri 通过
- [ ] Commit

### Task 2: 主题令牌系统
- [ ] tokens.css：定义 --bg-0/--bg-1/--bg-2/--fg-1/--fg-2/--accent 及 data-theme=light/dark 两套值
- [ ] App.vue 挂载主题切换（跟随系统 via matchMedia）
- [ ] Commit

### Task 3: Rust 媒体库层（TDD）
- [ ] db.rs：rusqlite(bundled) 建 media_files 表（按设计文档 §7 字段），insert/list 测试
- [ ] media.rs：递归扫描目录，按扩展名过滤（mp4/m4a/mp3/wav/flac/webm/mkv），返回路径列表；纯函数 filter_media_files 单测
- [ ] lib.rs 注册 commands：import_folder(path) → 探测并入库；list_media() → Vec<MediaItem>
- [ ] cargo test 通过后 Commit

### Task 4: 前端媒体库视图
- [ ] Pinia store 调用 invoke("list_media")
- [ ] LibraryView：网格卡片（文件名+类型图标），顶部"导入文件夹"按钮调 import_folder
- [ ] 点击卡片进入 PlayerView，HTML5 video/audio 播放 convertFileSrc 资源
- [ ] 记住播放位置（timeupdate 节流写回 SQLite playback_position）
- [ ] 手动验证 + Commit

### Task 5: 迷行验证
- [ ] tauri dev 启动、导入含真实文件的文件夹、播放、重启后位置恢复
- [ ] 打标签 milestone-1
