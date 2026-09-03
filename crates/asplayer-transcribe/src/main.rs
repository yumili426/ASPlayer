use anyhow::Result;
use asplayer_transcribe::{audio, srt, translate, whisper};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "asplayer-transcribe", about = "ASPlayer 里程碑0 转写验证管线")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 媒体 → 音轨 → Whisper → SRT + segments.json
    Transcribe {
        /// 媒体文件路径
        #[arg(long)]
        media: String,
        /// whisper 模型路径（GGML/GGUF）
        #[arg(long)]
        model: String,
        /// 语言代码，如 ja/en；缺省自动检测
        #[arg(long)]
        lang: Option<String>,
        /// 输出目录，默认当前目录
        #[arg(long, default_value = ".")]
        out: String,
    },
    /// 对 transcribe 产物 segments.json 做批量翻译，输出双语 txt
    Translate {
        /// transcribe 生成的 segments.json
        #[arg(long)]
        input: String,
        /// OpenAI 兼容 API 地址，如 https://api.deepseek.com/v1
        #[arg(long, env = "ASPLAYER_API_BASE")]
        api_base: String,
        /// API Key
        #[arg(long, env = "ASPLAYER_API_KEY")]
        api_key: String,
        /// 模型名
        #[arg(long, default_value = "gpt-4o-mini")]
        model: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Transcribe { media, model, lang, out } => {
            let media_path = std::path::PathBuf::from(&media);
            let out_dir = std::path::PathBuf::from(&out);
            std::fs::create_dir_all(&out_dir)?;

            println!("[1/3] ffmpeg 抽取音轨…");
            let wav = audio::extract_wav(&media_path, &out_dir)?;

            println!("[2/3] whisper.cpp 转写中（模型：{model}）…");
            let samples = audio::read_samples_f32(&wav)?;
            let segments = whisper::transcribe(&model, lang.as_deref(), None, &samples)?;

            println!("[3/3] 写出结果（{} 段）", segments.len());
            let stem = media_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output")
                .to_string();
            let srt_path = out_dir.join(format!("{stem}.srt"));
            let json_path = out_dir.join(format!("{stem}.segments.json"));
            std::fs::write(&srt_path, srt::render_srt(&segments))?;
            std::fs::write(&json_path, serde_json::to_string_pretty(&segments)?)?;
            println!("完成：\n  {}\n  {}", srt_path.display(), json_path.display());
        }
        Cmd::Translate { input, api_base, api_key, model } => {
            let raw = std::fs::read_to_string(&input)?;
            let segments: Vec<srt::Segment> = serde_json::from_str(&raw)?;
            println!(
                "共 {} 段，开始批量翻译（每批 {} 句）…",
                segments.len(),
                translate::BATCH_SIZE
            );
            let map = translate::translate_segments(
                &segments,
                &api_base,
                &api_key,
                &model,
                "Simplified Chinese",
            )?;

            let mut bilingual = String::new();
            for (i, seg) in segments.iter().enumerate() {
                match map.get(&i) {
                    Some(trans) => bilingual.push_str(&format!("{}\n{}\n\n", seg.text.trim(), trans)),
                    None => bilingual.push_str(&format!("{}\n[未翻译]\n\n", seg.text.trim())),
                }
            }
            let out_txt = std::path::Path::new(&input).with_extension("bilingual.txt");
            std::fs::write(&out_txt, &bilingual)?;
            println!("完成：{}", out_txt.display());
        }
    }
    Ok(())
}
