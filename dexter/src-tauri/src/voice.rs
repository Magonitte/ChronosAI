use crate::{AppState, ChatMessage, VoiceConfig};
use base64::{engine::general_purpose::STANDARD, Engine};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::sync::OnceLock;
use tauri::{Emitter, Manager};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Removes Chatterbox-Turbo-style paralinguistic markers from text.
/// The bundled TTS stack (multilingual / Windows SAPI) does not interpret these;
/// they would be spoken as literal words or garbled audio.
fn paralinguistic_bracket_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\[\s*(?:laugh|chuckle|sigh|gasp|cough|clear\s+throat|sniff|groan|shush)\s*\]",
        )
        .expect("static regex")
    })
}

fn squeeze_spaces(s: &str) -> String {
    let s = paralinguistic_bracket_pattern().replace_all(s, "");
    let s = Regex::new(r" {2,}").expect("static regex").replace_all(&s, " ");
    s.trim().to_string()
}

pub fn strip_paralinguistic_brackets(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    squeeze_spaces(text)
}

fn markdown_for_tts_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?mx)
            \*\*([^*]+)\*\*          # **bold**
            | \*([^*]+)\*            # *italic*
            | __([^_]+)__            # __bold__
            | _([^_]+)_              # _italic_
            | `([^`]+)`              # `code`
            | ^\s*#{1,6}\s+          # headings
            | ^\s*[-*+]\s+           # bullets
            | ^\s*\d+\.\s+           # numbered lists
            ",
        )
        .expect("static regex")
    })
}

fn spoken_punctuation_words_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:v[ií]rgula|ponto(?:\s+final)?|ponto\s+e\s+v[ií]rgula|dois\s+pontos|tr[eê]s\s+pontos|retic[eê]ncias|interroga[cç][aã]o|exclama[cç][aã]o|abre\s+par[eê]nteses?|fecha\s+par[eê]nteses?|aspas|travess[aã]o|h[ií]fen|barra|asterisco|s[ií]mbolo|comma|period|semicolon|colon|ellipsis|question\s+mark|exclamation\s+mark)\b[,.]?",
        )
        .expect("static regex")
    })
}

/// Limite de caracteres por sessão de leitura em voz (~8–12 min de TTS).
pub const READ_ALOUD_MAX_CHARS: usize = 6_000;

const READ_ALOUD_TRUNCATED_SUFFIX: &str =
    " Fim desta parte. Diga continuar a leitura se quiser o restante.";

/// Remove bloco YAML `---` no início do arquivo.
pub fn strip_yaml_frontmatter(md: &str) -> String {
    let trimmed = md.trim_start();
    if !trimmed.starts_with("---") {
        return md.to_string();
    }
    let after_open = &trimmed[3..];
    if let Some(end) = after_open.find("\n---") {
        return after_open[end + 4..].trim_start().to_string();
    }
    md.to_string()
}

fn md_fence_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)```[^\n]*\n.*?```").expect("static regex"))
}

fn md_link_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\]]+)\]\([^)]+\)").expect("static regex"))
}

fn md_image_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"!\[([^\]]*)\]\([^)]+\)").expect("static regex"))
}

fn md_wiki_link_alias_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[[^\]|#]+\|([^\]]+)\]\]").expect("static regex"))
}

fn md_wiki_link_plain_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[([^\]]+)\]\]").expect("static regex"))
}

/// Remove bloco de notas de rodapé (`[^1]:` / Notas de Rodapé) — não narrar.
fn strip_markdown_footnotes_block(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut cut = s.len();
    for marker in [
        "**notas de rodapé:**",
        "**notas de rodape:**",
        "\n[^1]:",
    ] {
        if let Some(i) = lower.find(marker) {
            cut = cut.min(i);
        }
    }
    s[..cut].trim().to_string()
}

/// Converte Markdown (Obsidian/LDS: wikilinks, notas, ^vN) em texto corrido para narração.
pub fn markdown_file_to_plain_speech(md: &str) -> String {
    let mut s = strip_yaml_frontmatter(md);
    s = strip_markdown_footnotes_block(&s);

    // Navegação Obsidian: << Início | [[capítulo]] >>
    s = Regex::new(r"<<[^>]*>>")
        .expect("static regex")
        .replace_all(&s, " ")
        .into_owned();

    s = md_fence_pattern().replace_all(&s, " ").into_owned();
    s = md_image_pattern()
        .replace_all(&s, |caps: &regex::Captures| {
            caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string()
        })
        .into_owned();

    // [[alias|texto falado]] depois [[link]]
    s = md_wiki_link_alias_pattern()
        .replace_all(&s, "$1")
        .into_owned();
    s = md_wiki_link_plain_pattern()
        .replace_all(&s, "$1")
        .into_owned();
    s = md_link_pattern()
        .replace_all(&s, "$1")
        .into_owned();

    // Referências de nota [^1] e âncoras de versículo ^v1
    s = Regex::new(r"\[\^\d+\]")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();
    s = Regex::new(r"\^v\d+")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();

    // Número do versículo em negrito no início da linha: **1** Eu, ...
    s = Regex::new(r"(?m)^\*\*\d+\*\*\s*")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();
    // Negrito restante → texto simples
    s = Regex::new(r"\*\*([^*]+)\*\*")
        .expect("static regex")
        .replace_all(&s, "$1")
        .into_owned();

    s = Regex::new(r"(?m)^#{1,6}\s*")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();
    s = Regex::new(r"(?m)^\s*[-*+]\s+")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();
    s = Regex::new(r"(?m)^\s*\d+\.\s+")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();
    s = Regex::new(r"(?m)^\s*>\s*")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();
    s = Regex::new(r"(?m)^\s*[-*_]{3,}\s*$")
        .expect("static regex")
        .replace_all(&s, " ")
        .into_owned();
    s = Regex::new(r"`([^`]+)`")
        .expect("static regex")
        .replace_all(&s, "$1")
        .into_owned();

    // Linhas só com título duplicado após remover #
    s = Regex::new(r"(?m)^\s*1\s+n[eé]fi\s+1\s*$")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();

    sanitize_for_read_aloud(&s)
}

/// Sanitização para narração de arquivos: mantém `.` para cortar frases; sem metadados YAML falados.
pub fn sanitize_for_read_aloud(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut s = strip_paralinguistic_brackets(text);
    loop {
        let prev = s.clone();
        s = markdown_for_tts_pattern()
            .replace_all(&s, |caps: &regex::Captures| {
                caps.get(1)
                    .or_else(|| caps.get(2))
                    .or_else(|| caps.get(3))
                    .or_else(|| caps.get(4))
                    .or_else(|| caps.get(5))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            })
            .into_owned();
        if s == prev {
            break;
        }
    }
    s = spoken_punctuation_words_pattern()
        .replace_all(&s, " ")
        .into_owned();
    s = s
        .replace(['*', '_', '`', '#', '~', '^', '|', '\\', '{', '}', '[', ']'], " ")
        .replace('…', ".")
        .replace("...", ".")
        .replace('—', " ")
        .replace('–', " ")
        .replace('«', " ")
        .replace('»', " ")
        .replace('“', " ")
        .replace('”', " ")
        .replace('‘', " ")
        .replace('’', " ")
        .replace('\"', " ");
    s = Regex::new(r"\(\s*\)")
        .expect("static regex")
        .replace_all(&s, " ")
        .into_owned();
    s = Regex::new(r"[.!?]{2,}")
        .expect("static regex")
        .replace_all(&s, ". ")
        .into_owned();
    squeeze_spaces(&s)
}

fn truncate_at_sentence(text: String, max_chars: usize, suffix: &str) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }
    let budget = max_chars.saturating_sub(suffix.chars().count());
    let partial: String = text.chars().take(budget).collect();
    let cut = partial
        .rfind('.')
        .or_else(|| partial.rfind('!'))
        .or_else(|| partial.rfind('?'));
    let kept = if let Some(idx) = cut {
        if idx > 80 {
            partial.chars().take(idx + 1).collect()
        } else {
            partial
        }
    } else {
        partial
    };
    format!("{kept}{suffix}")
}

/// Prepara conteúdo de arquivo para narração (MD limpo, limite por sessão).
pub fn prepare_read_aloud_for_tts(content: &str, file_path: Option<&str>) -> String {
    let is_md = file_path
        .map(|p| p.to_lowercase().ends_with(".md"))
        .unwrap_or_else(|| content.trim_start().starts_with("---"));
    let plain = if is_md {
        markdown_file_to_plain_speech(content)
    } else {
        sanitize_for_read_aloud(content)
    };
    truncate_at_sentence(plain, READ_ALOUD_MAX_CHARS, READ_ALOUD_TRUNCATED_SUFFIX)
}

/// Resumo para histórico de chat (não gravar milhares de caracteres).
pub fn read_aloud_history_preview(spoken: &str) -> String {
    const N: usize = 280;
    let t = spoken.trim();
    if t.chars().count() <= N {
        format!("[Leitura em voz] {t}")
    } else {
        format!(
            "[Leitura em voz] {}…",
            t.chars().take(N).collect::<String>()
        )
    }
}

/// Texto para TTS após `read_file`.
pub fn spoken_read_file_result(tool_name: &str, result: &str, file_path: Option<&str>) -> String {
    if tool_name != "read_file" {
        return result.to_string();
    }
    let r = result.trim();
    if r.starts_with("Erro")
        || r.starts_with("Acesso negado")
        || r.starts_with("Arquivo não encontrado")
        || r.starts_with("Faltou")
        || r.starts_with("Não foi possível")
    {
        if r.contains("não encontrado") || r.contains("nao encontrado") {
            return "Não encontrei esse arquivo.".to_string();
        }
        if r.contains("Acesso negado") {
            return "Não tenho permissão para ler esse arquivo.".to_string();
        }
        return "Não consegui ler o arquivo.".to_string();
    }
    prepare_read_aloud_for_tts(r, file_path)
}

/// Limite seguro do XTTS em português (~203); margem para não cair no SAPI do Windows.
pub fn tts_xtts_safe_chunk_chars() -> usize {
    std::env::var("DEXTER_TTS_MAX_CHUNK_CHARS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|n| n.clamp(40, 200))
        .unwrap_or(180)
}

/// Próximo pedaço para TTS (nunca passa de `max_chars`; corta em frase ou espaço).
pub fn split_next_tts_chunk(text: &str, max_chars: usize) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= max_chars {
        return Some((text, ""));
    }

    if let Some(pos) = find_tts_chunk_end(text) {
        let head = text[..pos].trim();
        if !head.is_empty() && head.chars().count() <= max_chars {
            return Some((head, text[pos..].trim_start()));
        }
    }

    let hard_end = text
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let head = &text[..hard_end];
    let split_at = head
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)
        .filter(|&i| i >= 20)
        .unwrap_or(hard_end);
    let chunk = text[..split_at].trim();
    if chunk.is_empty() {
        return None;
    }
    Some((chunk, text[split_at..].trim_start()))
}

/// Envia TTS em chunks; respeita `cancel` (atalho parar, padrão Control+5).
pub async fn emit_chunked_tts_cancellable(
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    cancel: &CancellationToken,
    text: &str,
    read_aloud: bool,
) -> Result<(), String> {
    let max_chars = tts_xtts_safe_chunk_chars();
    let mut chunk_idx: u32 = 0;
    let mut remaining = text.trim();
    while !remaining.is_empty() {
        if cancel.is_cancelled() {
            return Err("interrupted".to_string());
        }
        let Some((chunk, rest)) = split_next_tts_chunk(remaining, max_chars) else {
            break;
        };
        remaining = rest;
        if chunk.is_empty() {
            continue;
        }
        let synth = if read_aloud {
            synthesize_read_aloud(config, chunk, chunk_idx).await
        } else {
            synthesize(config, chunk, chunk_idx).await
        };
        match synth {
            Ok(audio) => {
                app.emit(
                    "play_audio_chunk",
                    crate::AudioChunk {
                        index: chunk_idx,
                        audio,
                    },
                )
                .map_err(|e: tauri::Error| e.to_string())?;
            }
            Err(e) => {
                eprintln!("TTS chunk {} failed: {}", chunk_idx, e);
            }
        }
        chunk_idx += 1;
    }
    if cancel.is_cancelled() {
        return Err("interrupted".to_string());
    }
    app.emit("play_audio_done", chunk_idx)
        .map_err(|e: tauri::Error| e.to_string())?;
    Ok(())
}

/// TTS de leitura de arquivo: sanitização leve + sem fallback Windows em chunk grande.
pub async fn synthesize_read_aloud(
    config: &VoiceConfig,
    text: &str,
    seq: u32,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // XTTS em pt-BR vocaliza "." como "ponto"; normalize_periods só existia em sanitize_for_tts.
    let cleaned = normalize_periods_for_xtts(&sanitize_for_read_aloud(text));
    if cleaned.trim().is_empty() {
        return Err("texto vazio apos sanitizar leitura".into());
    }
    let max = tts_xtts_safe_chunk_chars();
    if cleaned.chars().count() > max {
        return Err(format!(
            "chunk de leitura excede {} caracteres (tem {})",
            max,
            cleaned.chars().count()
        )
        .into());
    }
    synthesize_xtts_only(config, &cleaned, seq, false).await
}

/// TTS após `search_files` — pasta amigável, sem caminho `C:\...`.
/// Corpo da tradução (sem cabeçalho) para exibir no chat.
pub fn translation_body(result: &str) -> String {
    strip_translation_header(result.trim())
}

/// Texto para TTS após `translate_selection`.
pub fn spoken_translate_tts(
    result: &str,
    target: &crate::system_tools::TranslateTarget,
) -> String {
    let r = result.trim();
    if r.starts_with("translate_selection:")
        || r.starts_with("Erro")
        || r.starts_with("Nenhum texto")
        || r.starts_with("Falha")
    {
        if r.contains("Nenhum texto") {
            return "Não encontrei texto selecionado nem no clipboard.".to_string();
        }
        return "Não consegui traduzir o texto.".to_string();
    }
    if target.voice_reads_translation_aloud() {
        translation_body(r)
    } else {
        format!(
            "Tradução para {} concluída. O texto completo está na área de transferência.",
            target.label
        )
    }
}

/// Remove cabeçalho `Tradução (idioma):` antes do TTS ou preview.
fn strip_translation_header(r: &str) -> String {
    let r = r.trim();
    if let Some(rest) = r.strip_prefix("Tradução (") {
        if let Some((_, body)) = rest.split_once("):\n") {
            return body.trim().to_string();
        }
        if let Some((_, body)) = rest.split_once("):") {
            return body.trim().to_string();
        }
    }
    r.to_string()
}

pub fn spoken_search_files_result(result: &str) -> String {
    let r = result.trim();
    if r.starts_with("Nenhum arquivo") {
        return "Nenhum arquivo encontrado.".to_string();
    }
    if let Some((_header, body)) = r.split_once('\n') {
        let lines: Vec<&str> = body.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if lines.len() == 1 {
            let line = lines[0].trim_start_matches('•').trim();
            if let Some((name, loc)) = line.split_once(" — ") {
                return format!("Achei {name} em {loc}.");
            }
            return format!("Achei {line}.");
        }
        if !lines.is_empty() {
            return format!("Achei {} arquivos.", lines.len());
        }
    }
    r.to_string()
}

pub fn spoken_write_file_result(tool_name: &str, result: &str) -> String {
    if tool_name != "write_file" {
        return result.to_string();
    }
    let r = result.trim();
    if r.starts_with("Erro")
        || r.starts_with("Faltou")
        || r.starts_with("Não foi possível")
        || r.starts_with("Não encontrei")
        || r.contains("Acesso negado")
    {
        return "Não consegui criar o arquivo.".to_string();
    }
    if r.starts_with("Arquivo salvo:") {
        return "Arquivo criado.".to_string();
    }
    result.to_string()
}

/// Text destined for TTS: no markdown, no spoken punctuation names, gentler symbols for XTTS/SAPI.
pub fn sanitize_for_tts(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    if lower.starts_with("arquivo salvo:") {
        return "Arquivo criado.".to_string();
    }
    let mut s = strip_paralinguistic_brackets(text);

    // Unwrap markdown inline styles to plain words.
    loop {
        let prev = s.clone();
        s = markdown_for_tts_pattern()
            .replace_all(&s, |caps: &regex::Captures| {
                caps.get(1)
                    .or_else(|| caps.get(2))
                    .or_else(|| caps.get(3))
                    .or_else(|| caps.get(4))
                    .or_else(|| caps.get(5))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            })
            .into_owned();
        if s == prev {
            break;
        }
    }

    s = spoken_punctuation_words_pattern()
        .replace_all(&s, " ")
        .into_owned();

    s = trim_wrapping_quotes(&s);

    // Symbols XTTS often reads aloud literally.
    s = s
        .replace(['*', '_', '`', '#', '~', '^', '|', '\\', '{', '}', '[', ']'], " ")
        .replace('…', ".")
        .replace("...", ".")
        .replace('—', " ")
        .replace('–', " ")
        .replace('«', " ")
        .replace('»', " ")
        .replace('“', " ")
        .replace('”', " ")
        .replace('‘', " ")
        .replace('’', " ")
        .replace('\"', " ");

    // Colons often become "dois pontos" in bad TTS; use a short pause via comma.
    s = Regex::new(r"\s*:\s*")
        .expect("static regex")
        .replace_all(&s, ", ")
        .into_owned();

    // Drop empty parentheses and collapse repeated sentence enders.
    s = Regex::new(r"\(\s*\)")
        .expect("static regex")
        .replace_all(&s, " ")
        .into_owned();
    s = Regex::new(r"[.!?]{2,}")
        .expect("static regex")
        .replace_all(&s, ". ")
        .into_owned();
    s = Regex::new(r",\s*,")
        .expect("static regex")
        .replace_all(&s, ", ")
        .into_owned();

    s = normalize_periods_for_xtts(&s);

    squeeze_spaces(&s)
}

/// XTTS em português costuma vocalizar `.` como a palavra "ponto" (e o LLM às vezes escreve "ponto" no fim).
fn normalize_periods_for_xtts(s: &str) -> String {
    let mut s = s.trim().to_string();
    if s.is_empty() {
        return s;
    }

    // "… frase. ponto" / "… frase ponto final" escrito pelo modelo
    let trailing_spoken = Regex::new(r"(?i)(?:\s*\.?\s*ponto(?:\s+final)?)+\s*$").expect("static regex");
    loop {
        let prev = s.clone();
        s = trailing_spoken.replace_all(&s, "").into_owned();
        s = s.trim().to_string();
        if s == prev {
            break;
        }
    }

    // Pausa entre frases no mesmo chunk sem vocalizar "ponto"
    s = Regex::new(r"\.(\s+)")
        .expect("static regex")
        .replace_all(&s, ",$1")
        .into_owned();

    // Fim do chunk: sem . ! ? (a pausa vem do fim do áudio / próximo chunk)
    s = Regex::new(r"[.!?]+\s*$")
        .expect("static regex")
        .replace_all(&s, "")
        .into_owned();

    s.trim().trim_end_matches(',').trim().to_string()
}

fn trim_wrapping_quotes(s: &str) -> String {
    let mut s = s.trim().to_string();
    loop {
        let trimmed = s.trim_matches(|c: char| {
            matches!(c, '"' | '\'' | '«' | '»' | '“' | '”' | '‘' | '’')
        });
        if trimmed.len() == s.len() {
            break;
        }
        s = trimmed.trim().to_string();
    }
    while s.starts_with('"') || s.starts_with('«') || s.starts_with('“') {
        s = s[s.chars().next().map(|c| c.len_utf8()).unwrap_or(1)..]
            .trim_start()
            .to_string();
    }
    while s.ends_with('"') || s.ends_with('»') || s.ends_with('”') {
        s = s[..s.len().saturating_sub(1)].trim_end().to_string();
    }
    s
}

/// Remove JSON de tool call vazado no fim de uma frase (mantém o preâmbulo falável).
pub fn strip_leaked_tool_json(text: &str) -> String {
    let Some(idx) = text.find('{') else {
        return text.trim().to_string();
    };
    let before = text[..idx].trim();
    if before.is_empty() {
        return String::new();
    }
    let mut out = before.to_string();
    if out.ends_with('"') {
        out.pop();
        out = out.trim_end().to_string();
    }
    out
}

/// LLM sometimes emits raw tool-call syntax as spoken text; never send that to TTS.
pub fn looks_like_leaked_tool_call(text: &str) -> bool {
    let t = text.to_lowercase();
    if t.contains("tool_call") || t.contains("<tool_call>") {
        return true;
    }
    if t.contains("parameters")
        && (t.contains("name,")
            || t.contains("name ")
            || t.contains("\"name\"")
            || t.contains("function"))
    {
        return true;
    }
    const TOOL_NAMES: &[&str] = &[
        "web_fetch",
        "web fetch",
        "fetch_fx_quote",
        "fetch_weather",
        "launch_desktop_app",
        "launchdesktopapp",
        "close_desktop_app",
        "play_music_query",
        "control_media_playback",
        "open_url",
    ];
    if TOOL_NAMES.iter().any(|n| t.contains(n)) {
        return true;
    }
    t.contains("https://") && t.contains("url") && t.chars().count() < 140
}

/// Chunks that should not be sent to TTS (meta phrases, not answer content).
pub fn is_voice_filler_chunk(text: &str) -> bool {
    let normalized = text
        .trim()
        .trim_end_matches(|c: char| matches!(c, ',' | '.' | ':' | ';'))
        .to_lowercase();
    const FILLERS: &[&str] = &[
        "a resposta é",
        "a resposta e",
        "em resumo",
        "resumindo",
        "em suma",
        "ou seja",
        "portanto",
        "então",
        "veja só",
        "veja bem",
    ];
    if FILLERS.iter().any(|f| normalized == *f) {
        return true;
    }
    if normalized.starts_with("a resposta") && normalized.chars().count() <= 24 {
        return true;
    }
    false
}

/// Full pipeline: sanitize + drop filler-only lines. Returns None to skip TTS for this chunk.
pub fn prepare_voice_sentence_for_tts(raw: &str) -> Option<String> {
    let raw_for_tts = if looks_like_leaked_tool_call(raw) {
        let stripped = strip_leaked_tool_json(raw);
        if stripped.is_empty() || looks_like_leaked_tool_call(&stripped) {
            eprintln!(
                "[voice] skip_tts_chunk | tool_leak | text=\"{}\"",
                raw.chars().take(80).collect::<String>()
            );
            return None;
        }
        eprintln!(
            "[voice] tool_leak_recovered | spoken=\"{}\"",
            stripped.chars().take(80).collect::<String>()
        );
        stripped
    } else {
        raw.to_string()
    };
    let s = sanitize_for_tts(&raw_for_tts);
    if s.is_empty() || is_voice_filler_chunk(&s) || looks_like_leaked_tool_call(&s) {
        if !s.is_empty() {
            eprintln!(
                "[voice] skip_tts_chunk | filler | text=\"{}\"",
                s.chars().take(80).collect::<String>()
            );
        }
        return None;
    }
    Some(s)
}

/// Play a short feedback beep via Windows system speaker.
/// Only plays when `audio_feedback` is enabled in config.
pub fn play_mic_beep(config: &VoiceConfig) {
    if !config.audio_feedback {
        return;
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-Command", "[Console]::Beep(800, 80)"])
            .spawn();
    }
}

// ── Audio Recording (cpal) ──

/// Record audio on the current thread until `is_recording` is set to false.
/// Writes samples directly into AppState's shared buffer.
pub fn record_audio(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("Nenhum dispositivo de entrada de áudio disponível")?;

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;

    // Store sample rate in state
    {
        let state = app.state::<AppState>();
        *state.recording_sample_rate.lock().unwrap() = sample_rate;
    }

    let app_clone = app.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let app_ref = app.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let state = app_ref.state::<AppState>();
                    state.recorded_samples.lock().unwrap().extend_from_slice(data);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let app_ref = app.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let state = app_ref.state::<AppState>();
                    state.recorded_samples.lock().unwrap().extend_from_slice(&floats);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?
        }
        format => {
            return Err(format!("Formato de amostra de áudio não suportado: {:?}", format).into());
        }
    };

    stream.play()?;

    // Play mic-open feedback beep if enabled.
    {
        let state = app_clone.state::<AppState>();
        let cfg = state.config.lock().unwrap().clone();
        play_mic_beep(&cfg);
    }

    // Spin until recording is stopped
    loop {
        let is_rec = *app_clone.state::<AppState>().is_recording.lock().unwrap();
        if !is_rec {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Stream drops here, stopping recording
    Ok(())
}

// ── Whisper Transcription (HTTP-based) ──

/// Transcribe audio using an HTTP whisper server (OpenAI-compatible /v1/audio/transcriptions).
pub async fn transcribe_audio(
    whisper_url: &str,
    samples: &[f32],
    source_sample_rate: u32,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Resample to 16kHz mono if needed
    let audio_16k = if source_sample_rate != 16000 {
        resample(samples, source_sample_rate, 16000)
    } else {
        samples.to_vec()
    };

    // Convert f32 samples to WAV bytes
    let wav_bytes = samples_to_wav(&audio_16k, 16000);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let part = reqwest::multipart::Part::bytes(wav_bytes.clone())
        .file_name("audio.wav")
        .mime_str("audio/wav")?;

    let form = reqwest::multipart::Form::new()
        .text("model", "whisper")
        .text("language", "pt")
        .part("file", part);

    let base_url = whisper_url.trim_end_matches('/');
    let primary_url = format!("{}/v1/audio/transcriptions", base_url);
    let fallback_url = format!("{}/inference", base_url);

    let resp = client
        .post(&primary_url)
        .multipart(form)
        .send()
        .await?;

    let status = resp.status();

    // On 404 — the whisper-server likely uses the legacy /inference route.
    // Rebuild the multipart (it was consumed) and retry.
    if status.as_u16() == 404 {
        eprintln!("[STT] Rota {} retornou 404, tentando fallback {}...", primary_url, fallback_url);

        let fallback_part = reqwest::multipart::Part::bytes(wav_bytes.clone())
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let fallback_form = reqwest::multipart::Form::new()
            .text("model", "whisper")
            .text("language", "pt")
            .text("response_format", "json")
            .part("file", fallback_part);

        let fallback_resp = client
            .post(&fallback_url)
            .multipart(fallback_form)
            .send()
            .await?;

        let fb_status = fallback_resp.status();
        if !fb_status.is_success() {
            let body = fallback_resp.text().await.unwrap_or_default();
            return Err(format!(
                "Whisper API: rota primária (/v1/audio/transcriptions) retornou 404 e fallback (/inference) falhou com {}: {}. \
                 Verifique se o whisper-server está rodando com --request-path \"/v1/audio\" --inference-path \"/transcriptions\", \
                 ou se o servidor suporta a rota /inference.",
                fb_status, body
            ).into());
        }

        #[derive(Deserialize)]
        struct TranscriptionResponse {
            text: String,
        }

        let result: TranscriptionResponse = fallback_resp.json().await?;
        return Ok(result.text.trim().to_string());
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let hint = if status.as_u16() == 501 && body.contains("does not support audio") {
            " — Esta URL provavelmente aponta para um servidor LLM somente texto, não para um servidor Whisper STT. \
             Verifique se Whisper Server URL nas Configurações aponta para um servidor whisper.cpp dedicado \
             (ex: http://localhost:8081), não a mesma porta do LLM."
        } else if status.as_u16() == 404 {
            " — Rota /v1/audio/transcriptions não encontrada. Inicie o whisper-server com \
             --request-path \"/v1/audio\" --inference-path \"/transcriptions\" ou verifique a URL."
        } else {
            ""
        };
        return Err(format!("Whisper API erro {}{}: {}", status, hint, body).into());
    }

    #[derive(Deserialize)]
    struct TranscriptionResponse {
        text: String,
    }

    let result: TranscriptionResponse = resp.json().await?;
    Ok(result.text.trim().to_string())
}

/// Convert f32 mono audio samples to WAV bytes.
fn samples_to_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    use std::io::Cursor;

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut buf, spec).expect("Failed to create WAV writer");
        for &sample in samples {
            let s = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(s).expect("Failed to write WAV sample");
        }
        writer.finalize().expect("Failed to finalize WAV");
    }
    buf.into_inner()
}

fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = to_rate as f64 / from_rate as f64;
    let output_len = (input.len() as f64 * ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 / ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;

        let sample = if idx + 1 < input.len() {
            input[idx] as f64 * (1.0 - frac) + input[idx + 1] as f64 * frac
        } else if idx < input.len() {
            input[idx] as f64
        } else {
            0.0
        };

        output.push(sample as f32);
    }

    output
}

// ── LLM Chat (llama.cpp / OpenAI-compatible API) ──

// ── OpenAI-compatible message types ──

#[derive(Serialize, Clone)]
struct OpenAIMessage {
    role: String,
    content: serde_json::Value, // String or array for vision
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Clone)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIToolFunction,
}

#[derive(Serialize, Clone)]
struct OpenAIToolFunction {
    name: String,
    arguments: String, // JSON string
}

#[derive(Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    stream: bool,
    max_tokens: u32,
    temperature: f32,
    chat_template_kwargs: serde_json::Value,
    thinking_budget_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

// ── SSE streaming response types ──

#[derive(Deserialize)]
struct OpenAIStreamChunk {
    choices: Vec<OpenAIStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAIStreamChoice {
    delta: Option<OpenAIStreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Clone)]
struct OpenAIStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIStreamToolCallDelta>>,
}

#[derive(Deserialize, Clone)]
struct OpenAIStreamToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAIStreamFunctionDelta>,
}

#[derive(Deserialize, Clone)]
struct OpenAIStreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

// ── Shared tool call type (used across voice.rs and lib.rs) ──

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

/// Serializable tool call output (for assistant messages).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolCallOut {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunctionOut,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolCallFunctionOut {
    pub name: String,
    pub arguments: String, // JSON string
}

impl ToolCall {
    pub fn to_out(&self) -> ToolCallOut {
        ToolCallOut {
            id: self.id.clone(),
            call_type: "function".to_string(),
            function: ToolCallFunctionOut {
                name: self.name.clone(),
                arguments: serde_json::to_string(&self.arguments).unwrap_or_default(),
            },
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ToolCategory — roteamento de ferramentas (Tier 1 scaffolding, ativo no Tier 2)
// ─────────────────────────────────────────────────────────────────────────────

/// Categoria de ferramenta usada para restringir o conjunto exposto ao LLM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolCategory {
    System,
    Media,
    Files,
    Web,
    Knowledge,
    Automation,
}

/// Detecta categorias relevantes com base na transcrição do usuário.
/// Retorna slice vazio quando sem contexto suficiente (→ todas as ferramentas expostas).
pub fn detect_tool_categories(transcript: &str) -> Vec<ToolCategory> {
    let t = transcript.to_lowercase();
    let mut cats = Vec::new();

    // Sistema
    if t.contains("volume") || t.contains("bateria") || t.contains("ram") || t.contains("processador")
        || t.contains("janela") || t.contains("clipboard") || t.contains("transferência")
        || t.contains("notificaç") || t.contains("lembret") || t.contains("sistema")
        || t.contains("rede") || t.contains("ip") || t.contains("internet") || t.contains("mac")
        || t.contains("gateway") || t.contains("dns") || t.contains("wi-fi") || t.contains("wifi")
        || t.contains("tecla") || t.contains("digita") || t.contains("pressiona")
        || t.contains("disco") || t.contains("limpeza") || t.contains("espaço") || t.contains("espaco")
        || t.contains("temp") || t.contains("lixeira") || t.contains("cache")
    {
        cats.push(ToolCategory::System);
    }

    // Arquivos
    if t.contains("arquivo") || t.contains("pasta") || t.contains("recente")
        || t.contains("lê") || t.contains("abr") || t.contains("salva")
        || t.contains("escreve") || t.contains("busca") || t.contains("procura")
        || t.contains("vigi") || t.contains("monitor") || t.contains("modificaç")
    {
        cats.push(ToolCategory::Files);
    }

    // Mídia
    if t.contains("música") || t.contains("toca") || t.contains("pause")
        || t.contains("volume") || t.contains("playlist") || t.contains("canção")
        || t.contains("youtube") || t.contains("radio") || t.contains("próxima")
        || t.contains("áudio") || t.contains("som") || t.contains("fone")
        || t.contains("dispositivo") || t.contains("saída")
    {
        cats.push(ToolCategory::Media);
    }

    // Web
    if t.contains("site") || t.contains("abre") || t.contains("navega")
        || t.contains("pesquisa") || t.contains("googl") || t.contains("notícia")
        || t.contains("clima") || t.contains("dólar") || t.contains("cotaç")
        || t.contains("email") || t.contains("calendário") || t.contains("compromisso")
        || t.contains("evento") || t.contains("outlook")
        || t.contains("imagem") || t.contains("gera") || t.contains("desenha") || t.contains("cria imagem")
        || t.contains("stable diffusion") || t.contains("ilustraç")
    {
        cats.push(ToolCategory::Web);
    }

    // Base de conhecimento
    if t.contains("base de conhecimento") || t.contains("meus documentos")
        || t.contains("minhas notas") || t.contains("o que salvei")
        || t.contains("snippet") || t.contains("atalho") || t.contains("trecho")
    {
        cats.push(ToolCategory::Knowledge);
    }

    // Automação / comandos
    if t.contains("executa") || t.contains("roda") || t.contains("comando")
        || t.contains("script") || t.contains("powershell") || t.contains("terminal")
        || t.contains("clique") || t.contains("click") || t.contains("scroll") || t.contains("rola")
        || t.contains("mouse") || t.contains("cursor") || t.contains("coordenada")
        || t.contains("traduz") || t.contains("tradução") || t.contains("traducao")
        || t.contains("translate")
    {
        cats.push(ToolCategory::Automation);
    }

    cats
}

/// Build tool definitions based on enabled tools in config.
/// Tools are ordered by frequency of use — LLMs have primacy bias, so
/// the most-used tools appear first to improve selection accuracy.
/// `_categories`: Tier 2 routing (no-op when empty — all tools returned).
pub fn build_tools(tools_config: &crate::ToolsConfig, categories: &[ToolCategory]) -> Vec<serde_json::Value> {
    let mut tools = Vec::new();

    // ── TIER 1: Comandos mais frequentes ──

    // 1. get_current_time — "que horas são?", "qual a data?"
    if tools_config.get_current_time {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Obtém data, hora e dia da semana atuais.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    // 2. adjust_system_volume — "aumenta volume", "silenciar"
    if tools_config.media_controls {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "adjust_system_volume",
                "description": "Ajusta o volume principal do Windows: aumentar, diminuir ou silenciar. Use esta tool, não run_command, para controle de volume.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["up", "down", "mute_toggle"],
                            "description": "up: volume mais alto; down: volume mais baixo; mute_toggle: alternar mudo"
                        },
                        "steps": {
                            "type": "integer",
                            "description": "Só para up/down: quantas vezes pressionar a tecla (padrão 3, máx. 50)"
                        }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    // 3. launch_desktop_app — "abre o Chrome", "abre o VS Code"
    if tools_config.launch_desktop_app {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "launch_desktop_app",
                "description": "Abre um app de desktop. Use para: Chrome, VS Code, Terminal, Edge, Discord, Office, Paint, etc.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "app": {
                            "type": "string",
                            "enum": ["cursor","vscode","terminal","chrome","edge","discord","obs","snipping_tool","media_player","groove","excel","word","powerpoint","outlook","paint"],
                            "description": "Id do app: cursor, vscode, terminal, chrome, edge, discord, obs, snipping_tool, media_player/groove, excel, word, powerpoint, outlook, paint"
                        }
                    },
                    "required": ["app"]
                }
            }
        }));

        // 4. close_desktop_app — "fecha o Chrome"
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "close_desktop_app",
                "description": "Fecha um app de desktop. Mesmos apps que launch_desktop_app.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "app": {
                            "type": "string",
                            "description": "Mesmo id de launch_desktop_app (cursor, vscode, terminal, chrome, edge, discord, obs, snipping_tool, media_player, excel, word, powerpoint, outlook, paint)"
                        }
                    },
                    "required": ["app"]
                }
            }
        }));
    }

    // 5. control_media_playback — "pausa", "próxima faixa"
    if tools_config.media_controls {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "control_media_playback",
                "description": "Controla a reprodução de mídia: play, pause, toggle, next, previous, stop, status. Requer que um player ou aba de vídeo já esteja tocando. Para tocar uma música pelo nome, use play_music_query.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["play", "pause", "toggle", "next", "previous", "stop", "status"],
                            "description": "play: retoma ou inicia se o app permitir (costuma precisar de YouTube ou Spotify abertos); pause; toggle; next/previous; stop; status: tocando agora"
                        }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    // ── TIER 2: Comandos de frequência média ──

    // 6. take_screenshot — "o que está na tela?"
    if tools_config.screenshot {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "take_screenshot",
                "description": "Captura e descreve a tela. Use APENAS quando perguntar 'o que está na tela?', 'descreve a tela', 'o que você vê?'. NÃO use para abrir pastas/arquivos/comandos.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "O que observar ou descrever na captura. O padrão é uma descrição geral."
                        }
                    }
                }
            }
        }));
    }

    // 7. list_running_apps — "quais apps estão abertos?"
    if tools_config.list_apps {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_running_apps",
                "description": "Lista os aplicativos em execução no PC.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    // 8. open_url — "abre o site X"
    if tools_config.open_url {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "open_url",
                "description": "Abre uma URL no navegador. Não use para tocar música no YouTube — use play_music_query.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL a abrir"
                        }
                    },
                    "required": ["url"]
                }
            }
        }));
    }

    // 9. fetch_fx_quote + web_fetch — cotação e páginas web
    if tools_config.web_fetch {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "fetch_fx_quote",
                "description": "Cotação cambial atual (dólar, euro, iene, libra em reais). Preferir sobre web_fetch. Pares: USD-BRL, EUR-BRL, JPY-BRL, GBP-BRL.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pair": {
                            "type": "string",
                            "description": "Par cambial, ex.: USD-BRL, EUR-BRL"
                        }
                    },
                    "required": ["pair"]
                }
            }
        }));
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "fetch_weather",
                "description": "Clima atual ou previsão diária. day_offset: omitir = agora; 0 = hoje (máx/mín); 1 = amanhã; 2 = depois de amanhã. location vazio = IP ou DEXTER_WEATHER_CITY.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "Cidade ou região, ex.: São Paulo, Juiz de Fora, JV"
                        },
                        "day_offset": {
                            "type": "integer",
                            "description": "0=hoje, 1=amanhã, 2=depois de amanhã; omitir para temperatura atual"
                        }
                    },
                    "required": []
                }
            }
        }));
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Baixa uma página web e retorna o conteúdo textual.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL a buscar"
                        }
                    },
                    "required": ["url"]
                }
            }
        }));
    }

    // ── TIER 3: Ferramentas de música (menos frequentes) ──

    if tools_config.media_controls {
        // 10. play_music_query — "toque After Insanity"
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "play_music_query",
                "description": "Toca música por nome (artista opcional). Busca local primeiro, fallback YouTube. Use prefer_youtube=true só se usuário pedir YouTube. Use prefer_native_player=true se pedir reprodutor/Groove.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Título da música (obrigatório), ex.: After Insanity"
                        },
                        "artist": {
                            "type": "string",
                            "description": "Artista ou banda opcional para refinar o resultado"
                        },
                        "prefer_youtube": {
                            "type": "boolean",
                            "description": "true só quando o usuário pedir explicitamente YouTube; pula busca local"
                        },
                        "prefer_native_player": {
                            "type": "boolean",
                            "description": "true quando pedir player de música / reprodutor / Groove no Windows"
                        }
                    },
                    "required": ["query"]
                }
            }
        }));

        // 11. native_music_library_shuffle_play — "toca todas as músicas"
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "native_music_library_shuffle_play",
                "description": "Abre a biblioteca inteira do player de música (Groove) no modo aleatório. Rápido, sem varredura de disco.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));

        // 12. play_local_music_playlist — "playlist do Metallica"
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "play_local_music_playlist",
                "description": "Cria uma playlist M3U com as músicas de um artista ou banda e abre no player.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "artist": {
                            "type": "string",
                            "description": "Nome do artista ou banda como nas pastas/arquivos, ex.: Linkin Park"
                        }
                    },
                    "required": ["artist"]
                }
            }
        }));

        // 13. play_full_local_music_library — export M3U (raro)
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "play_full_local_music_library",
                "description": "Varredura completa do disco gerando M3U gigante. Use apenas para exportação explícita solicitada pelo usuário.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "explicit_m3u_export_request": {
                            "type": "boolean",
                            "description": "Deve ser true. Só true quando o usuário pediu explicitamente exportar/criar M3U gigante por varredura — nunca para um «toca tudo» comum."
                        },
                        "include_downloads_documents": {
                            "type": "boolean",
                            "description": "Se true ou omitido, após as pastas principais de Música também varre Downloads, Vídeos, Documentos, Área de trabalho e OneDrive Documentos. Se false, só biblioteca de Música + DEXTER_MUSIC_PATHS."
                        }
                    },
                    "required": ["explicit_m3u_export_request"]
                }
            }
        }));
    }

    // ── TIER 4: Ferramentas de uso esporádico ──

    // 14. search_knowledge — busca em documentos locais
    if tools_config.search_knowledge {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_knowledge",
                "description": "Pesquisa a base de conhecimento local por informações relevantes.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Consulta de busca para encontrar conhecimento relevante"
                        }
                    },
                    "required": ["query"]
                }
            }
        }));
    }

    // 15. read_clipboard — "o que eu copiei?"
    if tools_config.read_clipboard {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_clipboard",
                "description": "Lê o texto atual da área de transferência. Use quando o usuário perguntar o que está copiado, o que tem no clipboard ou na área de transferência. NÃO use run_command.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    // 16. run_command — fallback de último recurso
    if tools_config.run_command {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Executa um comando PowerShell no PC e retorna a saída. Use apenas para tarefas que não têm ferramenta dedicada (apps, música, volume, tela, web, clipboard, etc. têm tools próprias). NÃO use para ler a área de transferência — use read_clipboard.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Comando shell a executar (PowerShell)"
                        }
                    },
                    "required": ["command"]
                }
            }
        }));
    }

    // ── Tier 1: novas ferramentas de sistema ─────────────────────────────────

    if tools_config.write_clipboard {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "write_clipboard",
                "description": "Copia um texto para a área de transferência do Windows.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Texto a copiar para o clipboard" }
                    },
                    "required": ["text"]
                }
            }
        }));
    }

    if tools_config.get_active_window {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_active_window",
                "description": "Retorna o título da janela em foco e o nome do processo.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.system_info {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "system_info",
                "description": "Informa uso de CPU, RAM livre, espaço em disco, bateria e tempo de atividade.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.schedule_notification {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "schedule_notification",
                "description": "Agenda lembrete Toast no Windows com som. delay_seconds OU datetime HH:MM. sound: reminder (padrão), alarm, chime, default, silent. NÃO use run_command.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "message": { "type": "string", "description": "Texto do lembrete (padrão: Lembrete)" },
                        "delay_seconds": { "type": "integer", "description": "Atraso em segundos (ex.: 30, 300)" },
                        "datetime": { "type": "string", "description": "Hora alvo HH:MM (ex.: 14:30)" },
                        "sound": {
                            "type": "string",
                            "enum": ["reminder", "alarm", "chime", "default", "silent"],
                            "description": "Perfil de som do lembrete"
                        }
                    },
                    "required": []
                }
            }
        }));
    }

    if tools_config.clipboard_history {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "clipboard_history",
                "description": "Acessa o histórico da área de transferência. action=list lista todas; action=get retorna uma entrada pelo índice.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["list", "get"] },
                        "index": { "type": "integer", "description": "Índice da entrada (0 = mais recente). Obrigatório com action=get." }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    if tools_config.search_files {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Busca arquivos por nome (fuzzy) em Área de trabalho, Documentos, Downloads e Imagens. Resposta só com nome e pasta (sem caminho C:\\). Use PRIMEIRO se o caminho completo for desconhecido.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Parte do nome do arquivo (ex.: relatorio, config.json)" },
                        "max_results": { "type": "integer", "description": "Máximo de resultados (padrão 10)" }
                    },
                    "required": ["query"]
                }
            }
        }));
    }

    if tools_config.get_recent_files {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_recent_files",
                "description": "Lista os arquivos mais recentemente acessados no Windows.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "max": { "type": "integer", "description": "Quantidade máxima (padrão 10)" }
                    }
                }
            }
        }));
    }

    if tools_config.read_file {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Lê o conteúdo de um arquivo de texto dentro das pastas permitidas (máx 512KB). Nome simples (ex.: love.txt) resolve para a Área de trabalho. Se não souber o caminho, use search_files antes.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Caminho do arquivo. ~/Desktop/arquivo.txt, ~/Documents/… ou nome simples (love.txt → Área de trabalho)." }
                    },
                    "required": ["path"]
                }
            }
        }));
    }

    if tools_config.write_file {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Cria ou sobrescreve um arquivo de texto dentro das pastas permitidas (Desktop, Documents, Downloads). Use APENAS quando o usuário pede para CRIAR, SALVAR ou ESCREVER. Caminho: ~/Desktop/arquivo.txt para área de trabalho; nome simples (oi.txt) também grava na Área de trabalho. NÃO use para ABRIR arquivo — use open_folder.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Caminho do arquivo. Use ~/Desktop/nome.txt para área de trabalho, ~/Documents/ etc. Nome simples (ex.: oi.txt) grava na Área de trabalho." },
                        "content": { "type": "string", "description": "Conteúdo a escrever" },
                        "overwrite": { "type": "boolean", "description": "Se true, sobrescreve arquivo existente (padrão false)" }
                    },
                    "required": ["path", "content"]
                }
            }
        }));
    }

    // ── Tier 2: ferramentas avançadas ────────────────────────────────────────

    if tools_config.manage_processes {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "manage_processes",
                "description": "Lista processos ativos ou encerra um processo pelo nome. action=list mostra os principais processos; action=kill encerra process_name.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["list", "kill"], "description": "list = listar; kill = encerrar" },
                        "process_name": { "type": "string", "description": "Nome do processo a encerrar (sem .exe). Obrigatório quando action=kill." }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    if tools_config.lock_screen {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "lock_screen",
                "description": "Bloqueia a tela / estação de trabalho do Windows imediatamente.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.open_folder {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "open_folder",
                "description": "Abre uma pasta no Explorador de Arquivos do Windows. Use esta tool quando o usuário pede para ABRIR uma pasta, diretório ou localização. NUNCA use take_screenshot ou write_file quando o pedido for abrir uma pasta. Se o usuário disser 'abra essa pasta' sem especificar o caminho, use o caminho da pasta atualmente visível ou ~/Downloads como fallback.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Caminho da pasta (use ~/ para home)" }
                    },
                    "required": ["path"]
                }
            }
        }));
    }

    if tools_config.set_wallpaper {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "set_wallpaper",
                "description": "Define uma imagem como papel de parede do Windows.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Caminho completo para o arquivo de imagem (.jpg, .png, .bmp)" }
                    },
                    "required": ["path"]
                }
            }
        }));
    }

    if tools_config.get_open_windows {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_open_windows",
                "description": "Lista todas as janelas abertas com título visível.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.read_selected_text {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_selected_text",
                "description": "Lê o texto selecionado na janela ativa simulando Ctrl+C. Use para processar texto selecionado pelo usuário.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.translate_selection {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "translate_selection",
                "description": "Traduz o texto selecionado ou copiado para o idioma pedido. Detecta o idioma de origem automaticamente. Padrão: português do Brasil (pt-BR). Exemplos de destino: ja (japonês), en (inglês), es (espanhol), fr (francês), de (alemão), zh (chinês), ko (coreano). Por padrão tenta a seleção ativa e, se vazia, usa o clipboard. A tradução é copiada de volta para o clipboard.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "source": {
                            "type": "string",
                            "enum": ["auto", "selection", "clipboard"],
                            "description": "Origem do texto: auto (padrão), selection ou clipboard"
                        },
                        "target_language": {
                            "type": "string",
                            "description": "Idioma de destino: pt-BR (padrão), ja, en, es, fr, de, it, zh, ko, ru, ar, etc. Ou nome falado: japonês, inglês, espanhol..."
                        }
                    }
                }
            }
        }));
    }

    if tools_config.paste_to_active_window {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "paste_to_active_window",
                "description": "Cola um texto na janela ativa via clipboard + Ctrl+V. Ideal para entregar respostas geradas diretamente ao editor em foco.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Texto a colar na janela ativa" }
                    },
                    "required": ["text"]
                }
            }
        }));
    }

    if tools_config.toggle_do_not_disturb {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "toggle_do_not_disturb",
                "description": "Ativa ou desativa o Modo Foco (não perturbe) do Windows, silenciando notificações Toast.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.session_notes {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "session_notes",
                "description": "Gerencia notas rápidas da sessão atual em memória. action=add adiciona nota; action=list lista todas; action=clear apaga todas.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["add", "list", "clear"] },
                        "text": { "type": "string", "description": "Texto da nota. Obrigatório quando action=add." }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    if tools_config.diff_clipboard {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "diff_clipboard",
                "description": "Compara as duas entradas mais recentes do histórico de clipboard e mostra as diferenças linha a linha.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.ocr_image {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "ocr_image",
                "description": "Tira um screenshot da tela e extrai todo o texto visível via OCR (visão de IA). Não descreve a imagem — apenas transcreve o texto.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.transcribe_audio_file {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "transcribe_audio_file",
                "description": "Transcreve um arquivo de áudio local usando o servidor Whisper. Suporta .wav, .mp3, .m4a, .ogg.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Caminho do arquivo de áudio (use ~/ para home)" }
                    },
                    "required": ["path"]
                }
            }
        }));
    }

    if tools_config.audio_device_switch {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "audio_device_switch",
                "description": "Lista dispositivos de áudio disponíveis ou troca o dispositivo de saída padrão pelo nome.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["list", "switch"], "description": "list = listar dispositivos; switch = trocar para device_name" },
                        "device_name": { "type": "string", "description": "Parte do nome do dispositivo alvo. Obrigatório com action=switch." }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    if tools_config.run_powershell_script {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "run_powershell_script",
                "description": "Executa um script PowerShell (.ps1) que esteja dentro das pastas permitidas do sandbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Caminho do arquivo .ps1 a executar (use ~/ para home)" }
                    },
                    "required": ["path"]
                }
            }
        }));
    }

    // ── Tier 3: Network, Calendar, Email, Keys, Watch, Snippets, Volume App ─

    if tools_config.get_network_info {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_network_info",
                "description": "Retorna informações de rede: IP local, gateway, DNS, adaptadores ativos.",
                "parameters": { "type": "object", "properties": {} }
            }
        }));
    }

    if tools_config.take_screenshot_region {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "take_screenshot_region",
                "description": "Captura uma região específica da tela e descreve o que está visível. Use coordenadas x, y, width, height para selecionar a área.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "integer", "description": "Coordenada X do canto superior esquerdo (pixels)" },
                        "y": { "type": "integer", "description": "Coordenada Y do canto superior esquerdo (pixels)" },
                        "width": { "type": "integer", "description": "Largura da região (pixels)" },
                        "height": { "type": "integer", "description": "Altura da região (pixels)" },
                        "question": { "type": "string", "description": "O que observar na região capturada" }
                    }
                }
            }
        }));
    }

    if tools_config.calendar_events {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "calendar_events",
                "description": "Obtém eventos do calendário do Outlook para os próximos dias. Requer Outlook instalado e configurado.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "days_ahead": { "type": "integer", "description": "Número de dias à frente a consultar (padrão 7)" }
                    }
                }
            }
        }));
    }

    if tools_config.send_email {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "send_email",
                "description": "Cria um rascunho de email no Outlook. O email é salvo como rascunho na pasta Rascunhos do Outlook — não é enviado automaticamente. Requer Outlook instalado.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "to": { "type": "string", "description": "Endereço de email do destinatário" },
                        "subject": { "type": "string", "description": "Assunto do email" },
                        "body": { "type": "string", "description": "Corpo/texto do email" }
                    },
                    "required": ["to"]
                }
            }
        }));
    }

    if tools_config.send_keys {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "send_keys",
                "description": "Envia teclas ou texto para a janela ativa. Aceita nomes de teclas especiais (enter, tab, shift, escape, backspace, delete, f1-f12, setas, etc.) ou texto livre (colado via clipboard).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "keys": { "type": "string", "description": "Tecla especial (ex.: enter, tab, escape, f5) ou texto a enviar" }
                    },
                    "required": ["keys"]
                }
            }
        }));
    }

    if tools_config.watch_file {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "watch_file",
                "description": "Vigia um arquivo ou diretório por mudanças durante um período de tempo. Retorna quando o arquivo é modificado ou quando o tempo expira.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Caminho do arquivo ou diretório a vigiar (use ~/ para home)" },
                        "duration_seconds": { "type": "integer", "description": "Tempo máximo de vigilância em segundos (padrão 60)" },
                        "on_change": { "type": "string", "description": "Mensagem opcional a retornar quando o arquivo mudar" }
                    },
                    "required": ["path"]
                }
            }
        }));
    }

    if tools_config.snippet_library {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "snippet_library",
                "description": "Gerencia snippets de texto salvos localmente. action=save salva um snippet (name + content); action=get recupera pelo nome; action=list lista todos; action=delete remove pelo nome.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["list", "save", "get", "delete"], "description": "list = listar todos; save = salvar/atualizar; get = recuperar; delete = remover" },
                        "name": { "type": "string", "description": "Nome do snippet (obrigatório para save/get/delete)" },
                        "content": { "type": "string", "description": "Conteúdo do snippet (obrigatório para save)" }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    if tools_config.set_audio_volume_app {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "set_audio_volume_app",
                "description": "Ajusta o volume de um aplicativo específico pelo nome do processo (ex.: chrome, spotify, discord). Volume de 0 a 100.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "app_name": { "type": "string", "description": "Nome do aplicativo (ex.: chrome, spotify, discord)" },
                        "volume": { "type": "integer", "description": "Volume de 0 a 100 (padrão 50)" }
                    },
                    "required": ["app_name", "volume"]
                }
            }
        }));
    }

    // ── Tier 4 ──────────────────────────────────────────────────────────
    if tools_config.disk_cleanup {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "disk_cleanup",
                "description": "Analisa o espaço em disco ou executa limpeza (arquivos temporários, lixeira, prefetch, cache DNS). Use action='analyze' para ver o estado do disco ou action='clean' para executar a limpeza.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["analyze", "clean"],
                            "description": "analyze = análise de espaço; clean = executar limpeza segura"
                        },
                        "drive": {
                            "type": "string",
                            "description": "Letra da unidade (ex.: C:). Padrão: C:"
                        }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    if tools_config.ui_automation {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "ui_automation",
                "description": "Automação de interface: clique do mouse em coordenadas, scroll, digitação de texto, selecionar tudo. Use action='click' para clique esquerdo (x,y obrigatórios), 'double_click' para duplo clique, 'right_click' para clique direito, 'scroll' para rolar com direction='up'/'down', 'type_text' para digitar texto, 'select_all' para Ctrl+A.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["click", "double_click", "right_click", "scroll", "type_text", "select_all"],
                            "description": "Tipo de ação de UI"
                        },
                        "x": { "type": "integer", "description": "Coordenada X (obrigatório para click/double_click/right_click)" },
                        "y": { "type": "integer", "description": "Coordenada Y (obrigatório para click/double_click/right_click)" },
                        "text": { "type": "string", "description": "Texto a digitar (obrigatório para type_text)" },
                        "direction": { "type": "string", "enum": ["up", "down"], "description": "Direção do scroll (padrão: down)" },
                        "amount": { "type": "integer", "description": "Quantidade de passos do scroll (padrão: 3)" }
                    },
                    "required": ["action"]
                }
            }
        }));
    }

    if tools_config.image_generation {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "image_generation",
                "description": "Gera uma imagem usando Stable Diffusion local (requer Automatic1111 WebUI rodando em http://127.0.0.1:7860). A imagem gerada é salva e aberta automaticamente.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "Descrição da imagem a gerar (em inglês para melhor qualidade)" },
                        "negative_prompt": { "type": "string", "description": "O que evitar na imagem (opcional)" },
                        "width": { "type": "integer", "description": "Largura em pixels (padrão: 512)" },
                        "height": { "type": "integer", "description": "Altura em pixels (padrão: 512)" },
                        "steps": { "type": "integer", "description": "Passos de inferência (padrão: 20)" },
                        "sd_url": { "type": "string", "description": "URL do servidor Stable Diffusion (padrão: http://127.0.0.1:7860)" }
                    },
                    "required": ["prompt"]
                }
            }
        }));
    }

    // Category filtering: se categories não está vazio, filtra mantendo always-on
    let always_on: &[&str] = &["get_current_time", "take_screenshot", "ocr_image"];
    let tools = if categories.is_empty() {
        tools
    } else {
        let allowed_names: std::collections::HashSet<&str> = categories
            .iter()
            .flat_map(|c| match c {
                ToolCategory::System => &[
                    "run_command", "get_active_window", "system_info",
                    "launch_desktop_app", "close_desktop_app", "list_running_apps",
                    "manage_processes", "lock_screen", "open_folder", "set_wallpaper",
                    "get_open_windows", "toggle_do_not_disturb",
                    "get_network_info", "take_screenshot_region",
                    "send_keys", "set_audio_volume_app",
                    "disk_cleanup",
                ][..],
                ToolCategory::Media => &[
                    "control_media_playback", "adjust_system_volume", "play_music_query",
                    "play_local_music_playlist", "play_full_local_music_library",
                    "native_music_library_shuffle_play", "audio_device_switch",
                    "set_audio_volume_app",
                ][..],
                ToolCategory::Files => &[
                    "search_files", "get_recent_files", "read_file", "write_file",
                    "transcribe_audio_file", "run_powershell_script",
                    "watch_file",
                ][..],
                ToolCategory::Web => &[
                    "open_url", "fetch_fx_quote", "fetch_weather", "web_fetch",
                    "calendar_events", "send_email",
                    "image_generation",
                ][..],
                ToolCategory::Knowledge => &[
                    "search_knowledge", "clipboard_history", "read_clipboard",
                    "write_clipboard", "diff_clipboard", "session_notes",
                    "snippet_library",
                ][..],
                ToolCategory::Automation => &[
                    "paste_to_active_window", "read_selected_text", "translate_selection",
                    "schedule_notification", "send_keys",
                    "ui_automation",
                ][..],
            })
            .copied()
            .collect();

        tools
            .into_iter()
            .filter(|t| {
                let name = t
                    .pointer("/function/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                always_on.contains(&name) || allowed_names.contains(name)
            })
            .collect()
    };

    tools
}

/// Result of a streaming chat — either the model streamed content (sentences sent
/// via channel) or it requested tool calls.
pub enum StreamResult {
    /// Model streamed a text response. Full text returned here.
    Content(String),
    /// Model requested tool calls. May include pre-tool-call narration text.
    /// Fields: (tool_calls, spoken_preamble, xml_parsed)
    ToolCalls(Vec<ToolCall>, String, bool),
}

/// Token chunk emitted during text-chat streaming.
#[derive(Clone, Serialize)]
pub struct ChatTokenChunk {
    pub token: String,
}

/// Result of a streaming text-chat round.
pub enum ChatStreamResult {
    Content(String),
    ToolCalls(Vec<ToolCall>, String, bool),
}

fn should_use_text_chat_thinking(
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
) -> bool {
    if config.enable_thinking {
        return true;
    }

    let Some(last_user) = messages.iter().rev().find(|msg| msg.role == "user") else {
        return false;
    };

    let text = last_user.content.to_lowercase();
    let word_count = text.split_whitespace().count();
    let simple_greeting = word_count <= 10
        && [
            "oi",
            "ola",
            "olá",
            "bom dia",
            "boa tarde",
            "boa noite",
            "como vai",
            "tudo bem",
        ]
        .iter()
        .any(|term| text.contains(term));

    if simple_greeting {
        return false;
    }

    let complexity_markers = [
        "analise",
        "analisa",
        "avaliar",
        "compare",
        "planeje",
        "estrategia",
        "arquitetura",
        "debug",
        "erro",
        "bug",
        "corrigir",
        "refator",
        "implemente",
        "codigo",
        "código",
        "script",
        "sql",
        "regex",
        "explique",
        "por que",
        "passo a passo",
        "melhorar",
        "otimizar",
        "performance",
        "seguranca",
        "segurança",
        "trade-off",
        "decisao",
        "decisão",
        "investigue",
        "olha essa tela",
        "olhada nessa tela",
        "design",
    ];

    word_count > 35
        || !tools.is_empty()
            && complexity_markers.iter().any(|term| text.contains(term))
        || complexity_markers.iter().filter(|term| text.contains(**term)).count() >= 2
}

async fn emit_voice_sentence_chunk(
    sentence_tx: &mpsc::Sender<String>,
    raw: &str,
    spoken_text: &mut String,
    sentence_seq: &mut u32,
    llm_start: std::time::Instant,
) {
    let Some(sentence) = prepare_voice_sentence_for_tts(raw) else {
        return;
    };
    let preview: String = sentence.chars().take(40).collect();
    eprintln!(
        "[perf] llm_sentence | seq={} | chars={} | elapsed_ms={} | text_preview=\"{}\"",
        *sentence_seq,
        sentence.chars().count(),
        llm_start.elapsed().as_millis(),
        preview
    );
    *sentence_seq += 1;
    spoken_text.push_str(&sentence);
    spoken_text.push(' ');
    let _ = sentence_tx.send(sentence).await;
}

/// Unified streaming chat using OpenAI-compatible API (llama.cpp).
pub async fn chat_streaming(
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    sentence_tx: &mpsc::Sender<String>,
    round: usize,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    const VOICE_BREVITY_SUFFIX: &str = "\n\nLIMITE RIGIDO (modo voz): no maximo 3 frases curtas por turno. \
Nao use listas, titulos nem paragrafos longos. Resuma em voz e sugira chat de texto quando necessario. \
Nunca soletre pontuacao (vírgula, ponto, etc.) — apenas fale naturalmente, sem markdown. \
Nao comece com 'A resposta é' ou 'Em resumo'. Para perguntas simples, responda direto sem ferramentas.";
    const VOICE_TOOL_ROUTING_SUFFIX: &str = "\n\nRoteamento: cotacao use fetch_fx_quote (pair USD-BRL, EUR-BRL, JPY-BRL, GBP-BRL). \
Temperatura use fetch_weather (day_offset 1 ou 2 para previsao, vazio=hoje; location vazio=IP). \
Busca web use web_fetch. search_knowledge so para docs RAG locais. \
Abrir apps: launch_desktop_app. Tocar musica: SEMPRE play_music_query (query+artist), nunca open_url com YouTube. \
Arquivos: se nao souber o caminho use search_files; para ler use read_file. Quando read_file retornar conteudo, \
leia ou resuma em voz como o usuario pediu — nao recuse por ser texto pessoal (e arquivo local dele).";

    let voice_system_prompt = if !config.system_prompt.trim().is_empty() {
        let base = format!("{}{}", config.system_prompt.trim(), VOICE_BREVITY_SUFFIX);
        if tools.is_empty() {
            base
        } else {
            format!("{}{}", base, VOICE_TOOL_ROUTING_SUFFIX)
        }
    } else if config.personality == "coder" {
        let base = "Voce e um assistente de programacao. Responda em portugues do Brasil. \
         Mantenha respostas curtas e conversacionais — 2-3 frases no maximo. \
         Sem markdown, sem blocos de codigo. Escreva exatamente como falaria em voz alta. \
         Voce tem acesso a ferramentas para interagir com o sistema do usuario.".to_string()
            + VOICE_BREVITY_SUFFIX;
        if tools.is_empty() {
            base
        } else {
            base + VOICE_TOOL_ROUTING_SUFFIX
        }
    } else if config.personality == "creative" {
        let base = "Voce e um assistente criativo. Responda em portugues do Brasil. \
         Mantenha respostas curtas e conversacionais — 2-3 frases no maximo. \
         Sem markdown, sem blocos de codigo. Escreva exatamente como falaria em voz alta. \
         Ofereca perspectivas variadas quando relevante.".to_string()
            + VOICE_BREVITY_SUFFIX;
        if tools.is_empty() {
            base
        } else {
            base + VOICE_TOOL_ROUTING_SUFFIX
        }
    } else {
        let base = format!("{}{}", config.system_prompt.trim(), VOICE_BREVITY_SUFFIX);
        if tools.is_empty() {
            base
        } else {
            format!("{}{}", base, VOICE_TOOL_ROUTING_SUFFIX)
        }
    };

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".to_string(),
        content: serde_json::Value::String(voice_system_prompt),
        tool_calls: None,
        tool_call_id: None,
    }];

    for msg in messages {
        let tool_calls: Option<Vec<OpenAIToolCall>> = msg.tool_calls.as_ref().map(|tcs| {
            tcs.iter().map(|tc| OpenAIToolCall {
                id: tc.id.clone(),
                call_type: tc.call_type.clone(),
                function: OpenAIToolFunction {
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                },
            }).collect()
        });

        openai_messages.push(OpenAIMessage {
            role: msg.role.clone(),
            content: serde_json::Value::String(msg.content.clone()),
            tool_calls,
            tool_call_id: msg.tool_call_id.clone(),
        });
    }

    // Voice mode always uses short replies — ignore config.response_style (that setting is for text chat).
    let has_tool_history = messages.iter().any(|m| m.role == "tool");
    let max_tokens = voice_max_tokens(!tools.is_empty(), has_tool_history);
    // Never enable "thinking" on the voice pipeline (saves tokens/latency; text chat uses enable_thinking).
    let thinking_budget_tokens = 0;
    eprintln!(
        "[voice] max_tokens={} | thinking=off | tools={} | tool_history={}",
        max_tokens,
        !tools.is_empty(),
        has_tool_history
    );

    let llm_model = config.effective_llm_model_voice().to_string();
    let llm_url = config.effective_llm_url_voice().to_string();

    let request = OpenAIChatRequest {
        model: llm_model,
        messages: openai_messages,
        stream: true,
        max_tokens,
        temperature: config.temperature,
        chat_template_kwargs: serde_json::json!({
            "enable_thinking": false
        }),
        thinking_budget_tokens,
        tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
    };

    let llm_start = std::time::Instant::now();
    let resp = client
        .post(format!("{}/v1/chat/completions", llm_url.trim_end_matches('/')))
        .json(&request)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Erro na API do LLM {}: {}", status, body).into());
    }

    let mut byte_stream = resp.bytes_stream();
    use tokio_stream::StreamExt;

    let mut full_response = String::new();
    let mut sentence_buffer = String::new();
    let mut spoken_text = String::new();
    let mut reasoning_chars: usize = 0;

    // Tool call accumulation
    let mut pending_tool_calls: Vec<ToolCallAccumulator> = Vec::new();
    let mut collected_tool_calls: Vec<ToolCall> = Vec::new();
    let has_tools = !tools.is_empty();

    // XML tool call detection (for models that emit XML instead of native tool calls)
    let mut xml_collecting = false;
    let mut xml_buffer = String::new();
    let xml_open_re = regex::Regex::new(r"<(?:\w+:)?tool_call>").unwrap();
    let xml_close_re = regex::Regex::new(r"</(?:\w+:)?tool_call>").unwrap();

    // Perf tracking
    let mut first_content_token = false;
    let mut sentence_seq: u32 = 0;

    // Buffer for SSE line parsing
    let mut line_buffer = Vec::new();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk?;
        line_buffer.extend_from_slice(&chunk);

        // Parse complete lines
        while let Some(newline_pos) = line_buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = line_buffer.drain(..=newline_pos).collect();
            let line_str = String::from_utf8_lossy(&line);
            let line_str = line_str.trim();

            if line_str.is_empty() {
                continue;
            }

            // SSE format: lines start with "data: "
            if let Some(data) = line_str.strip_prefix("data: ") {
                // Check for [DONE] marker
                if data == "[DONE]" {
                    break;
                }

                if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                    for choice in &chunk.choices {
                        // Check finish_reason
                        if let Some(reason) = &choice.finish_reason {
                            if reason == "tool_calls" || reason == "stop" {
                                // Flush pending tool calls
                                for acc in &pending_tool_calls {
                                    if let Ok(args) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&acc.arguments) {
                                        collected_tool_calls.push(ToolCall {
                                            id: acc.id.clone(),
                                            name: acc.name.clone(),
                                            arguments: args,
                                        });
                                    }
                                }
                                pending_tool_calls.clear();
                                continue;
                            }
                        }

                        if let Some(delta) = &choice.delta {
                            if let Some(reasoning_content) = &delta.reasoning_content {
                                reasoning_chars += reasoning_content.chars().count();
                            }

                            // Handle tool calls
                            if let Some(tool_call_deltas) = &delta.tool_calls {
                                for tc_delta in tool_call_deltas {
                                    // Ensure accumulator exists for this index
                                    while pending_tool_calls.len() <= tc_delta.index {
                                        pending_tool_calls.push(ToolCallAccumulator {
                                            id: String::new(),
                                            name: String::new(),
                                            arguments: String::new(),
                                        });
                                    }

                                    let acc = &mut pending_tool_calls[tc_delta.index];
                                    if let Some(ref id) = tc_delta.id {
                                        acc.id = id.clone();
                                    }
                                    if let Some(ref func) = tc_delta.function {
                                        if let Some(ref name) = func.name {
                                            acc.name = name.clone();
                                        }
                                        if let Some(ref args) = func.arguments {
                                            acc.arguments.push_str(args);
                                        }
                                    }
                                }
                            }

                            // Handle content
                            if let Some(content) = &delta.content {
                                if !content.is_empty() {
                                    if !first_content_token {
                                        first_content_token = true;
                                        eprintln!(
                                            "[perf] llm_ttft | round={} | ttft_ms={}",
                                            round,
                                            llm_start.elapsed().as_millis()
                                        );
                                    }
                                    full_response.push_str(content);

                                    if xml_collecting {
                                        xml_buffer.push_str(content);
                                        if xml_close_re.is_match(&xml_buffer) {
                                            let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                            if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                                collected_tool_calls.extend(parsed);
                                            }
                                            xml_buffer.clear();
                                            xml_collecting = false;
                                        }
                                    } else {
                                        sentence_buffer.push_str(content);

                                        if has_tools && xml_open_re.find(&sentence_buffer).is_some() {
                                            let m = xml_open_re.find(&sentence_buffer).unwrap();
                                            let before = &sentence_buffer[..m.start()];
                                            emit_voice_sentence_chunk(
                                                sentence_tx,
                                                before,
                                                &mut spoken_text,
                                                &mut sentence_seq,
                                                llm_start,
                                            )
                                            .await;
                                            let after_tag = &sentence_buffer[m.end()..];
                                            xml_buffer = after_tag.to_string();
                                            sentence_buffer.clear();
                                            xml_collecting = true;

                                            if xml_close_re.is_match(&xml_buffer) {
                                                let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                                if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                                    collected_tool_calls.extend(parsed);
                                                }
                                                xml_buffer.clear();
                                                xml_collecting = false;
                                            }
                                        } else {
                                            while let Some(split_pos) = find_tts_chunk_end(&sentence_buffer) {
                                                let sentence: String =
                                                    sentence_buffer.drain(..=split_pos).collect();
                                                emit_voice_sentence_chunk(
                                                    sentence_tx,
                                                    sentence.trim(),
                                                    &mut spoken_text,
                                                    &mut sentence_seq,
                                                    llm_start,
                                                )
                                                .await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Flush remaining sentence buffer
    if !xml_collecting {
        emit_voice_sentence_chunk(
            sentence_tx,
            sentence_buffer.trim(),
            &mut spoken_text,
            &mut sentence_seq,
            llm_start,
        )
        .await;
    }

    // Flush any remaining pending tool calls
    for acc in &pending_tool_calls {
        if !acc.name.is_empty() {
            if let Ok(args) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&acc.arguments) {
                collected_tool_calls.push(ToolCall {
                    id: acc.id.clone(),
                    name: acc.name.clone(),
                    arguments: args,
                });
            }
        }
    }

    if !collected_tool_calls.is_empty() {
        let _ = sentence_seq;
        return Ok(StreamResult::ToolCalls(collected_tool_calls, spoken_text.trim().to_string(), false));
    }

    if full_response.trim().is_empty() && reasoning_chars > 0 {
        eprintln!(
            "[LLM] Modelo retornou {} caracteres de reasoning_content, mas nenhum conteudo visivel.",
            reasoning_chars
        );
    }

    // Last-resort XML fallback
    if has_tools && !full_response.is_empty() {
        if let Some(parsed) = parse_xml_tool_calls(&full_response) {
            if !parsed.is_empty() {
                let _ = sentence_seq;
                return Ok(StreamResult::ToolCalls(parsed, spoken_text.trim().to_string(), true));
            }
        }
    }

    let _ = sentence_seq;
    Ok(StreamResult::Content(strip_paralinguistic_brackets(
        full_response.trim(),
    )))
}

/// Streaming chat for text mode: full responses, markdown-friendly output, no TTS splitting.
pub async fn chat_streaming_text(
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    token_tx: &mpsc::Sender<ChatTokenChunk>,
) -> Result<ChatStreamResult, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let text_system_prompt = if !config.system_prompt_text.trim().is_empty() {
        config.system_prompt_text.clone()
    } else if config.personality == "coder" {
        "Voce e um assistente de programacao. Responda em portugues do Brasil. \
         Use blocos de codigo com syntax highlighting quando relevante. \
         Seja detalhado e tecnico. Explique o raciocinio por tras das solucoes. \
         Use markdown para estruturar a resposta."
            .to_string()
    } else if config.personality == "creative" {
        "Voce e um assistente criativo. Responda em portugues do Brasil. \
         Pense fora da caixa, ofereca multiplas perspectivas. \
         Respostas podem ser mais longas e elaboradas. \
         Use markdown para estruturar a resposta."
            .to_string()
    } else {
        "Voce e um assistente de IA rodando no desktop do usuario. \
         Responda em portugues do Brasil. \
         Seja detalhado, use markdown para estruturar a resposta. \
         Use blocos de codigo com syntax highlighting quando relevante. \
         O historico completo da conversa vem nas mensagens anteriores; use-o para entender \
         referencias como \"isso\", \"isso ai\", \"o que voce disse\", etc., sem dizer que a conversa comecou agora. \
         Voce tem acesso a ferramentas para interagir com o sistema do usuario \
         (captura de tela, comandos, busca na web, etc.)."
            .to_string()
    };

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".to_string(),
        content: serde_json::Value::String(text_system_prompt),
        tool_calls: None,
        tool_call_id: None,
    }];

    for msg in messages {
        let tool_calls: Option<Vec<OpenAIToolCall>> = msg.tool_calls.as_ref().map(|tcs| {
            tcs.iter()
                .map(|tc| OpenAIToolCall {
                    id: tc.id.clone(),
                    call_type: tc.call_type.clone(),
                    function: OpenAIToolFunction {
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    },
                })
                .collect()
        });

        openai_messages.push(OpenAIMessage {
            role: msg.role.clone(),
            content: serde_json::Value::String(msg.content.clone()),
            tool_calls,
            tool_call_id: msg.tool_call_id.clone(),
        });
    }

    let has_tool_history = messages.iter().any(|m| m.role == "tool");
    let max_tokens = match config.response_style.as_str() {
        "concise" => 1024,
        "detailed" => 4096,
        _ => {
            if has_tool_history {
                3072
            } else {
                2048
            }
        }
    };
    let use_thinking = should_use_text_chat_thinking(config, messages, tools);

    let llm_model = config.effective_llm_model_text().to_string();
    let llm_url = config.effective_llm_url_text().to_string();

    let request = OpenAIChatRequest {
        model: llm_model,
        messages: openai_messages,
        stream: true,
        max_tokens,
        temperature: config.temperature,
        chat_template_kwargs: serde_json::json!({
            "enable_thinking": use_thinking
        }),
        thinking_budget_tokens: if use_thinking { 2048 } else { 0 },
        tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
    };

    let llm_start = std::time::Instant::now();
    let resp = client
        .post(format!("{}/v1/chat/completions", llm_url.trim_end_matches('/')))
        .json(&request)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Erro na API do LLM {}: {}", status, body).into());
    }

    let mut byte_stream = resp.bytes_stream();
    use tokio_stream::StreamExt;

    let mut full_response = String::new();
    let mut pending_tool_calls: Vec<ToolCallAccumulator> = Vec::new();
    let mut collected_tool_calls: Vec<ToolCall> = Vec::new();
    let has_tools = !tools.is_empty();

    let xml_open_re = regex::Regex::new(r"<(?:\w+:)?tool_call>").unwrap();
    let xml_close_re = regex::Regex::new(r"</(?:\w+:)?tool_call>").unwrap();
    let mut xml_collecting = false;
    let mut xml_buffer = String::new();
    let mut xml_check_buffer = String::new();
    let acc_chars_for_xml: usize = 200;

    let mut first_content_token = false;
    let mut line_buffer = Vec::new();
    let mut stream_done = false;

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk?;
        line_buffer.extend_from_slice(&chunk);

        while let Some(newline_pos) = line_buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = line_buffer.drain(..=newline_pos).collect();
            let line_str = String::from_utf8_lossy(&line);
            let line_str = line_str.trim();

            if line_str.is_empty() {
                continue;
            }

            if let Some(data) = line_str.strip_prefix("data: ") {
                if data == "[DONE]" {
                    stream_done = true;
                    break;
                }

                if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                    for choice in &chunk.choices {
                        if let Some(reason) = &choice.finish_reason {
                            if reason == "tool_calls" || reason == "stop" {
                                for acc in &pending_tool_calls {
                                    if let Ok(args) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&acc.arguments) {
                                        collected_tool_calls.push(ToolCall {
                                            id: acc.id.clone(),
                                            name: acc.name.clone(),
                                            arguments: args,
                                        });
                                    }
                                }
                                pending_tool_calls.clear();
                                continue;
                            }
                        }

                        if let Some(delta) = &choice.delta {
                            if let Some(tool_call_deltas) = &delta.tool_calls {
                                for tc_delta in tool_call_deltas {
                                    while pending_tool_calls.len() <= tc_delta.index {
                                        pending_tool_calls.push(ToolCallAccumulator {
                                            id: String::new(),
                                            name: String::new(),
                                            arguments: String::new(),
                                        });
                                    }

                                    let acc = &mut pending_tool_calls[tc_delta.index];
                                    if let Some(ref id) = tc_delta.id {
                                        acc.id = id.clone();
                                    }
                                    if let Some(ref func) = tc_delta.function {
                                        if let Some(ref name) = func.name {
                                            acc.name = name.clone();
                                        }
                                        if let Some(ref args) = func.arguments {
                                            acc.arguments.push_str(args);
                                        }
                                    }
                                }
                            }

                            if let Some(content) = &delta.content {
                                if content.is_empty() {
                                    continue;
                                }

                                if !first_content_token {
                                    first_content_token = true;
                                    eprintln!(
                                        "[chat-text] ttft_ms={}",
                                        llm_start.elapsed().as_millis()
                                    );
                                }

                                full_response.push_str(content);

                                if !has_tools {
                                    let _ = token_tx
                                        .send(ChatTokenChunk {
                                            token: content.clone(),
                                        })
                                        .await;
                                    continue;
                                }

                                if xml_collecting {
                                    xml_buffer.push_str(content);
                                    if xml_close_re.is_match(&xml_buffer) {
                                        let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                        if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                            collected_tool_calls.extend(parsed);
                                        }
                                        xml_buffer.clear();
                                        xml_collecting = false;
                                    }
                                } else {
                                    xml_check_buffer.push_str(content);
                                    let should_check = xml_check_buffer.len() >= acc_chars_for_xml
                                        || !content.contains(|c: char| c.is_alphanumeric());

                                    if xml_open_re.find(&xml_check_buffer).is_some() && should_check {
                                        let m = xml_open_re.find(&xml_check_buffer).unwrap();
                                        let before = xml_check_buffer[..m.start()].to_string();
                                        let after_tag = xml_check_buffer[m.end()..].to_string();
                                        xml_check_buffer.clear();

                                        if !before.is_empty() {
                                            let _ = token_tx
                                                .send(ChatTokenChunk { token: before })
                                                .await;
                                        }

                                        xml_buffer = after_tag;
                                        xml_collecting = true;

                                        if xml_close_re.is_match(&xml_buffer) {
                                            let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                            if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                                collected_tool_calls.extend(parsed);
                                            }
                                            xml_buffer.clear();
                                            xml_collecting = false;
                                        }
                                    } else if !xml_check_buffer.is_empty() {
                                        let _ = token_tx
                                            .send(ChatTokenChunk {
                                                token: xml_check_buffer.clone(),
                                            })
                                            .await;
                                        xml_check_buffer.clear();
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if stream_done {
                break;
            }
        }

        if stream_done {
            break;
        }
    }

    if !xml_check_buffer.is_empty() && !xml_collecting {
        let _ = token_tx
            .send(ChatTokenChunk {
                token: xml_check_buffer,
            })
            .await;
    }

    for acc in &pending_tool_calls {
        if !acc.name.is_empty() {
            if let Ok(args) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&acc.arguments) {
                collected_tool_calls.push(ToolCall {
                    id: acc.id.clone(),
                    name: acc.name.clone(),
                    arguments: args,
                });
            }
        }
    }

    if !collected_tool_calls.is_empty() {
        return Ok(ChatStreamResult::ToolCalls(
            collected_tool_calls,
            String::new(),
            false,
        ));
    }

    if has_tools && !full_response.is_empty() {
        if let Some(parsed) = parse_xml_tool_calls(&full_response) {
            if !parsed.is_empty() {
                return Ok(ChatStreamResult::ToolCalls(
                    parsed,
                    String::new(),
                    true,
                ));
            }
        }
    }

    Ok(ChatStreamResult::Content(full_response.trim().to_string()))
}

struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

/// Parse XML-style tool calls that some models emit as text.
fn parse_xml_tool_calls(content: &str) -> Option<Vec<ToolCall>> {
    let re_block = regex::Regex::new(r"(?s)<(?:\w+:)?tool_call>(.*?)</(?:\w+:)?tool_call>").ok()?;
    let re_invoke = regex::Regex::new(r#"(?s)<invoke\s+name="([^"]+)">(.*?)</invoke>"#).ok()?;
    let re_param = regex::Regex::new(r#"(?s)<parameter\s+name="([^"]+)">(.*?)</parameter>"#).ok()?;

    let mut calls = Vec::new();
    let mut call_counter: u32 = 0;

    for block in re_block.captures_iter(content) {
        let inner = &block[1];
        for invoke_match in re_invoke.captures_iter(inner) {
            let func_name = invoke_match[1].to_string();
            let params_str = &invoke_match[2];

            let mut arguments = HashMap::new();
            for param in re_param.captures_iter(params_str) {
                let key = param[1].trim().to_string();
                let value = param[2].trim().to_string();
                let json_val = serde_json::from_str::<serde_json::Value>(&value)
                    .unwrap_or(serde_json::Value::String(value));
                arguments.insert(key, json_val);
            }

            call_counter += 1;
            calls.push(ToolCall {
                id: format!("xml_call_{}", call_counter),
                name: func_name,
                arguments,
            });
        }
    }

    if calls.is_empty() { None } else { Some(calls) }
}

/// True when `chars[i]` sits between two ASCII digits (e.g. "1,5" or "10,000").
fn is_number_context(chars: &[(usize, char)], i: usize) -> bool {
    i > 0
        && i + 1 < chars.len()
        && chars[i - 1].1.is_ascii_digit()
        && chars[i + 1].1.is_ascii_digit()
}

/// Whether the voice pipeline should expose tool definitions to the LLM for this utterance.
/// Informational questions (explain, compare, define) should answer directly — not call web_fetch.
pub fn should_attach_voice_tools(transcript: &str) -> bool {
    if std::env::var("DEXTER_VOICE_TOOLS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on"))
        .unwrap_or(false)
    {
        return true;
    }
    if std::env::var("DEXTER_VOICE_TOOLS")
        .map(|v| v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        .unwrap_or(false)
    {
        return false;
    }

    let t = transcript.to_lowercase();

    const EXPLICIT_TOOL: &[&str] = &[
        // ── Visão ──
        "screenshot",
        "captura de tela",
        "captura da tela",
        "print da tela",
        "olha minha tela",
        "veja minha tela",
        // ── Tempo ──
        "que horas",
        "horas são",
        "que dia é",
        // ── Apps ──
        "abre o ",
        "abrir o ",
        "abra o ",
        "abre ",
        "abrir ",
        "abra ",
        "abre o chrome",
        "abre o spotify",
        "bloco de notas",
        "notepad",
        // ── Clipboard ──
        "clipboard",
        "área de transferência",
        "leia o que",
        "lê o que",
        "ler o que",
        "o que eu copiei",
        "o que copiei",
        "o que está na",
        "o que esta na",
        "o que tem na",
        "o que tem no clipboard",
        "leia o clipboard",
        "lê o clipboard",
        "copiei",
        "copia ",
        "copia isto",
        "copia isso",
        "copia o texto",
        "joga pro clipboard",
        "joga para o clipboard",
        "cola ",
        "colar ",
        "cole ",
        "cola na janela",
        "colar na janela",
        "histórico do clipboard",
        "histórico da área",
        "últimas cópias",
        // ── Música ──
        "toca ",
        "toque ",
        "música",
        "musica",
        "youtube",
        "no youtube",
        "volume ",
        "pausa a música",
        "pausa a musica",
        // ── Comandos / Scripts ──
        "executa ",
        "rode o comando",
        "script",
        "powershell",
        // ── Web ──
        "busca na web",
        "pesquisa na internet",
        "pesquisa ",
        "pesquise",
        "procura na web",
        "procura o valor",
        "cotação",
        "cotacao",
        "iene",
        "yen",
        "euro",
        "temperatura",
        "clima",
        "previsao",
        "previsão",
        "previsao do tempo",
        "vai chover",
        "como esta o tempo",
        // ── Arquivos & Pastas ──
        "arquivo",
        "pasta",
        "arquivos recentes",
        "abertos recentemente",
        "abri recentemente",
        "ache ",
        "acha ",
        "ache a pasta",
        "ache o arquivo",
        "acha a pasta",
        "acha o arquivo",
        "procura o arquivo",
        "procura a pasta",
        "encontra o arquivo",
        "encontra a pasta",
        "busca o arquivo",
        "busca a pasta",
        "cade o arquivo",
        "cadê o arquivo",
        "cade a pasta",
        "cadê a pasta",
        "abre a pasta",
        "abrir a pasta",
        "abrir o diretório",
        "abre o diretório",
        "cria arquivo",
        "criar arquivo",
        "cria um arquivo",
        "criar um arquivo",
        "salva o arquivo",
        "salvar o arquivo",
        "lê o arquivo",
        "ler o arquivo",
        "leia o arquivo",
        "lê a pasta",
        // ── Notas / Sessão ──
        "anota",
        "anotar",
        "nota",
        "notas",
        "minhas notas",
        "lista as notas",
        "listar as notas",
        "limpa as notas",
        "limpar as notas",
        // ── Teclas / Digitação ──
        "pressiona",
        "pressione",
        "tecla",
        "digita ",
        "digite ",
        "aperta ",
        "aperte ",
        "ctrl",
        "shift",
        "f5",
        "enter",
        // ── Email / Calendário ──
        "rascunho",
        "email",
        "e-mail",
        "agenda",
        "eventos",
        "compromissos",
        // ── Sistema ──
        "processo",
        "processos",
        "fecha o processo",
        "bloqueia a tela",
        "trava o pc",
        "travar o pc",
        "limpa o disco",
        "limpar o disco",
        "espaço em disco",
        "espaço livre",
        "lixeira",
        "esvazia a lixeira",
        "modo foco",
        "não perturbe",
        "nao perturbe",
        // ── Notificações ──
        "avisa ",
        "avise ",
        "me avisa",
        "me avise",
        "daqui ",
        "segundos",
        "minutos",
        "lembra ",
        "lembre ",
        "me lembra",
        "me lembre",
        "lembrete",
        "alarme",
        "com alarme",
        "com som",
        "silencioso",
        "sem som",
        "timer",
        "da manha",
        "da manhã",
        "da tarde",
        "da noite",
        // ── Seleção ──
        "selecionado",
        "selecionada",
        "seleção",
        "selecao",
        "texto selecionado",
        "lê a seleção",
        "leia a seleção",
        "traduz",
        "traduza",
        "tradução",
        "traducao",
        "traduzir",
        "translate",
        "traduz para português",
        "traduz para portugues",
        // ── Snippets ──
        "snippet",
        "salva snippet",
        "atalho",
        // ── Áudio ──
        "dispositivo de áudio",
        "dispositivo de audio",
        "fone de ouvido",
        "fones de ouvido",
        "alto-falante",
        // ── Rede ──
        "ip",
        "rede",
        "wifi",
        "wi-fi",
        "gateway",
        "dns",
        // ── Automação ──
        "clique",
        "click",
        "scroll",
        "mouse",
        "coordenada",
        // ── Imagem ──
        "gera uma imagem",
        "gerar uma imagem",
        "cria uma imagem",
        "criar uma imagem",
    ];

    if EXPLICIT_TOOL.iter().any(|k| t.contains(k)) {
        return true;
    }

    const INFO_ONLY: &[&str] = &[
        "explique",
        "explica ",
        "me explique",
        "qual a diferença",
        "quais as diferenças",
        "diferença entre",
        "o que é",
        "o que e ",
        "como funciona",
        "por que ",
        "porque ",
        "em três frases",
        "em 3 frases",
        "três frases curtas",
        "3 frases curtas",
        "resuma",
        "defina ",
        "me diga o que",
    ];

    if INFO_ONLY.iter().any(|k| t.contains(k)) {
        return false;
    }

    false
}

/// Token budget for voice-only streaming (`chat_streaming`). Not tied to `config.response_style`.
pub fn voice_max_tokens(tools_enabled: bool, has_tool_history: bool) -> u32 {
    if tools_enabled {
        if has_tool_history {
            180
        } else {
            220
        }
    } else if has_tool_history {
        160
    } else {
        120
    }
}

/// Max characters per TTS HTTP request (`DEXTER_TTS_MAX_CHUNK_CHARS`, default 180).
pub fn tts_max_chunk_chars() -> usize {
    tts_xtts_safe_chunk_chars()
}

/// Split at commas/colons mid-stream (`DEXTER_TTS_SPLIT_COMMA=0` → sentence boundaries only).
pub fn tts_split_on_commas() -> bool {
    match std::env::var("DEXTER_TTS_SPLIT_COMMA")
        .ok()
        .map(|v| v.trim().to_lowercase())
        .as_deref()
    {
        Some("0") | Some("false") | Some("off") | Some("no") => false,
        Some("1") | Some("true") | Some("on") | Some("yes") => true,
        _ => true,
    }
}

/// Find a good TTS chunk boundary in the buffer.
/// Returns the byte index of the first character AFTER the chunk boundary.
pub fn find_tts_chunk_end(text: &str) -> Option<usize> {
    const MIN_SOFT_CHUNK_CHARS: usize = 20;
    let max_chunk_chars = tts_max_chunk_chars();
    let split_commas = tts_split_on_commas();

    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return None;
    }

    if split_commas {
        // Voice-first: split at comma/semicolon/colon as soon as there's a
        // meaningful word before it (≥6 chars).  Avoids splitting "1,5", "10,000".
        for i in 0..chars.len() {
            let (_byte_idx, ch) = chars[i];
            if (ch == ',' || ch == ';' || ch == ':') && i >= 6 {
                let next_idx = i + 1;
                if next_idx < chars.len() {
                    let (_, next_ch) = chars[next_idx];
                    if next_ch.is_whitespace() && !is_number_context(&chars, i) {
                        return Some(chars[next_idx].0);
                    }
                }
            }
        }
    }

    // Sentence-ending punctuation — always split immediately.
    for i in 0..chars.len() {
        let (_byte_idx, ch) = chars[i];
        if ch == '.' || ch == '!' || ch == '?' {
            let next_idx = i + 1;
            if next_idx < chars.len() {
                let (_, next_ch) = chars[next_idx];
                if next_ch.is_whitespace() {
                    return Some(chars[next_idx].0);
                }
            }
        }
    }

    // Don't force-split tiny buffers (wait for more content).
    if chars.len() < max_chunk_chars {
        return None;
    }

    let soft_limit = chars.len().min(max_chunk_chars);
    if split_commas {
        // Fallback comma/semicolon/colon between MIN_SOFT_CHUNK_CHARS..soft_limit.
        for i in (MIN_SOFT_CHUNK_CHARS..soft_limit).rev() {
            let (_byte_idx, ch) = chars[i];
            if ch == ',' || ch == ';' || ch == ':' {
                let next_idx = i + 1;
                if next_idx < chars.len() {
                    let (_, next_ch) = chars[next_idx];
                    if next_ch.is_whitespace() {
                        return Some(chars[next_idx].0);
                    }
                }
            }
        }
    }

    // Last resort: split at whitespace.
    for i in (MIN_SOFT_CHUNK_CHARS..soft_limit).rev() {
        let (byte_idx, ch) = chars[i];
        if ch.is_whitespace() {
            return Some(byte_idx);
        }
    }

    chars
        .get(max_chunk_chars.saturating_sub(1))
        .map(|(byte_idx, _)| *byte_idx)
}

// ── Lembrete agendado: falar no horário do disparo ──

#[derive(Serialize, Clone)]
struct ReminderProcessingBubble {
    stage: String,
    text: String,
}

#[derive(Serialize, Clone)]
struct ReminderAudioChunk {
    index: u32,
    audio: String,
}

fn reminder_topic_for_speech(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("lembrete") {
        return "seu lembrete".to_string();
    }
    let mut chars = t.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().chain(chars).collect(),
    }
}

fn polish_reminder_topic_for_speech(topic: &str) -> String {
    let t = topic.trim();
    let t = if let Some(rest) = t.strip_prefix("eu tenho que ") {
        format!("que você precisa {rest}")
    } else if let Some(rest) = t.strip_prefix("eu preciso ") {
        format!("que você precisa {rest}")
    } else {
        t.to_string()
    };
    t.replace(" minha ", " sua ")
        .replace(" meu ", " seu ")
        .replace(" eu amo ", " você ama ")
}

/// Texto natural para TTS no disparo do lembrete.
pub fn format_reminder_fire_speech(reminder_message: &str) -> String {
    let topic = polish_reminder_topic_for_speech(&reminder_topic_for_speech(reminder_message));
    if topic.starts_with("que você") {
        format!("Senhor, estou passando para lembrá-lo {topic}.")
    } else {
        format!("Senhor, estou passando para lembrá-lo de {topic}.")
    }
}

/// XTTS ao disparar um lembrete (após o som do alarme/toast).
pub async fn speak_reminder_fire(
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    reminder_message: &str,
) {
    let speech = format_reminder_fire_speech(reminder_message);
    let cleaned = strip_paralinguistic_brackets(&speech);
    if cleaned.trim().is_empty() {
        return;
    }

    let _ = app.emit(
        "processing",
        ReminderProcessingBubble {
            stage: "speaking".to_string(),
            text: cleaned.clone(),
        },
    );

    eprintln!(
        "[toast] reminder_tts | text_preview=\"{}\"",
        cleaned.chars().take(60).collect::<String>()
    );

    let mut chunk_idx: u32 = 0;
    let mut remaining: &str = &cleaned;
    while !remaining.is_empty() {
        let chunk = if let Some(pos) = find_tts_chunk_end(remaining) {
            let c = remaining[..pos].trim().to_string();
            remaining = &remaining[pos..];
            c
        } else {
            let c = remaining.trim().to_string();
            remaining = "";
            c
        };
        if chunk.is_empty() {
            continue;
        }
        match synthesize(config, &chunk, chunk_idx).await {
            Ok(audio) => {
                let _ = app.emit(
                    "play_audio_chunk",
                    ReminderAudioChunk {
                        index: chunk_idx,
                        audio,
                    },
                );
            }
            Err(e) => eprintln!("[toast] reminder_tts failed | chunk={chunk_idx} | err={e}"),
        }
        chunk_idx += 1;
    }
    let _ = app.emit("play_audio_done", chunk_idx);
}

#[cfg(test)]
mod read_aloud_tests {
    use super::{
        markdown_file_to_plain_speech, prepare_read_aloud_for_tts, strip_yaml_frontmatter,
        READ_ALOUD_MAX_CHARS,
    };

    #[test]
    fn strips_yaml_frontmatter_and_tags() {
        let md = "---\ntitle: 1 Néfi 1\ntags:\n - livro-de-mormon\n---\n\nCorpo do texto aqui.";
        let plain = markdown_file_to_plain_speech(md);
        assert!(!plain.to_lowercase().contains("tags"));
        assert!(!plain.to_lowercase().contains("livro-de-mormon"));
        assert!(plain.contains("Corpo do texto"));
    }

    #[test]
    fn obsidian_lds_scripture_markdown() {
        let md = r#"---
title: 1 Néfi 1
tags:
  - livro-de-mormon
---

# 1 Néfi 1

<< Início | [[1 Néfi]] | [[1 Néfi 2]] >>

---

**1** Eu, [[Néfi, Filho de Leí|Néfi]][^1], tendo nascido de **bons**[^2] [[Pais|pais]][^3], recebi, portanto, alguma [[Ensinar, Mestre|instrução]][^4] em todo o conhecimento de meu pai. ^v1

**2** Sim, faço um registro na **língua**[^8] de meu pai. ^v2

**Notas de Rodapé:**

[^1]: GEE [[Néfi, Filho de Leí]]
"#;
        let plain = markdown_file_to_plain_speech(md);
        let lower = plain.to_lowercase();
        assert!(!plain.contains("<<"), "nav markers: {plain}");
        assert!(!lower.contains("início"));
        assert!(!lower.contains("gee"));
        assert!(!plain.contains("^v"));
        assert!(!plain.contains("[^"));
        assert!(plain.contains("Eu,"));
        assert!(plain.contains("Néfi"));
        assert!(plain.contains("instrução"));
        assert!(plain.contains("pais"));
        assert!(plain.contains("Sim, faço um registro"));
    }

    #[test]
    fn frontmatter_strip_only() {
        let body = strip_yaml_frontmatter("---\na: 1\n---\n\nOlá.");
        assert!(body.starts_with("Olá"));
    }

    #[test]
    fn long_text_gets_truncation_suffix() {
        let huge = "Palavra. ".repeat(READ_ALOUD_MAX_CHARS / 4);
        let out = prepare_read_aloud_for_tts(&huge, Some("x.md"));
        assert!(out.contains("Fim desta parte"));
        assert!(out.chars().count() <= READ_ALOUD_MAX_CHARS + 120);
    }

    #[test]
    fn split_next_respects_xtts_limit() {
        let text = "a ".repeat(500);
        let (chunk, rest) = super::split_next_tts_chunk(&text, 180).expect("chunk");
        assert!(chunk.chars().count() <= 180);
        assert!(!rest.is_empty());
    }

    #[test]
    fn normalize_periods_avoids_spoken_dot_for_xtts() {
        let out = super::normalize_periods_for_xtts("Honda inclina-se. Aproximando-se dele.");
        assert!(!out.contains('.'), "periods must become pauses, got: {out}");
        assert!(out.contains(','));
    }

    #[test]
    fn read_aloud_synthesis_strips_trailing_ponto_word() {
        let out = super::normalize_periods_for_xtts("Uma frase qualquer. ponto");
        assert!(!out.to_lowercase().contains("ponto"));
    }

    #[test]
    fn foreign_translation_tts_is_short_confirmation() {
        let target = crate::system_tools::TranslateTarget {
            code: "ja".into(),
            label: "japonês".into(),
        };
        let tts = super::spoken_translate_tts("Tradução (japonês):\nホンダは", &target);
        assert!(tts.contains("transferência"));
        assert!(!tts.contains('ホ'));
    }
}

// ── Chatterbox TTS ──

#[derive(Serialize)]
struct ChatterboxRequest {
    input: String,
    voice: String,
    model: String,
    response_format: String,
}

pub async fn synthesize(
    config: &VoiceConfig,
    text: &str,
    seq: u32,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let text = prepare_voice_sentence_for_tts(text)
        .ok_or_else(|| "texto vazio ou filler apos sanitizar para TTS".to_string())?;

    let tts_mode = std::env::var("DEXTER_TTS_MODE").unwrap_or_default();
    if tts_mode.eq_ignore_ascii_case("windows") {
        eprintln!(
            "[perf] tts_start | seq={} | chars={} | backend=windows_sapi | text_preview=\"{}\"",
            seq,
            text.chars().count(),
            text.chars().take(40).collect::<String>()
        );
        return synthesize_windows_sapi(&text, config.tts_volume).await;
    }

    synthesize_xtts_only(config, &text, seq, true).await
}

async fn synthesize_xtts_only(
    config: &VoiceConfig,
    text: &str,
    seq: u32,
    allow_windows_fallback: bool,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let preview: String = text.chars().take(40).collect();
    eprintln!(
        "[perf] tts_start | seq={} | chars={} | backend=xtts | text_preview=\"{}\"",
        seq,
        text.chars().count(),
        preview
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let request = ChatterboxRequest {
        input: text.to_string(),
        voice: config.chatterbox_voice.clone(),
        model: "xtts".to_string(),
        response_format: "wav".to_string(),
    };

    let http_start = std::time::Instant::now();
    let chatterbox_result = client
        .post(format!("{}/v1/audio/speech", config.chatterbox_url))
        .json(&request)
        .send()
        .await;

    let resp = match chatterbox_result {
        Ok(resp) => {
            eprintln!(
                "[perf] tts_http_ok | seq={} | http_ms={}",
                seq,
                http_start.elapsed().as_millis()
            );
            resp
        }
        Err(err) => {
            if allow_windows_fallback {
                eprintln!("[TTS] TTS indisponivel, usando Windows TTS: {}", err);
                return synthesize_windows_sapi(text, config.tts_volume).await;
            }
            return Err(err.into());
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        if allow_windows_fallback {
            eprintln!("[TTS] TTS API error {}: {}. Usando Windows TTS.", status, body);
            return synthesize_windows_sapi(text, config.tts_volume).await;
        }
        return Err(format!("TTS API error {status}: {body}").into());
    }

    let body_start = std::time::Instant::now();
    let audio_bytes = resp.bytes().await?;
    eprintln!(
        "[perf] tts_body_ok | seq={} | body_ms={} | bytes={}",
        seq,
        body_start.elapsed().as_millis(),
        audio_bytes.len()
    );

    let b64 = STANDARD.encode(&audio_bytes);
    eprintln!(
        "[perf] tts_done | seq={} | total_ms={}",
        seq,
        http_start.elapsed().as_millis()
    );
    Ok(b64)
}

async fn synthesize_windows_sapi(
    text: &str,
    volume: u8,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let text = text.to_string();

    let audio_bytes = tokio::task::spawn_blocking(
        move || -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
            let output_path = std::env::temp_dir().join(format!(
                "dexter-tts-{}-{}.wav",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            let output_path_str = output_path.to_string_lossy().replace('\'', "''");

            let script = format!(
                r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
$text = [Console]::In.ReadToEnd()
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
try {{
    $culture = [System.Globalization.CultureInfo]::GetCultureInfo('pt-BR')
    $voice = $synth.GetInstalledVoices($culture) | Select-Object -First 1
    if ($voice) {{
        $synth.SelectVoice($voice.VoiceInfo.Name)
    }}
}} catch {{}}
$synth.Rate = 1
$synth.Volume = {volume}
$synth.SetOutputToWaveFile('{output_path_str}')
$synth.Speak($text)
$synth.Dispose()
"#
            );

            let mut child = std::process::Command::new("powershell")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()?;

            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(text.as_bytes())?;
            }

            let output = child.wait_with_output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Falha no TTS do Windows: {}", stderr).into());
            }

            let bytes = std::fs::read(&output_path)?;
            let _ = std::fs::remove_file(&output_path);
            Ok(bytes)
        },
    )
    .await??;

    Ok(STANDARD.encode(&audio_bytes))
}
