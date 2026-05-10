use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    webview::WebviewWindowBuilder,
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio_util::sync::CancellationToken;

mod media_controls;
mod rag;
mod sandbox;
mod tools;
mod voice;

#[derive(Clone, Serialize)]
struct ProcessingState {
    stage: String,
    text: String,
}

#[derive(Clone, Serialize)]
struct AudioChunk {
    index: u32,
    audio: String, // base64 WAV
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Preserved tool_calls from assistant messages (OpenAI-compatible format).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<voice::ToolCallOut>>,
    /// Tool call ID for tool result messages (OpenAI-compatible format).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
}

pub struct AppState {
    messages: Mutex<Vec<ChatMessage>>,
    config: Mutex<VoiceConfig>,
    rag_store: rag::RagStore,
    audit_log: Mutex<sandbox::AuditLog>,
    // Audio samples collected by the recording thread
    recorded_samples: Mutex<Vec<f32>>,
    recording_sample_rate: Mutex<u32>,
    is_recording: Mutex<bool>,
    // Cancellation token for the active pipeline — cancelled when user interrupts
    pipeline_cancel: Mutex<CancellationToken>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub search_knowledge: bool,
    #[serde(default = "default_true")]
    pub screenshot: bool,
    #[serde(default = "default_true")]
    pub read_clipboard: bool,
    #[serde(default = "default_true")]
    pub open_url: bool,
    #[serde(default = "default_true")]
    pub get_current_time: bool,
    #[serde(default = "default_true")]
    pub list_apps: bool,
    #[serde(default = "default_true")]
    pub run_command: bool,
    #[serde(default = "default_true")]
    pub web_fetch: bool,
    #[serde(default = "default_true")]
    pub launch_desktop_app: bool,
    #[serde(default = "default_true")]
    pub media_controls: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            search_knowledge: true,
            screenshot: true,
            read_clipboard: true,
            open_url: true,
            get_current_time: true,
            list_apps: true,
            run_command: true,
            web_fetch: true,
            launch_desktop_app: true,
            media_controls: true,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    pub whisper_model_path: String,
    #[serde(default = "default_whisper_url")]
    pub whisper_url: String,
    pub llm_url: String,
    pub llm_model: String,
    pub embed_model: String,
    #[serde(default)]
    pub vision_model: String,
    pub chatterbox_url: String,
    pub chatterbox_voice: String,
    pub system_prompt: String,
    /// Pastas extra onde procurar música (uma por linha ou separadas por ; ou |). Junta-se à pasta Música do Windows e a DEXTER_MUSIC_PATHS.
    #[serde(default)]
    pub music_library_paths: String,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub sandbox: sandbox::SandboxConfig,
}

fn default_whisper_url() -> String {
    "http://localhost:8081".to_string()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        let default_whisper = r"J:\Modelos LLM\manifests\registry.ollama.ai\library\whisper\ggml-small.bin".to_string();
        Self {
            whisper_model_path: default_whisper,
            whisper_url: "http://localhost:8081".to_string(),
            llm_url: "http://localhost:8080".to_string(),
            llm_model: "gemma-4-26B-A4B".to_string(),
            embed_model: "gemma-4-26B-A4B".to_string(),
            vision_model: String::new(), // Use llm_model for vision if empty
            chatterbox_url: "http://localhost:8005".to_string(),
            chatterbox_voice: "dexter-ptbr".to_string(),
            system_prompt: "Você é um assistente de voz rodando no desktop do usuário. A conversa acontece inteiramente por voz — o usuário fala no microfone, a fala é transcrita via Whisper (STT), enviada como mensagem para você, e sua resposta é convertida de volta em fala via Chatterbox Turbo (TTS) e reproduzida nos alto-falantes. Você pode ouvir o usuário e ele pode ouvir você — trate como uma conversa falada natural. Se perguntarem \"você me ouve\" a resposta é sim.\n\nIMPORTANTE: Responda SEMPRE em português do Brasil, independentemente do idioma da pergunta.\n\nMantenha respostas curtas e conversacionais — 2-3 frases no máximo. Sem markdown, sem blocos de código, sem bullet points, sem listas numeradas, sem formatação especial. Escreva exatamente como falaria em voz alta. Evite dois-pontos nas respostas pois causam pausas estranhas no TTS.\n\nVocê pode expressar emoções naturalmente usando estas tags paralinguísticas no meio da fala — use com moderação e só quando realmente encaixar:\n[laugh] [chuckle] [sigh] [gasp] [cough] [clear throat] [sniff] [groan] [shush]\nExemplo — \"Nossa, isso é muito engraçado [laugh] não esperava isso de jeito nenhum.\"\nNÃO exagere. A maioria das respostas não precisa de nenhuma tag. Use só quando um humano genuinamente faria aquele som.\n\nQuando decidir usar uma ferramenta, SEMPRE diga o que vai fazer antes em uma frase curta e natural antes de chamar a ferramenta. Por exemplo — \"Deixa eu olhar sua tela\" antes de tirar screenshot, \"Vou procurar isso na web\" antes de buscar uma página, \"Deixa eu ver que horas são\" antes de checar o horário, \"Um segundo, vou rodar esse comando\" antes de executar um comando. Para música: se não houver player ou aba com vídeo aberta, abra com launch_desktop_app media_player ou open_url no YouTube ou Spotify antes de pedir play no control_media_playback. Se o usuário pedir uma música pelo NOME da faixa ou artista, use play_music_query com o título — nunca use open_url para YouTube nesse caso. Essa ferramenta varre primeiro a pasta Música do Windows, pastas equivalentes, as pastas que o usuário configurou nas Configurações em Pastas de música e só depois tenta o YouTube. Se pedirem tocar ou embaralhar TODA a biblioteca de música do PC, tudo de uma vez, ou equivalente, use SEMPRE native_music_library_shuffle_play — abre o Reprodutor Multimédia e usa o fluxo interno «Biblioteca de músicas» e o botão «Ordem aleatoria e reproduzir» (texto visível na UI, sem acento em aleatoria). Nunca varra disco nem gere M3U gigante para isso. play_local_music_playlist só quando quiserem várias faixas locais por um artista ou pasta concreta e aceitarem lista M3U para esse caso. play_full_local_music_library só se pedirem explicitamente exportar ou criar arquivo de lista M3U enorme por varredura — e a ferramenta exige confirmação no parâmetro; caso contrário não chame. Assim o usuário ouve o que está acontecendo em vez de esperar em silêncio.".to_string(),
            music_library_paths: String::new(),
            tools: ToolsConfig::default(),
            sandbox: sandbox::SandboxConfig::default(),
        }
    }
}

impl VoiceConfig {
    fn config_path() -> std::path::PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
            .join("voice-assistant")
            .join("config.json")
    }

    fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str(&data) {
                    return config;
                }
            }
        }
        Self::default()
    }

    fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, data);
        }
    }
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> VoiceConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn set_config(state: tauri::State<AppState>, config: VoiceConfig) {
    config.save();
    *state.config.lock().unwrap() = config;
}

#[tauri::command]
fn get_messages(state: tauri::State<AppState>) -> Vec<ChatMessage> {
    state.messages.lock().unwrap().clone()
}

#[tauri::command]
fn clear_messages(state: tauri::State<AppState>) {
    state.messages.lock().unwrap().clear();
}

#[tauri::command]
fn show_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn hide_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

// ── RAG Commands ──

#[tauri::command]
async fn ingest_text(
    app: tauri::AppHandle,
    source: String,
    text: String,
) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();
    state
        .rag_store
        .ingest(&source, &text, &config.llm_url, &config.embed_model)
        .await
        .map_err(|e| format!("Ingest failed: {}", e))
}

#[tauri::command]
async fn ingest_file(app: tauri::AppHandle, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Read failed: {}", e))?;
    let source = std::path::Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();
    state
        .rag_store
        .ingest(&source, &text, &config.llm_url, &config.embed_model)
        .await
        .map_err(|e| format!("Ingest failed: {}", e))
}

#[tauri::command]
fn list_knowledge_sources(app: tauri::AppHandle) -> Result<Vec<(String, usize)>, String> {
    let state = app.state::<AppState>();
    state
        .rag_store
        .list_sources()
        .map_err(|e| format!("List failed: {}", e))
}

#[tauri::command]
fn delete_knowledge_source(app: tauri::AppHandle, source: String) -> Result<usize, String> {
    let state = app.state::<AppState>();
    state
        .rag_store
        .delete_source(&source)
        .map_err(|e| format!("Delete failed: {}", e))
}

#[tauri::command]
fn start_recording(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    // Check if already recording
    {
        let is_rec = state.is_recording.lock().unwrap();
        if *is_rec {
            return Ok(());
        }
    }

    // Clear previous samples
    state.recorded_samples.lock().unwrap().clear();
    *state.is_recording.lock().unwrap() = true;

    let app_handle = app.clone();

    // Spawn recording on a dedicated thread (cpal::Stream isn't Send)
    std::thread::spawn(move || {
        if let Err(e) = voice::record_audio(&app_handle) {
            eprintln!("Recording error: {}", e);
        }
    });

    Ok(())
}

#[tauri::command]
fn stop_recording_and_process(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    // Signal recording to stop
    *state.is_recording.lock().unwrap() = false;

    // Give a moment for the recording thread to finish writing samples
    std::thread::sleep(std::time::Duration::from_millis(100));

    let samples = state.recorded_samples.lock().unwrap().clone();
    let sample_rate = *state.recording_sample_rate.lock().unwrap();
    let config = state.config.lock().unwrap().clone();

    if samples.is_empty() {
        return Err("No audio recorded".to_string());
    }

    // Process in background
    let cancel_token = state.pipeline_cancel.lock().unwrap().clone();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = process_pipeline(app_handle.clone(), samples, sample_rate, config, cancel_token).await {
            if e != "interrupted" {
                eprintln!("Pipeline error: {}", e);
                let _ = app_handle.emit(
                    "processing",
                    ProcessingState {
                        stage: "error".to_string(),
                        text: e,
                    },
                );
            }
        }
    });

    Ok(())
}

async fn process_pipeline(
    app: tauri::AppHandle,
    samples: Vec<f32>,
    sample_rate: u32,
    config: VoiceConfig,
    cancel: CancellationToken,
) -> Result<(), String> {
    let pipeline_started = std::time::Instant::now();

    // Stage 1: Transcribe
    app.emit(
        "processing",
        ProcessingState {
            stage: "transcribing".to_string(),
            text: "Transcribing...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    let whisper_url = config.whisper_url.clone();
    let stt_started = std::time::Instant::now();
    let transcript = voice::transcribe_audio(&whisper_url, &samples, sample_rate).await
        .map_err(|e| format!("Transcription failed: {}", e))?;
    eprintln!("[perf] STT finished in {:.2}s", stt_started.elapsed().as_secs_f32());

    if cancel.is_cancelled() { return Err("interrupted".to_string()); }

    if transcript.trim().is_empty() {
        app.emit(
            "processing",
            ProcessingState {
                stage: "idle".to_string(),
                text: String::new(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;
        return Err("No speech detected".to_string());
    }

    app.emit(
        "processing",
        ProcessingState {
            stage: "transcribed".to_string(),
            text: transcript.clone(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    // Add user message
    {
        app.state::<AppState>().messages.lock().unwrap().push(ChatMessage {
            role: "user".to_string(),
            content: transcript.clone(),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // Stage 2: LLM with tool calling → streaming TTS
    app.emit(
        "processing",
        ProcessingState {
            stage: "thinking".to_string(),
            text: "Thinking...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    let all_messages = app.state::<AppState>().messages.lock().unwrap().clone();

    let tools = voice::build_tools(&config.tools);
    let max_tool_rounds = 5;

    // Single streaming loop: stream with tools → if model returns tool calls,
    // execute them and stream again. If it returns content, sentences flow to TTS.
    let (sentence_tx, mut sentence_rx) = tokio::sync::mpsc::channel::<String>(16);
    let mut sentence_index: u32 = 0;
    let mut full_text = String::new();
    let llm_started = std::time::Instant::now();
    let mut first_audio_logged = false;

    let app_clone = app.clone();
    let config_clone = config.clone();
    let cancel_llm = cancel.clone();

    let llm_handle = {
        let tools = tools.clone();
        let sentence_tx = sentence_tx.clone();
        let app = app_clone.clone();
        let config = config_clone.clone();

        tokio::spawn(async move {
            let mut all_msgs = all_messages;

            for _round in 0..max_tool_rounds {
                if cancel_llm.is_cancelled() { return Err("interrupted".to_string()); }

                let result = tokio::select! {
                    _ = cancel_llm.cancelled() => { return Err("interrupted".to_string()); }
                    r = voice::chat_streaming(&config, &all_msgs, &tools, &sentence_tx) => {
                        r.map_err(|e| format!("LLM failed: {}", e))?
                    }
                };

                match result {
                    voice::StreamResult::Content(text) => {
                        return Ok::<String, String>(text);
                    }
                    voice::StreamResult::ToolCalls(tool_calls, preamble, xml_parsed) => {
                        if cancel_llm.is_cancelled() { return Err("interrupted".to_string()); }

                        if xml_parsed {
                            // XML-parsed tool calls: model emitted XML as text.
                            if !preamble.is_empty() {
                                let m = ChatMessage {
                                    role: "assistant".to_string(),
                                    content: preamble.clone(),
                                    tool_calls: None,
                                    tool_call_id: None,
                                };
                                all_msgs.push(m.clone());
                                app.state::<AppState>().messages.lock().unwrap().push(m);
                            }

                            let mut tool_results = String::new();
                            for tool_call in &tool_calls {
                                if cancel_llm.is_cancelled() { return Err("interrupted".to_string()); }

                                let _ = app.emit(
                                    "processing",
                                    ProcessingState {
                                        stage: "tool_call".to_string(),
                                        text: tool_call.name.clone(),
                                    },
                                );

                                let result_text = execute_tool(&app, &config, tool_call).await;
                                tool_results.push_str(&format!(
                                    "[Tool result for {}]: {}\n",
                                    tool_call.name, result_text
                                ));
                            }

                            let follow_up = format!(
                                "Tool results for this reply:\n\n{}",
                                tool_results.trim()
                            );
                            let um = ChatMessage {
                                role: "user".to_string(),
                                content: follow_up,
                                tool_calls: None,
                                tool_call_id: None,
                            };
                            all_msgs.push(um.clone());
                            app.state::<AppState>().messages.lock().unwrap().push(um);
                        } else {
                            // Native tool calls: use OpenAI-compatible protocol — must persist assistant+tool
                            // messages so the next user utterance still has a valid toolCalling transcript.
                            let tool_calls_out: Vec<voice::ToolCallOut> =
                                tool_calls.iter().map(|tc| tc.to_out()).collect();
                            let assistant_msg = ChatMessage {
                                role: "assistant".to_string(),
                                content: preamble.clone(),
                                tool_calls: Some(tool_calls_out.clone()),
                                tool_call_id: None,
                            };
                            all_msgs.push(assistant_msg.clone());
                            app.state::<AppState>().messages.lock().unwrap().push(assistant_msg);

                            for tool_call in &tool_calls {
                                if cancel_llm.is_cancelled() { return Err("interrupted".to_string()); }

                                let _ = app.emit(
                                    "processing",
                                    ProcessingState {
                                        stage: "tool_call".to_string(),
                                        text: tool_call.name.clone(),
                                    },
                                );

                                let result_text = execute_tool(&app, &config, tool_call).await;

                                let tool_msg = ChatMessage {
                                    role: "tool".to_string(),
                                    content: result_text,
                                    tool_calls: None,
                                    tool_call_id: Some(tool_call.id.clone()),
                                };
                                all_msgs.push(tool_msg.clone());
                                app.state::<AppState>().messages.lock().unwrap().push(tool_msg);
                            }
                        }

                        let _ = app.emit(
                            "processing",
                            ProcessingState {
                                stage: "thinking".to_string(),
                                text: "Thinking...".to_string(),
                            },
                        );
                    }
                }
            }

            // Hit max rounds — do one final stream without tools
            if cancel_llm.is_cancelled() { return Err("interrupted".to_string()); }

            let result = voice::chat_streaming(&config, &all_msgs, &[], &sentence_tx)
                .await
                .map_err(|e| format!("LLM failed: {}", e))?;

            match result {
                voice::StreamResult::Content(text) => Ok(text),
                voice::StreamResult::ToolCalls(_, _, _) => Err("Model returned tool calls after max rounds".to_string()),
            }
        })
    };

    // Drop our copy of sentence_tx so the channel closes when the spawned task finishes
    drop(sentence_tx);

    // Process sentences as they arrive from the stream → TTS → audio
    while let Some(sentence) = sentence_rx.recv().await {
        if cancel.is_cancelled() { break; }

        full_text.push_str(&sentence);
        full_text.push(' ');

        app.emit(
            "processing",
            ProcessingState {
                stage: "speaking".to_string(),
                text: full_text.trim().to_string(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;

        // Race TTS synthesis against cancellation
        let tts_started = std::time::Instant::now();
        let tts_result = tokio::select! {
            _ = cancel.cancelled() => { break; }
            r = voice::synthesize(&config, &sentence) => r
        };

        match tts_result {
            Ok(audio_base64) => {
                eprintln!(
                    "[perf] TTS chunk {} finished in {:.2}s",
                    sentence_index,
                    tts_started.elapsed().as_secs_f32()
                );
                if !first_audio_logged {
                    eprintln!(
                        "[perf] First audio ready in {:.2}s",
                        pipeline_started.elapsed().as_secs_f32()
                    );
                    first_audio_logged = true;
                }
                if cancel.is_cancelled() { break; }
                app.emit("play_audio_chunk", AudioChunk {
                    index: sentence_index,
                    audio: audio_base64,
                })
                .map_err(|e: tauri::Error| e.to_string())?;
                sentence_index += 1;
            }
            Err(e) => {
                eprintln!("TTS failed for sentence: {}", e);
            }
        }
    }

    if cancel.is_cancelled() {
        llm_handle.abort(); // kill the LLM task
        return Err("interrupted".to_string());
    }

    let full_response = llm_handle
        .await
        .map_err(|e| format!("LLM task failed: {}", e))?
        .map_err(|e| e)?;
    eprintln!("[perf] LLM stream finished in {:.2}s", llm_started.elapsed().as_secs_f32());

    app.emit("play_audio_done", sentence_index)
        .map_err(|e: tauri::Error| e.to_string())?;

    // Add assistant message to history
    app.state::<AppState>().messages.lock().unwrap().push(ChatMessage {
        role: "assistant".to_string(),
        content: full_response,
        tool_calls: None,
        tool_call_id: None,
    });

    Ok(())
}

/// Execute a single tool call and return the result text.
fn json_tool_bool(
    arguments: &std::collections::HashMap<String, serde_json::Value>,
    key: &str,
    default: bool,
) -> bool {
    match arguments.get(key) {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => {
            let l = s.trim().to_ascii_lowercase();
            matches!(l.as_str(), "true" | "1" | "yes" | "sim")
        }
        Some(serde_json::Value::Number(n)) => n.as_i64().map(|i| i != 0).unwrap_or(default),
        None => default,
        _ => default,
    }
}

async fn execute_tool(
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    tool_call: &voice::ToolCall,
) -> String {
    let rag_store = &app.state::<AppState>().rag_store;
    let music_paths_opt = {
        let s = config.music_library_paths.trim();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };

    match tool_call.name.as_str() {
        "search_knowledge" => {
            let query = tool_call.arguments.get("query")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();

            let results = rag_store
                .search(&query, &config.llm_url, &config.embed_model, 5)
                .await
                .unwrap_or_default();

            if results.is_empty() {
                "No relevant results found in the knowledge base.".to_string()
            } else {
                results.iter().enumerate()
                    .map(|(i, r)| format!("[{}] (source: {}, relevance: {:.2})\n{}", i + 1, r.source, r.score, r.text))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        }
        "take_screenshot" => {
            let question = tool_call.arguments.get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("Describe what you see on this screen in detail.")
                .to_string();
            let monitor = tool_call.arguments.get("monitor")
                .and_then(|v| v.as_u64()).map(|n| n as u32);

            let _ = app.emit("processing", ProcessingState {
                stage: "thinking".to_string(),
                text: "Looking at your screen...".to_string(),
            });

            match tools::take_screenshot(monitor) {
                Ok(image_b64) => {
                    let vision_model = if config.vision_model.is_empty() {
                        &config.llm_model
                    } else {
                        &config.vision_model
                    };
                    match tools::describe_screenshot(&config.llm_url, vision_model, &image_b64, &question).await {
                        Ok(desc) => desc,
                        Err(e) => format!("Screenshot captured but vision model failed: {}. Make sure the model supports image inputs (multimodal).", e),
                    }
                }
                Err(e) => format!("Failed to capture screenshot: {}", e),
            }
        }
        "read_clipboard" => match tools::read_clipboard() {
            Ok(text) => if text.trim().is_empty() { "The clipboard is empty.".to_string() } else { format!("Clipboard contents:\n{}", text) },
            Err(e) => format!("Failed to read clipboard: {}", e),
        },
        "open_url" => {
            let url = tool_call.arguments.get("url")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if url.is_empty() { "No URL provided.".to_string() }
            else { match tools::open_url(&url) { Ok(msg) => msg, Err(e) => format!("Failed to open URL: {}", e) } }
        }
        "get_current_time" => tools::get_current_time(),
        "list_running_apps" => match tools::list_running_apps() {
            Ok(apps) => format!("Currently running applications:\n{}", apps),
            Err(e) => format!("Failed to list apps: {}", e),
        },
        "web_fetch" => {
            let url = tool_call.arguments.get("url")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if url.is_empty() { "No URL provided.".to_string() }
            else { match tools::web_fetch(&url).await { Ok(text) => text, Err(e) => format!("Failed to fetch {}: {}", url, e) } }
        }
        "run_command" => {
            let command = tool_call.arguments.get("command")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if command.is_empty() {
                "No command provided.".to_string()
            } else {
                let _ = app.emit("processing", ProcessingState {
                    stage: "thinking".to_string(),
                    text: format!("Running: {}", command),
                });
                let audit = &app.state::<AppState>().audit_log;
                match sandbox::execute(&command, &config.sandbox, audit) {
                    Ok(output) => output,
                    Err(e) => format!("Sandbox: {}", e),
                }
            }
        }
        "launch_desktop_app" => {
            let app_id = tool_call.arguments.get("app")
                .and_then(|v| v.as_str()).unwrap_or("").trim();
            if app_id.is_empty() {
                "No app id provided. Use app names like cursor, vscode, terminal, chrome, edge, discord, obs, snipping_tool, media_player, groove, excel, word, powerpoint, outlook.".to_string()
            } else {
                let _ = app.emit("processing", ProcessingState {
                    stage: "thinking".to_string(),
                    text: format!("Opening {}", app_id),
                });
                match tools::launch_desktop_app(app_id) {
                    Ok(msg) => msg,
                    Err(e) => format!("Failed to launch app: {}", e),
                }
            }
        }
        "close_desktop_app" => {
            let app_id = tool_call.arguments.get("app")
                .and_then(|v| v.as_str()).unwrap_or("").trim();
            if app_id.is_empty() {
                "No app id provided. Same ids as launch_desktop_app.".to_string()
            } else {
                let _ = app.emit("processing", ProcessingState {
                    stage: "thinking".to_string(),
                    text: format!("Closing {}", app_id),
                });
                match tools::close_desktop_app(app_id) {
                    Ok(msg) => msg,
                    Err(e) => format!("Failed to close app: {}", e),
                }
            }
        }
        "control_media_playback" => {
            let action = tool_call
                .arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if action.is_empty() {
                "Faltou action. Use play, pause, toggle, next, previous, stop ou status.".to_string()
            } else {
                let _ = app.emit(
                    "processing",
                    ProcessingState {
                        stage: "thinking".to_string(),
                        text: format!("Mídia: {}", action),
                    },
                );
                let action_clone = action.clone();
                match tokio::task::spawn_blocking(move || {
                    media_controls::control_playback(&action_clone)
                })
                .await
                {
                    Ok(Ok(msg)) => msg,
                    Ok(Err(e)) => e,
                    Err(e) => format!("Erro ao controlar mídia: {}", e),
                }
            }
        }
        "adjust_system_volume" => {
            let action = tool_call
                .arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let steps = tool_call
                .arguments
                .get("steps")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u32;
            if action.is_empty() {
                "Faltou action. Use up, down ou mute_toggle.".to_string()
            } else {
                let _ = app.emit(
                    "processing",
                    ProcessingState {
                        stage: "thinking".to_string(),
                        text: "Ajustando volume".to_string(),
                    },
                );
                let action_clone = action.clone();
                match tokio::task::spawn_blocking(move || {
                    media_controls::adjust_volume(&action_clone, steps)
                })
                .await
                {
                    Ok(Ok(msg)) => msg,
                    Ok(Err(e)) => e,
                    Err(e) => format!("Erro ao ajustar volume: {}", e),
                }
            }
        }
        "play_music_query" => {
            let query = tool_call
                .arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let artist = tool_call
                .arguments
                .get("artist")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if query.is_empty() {
                "Faltou query com o título da música.".to_string()
            } else {
                let _ = app.emit(
                    "processing",
                    ProcessingState {
                        stage: "thinking".to_string(),
                        text: format!("Procurando: {}", query),
                    },
                );
                let artist_ref = artist.as_deref();
                match tools::play_music_query(&query, artist_ref, music_paths_opt).await {
                    Ok(msg) => msg,
                    Err(e) => format!("Não foi possível abrir a música: {}", e),
                }
            }
        }
        "play_local_music_playlist" => {
            let artist = tool_call
                .arguments
                .get("artist")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if artist.is_empty() {
                "Faltou o nome do artista ou pasta para a playlist.".to_string()
            } else {
                let _ = app.emit(
                    "processing",
                    ProcessingState {
                        stage: "thinking".to_string(),
                        text: format!("Playlist: {}", artist),
                    },
                );
                match tools::play_local_music_playlist(&artist, music_paths_opt).await {
                    Ok(msg) => msg,
                    Err(e) => format!("Playlist local: {}", e),
                }
            }
        }
        "play_full_local_music_library" => {
            let explicit =
                json_tool_bool(&tool_call.arguments, "explicit_m3u_export_request", false);
            if !explicit {
                return "Para tocar ou embaralhar toda a biblioteca sem custo de varredura, use native_music_library_shuffle_play. play_full_local_music_library só pode ser usada com explicit_m3u_export_request verdadeiro quando o usuário pedir explicitamente criar ou exportar um arquivo M3U grande pela varredura do disco.".to_string();
            }
            let include_secondary =
                json_tool_bool(&tool_call.arguments, "include_downloads_documents", true);
            let _ = app.emit(
                "processing",
                ProcessingState {
                    stage: "thinking".to_string(),
                    text: "Exportar lista M3U (varredura)".to_string(),
                },
            );
            match tools::play_full_local_music_library(include_secondary, music_paths_opt).await {
                Ok(msg) => msg,
                Err(e) => format!("Biblioteca local: {}", e),
            }
        }
        "native_music_library_shuffle_play" => {
            let _ = app.emit(
                "processing",
                ProcessingState {
                    stage: "thinking".to_string(),
                    text: "Reprodutor: biblioteca".to_string(),
                },
            );
            match tools::native_music_library_shuffle_play() {
                Ok(msg) => msg,
                Err(e) => format!("Reprodutor multimédia: {}", e),
            }
        }
        unknown => format!("Unknown tool: {}", unknown),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            messages: Mutex::new(Vec::new()),
            config: Mutex::new(VoiceConfig::load()),
            rag_store: rag::RagStore::new().expect("Failed to initialize RAG store"),
            audit_log: Mutex::new(sandbox::AuditLog::new()),
            recorded_samples: Mutex::new(Vec::new()),
            recording_sample_rate: Mutex::new(44100),
            is_recording: Mutex::new(false),
            pipeline_cancel: Mutex::new(CancellationToken::new()),
        })
        .setup(|app| {
            // Build tray menu
            let show_item =
                MenuItemBuilder::with_id("show", "Show Window").build(app)?;
            let settings_item =
                MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let clear_item =
                MenuItemBuilder::with_id("clear", "Clear Chat").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&settings_item)
                .item(&clear_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // Build tray icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("Voice Assistant — Hold Shift+Z to talk")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "settings" => {
                        // If settings window already exists, just focus it
                        if let Some(window) = app.get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        } else {
                            // Create a new settings window
                            let url = tauri::WebviewUrl::App("index.html?view=settings".into());
                            let _ = WebviewWindowBuilder::new(app, "settings", url)
                                .title("Voice Assistant — Settings")
                                .inner_size(720.0, 700.0)
                                .min_inner_size(600.0, 500.0)
                                .resizable(true)
                                .build();
                        }
                    }
                    "clear" => {
                        let state = app.state::<AppState>();
                        state.messages.lock().unwrap().clear();
                        let _ = app.emit("messages_cleared", ());
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Register global shortcut in Rust so it works when window is hidden
            app.global_shortcut().on_shortcut("Shift+Z", |app, _shortcut, event| {
                match event.state {
                    ShortcutState::Pressed => {
                        // Cancel any running pipeline first
                        {
                            let state = app.state::<AppState>();
                            let mut cancel = state.pipeline_cancel.lock().unwrap();
                            cancel.cancel(); // signal the running pipeline to stop
                            *cancel = CancellationToken::new(); // fresh token for next pipeline
                        }

                        // Tell frontend to stop audio and reset
                        let _ = app.emit("pipeline_interrupted", ());

                        // Show window at bottom-right and start recording
                        if let Some(window) = app.get_webview_window("main") {
                            if let Ok(Some(monitor)) = window.current_monitor() {
                                let screen = monitor.size();
                                let scale = monitor.scale_factor();
                                let win_w = 320.0;
                                let win_h = 400.0;
                                let padding = 20.0;
                                let dock_offset = 60.0; // Taskbar offset for Windows
                                let x = (screen.width as f64 / scale) - win_w - padding;
                                let y = (screen.height as f64 / scale) - win_h - padding - dock_offset;
                                let _ = window.set_position(tauri::PhysicalPosition::new(
                                    (x * scale) as i32,
                                    (y * scale) as i32,
                                ));
                            }
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                        let _ = app.emit("hotkey_pressed", ());

                        // Start recording
                        let state = app.state::<AppState>();
                        let is_rec = *state.is_recording.lock().unwrap();
                        if !is_rec {
                            state.recorded_samples.lock().unwrap().clear();
                            *state.is_recording.lock().unwrap() = true;
                            let app_clone = app.clone();
                            std::thread::spawn(move || {
                                if let Err(e) = voice::record_audio(&app_clone) {
                                    eprintln!("Recording error: {}", e);
                                }
                            });
                        }
                    }
                    ShortcutState::Released => {
                        let _ = app.emit("hotkey_released", ());

                        // Stop recording and process
                        let state = app.state::<AppState>();
                        *state.is_recording.lock().unwrap() = false;

                        // Grab the current cancel token for this pipeline
                        let cancel_token = state.pipeline_cancel.lock().unwrap().clone();

                        let app_clone = app.clone();
                        // Small delay to let recording thread finish, then process
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(100));

                            let state = app_clone.state::<AppState>();
                            let samples = state.recorded_samples.lock().unwrap().clone();
                            let sample_rate = *state.recording_sample_rate.lock().unwrap();
                            let config = state.config.lock().unwrap().clone();

                            if samples.is_empty() {
                                let _ = app_clone.emit("processing", ProcessingState {
                                    stage: "error".to_string(),
                                    text: "No audio recorded".to_string(),
                                });
                                return;
                            }

                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = process_pipeline(app_clone.clone(), samples, sample_rate, config, cancel_token).await {
                                    if e != "interrupted" {
                                        eprintln!("Pipeline error: {}", e);
                                        let _ = app_clone.emit("processing", ProcessingState {
                                            stage: "error".to_string(),
                                            text: e,
                                        });
                                    }
                                }
                            });
                        });
                    }
                }
            })?;

            // Register Shift+X to hide/dismiss the window
            app.global_shortcut().on_shortcut("Shift+X", |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
            })?;

            // Make webview background transparent and hide on launch
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
                let _ = window.hide();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            get_messages,
            clear_messages,
            show_window,
            hide_window,
            ingest_text,
            ingest_file,
            list_knowledge_sources,
            delete_knowledge_source,
            start_recording,
            stop_recording_and_process,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
