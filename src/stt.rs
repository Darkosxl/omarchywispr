//! whisper.cpp wrapper: load once, transcribe many.

use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Stt {
    ctx: WhisperContext,
    lang: Option<String>,
    prompt: Option<String>,
}

impl Stt {
    pub fn load(model: &Path, lang: &str, prompt: Option<String>) -> Result<Stt, String> {
        let mut p = WhisperContextParameters::new();
        p.use_gpu(true).flash_attn(true);
        let path = model.to_str().ok_or("model path is not UTF-8")?;
        let ctx = WhisperContext::new_with_params(path, p).map_err(|e| format!("load {path}: {e}"))?;
        let lang = if lang == "auto" { None } else { Some(lang.to_string()) };
        Ok(Stt { ctx, lang, prompt })
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String, String> {
        let mut state = self.ctx.create_state().map_err(|e| e.to_string())?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(self.lang.as_deref());
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(p) = &self.prompt {
            params.set_initial_prompt(p);
        }
        state.full(params, samples).map_err(|e| e.to_string())?;

        let mut raw = String::new();
        for i in 0..state.full_n_segments() {
            let Some(seg) = state.get_segment(i) else { continue };
            if seg.no_speech_probability() > 0.6 {
                continue;
            }
            raw.push_str(&seg.to_str_lossy().map_err(|e| e.to_string())?);
            raw.push(' ');
        }
        Ok(clean(&raw))
    }
}

/// Whisper's well-known Turkish silence hallucinations (subtitle credits).
const HALLUCINATIONS: &[&str] = &["altyazı m.k", "izlediğiniz için teşekkürler", "abone olmayı unutmayın"];

/// Drop bracketed/parenthesised event tags, music glyphs and known
/// hallucinations; collapse whitespace.
///
/// ponytail: also eats real spoken parentheses — whisper almost never emits
/// them for speech, so not worth tracking token types.
pub fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0u32;
    for c in raw.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            '♪' => {}
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = text.to_lowercase();
    if HALLUCINATIONS.iter().any(|h| lower.trim_end_matches('.') == *h) {
        return String::new();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn clean_strips_tags_and_hallucinations() {
        assert_eq!(clean("  [BLANK_AUDIO] merhaba   (müzik çalıyor) dünya ♪ "), "merhaba dünya");
        assert_eq!(clean(" Altyazı M.K. "), "");
        assert_eq!(clean(" Deploy'u staging'e push ettim. "), "Deploy'u staging'e push ettim.");
        assert_eq!(clean(""), "");
    }
}
