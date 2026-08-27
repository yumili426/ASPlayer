use crate::srt::Segment;
use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// 对 f32 采样做转写。language 为 None 时自动检测。
pub fn transcribe(
    model_path: &str,
    language: Option<&str>,
    samples: &[f32],
) -> Result<Vec<Segment>> {
    let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .with_context(|| format!("加载模型失败: {model_path}"))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(language);
    params.set_print_progress(false);
    params.set_print_special(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    let mut state = ctx.create_state()?;
    state.full(params, samples).context("whisper 推理失败")?;

    let n = state.full_n_segments()?;
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        // whisper-rs 时间戳单位为 10ms（厘秒），×10 转毫秒
        let start_ms = state.full_get_segment_t0(i)? as u64 * 10;
        let end_ms = state.full_get_segment_t1(i)? as u64 * 10;
        let text = state.full_get_segment_text(i)?;
        if !text.trim().is_empty() {
            out.push(Segment { start_ms, end_ms, text });
        }
    }
    Ok(out)
}
