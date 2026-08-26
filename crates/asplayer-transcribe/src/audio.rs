use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 解析 ffmpeg 可执行文件：
/// 1) 环境变量 ASPLAYER_FFMPEG 指定的完整路径
/// 2) PATH 中的 ffmpeg
/// 3) 工作区 tools/ 目录下的 ffmpeg.exe（随包分发形态）
fn ffmpeg_program() -> String {
    if let Ok(p) = std::env::var("ASPLAYER_FFMPEG") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    let local = PathBuf::from("tools").join("ffmpeg.exe");
    if local.is_file() {
        return local.to_string_lossy().into_owned();
    }
    // 向上再找两级（兼容从 crates/xxx 子目录运行）
    let parent = PathBuf::from("../tools").join("ffmpeg.exe");
    if parent.is_file() {
        return parent.to_string_lossy().into_owned();
    }
    "ffmpeg".to_string()
}

/// 用 ffmpeg 把任意媒体抽成 16kHz 单声道 WAV
pub fn extract_wav(media: &Path, out_dir: &Path) -> Result<PathBuf> {
    let name = media
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .to_string();
    let out = out_dir.join(format!("{name}.asplayer.wav"));
    let status = Command::new(ffmpeg_program())
        .arg("-y")
        .arg("-i")
        .arg(media)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "wav"])
        .arg(&out)
        .output()
        .context("启动 ffmpeg 失败：请设置 ASPLAYER_FFMPEG、安装 ffmpeg 到 PATH，或将 ffmpeg.exe 放入 tools/ 目录")?;
    if !status.status.success() {
        bail!(
            "ffmpeg 抽音轨失败:\n{}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    Ok(out)
}

/// 读 16kHz 单声道 WAV 为 whisper 所需的 f32 采样
pub fn read_samples_f32(wav: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(wav)?;
    let spec = *reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 {
        bail!(
            "期望 16kHz 单声道 WAV，实际 {ch} 声道 {sr}Hz",
            ch = spec.channels,
            sr = spec.sample_rate
        );
    }
    let i16s: Vec<i16> = reader.samples::<i16>().collect::<Result<_, _>>()?;
    Ok(whisper_rs::convert_integer_to_float_audio(&i16s)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_program_prefers_env_var() {
        unsafe { std::env::set_var("ASPLAYER_FFMPEG", "C:/fake/ffmpeg.exe") };
        assert_eq!(ffmpeg_program(), "C:/fake/ffmpeg.exe");
        unsafe { std::env::remove_var("ASPLAYER_FFMPEG") };
    }
}
