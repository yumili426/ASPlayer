# ASPlayer 里程碑0 环境脚本
# 用法（PowerShell）:  . .\scripts\m0-env.ps1
# 之后即可直接运行:
#   cargo run --release -- transcribe --media <文件> --model <模型> --out <目录>
#   cargo run --release -- translate --input <segments.json> --model deepseek-chat

$ErrorActionPreference = "Stop"

# --- libclang（whisper-rs-sys/bindgen 需要）---
$llvmBin = "$env:TEMP\clang+llvm-18.1.8-x86_64-pc-windows-msvc\bin"
if (Test-Path "$llvmBin\libclang.dll") {
    $env:LIBCLANG_PATH = $llvmBin
} else {
    Write-Warning "未找到 libclang.dll，请先完成 LLVM 解压步骤"
}

# --- MSVC 源码编码修复（中文 Windows 必需！）---
# whisper.cpp 源码含 UTF-8 字符（♪ 等），MSVC 默认按 GBK 解读会报
# error C3688 "文本后缀无效"。必须强制 /utf-8。
$env:CXXFLAGS = "/utf-8"
$env:CFLAGS = "/utf-8"


# --- ffmpeg ---
# 优先级：已有 PATH > tools\ffmpeg.exe
# 若你手动安装了 ffmpeg 到其他位置，取消下一行注释并修改路径：
# $env:ASPLAYER_FFMPEG = "C:\path\to\ffmpeg.exe"

# --- 模型与样本目录 ---
$env:ASPLAYER_MODELS = "$env:USERPROFILE\.asplayer\models"
$env:ASPLAYER_SAMPLES = "$env:USERPROFILE\.asplayer\samples"

Write-Host "m0 环境已就绪："
Write-Host "  LIBCLANG_PATH = $env:LIBCLANG_PATH"
Write-Host "  模型目录       = $env:ASPLAYER_MODELS"
Write-Host "  样本目录       = $env:ASPLAYER_SAMPLES"
