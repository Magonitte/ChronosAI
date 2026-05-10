use crate::{AppState, ChatMessage, VoiceConfig};
use base64::{engine::general_purpose::STANDARD, Engine};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use std::io::Write;
use tauri::Manager;
use tokio::sync::mpsc;
use std::collections::HashMap;

// ── Audio Recording (cpal) ──

/// Record audio on the current thread until `is_recording` is set to false.
/// Writes samples directly into AppState's shared buffer.
pub fn record_audio(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("No input device available")?;

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
            return Err(format!("Unsupported sample format: {:?}", format).into());
        }
    };

    stream.play()?;

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
pub fn build_tools(tools_config: &crate::ToolsConfig) -> Vec<serde_json::Value> {
    let mut tools = Vec::new();

    if tools_config.search_knowledge {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_knowledge",
                "description": "Search the user's local knowledge base for relevant information. Use this when the user asks about something that might be in their stored documents or notes.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query to find relevant knowledge"
                        }
                    },
                    "required": ["query"]
                }
            }
        }));
    }

    if tools_config.screenshot {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "take_screenshot",
                "description": "Capture a screenshot of the user's screen and describe what is visible. Use this when the user asks what's on their screen, asks you to look at something, or wants help with something they're looking at.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "What to look for or describe in the screenshot. Defaults to a general description."
                        }
                    }
                }
            }
        }));
    }

    if tools_config.read_clipboard {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_clipboard",
                "description": "Read the current text contents of the user's clipboard. Use this when the user says they copied something, or asks about what's in their clipboard.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.open_url {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "open_url",
                "description": "Open a URL in the user's default web browser. Do NOT use YouTube or Spotify links to play a song BY NAME when play_music_query is available — that tool searches local files first. Use open_url for generic websites, docs, maps, etc.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to open"
                        }
                    },
                    "required": ["url"]
                }
            }
        }));
    }

    if tools_config.get_current_time {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Get the current date, time, and day of week. Use when the user asks what time or date it is.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.list_apps {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_running_apps",
                "description": "List all currently running applications on the user's PC. Use when the user asks what apps are open or running.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.web_fetch {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Fetch a web page and return its text content. Use when the user asks about something online, wants you to read an article, check a website, look up documentation, or get current information from the web.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch"
                        }
                    },
                    "required": ["url"]
                }
            }
        }));
    }

    if tools_config.run_command {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Execute a PowerShell command on the user's PC and return output. For opening or closing the predefined desktop apps (Cursor, VS Code, Terminal, browsers, Office, etc.), prefer launch_desktop_app or close_desktop_app. For playing a song BY NAME use play_music_query; for whole-library shuffle use native_music_library_shuffle_play (no disk scan); for artist-scoped multi-track M3U use play_local_music_playlist only when that scope applies; NEVER use play_full_local_music_library unless the user explicitly asked for a giant exported M3U file (requires explicit_m3u_export_request true). For music or video play/pause/skip/volume on what is already playing, prefer control_media_playback and adjust_system_volume instead of simulating keys via PowerShell.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute (runs in PowerShell)"
                        }
                    },
                    "required": ["command"]
                }
            }
        }));
    }

    if tools_config.launch_desktop_app {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "launch_desktop_app",
                "description": "Open a predefined desktop application on Windows. Prefer this over run_command when the user asks to open Cursor, VS Code, Windows Terminal, Chrome, Edge, Discord, OBS, Snipping Tool, Groove/media player, Excel, Word, PowerPoint, or Outlook. To close those apps, use close_desktop_app.",
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
                                "outlook"
                            ],
                            "description": "Application id: cursor; vscode (VS Code); terminal (Windows Terminal); chrome; edge; discord; obs (OBS Studio); snipping_tool (capture tool); media_player or groove (Groove Music); excel; word; powerpoint; outlook."
                        }
                    },
                    "required": ["app"]
                }
            }
        }));
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "close_desktop_app",
                "description": "Close (quit) a predefined desktop application on Windows by stopping its main process. Same app ids as launch_desktop_app. Prefer this over run_command when the user asks to close or kill those apps.",
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
                                "outlook"
                            ],
                            "description": "Same ids as launch_desktop_app."
                        }
                    },
                    "required": ["app"]
                }
            }
        }));
    }

    if tools_config.media_controls {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "control_media_playback",
                "description": "Control whatever Windows considers the active media session (Groove Music is preferred when multiple sessions exist). CANNOT play a song BY TITLE alone — for named tracks use play_music_query first. For ALL tracks by an artist as an M3U use play_local_music_playlist (not whole-PC library). For shuffle-all / entire library use native_music_library_shuffle_play only (fast, uses Media Player UI). play_full_local_music_library only if user explicitly wants a scanned giant M3U export (explicit_m3u_export_request true). flow: (1) Specific song → play_music_query. (2) Artist-scoped playlist file → play_local_music_playlist. (3) Whole library → native_music_library_shuffle_play. (4) Explicit M3U export → play_full_local_music_library. (5) Otherwise launch_desktop_app groove/media_player or open_url THEN control_media_playback play or toggle. status shows title and artist when available.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["play", "pause", "toggle", "next", "previous", "stop", "status"],
                            "description": "play: resume or start if the app supports it (often needs YouTube or Spotify already open); pause; toggle; next/previous; stop; status: now playing"
                        }
                    },
                    "required": ["action"]
                }
            }
        }));
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "adjust_system_volume",
                "description": "Adjust Windows master volume using multimedia keys. Use when the user asks to raise, lower, or mute/unmute system volume (not in-app volume sliders). Each step is one volume-key press (~2% per step on typical setups).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["up", "down", "mute_toggle"],
                            "description": "up: volume up; down: volume down; mute_toggle: mute/unmute"
                        },
                        "steps": {
                            "type": "integer",
                            "description": "For up/down only: how many key presses (default 3, max 50)"
                        }
                    },
                    "required": ["action"]
                }
            }
        }));
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "play_music_query",
                "description": "Play a song by title (optional artist). ALWAYS use this when the user names a track — never open_url to YouTube for that. Step 1: full scan of the Windows Music library folder ([Environment]::MyMusic / pasta Música do perfil, OneDrive Music, Public Music), paths from Dexter Settings (Pastas de música), DEXTER_MUSIC_PATHS env, up to 200k entries per root, matching folder + file names. Step 2: scan Downloads/Documents/Desktop with a smaller limit. Step 3: YouTube only if still no match.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Song title (required), e.g. After Insanity"
                        },
                        "artist": {
                            "type": "string",
                            "description": "Optional artist or band to narrow results"
                        }
                    },
                    "required": ["query"]
                }
            }
        }));
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "play_local_music_playlist",
                "description": "Writes an M3U and opens it — use ONLY when the user clearly wants multiple local tracks scoped by artist/folder keywords (e.g. «playlist do Metallica», «todas as músicas do Linkin Park»). Same matching rules as play_music_query (words in paths/filenames). NEVER use for «whole PC library», «all my music», shuffle-everything — those MUST use native_music_library_shuffle_play (player built-in «Ordem aleatoria e reproduzir»). Whole-library phrases passed here are auto-redirected to native shuffle. Not for streaming.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "artist": {
                            "type": "string",
                            "description": "Artist or band name as in your folders/filenames, e.g. Linkin Park"
                        }
                    },
                    "required": ["artist"]
                }
            }
        }));
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "play_full_local_music_library",
                "description": "DISABLED unless explicit export: SLOW — full disk scan building a giant M3U. Call ONLY when the user verbatim asked to create/export a large playlist file, M3U from disk scan, or list every audio path (e.g. for VLC with a file). For normal playback or shuffle of their whole library you MUST use native_music_library_shuffle_play instead (no scan). You must pass explicit_m3u_export_request true or the tool refuses. Track cap: DEXTER_MUSIC_FULL_PLAYLIST_MAX.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "explicit_m3u_export_request": {
                            "type": "boolean",
                            "description": "Must be true. Set true ONLY when the user explicitly requested exporting/creating a giant M3U via disk scan — never for ordinary «play all my music»."
                        },
                        "include_downloads_documents": {
                            "type": "boolean",
                            "description": "If true or omitted, after scanning main Music folders also scan Downloads, Videos, Documents, Desktop, and OneDrive Documents for audio. If false, only Music library + DEXTER_MUSIC_PATHS."
                        }
                    },
                    "required": ["explicit_m3u_export_request"]
                }
            }
        }));
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "native_music_library_shuffle_play",
                "description": "FAST — preferred for whole-library playback: opens Windows Media Player / Groove (no disk scan, no M3U), then UI Automation clicks Music Library and the shuffle-all button whose visible label is «Ordem aleatoria e reproduzir» (Portuguese UI; often without accent on aleatoria). Uses the player's indexed library. If automation fails, user taps once. NOT for one song (play_music_query), NOT artist-scoped M3U (play_local_music_playlist), NOT giant scanned export — that requires play_full_local_music_library with explicit_m3u_export_request true.",
                "parameters": {
                    "type": "object",
                    "properties": {}
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

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".to_string(),
        content: serde_json::Value::String(config.system_prompt.clone()),
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

    // Fix: Gemma 4-style models spend 200-400 tokens in reasoning_content before producing
    // visible text. A thinking_budget of 0 (unlimited) can consume all tokens. Use 256 to
    // reserve space for a visible response (~0.5s at 21 tok/s).
    let thinking_budget_tokens = if tools.is_empty() {
        256
    } else {
        256
    };

    // Short replies for voice when no tools; larger budget when tools are enabled so tool_calls JSON
    // is not truncated. After a tool transcript exists in history, allow a bit more tokens for the final answer.
    // Bumped to 600 (was 220) to accommodate 200-400 tokens of reasoning + visible response.
    let max_tokens = if !tools.is_empty() {
        1024
    } else if messages.iter().any(|m| m.role == "tool") {
        512
    } else {
        600
    };

    let request = OpenAIChatRequest {
        model: config.llm_model.clone(),
        messages: openai_messages,
        stream: true,
        max_tokens,
        temperature: 0.7,
        chat_template_kwargs: serde_json::json!({ "enable_thinking": false }),
        thinking_budget_tokens,
        tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
    };

    let llm_start = std::time::Instant::now();
    let resp = client
        .post(format!("{}/v1/chat/completions", config.llm_url))
        .json(&request)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("LLM API error {}: {}", status, body).into());
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
                                            let before = sentence_buffer[..m.start()].trim().to_string();
                                            if !before.is_empty() {
                                                let preview: String = before.chars().take(40).collect();
                                                eprintln!(
                                                    "[perf] llm_sentence | seq={} | chars={} | elapsed_ms={} | text_preview=\"{}\"",
                                                    sentence_seq,
                                                    before.chars().count(),
                                                    llm_start.elapsed().as_millis(),
                                                    preview
                                                );
                                                sentence_seq += 1;
                                                spoken_text.push_str(&before);
                                                spoken_text.push(' ');
                                                let _ = sentence_tx.send(before).await;
                                            }
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
                                                let sentence: String = sentence_buffer.drain(..=split_pos).collect();
                                                let sentence = sentence.trim().to_string();
                                                if !sentence.is_empty() {
                                                    let preview: String = sentence.chars().take(40).collect();
                                                    eprintln!(
                                                        "[perf] llm_sentence | seq={} | chars={} | elapsed_ms={} | text_preview=\"{}\"",
                                                        sentence_seq,
                                                        sentence.chars().count(),
                                                        llm_start.elapsed().as_millis(),
                                                        preview
                                                    );
                                                    sentence_seq += 1;
                                                    spoken_text.push_str(&sentence);
                                                    spoken_text.push(' ');
                                                    let _ = sentence_tx.send(sentence).await;
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
    }

    // Flush remaining sentence buffer
    if !xml_collecting {
        let remaining = sentence_buffer.trim().to_string();
        if !remaining.is_empty() {
            let preview: String = remaining.chars().take(40).collect();
            eprintln!(
                "[perf] llm_sentence | seq={} | chars={} | elapsed_ms={} | text_preview=\"{}\"",
                sentence_seq,
                remaining.chars().count(),
                llm_start.elapsed().as_millis(),
                preview
            );
            sentence_seq += 1;
            spoken_text.push_str(&remaining);
            let _ = sentence_tx.send(remaining).await;
        }
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
    Ok(StreamResult::Content(full_response.trim().to_string()))
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

/// Find a good TTS chunk boundary in the buffer.
/// Returns the byte index of the last char of the chunk (inclusive).
fn find_tts_chunk_end(text: &str) -> Option<usize> {
    const MIN_SOFT_CHUNK_CHARS: usize = 60;
    const MAX_CHUNK_CHARS: usize = 140;

    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return None;
    }

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

    if chars.len() < MAX_CHUNK_CHARS {
        return None;
    }

    let soft_limit = chars.len().min(MAX_CHUNK_CHARS);
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

    for i in (MIN_SOFT_CHUNK_CHARS..soft_limit).rev() {
        let (byte_idx, ch) = chars[i];
        if ch.is_whitespace() {
            return Some(byte_idx);
        }
    }

    chars.get(MAX_CHUNK_CHARS.saturating_sub(1)).map(|(byte_idx, _)| *byte_idx)
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
    let tts_mode = std::env::var("DEXTER_TTS_MODE").unwrap_or_default();
    if tts_mode.eq_ignore_ascii_case("windows") {
        eprintln!(
            "[perf] tts_start | seq={} | chars={} | backend=windows_sapi | text_preview=\"{}\"",
            seq,
            text.chars().count(),
            text.chars().take(40).collect::<String>()
        );
        return synthesize_windows_sapi(text).await;
    }

    let preview: String = text.chars().take(40).collect();
    eprintln!(
        "[perf] tts_start | seq={} | chars={} | backend=chatterbox | text_preview=\"{}\"",
        seq,
        text.chars().count(),
        preview
    );

    // Chatterbox na GPU pode passar de 20s no primeiro chunk (contenda com LLM / JIT CUDA).
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let request = ChatterboxRequest {
        input: text.to_string(),
        voice: config.chatterbox_voice.clone(),
        model: "chatterbox".to_string(),
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
            eprintln!("[TTS] Chatterbox indisponivel, usando Windows TTS: {}", err);
            return synthesize_windows_sapi(text).await;
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[TTS] Chatterbox API error {}: {}. Usando Windows TTS.", status, body);
        return synthesize_windows_sapi(text).await;
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
$synth.Volume = 100
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
                return Err(format!("Windows TTS failed: {}", stderr).into());
            }

            let bytes = std::fs::read(&output_path)?;
            let _ = std::fs::remove_file(&output_path);
            Ok(bytes)
        },
    )
    .await??;

    Ok(STANDARD.encode(&audio_bytes))
}
