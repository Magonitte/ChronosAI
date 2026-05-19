use crate::{AppState, ChatMessage, VoiceConfig};
use base64::{engine::general_purpose::STANDARD, Engine};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::sync::OnceLock;
use tauri::Manager;
use tokio::sync::mpsc;

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

/// Text destined for TTS: no markdown, no spoken punctuation names, gentler symbols for XTTS/SAPI.
pub fn sanitize_for_tts(text: &str) -> String {
    if text.is_empty() {
        return String::new();
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
    if looks_like_leaked_tool_call(raw) {
        eprintln!(
            "[voice] skip_tts_chunk | tool_leak | text=\"{}\"",
            raw.chars().take(80).collect::<String>()
        );
        return None;
    }
    let s = sanitize_for_tts(raw);
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

/// Build tool definitions based on enabled tools in config.
/// Tools are ordered by frequency of use — LLMs have primacy bias, so
/// the most-used tools appear first to improve selection accuracy.
pub fn build_tools(tools_config: &crate::ToolsConfig) -> Vec<serde_json::Value> {
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
                "description": "Abre um aplicativo de desktop pré-definido. Use para abrir apps como Chrome, VS Code, Terminal, reprodutor de música, Office, Discord, etc. (veja o parâmetro 'app' para a lista completa).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "app": {
                            "type": "string",
                            "enum": [
                                "cursor",
                                "vscode",
                                "terminal",
                                "chrome",
                                "edge",
                                "discord",
                                "obs",
                                "snipping_tool",
                                "media_player",
                                "groove",
                                "excel",
                                "word",
                                "powerpoint",
                                "outlook",
                                "paint"
                            ],
                            "description": "Id do aplicativo: cursor; vscode (VS Code); terminal (Terminal do Windows); chrome; edge; discord; obs (OBS Studio); snipping_tool (captura); media_player ou groove (Groove Music); excel; word; powerpoint; outlook; paint (Paint)."
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
                "description": "Fecha um aplicativo pré-definido. Use para fechar/encerrar apps. Mesmos apps disponíveis que launch_desktop_app.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "app": {
                            "type": "string",
                            "enum": [
                                "cursor",
                                "vscode",
                                "terminal",
                                "chrome",
                                "edge",
                                "discord",
                                "obs",
                                "snipping_tool",
                                "media_player",
                                "groove",
                                "excel",
                                "word",
                                "powerpoint",
                                "outlook",
                                "paint"
                            ],
                            "description": "Mesmos ids que launch_desktop_app."
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
                "description": "Captura a tela e descreve o que está visível.",
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
                "description": "Toca uma música pelo nome (artista opcional). Por padrão busca nas pastas locais do usuário; se não achar, abre no YouTube. Se o usuário disser \"no YouTube\", use prefer_youtube=true. Se pedir \"no player de música\" / reprodutor, use prefer_native_player=true. Não use open_url para música.",
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
                "description": "Lê o texto atual da área de transferência.",
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
                "description": "Executa um comando PowerShell no PC e retorna a saída. Use apenas para tarefas que não têm ferramenta dedicada (apps, música, volume, tela, web, etc. têm tools próprias).",
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
Nao use listas, titulos nem paragrafos longos. Se a pergunta pedir muito detalhe, resuma em voz e sugira o chat de texto. \
Nunca escreva o nome dos sinais de pontuacao (vírgula, ponto, dois pontos, etc.) — apenas frases faladas naturais, sem markdown nem asteriscos. \
Nao comece com \"A resposta é\", \"Em resumo\" ou aspas. Para perguntas informativas simples, responda direto sem chamar ferramentas.";
    const VOICE_TOOL_ROUTING_SUFFIX: &str = "\n\nRoteamento de ferramentas: para cotacao use fetch_fx_quote (pair USD-BRL, EUR-BRL, JPY-BRL ou GBP-BRL). \
Para temperatura atual use fetch_weather sem day_offset. Para previsao (amanha, chuva) use day_offset 1 ou 2. location vazio = local por IP. \
Para outras buscas na web use web_fetch. Use search_knowledge apenas para documentos locais ja indexados no RAG. \
Para abrir apps use launch_desktop_app. Para tocar musica (incluindo no YouTube) use SEMPRE play_music_query \
com query e artist opcional — isso abre o video watch?v= com autoplay, nunca use open_url com pagina de busca do YouTube.";

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
        "screenshot",
        "captura de tela",
        "captura da tela",
        "print da tela",
        "que horas",
        "horas são",
        "que dia é",
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
        "clipboard",
        "área de transferência",
        "toca ",
        "toque ",
        "música",
        "musica",
        "youtube",
        "no youtube",
        "executa ",
        "rode o comando",
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
        "olha minha tela",
        "veja minha tela",
        "volume ",
        "pausa a música",
        "pausa a musica",
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

/// Max characters per TTS HTTP request (`DEXTER_TTS_MAX_CHUNK_CHARS`, default 140).
pub fn tts_max_chunk_chars() -> usize {
    std::env::var("DEXTER_TTS_MAX_CHUNK_CHARS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|n| n.clamp(40, 400))
        .unwrap_or(260)
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
    let text = text.as_str();

    let tts_mode = std::env::var("DEXTER_TTS_MODE").unwrap_or_default();
    if tts_mode.eq_ignore_ascii_case("windows") {
        eprintln!(
            "[perf] tts_start | seq={} | chars={} | backend=windows_sapi | text_preview=\"{}\"",
            seq,
            text.chars().count(),
            text.chars().take(40).collect::<String>()
        );
        return synthesize_windows_sapi(text, config.tts_volume).await;
    }

    let preview: String = text.chars().take(40).collect();
    eprintln!(
        "[perf] tts_start | seq={} | chars={} | backend=xtts | text_preview=\"{}\"",
        seq,
        text.chars().count(),
        preview
    );

    // XTTS v2 na GPU pode levar ate 15s no primeiro chunk (contenda com LLM / JIT CUDA).
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
            eprintln!("[TTS] TTS indisponivel, usando Windows TTS: {}", err);
            return synthesize_windows_sapi(text, config.tts_volume).await;
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[TTS] TTS API error {}: {}. Usando Windows TTS.", status, body);
        return synthesize_windows_sapi(text, config.tts_volume).await;
    }

    let body_start = std::time::Instant::now();
    let audio_bytes = resp.bytes().await?;
    let body_elapsed = body_start.elapsed();
    eprintln!(
        "[perf] tts_body_ok | seq={} | body_ms={} | bytes={}",
        seq,
        body_elapsed.as_millis(),
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
