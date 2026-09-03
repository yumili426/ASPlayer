//! 静音切块（能量阈值 VAD）。纯函数，可单测。

/// 一块音频区间（样本半开区间 + 毫秒）。块间首尾相接覆盖整段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub start_sample: usize,
    pub end_sample: usize,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// VAD 切块参数。`Default` 为面向 ASMR 的预设。
#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    pub sample_rate: u32,
    pub window_ms: u32,
    pub min_silence_ms: u32,
    pub min_chunk_ms: i64,
    pub max_chunk_ms: i64,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            window_ms: 30,
            min_silence_ms: 300,
            min_chunk_ms: 1000,
            max_chunk_ms: 30000,
        }
    }
}

impl VadConfig {
    /// 固定时长窗口切块：禁用静音检测，仅按 `max_chunk_ms` 硬切成等长首尾相接窗口。
    /// LLPlayer 式：不按停顿切，靠 whisper 自身的 30s 上下文 + no_speech 阈值在窗内断句。
    pub fn fixed_windows(max_chunk_ms: i64) -> Self {
        Self {
            sample_rate: 16000,
            window_ms: 30,       // 静音检测彻底关闭，此值不影响固定窗口
            min_silence_ms: u32::MAX,
            min_chunk_ms: 1000,  // 末尾不足 1s 的残窗并入前窗，避免产生垃圾行
            max_chunk_ms,
        }
    }
}

/// 能量阈值静音切块：静音孔为主，`max_chunk_ms` 硬切兜底，`min_chunk_ms` 并入前块。
/// 返回首尾相接、覆盖整段音频的块；空音频返回空 Vec。
pub fn split_samples(samples: &[f32], cfg: &VadConfig) -> Vec<Chunk> {
    if samples.is_empty() {
        return Vec::new();
    }
    let total = samples.len();
    let ms = |s: usize| (s as i64 * 1000) / cfg.sample_rate as i64;
    let sms = |m: i64| ((m * cfg.sample_rate as i64).div_euclid(1000)) as usize;

    if ms(total) <= cfg.min_chunk_ms {
        return vec![Chunk {
            start_sample: 0,
            end_sample: total,
            start_ms: 0,
            end_ms: ms(total),
        }];
    }

    let win = ((cfg.window_ms as i64 * cfg.sample_rate as i64) / 1000) as usize;
    let win = win.max(1);

    // 逐帧 RMS + 帧起点样本
    let mut rms: Vec<f32> = Vec::new();
    let mut fstart: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < total {
        fstart.push(i);
        let end = (i + win).min(total);
        let c = &samples[i..end];
        let m = (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt();
        rms.push(m);
        i = end;
    }

    // 自适应阈值：P90 参考电平 × 0.1，带绝对下限。
    // 用 P90 而非 P10：均匀响度（无静音）时 P10 会把全部帧误判为静音，P90 × 0.1 则正常。
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p90 = sorted[(sorted.len() - 1) * 9 / 10];
    let threshold = (p90 * 0.1).max(1e-5);

    // 静音孔 → 在孔中点切一刀；忽略贴近首/末的边缘孔（避免切出纯静音块）
    let edge_guard = sms(cfg.min_chunk_ms);
    let mut cuts: Vec<usize> = Vec::new();
    let mut k = 0;
    while k < rms.len() {
        if rms[k] >= threshold {
            k += 1;
            continue;
        }
        let s0 = k;
        while k < rms.len() && rms[k] < threshold {
            k += 1;
        }
        let gap_frames = k - s0;
        if gap_frames as i64 * cfg.window_ms as i64 >= cfg.min_silence_ms as i64 {
            let mid = fstart[s0 + gap_frames / 2];
            if mid >= edge_guard && mid <= total.saturating_sub(edge_guard) {
                cuts.push(mid);
            }
        }
    }

    // 由切点构造原始块
    let mut bounds = vec![0];
    for c in cuts {
        if *bounds.last().unwrap() < c && c < total {
            bounds.push(c);
        }
    }
    bounds.push(total);
    let raw: Vec<Chunk> = bounds
        .windows(2)
        .map(|w| Chunk {
            start_sample: w[0],
            end_sample: w[1],
            start_ms: ms(w[0]),
            end_ms: ms(w[1]),
        })
        .collect();

    // 合并 < min_chunk_ms 的块到前块
    let mut merged: Vec<Chunk> = Vec::new();
    for c in raw {
        if let Some(last) = merged.last_mut() {
            if c.end_ms - c.start_ms < cfg.min_chunk_ms {
                last.end_sample = c.end_sample;
                last.end_ms = c.end_ms;
                continue;
            }
        }
        merged.push(c);
    }

    // 拆分 > max_chunk_ms 的块（硬切）
    let mut out: Vec<Chunk> = Vec::new();
    for c in merged {
        let len = c.end_ms - c.start_ms;
        if len <= cfg.max_chunk_ms {
            out.push(c);
            continue;
        }
        let mut a = c.start_sample;
        loop {
            let a_ms = ms(a);
            let b = sms((a_ms + cfg.max_chunk_ms).min(ms(c.end_sample)))
                .max(a + 1)
                .min(c.end_sample);
            out.push(Chunk {
                start_sample: a,
                end_sample: b,
                start_ms: ms(a),
                end_ms: ms(b),
            });
            if b >= c.end_sample {
                break;
            }
            a = b;
        }
    }
    // 末块若小于 min_chunk_ms 且前面有块，并入前块（避免固定窗口末尾产生 <1s 残窗垃圾行）
    if out.len() >= 2 {
        let last_idx = out.len() - 1;
        if out[last_idx].end_ms - out[last_idx].start_ms < cfg.min_chunk_ms {
            let prev = last_idx - 1;
            let tail = out[last_idx];
            out[prev].end_sample = tail.end_sample;
            out[prev].end_ms = tail.end_ms;
            out.pop();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 16000;

    fn silence_ms(ms: i64) -> Vec<f32> {
        vec![0.0; (ms * SR as i64 / 1000) as usize]
    }
    fn tone_ms(ms: i64, freq: f32) -> Vec<f32> {
        let n = (ms * SR as i64 / 1000) as usize;
        (0..n).map(|i| 0.6 * (2.0 * PI * freq * i as f32 / SR as f32).sin()).collect()
    }

    #[test]
    fn empty_returns_empty() {
        assert_eq!(split_samples(&[], &VadConfig::default()), vec![]);
    }

    #[test]
    fn tiny_input_returns_single_chunk() {
        let s = tone_ms(500, 440.0); // 500ms < min_chunk 1000ms
        let chunks = split_samples(&s, &VadConfig::default());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_sample, 0);
        assert_eq!(chunks[0].end_sample, s.len());
        assert_eq!(chunks[0].start_ms, 0);
        assert_eq!(chunks[0].end_ms, 500);
    }

    #[test]
    fn splits_at_silence_gap() {
        let mut s = tone_ms(1000, 440.0);
        s.extend(silence_ms(500)); // 1.0s~1.5s 静音，≥ min_silence 300ms
        s.extend(tone_ms(1000, 440.0));
        let chunks = split_samples(&s, &VadConfig::default());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].start_ms, 0);
        assert_eq!(chunks[1].end_ms, 2500);
        // 静音孔 1.0s~1.5s 的中点约 1.25s，放宽容差
        assert!((1200..=1300).contains(&chunks[0].end_ms), "cut at {}", chunks[0].end_ms);
        assert_eq!(chunks[0].end_sample, chunks[1].start_sample);
    }

    #[test]
    fn short_silence_not_split() {
        let mut s = tone_ms(1000, 440.0);
        s.extend(silence_ms(200)); // < min_silence 300ms
        s.extend(tone_ms(1000, 440.0));
        let chunks = split_samples(&s, &VadConfig::default());
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn hard_split_max_chunk() {
        let s = tone_ms(90000, 440.0); // 90s 连续，无静音
        let chunks = split_samples(&s, &VadConfig::default());
        assert!(chunks.len() >= 3, "got {}", chunks.len());
        for c in &chunks {
            assert!(c.end_ms - c.start_ms <= 31000, "block too long: {}ms", c.end_ms - c.start_ms);
        }
        assert_eq!(chunks.last().unwrap().end_ms, 90000);
        // 首尾相接覆盖整段
        let mut prev = 0;
        for c in &chunks {
            assert_eq!(c.start_sample, prev);
            prev = c.end_sample;
        }
        assert_eq!(prev, s.len());
    }

    #[test]
    fn long_edge_silence_still_single_or_split_not_truncated() {
        // 首尾静音很长不重新切；首块必须从 0 开始、被完整覆盖
        let mut s = silence_ms(2000);
        s.extend(tone_ms(2000, 440.0));
        s.extend(silence_ms(2000));
        let chunks = split_samples(&s, &VadConfig::default());
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].start_ms, 0);
        let mut prev = 0;
        for c in &chunks {
            assert_eq!(c.start_sample, prev);
            prev = c.end_sample;
        }
        assert_eq!(prev, s.len());
    }

    #[test]
    fn fixed_windows_cut_contiguous_at_max() {
        // 45s 音频 → 20s + 20s + 5s，首尾相接覆盖整段
        let s = tone_ms(45000, 254.0);
        let chunks = split_samples(&s, &VadConfig::fixed_windows(20000));
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].start_ms, 0);
        assert_eq!(chunks[0].end_ms, 20000);
        assert_eq!(chunks[1].start_ms, 20000);
        assert_eq!(chunks[1].end_ms, 40000);
        assert_eq!(chunks[2].start_ms, 40000);
        assert_eq!(chunks[2].end_ms, 45000);
        let mut prev = 0;
        for c in &chunks {
            assert_eq!(c.start_sample, prev);
            prev = c.end_sample;
        }
        assert_eq!(prev, s.len());
    }

    #[test]
    fn fixed_windows_absorbs_tiny_tail() {
        // 40.5s 音频 → 20s + 20s + 0.5s，末残窗并入前块 → 20s + 20.5s
        let s = tone_ms(40500, 254.0);
        let chunks = split_samples(&s, &VadConfig::fixed_windows(20000));
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].start_ms, 0);
        assert_eq!(chunks[0].end_ms, 20000);
        assert_eq!(chunks[1].start_ms, 20000);
        assert_eq!(chunks[1].end_ms, 40500);
        let mut prev = 0;
        for c in &chunks {
            assert_eq!(c.start_sample, prev);
            prev = c.end_sample;
        }
        assert_eq!(prev, s.len());
    }
}
