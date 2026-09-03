use crate::srt::Segment;
use anyhow::{Context, Result};
use std::sync::Once;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, whisper_rs_sys,
};

/// whisper-rs-sys 在 debug/force-debug 构建下会用 `-DWHISPER_DEBUG` 编译 whisper.cpp，
/// 使每条 token 推理都走 `WHISPER_LOG_DEBUG` 打日志，刷屏且无诊断价值。此回调全局安装一次，
/// 丢弃 DEBUG 级、其余（ERROR/WARN/INFO）照旧写 stderr，行为与默认回调一致。
pub fn setup_log_filter() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        whisper_rs::set_log_callback(Some(drop_debug_logs), std::ptr::null_mut());
    });
}

unsafe extern "C" fn drop_debug_logs(
    level: whisper_rs_sys::ggml_log_level,
    text: *const std::os::raw::c_char,
    _user_data: *mut std::os::raw::c_void,
) {
    if level == whisper_rs_sys::ggml_log_level_GGML_LOG_LEVEL_DEBUG {
        return;
    }
    if text.is_null() {
        return;
    }
    // SAFETY: whisper.cpp 保证传递的是以 \0 结尾的合法字符串。
    let msg = unsafe { std::ffi::CStr::from_ptr(text) }.to_string_lossy();
    eprint!("{msg}");
}

/// 复用同一个 `WhisperContext`（模型只载一次）逐块解码。
pub struct Whisper {
    ctx: WhisperContext,
}

impl Whisper {
    /// 载入模型一次（含安装日志过滤）。整个转写任务只调用这一次。
    pub fn load(model_path: &str) -> Result<Self> {
        setup_log_filter();
        let ctx = WhisperContext::new_with_params(
            model_path,
            WhisperContextParameters::default(),
        )
        .with_context(|| format!("加载模型失败: {model_path}"))?;
        Ok(Self { ctx })
    }

    /// 对单块 samples 解码，返回「相对该块起点」的段时间戳（毫秒，0 基）。
    /// 每个块用独立的 `WhisperState`，块间无上下文串扰。绝对偏移由调用方统一加。
    /// `prompt` 填上一窗口尾部文本，作为 whisper 的 initial_prompt 提供跨窗口上下文：
    /// 让 whisper 能续写上一窗口未完成的句子，并沿上文稳住语种话题。
    pub fn transcribe(
        &mut self,
        language: Option<&str>,
        prompt: Option<&str>,
        samples: &[f32],
    ) -> Result<Vec<Segment>> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(language);
        // initial_prompt 携带上一窗口尾部文本，跨窗口续句 + 沿上文稳住语种；
        // 仅当有文本时设置（该字段接受 &str，空串无意义）。
        if let Some(prompt) = prompt {
            params.set_initial_prompt(prompt);
        }
        // 不用 no_context：no_context=true 会逐段独立解码，前段还是英文、后段就跳到中文/阿语，
        // 语种漂移反而更碎。保留默认「以上文为条件」让 whisper 沿上文保持语种与话题一致。
        // 抑制非语音占位 token（[MUSIC]/[BLANK_AUDIO]/♪），防幻觉主要靠 set_language 钉住语种。
        params.set_suppress_non_speech_tokens(true);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let mut state = self.ctx.create_state()?;
        state.full(params, samples).context("whisper 推理失败")?;

        let n = state.full_n_segments()?;
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            // whisper-rs 时间戳单位为 10ms（厘秒），×10 转毫秒
            let start_ms = state.full_get_segment_t0(i)? as u64 * 10;
            let end_ms = state.full_get_segment_t1(i)? as u64 * 10;
            // whisper 个别 token 映射为非 UTF-8 字节序列，`full_get_segment_text` 会直接报错中断；
            // 用 lossy 变体把非法字节替换成 U+FFFD，绝不让整轮转写因此失败。
            let text = state.full_get_segment_text_lossy(i)?;
            if !text.trim().is_empty() {
                out.push(Segment { start_ms, end_ms, text });
            }
        }
        Ok(out)
    }
}

/// 便捷封装：载模型一次并转写整段（CLI / 单次调用用）。
pub fn transcribe(
    model_path: &str,
    language: Option<&str>,
    prompt: Option<&str>,
    samples: &[f32],
) -> Result<Vec<Segment>> {
    Whisper::load(model_path)?.transcribe(language, prompt, samples)
}
