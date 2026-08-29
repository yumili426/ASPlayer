pub mod audio;
pub mod srt;
pub mod subtitle_import;
pub mod vad;
pub mod whisper;
pub mod translate;

// ggml 的 Windows 后端会调用注册表 API（RegQueryValueExA 等），
// MSVC 链接时需要显式引入 advapi32。
#[cfg(target_os = "windows")]
#[link(name = "advapi32")]
extern "system" {}

