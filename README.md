# 🎧 ASPlayer

> 为 **ASMR 爱好者**与**语言学习者**而生的双语字幕播放器。
> 给没有字幕的外语音视频，**一次转写、永久复用**的本地双语字幕资产。

<p align="center">
  <img src="https://img.shields.io/badge/license-GPL--3.0-blue.svg" alt="License" />
  <img src="https://img.shields.io/badge/platform-Windows-0078d4.svg" alt="Platform" />
  <img src="https://img.shields.io/badge/Tauri-2-green.svg" alt="Tauri" />
  <img src="https://img.shields.io/badge/Vue-3-42b883.svg" alt="Vue" />
  <img src="https://img.shields.io/badge/whisper.cpp-via%20whisper--rs-orange.svg" alt="whisper.cpp" />
  <img src="https://img.shields.io/badge/status-0.1.4-informational.svg" alt="Version" />
</p>

---

## 为什么做 ASPlayer

看外语音视频、听 ASMR 或练习英语/日语时，最大的障碍是 —— **没有字幕**。在线平台的自动字幕往往需要会员、会过期、也拿不出来复用。

ASPlayer 的定位很简单：**在本地把没有字幕的语言内容，一次性转写成可永久复用的双语字幕资产**，再用一套为「精听」设计的播放器体验去消费它。转写一次，之后每次播放都能复用，越用越熟。

- 🔒 **全程本地、离线可用**：转写、查词、翻译都可本地完成（翻译也支持云端 API，二选一）。
- 📖 **面向精听后行**：句末自动暂停、单句循环、AB 循环、盲听，把「看字幕」变成「学语言」。
- 🪟 **字幕不碍事**：迷你悬浮字幕窗可以穿透锁定，打游戏 / 多任务时也不错过每一句。
- 💾 **一次生成、永久复用**：字幕落到本地 SQLite，换设备 / 换播放器都不浪费。

---

## ✨ 核心特性

### 1. 一键生成双语字幕（本地转写）

内置 **whisper.cpp** 推理引擎 + **ffmpeg** 音频抽取，把任意音频/视频转成带时间轴的双语字幕：

- **逐句实时显示**：转写一边进行、字幕一边累积到右侧面板，不用等全片结束。
- **面向 ASMR 的智能分段**：能量阈值 VAD 静音切块，专为呼吸音、低语、轻音乐设计的默认参数，也会自动过滤 `[BLANK_AUDIO]` / `[MUSIC]` 等无意义占位。
- **断点续传**：取消或中断后保留已转写部分（`partial`），重跑时从断点继续。
- **转写 + 翻译**：可一键转写并翻成简体中文，或仅转写、稍后再翻。

> 引擎选择：模型/API 二选一 —— 本地 whisper 模型（离线）或任意 OpenAI 兼容 API。

### 2. 精听模式（语言学习）

在**连播 / 精听**双模式间一键切换（也可对单个文件单独覆盖）：

- **句末自动暂停**：每句结束精确停在句尾，看清再继续。
- **单句循环**：把当前句无限重播，直到听清。
- **AB 循环**：在进度条上随手标记 A / B 点，反复磨一段难点。
- **盲听**：隐藏译文只听原文，按住 **H** 临时揭示译文，练「先听懂再看」。
- **句末动作**：暂停后提供「↺ 重听本句」/「→ 下一句」按钮 + 快捷键。

> 仅在**有字幕**时生效 —— 精听依赖字幕的时间轴。

### 3. 迷你悬浮字幕窗（桌面歌词化）

网易云歌词式的玻璃字幕条，悬浮在任何窗口之上：

- **三种显示模式**：原文 / 双语 / 译文，就地切换，零通信开销。
- **Quiet Glass 玻璃条**：原文大字白 + 译文预设色，整条可拖拽。
- **锁定穿透**：锁定后鼠标穿透、完全透明，悬浮圈附近不再截鼠标 —— **打游戏常开**。
- **悬停工具栏**：上一句 / 播放暂停 / 下一句 / 显示模式 / ⚙ 就地设置 / 锁定 / 关闭。
- **可调设置**：译文颜色预设、字号、句间空隙行为（保留上一句 / 5 秒后淡出）。

### 4. 应用内查词（离线词典）

- **右键字幕行查词**：选中词优先，未选中则查整行；命中显示音标 / 假名 / 词性 / 释义。
- **离线可用**：英文 **ECDICT**、日文 **JMdict**，首次下载词典后在应用内即可离线查词。
- **相似词建议**：查不到时给出「是不是想找」候选。

### 5. 多引擎翻译（转写之后）

翻译引擎兼容 OpenAI 接口，云端与本地二选一：

- **云端预设**：DeepSeek、OpenAI、通义千问 Qwen、智谱 GLM、月之暗面 Kimi。
- **本地 Ollama**：接本机 Ollama，可一键拉取推荐模型（qwen2.5:3b / 7b）、断点进度、全离线。
- **环境变量优先**：`ASPLAYER_API_BASE` / `ASPLAYER_API_KEY` / `ASPLAYER_API_MODEL`。

### 6. 播放器基础体验

- **连播 / 精听**双模式，列表循环 + 单曲循环、自动连播下一集。
- **倍速**：0.5x – 2x 六档，音量 OSD、静音、±15 秒步进、全屏自动隐藏控制条。
- **逐句播放**：上/下一句一步到位；字幕行点击即跳转、进度条可拖动。
- **每文件记忆**：单独记住播放位置、速度、音量、模式覆盖（换文件不打断节奏）。

### 7. 媒体库

- **导入文件 / 文件夹**：自动扫描并分类音视频，识别同名字幕文件。
- **搜索 + 排序**：按标题 / 时长 / 字幕数排序，边输入边过滤。
- **右键菜单**：播放、播放模式（跟随全局 / 精听 / 连播）、导入字幕、在文件夹中显示、从列表移除、删除文件。
- **字幕状态标记**：`转写中 / 翻译中 / 已完成 / 部分 / 出错 / 无`，一目了然。

### 8. 快捷键（应用内 + 全局）

- **应用内可自定义**：列表、倍速、模式、字幕导航等全部可改键、Ctrl+Cmd 归一。
- **全局热键**：游戏 / 全屏下也能操作，注册失败自动降级。

### 9. 模型 / 词典一键下载

- **whisper 五档模型**：tiny(75MB) / base(142MB) / small(466MB) / medium(1.5GB) / large-v3(3.1GB)，下载带进度、官方 + 镜像源自动回退、支持断点续传、可选可删。
- **离线词典**：ECDICT / JMdict，内置镜像源，国内可直接下载。

---

## 📸 界面预览

> 截图将在后续版本补充 —— 主窗（播放器 + 播放列表 + 字幕面板）、悬浮字幕窗、设置面板、查词卡片。

---

## 🚀 安装

从 [GitHub Releases](https://github.com/yumili426/ASPlayer/releases) 下载安装包：

- **`.msi`**（Windows 安装程序）
- **`.exe`**（NSIS 安装程序）

> 首次使用转写，请在应用内「模型」页下载一个 whisper 模型（建议 `small`，466MB，兼顾体积与 ASMR 识别率），或在「翻译」页配置云端 API。

---

## 🖱 使用

### 快速开始

1. **导入媒体**：右上角「导入文件」或「导入文件夹」把你的音频 / 视频加进来。
2. **生成字幕**：选中一条，点工具栏「转写」（或「转写并翻译」）。字幕会边转边出现在右侧面板。
3. **精听 / 连播**：用控制栏「精听 / 连播」切换模式；精听下句末自动暂停、可单句循环或 AB 循环。
4. **悬浮字幕**：点控制栏的悬浮窗图标（或全局 `Ctrl+Alt+O`）弹出迷你字幕窗，游戏时可锁定穿透。
5. **查词**：在字幕面板右键任意一行 / 选中单词再右键，直接查词。

### 精听小技巧

- 精听 + 单句循环开启后，播放会停在每句末尾，自动循环本句。
- 盲听开关在「设置 → 播放 → 精听 · 盲听」，开启后隐藏译文，**按住 `H`** 临时显示。

### 悬浮字幕窗

- 默认可拖拽；悬停出现工具栏（上一句 / 播放暂停 / 下一句 / 模式 / 设置 / 锁定 / 关闭）。
- 锁定后鼠标穿透，悬浮时浮现「🔓 解锁」按钮，或按全局 `Ctrl+Alt+L` 解锁。

---

## ⌨️ 快捷键

### 应用内快捷键（设置 → 快捷键 可自定义）

| 动作 | 默认键 |
|---|---|
| 播放 / 暂停 | `Space` |
| 后退 15 秒 | `←` |
| 前进 15 秒 | `→` |
| 音量 + | `↑` |
| 音量 - | `↓` |
| 静音 | `M` |
| 全屏 | `F` |
| 下一句字幕 | `J` |
| 上一句字幕 | `K` |
| 切换播放列表 | `Ctrl+L` |
| 切换字幕面板 | `Ctrl+T` |
| 打开设置 | `Ctrl+,` |
| 切换连播 / 精听 | `Ctrl+Alt+S` |
| 重听本句 | `R` |
| 单句循环开关 | `Ctrl+Alt+L` |

### 全局快捷键（系统级，游戏 / 全屏下可用）

| 动作 | 默认键 |
|---|---|
| 播放 / 暂停 | `Ctrl+Alt+Space` |
| 悬浮字幕窗显示 / 隐藏 | `Ctrl+Alt+O` |
| 悬浮窗鼠标穿透锁定 | `Ctrl+Alt+L` |
| 上一句句首 | `Ctrl+Alt+←` |
| 下一句句首 | `Ctrl+Alt+→` |

> 每个全局热键都有 `Ctrl+Alt+Shift+…` 降级候选；被其他软件占用时自动切换。

---

## ⚙️ 设置

- **外观**：浅色 / 深色 / 跟随系统。
- **播放**：自动连播、连播/精听全局模式、精听三开关（自动暂停 / 单句循环 / 盲听）。
- **字幕**：字号、颜色、位置（上 / 中 / 下）、背景不透明度、显示模式（原文 / 双语 / 译文）**实时预览**。
- **翻译**：服务商预设 + 自定义 API 地址 / Key / 模型、本地 Ollama 地址、推荐模型下载。
- **模型**：whisper 五档模型下载 / 选为当前 / 删除；高级里可调转写分段参数（单段最长、停顿判定、最小分段、检测窗口）。
- **词典**：英文 ECDICT / 日文 JMdict 下载管理、词典源镜像地址。
- **快捷键**：各项动作点按后自定义按键、清除、恢复默认。

---

## 🧠 技术架构

ASPlayer 是一个 **Tauri 2 多窗口应用**：

- 一个 **主窗**（播放器 + 播放列表 + 字幕面板的自绘标题栏窗口）。
- 一个 **迷你悬浮字幕窗**，独立于主窗渲染，通过后端事件中转字幕数据（`overlay://subtitle`），与主窗零消息耦合。

**数据流**

- 转写 / 翻译 / 模型 / 词典 / Ollama 的后台任务，通过 Tauri 事件（`transcribe://*`、`model://*`、`dict://*`、`ollama://*`、`overlay://*`）把进度与结果推给前端。
- 悬浮窗外接由主窗的 `overlayFeed` 引擎根据媒体时钟「内容级去重」推送当前句，跳转 / 切句都会即时同步。

**核心 crates（工作区）**

| 位置 | 职责 |
|---|---|
| `app/src-tauri` | Tauri 应用本体：媒体库 SQLite、转写 / 翻译调度、模型 / 词典 / Ollama、全局快捷键、悬浮窗中转 |
| `crates/asplayer-transcribe` | 转写管线：音频抽取、VAD 切分、whisper 推理、SRT、翻译、外部字幕导入 |
| `crates/asplayer-dict` | 离线查词：ECDICT（英）/ JMdict（日）构建 + FTS 检索 |

---

## 🛠 从源码构建

### 环境准备

- **[Rust](https://rustup.rs)**（MSVC）+ **Visual Studio 2022 C++ 工作负载**
- **[Node.js](https://nodejs.org)**（≥ 18）
- **ffmpeg**：放入 `tools\ffmpeg.exe`，或装到 PATH，或设置环境变量 `ASPLAYER_FFMPEG` 指向它

### 开发

```bash
cd app
npm install
npm run tauri dev      # 启动开发窗口
```

### 测试

```bash
cargo test             # Rust 后端 + crates 单元/集成测试
cd app && npm run test # 前端 vitest
```

### 构建安装包

```bash
cd app
npm run tauri build    # 生成 MSI / NSIS 安装包
```

> 中文 Windows 若缺少前文提及的 ffmpeg / 编译环境，可参考 `scripts/m0-env.ps1` 加载环境。

---

## 📂 项目结构

```
ASPlayer
├─ app/                    # Tauri 2 应用
│  ├─ src/                 # Vue 3 + TS 前端
│  │  ├─ components/       # 播放器、字幕/字幕面板、设置、播放列表、查词
│  │  ├─ windows/          # 迷你悬浮字幕窗
│  │  ├─ stores/           # 播放/字幕/模型/快捷键/悬浮窗偏好
│  │  ├─ api/              # 后端命令的 TS 封装
│  │  └─ lib/              # 精听状态机等纯逻辑
│  └─ src-tauri/           # Rust 后端
├─ crates/
│  ├─ asplayer-transcribe/ # 转写管线
│  └─ asplayer-dict/       # 离线查词
├─ tools/                  # ffmpeg 等本地工具（不入库，分发时另行打包）
├─ scripts/                # 环境/构建脚本
└─ docs/                   # 设计与规划文档
```

### 数据目录

- **数据库**：应用数据目录（Windows 为 `%APPDATA%\com.asplayer.dev\asplayer.db`）
- **whisper 模型**：`~/.asplayer/models/ggml-<size>.bin`
- **离线词典**：`~/.asplayer/dict/`（`ecdict.csv` / `JMdict_e.gz`）

---

## 📄 许可

[GPL-3.0](https://github.com/yumili426/ASPlayer/blob/main/LICENSE) —— 自由软件，可自由使用、修改、分发，但分发衍生版本须保持同许可证并开源。

---

## 🙏 致谢

ASPlayer 站在众多优秀开源项目之上：

- [Tauri](https://tauri.app) & [Wry](https://github.com/tauri-apps/wry) —— 轻量跨平台桌面壳
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) / [whisper-rs](https://github.com/tazz4843/whisper-rs) —— 本地语音识别推理
- [GGML](https://github.com/ggerganov/ggml) —— 底层推理运行时
- [ECDICT](https://github.com/skywind3000/ECDICT) / [JMdict](https://www.edrdg.org/jmdict/) —— 开源离线词典数据
- [Ollama](https://ollama.com) —— 本地大模型服务
- [Vue](https://vuejs.org) / [Vite](https://vitejs.dev) —— 前端框架与构建

---

<p align="center">
  Made with ❤️ for ASMR lovers and language learners.
</p>
