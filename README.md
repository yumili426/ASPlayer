# ASPlayer

> 面向 ASMR 爱好者与语言学习者的双语字幕播放器。
> 为没有字幕的外语音视频**一次生成、永久复用**的双语字幕资产。

- **许可证**：GPL-3.0
- **技术栈**：Tauri 2 · Rust · whisper.cpp（whisper-rs）· Vue 3
- **平台**：Windows（架构预留跨平台）
- **状态**：🚧 里程碑 0 —— 转写验证管线开发中

## 文档

| 文档 | 说明 |
|---|---|
| [设计文档](docs/specs/2026-08-26-asplayer-design.md) | 产品定位、功能矩阵、架构、UI 设计语言 |
| [M0 实施计划](docs/superpowers/plans/2026-08-26-milestone-0-transcription-pipeline.md) | 转写验证管线的任务分解 |

## M0 命令行管线使用

### 环境准备

```powershell
# 1. 安装 Rust (https://rustup.rs) 与 Visual Studio 2022 C++ 工作负载
# 2. ffmpeg.exe 放入 tools\ 目录（或安装到 PATH，或设置 ASPLAYER_FFMPEG 指向它）
# 3. 下载 Whisper 模型到 %USERPROFILE%\.asplayer\models\
curl -L -o "$env:USERPROFILE\.asplayer\models\ggml-small.bin" https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
```

### 转写 + 翻译

```powershell
. .\scripts\m0-env.ps1   # 设置 LIBCLANG_PATH / CXXFLAGS 等（中文 Windows 必需）

# 转写：媒体 → SRT + segments.json
cargo run --release -- transcribe --media "视频.mp4" --model "$env:USERPROFILE\.asplayer\models\ggml-small.bin" --out 输出目录 --lang ja

# 翻译：segments.json → 双语对照 txt（任意 OpenAI 兼容 API）
$env:ASPLAYER_API_BASE = "https://api.deepseek.com/v1"
$env:ASPLAYER_API_KEY  = "<你的key>"
cargo run --release -- translate --input "视频.segments.json" --model deepseek-chat
```

## 开发

```powershell
. .\scripts\m0-env.ps1   # 编译 whisper 绑定前必须先加载环境
cargo test               # 运行单元测试与集成测试
```
