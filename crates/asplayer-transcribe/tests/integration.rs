use std::path::Path;

#[test]
fn wav_roundtrip_16k_mono() -> anyhow::Result<()> {
    // 合成 1 秒 440Hz 正弦波 16k 单声道
    let dir = tempfile::tempdir()?;
    let wav_path = dir.path().join("tone.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::create(&wav_path, spec)?;
        for i in 0..16_000u32 {
            let v =
                ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16_000.0).sin() * 10_000.0) as i16;
            writer.write_sample(v)?;
        }
        writer.finalize()?;
    }

    let samples = asplayer_transcribe::audio::read_samples_f32(Path::new(&wav_path))?;
    assert_eq!(samples.len(), 16_000);
    assert!(samples.iter().all(|s| s.abs() <= 1.0));
    Ok(())
}
