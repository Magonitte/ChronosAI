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
                "description": "Pesquisa a base de conhecimento local do usuário por informações relevantes. Use quando a pergunta puder estar em documentos ou anotações armazenados.",
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

    if tools_config.screenshot {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "take_screenshot",
                "description": "Captura a tela do usuário e descreve o que está visível. Use quando perguntarem o que há na tela, pedirem para olhar algo ou quiserem ajuda com o que estão vendo.",
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

    if tools_config.read_clipboard {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_clipboard",
                "description": "Lê o texto atual da área de transferência do usuário. Use quando disserem que copiaram algo ou perguntarem o que há copiado.",
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
                "description": "Abre uma URL no navegador padrão. NÃO use YouTube ou Spotify para tocar uma música PELO NOME quando play_music_query estiver disponível — essa ferramenta busca arquivos locais primeiro. Use open_url para sites em geral, documentos, mapas, etc.",
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

    if tools_config.get_current_time {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Obtém a data, a hora e o dia da semana atuais. Use quando perguntarem que horas são ou que dia é.",
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
                "description": "Lista os aplicativos em execução no PC. Use quando perguntarem quais programas estão abertos ou rodando.",
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
                "description": "Baixa uma página web e devolve o texto. Use para ler artigos, documentação, verificar um site ou obter informação atual da internet.",
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

    if tools_config.run_command {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Executa um comando PowerShell no PC e devolve a saída. Para abrir/fechar apps pré-definidos (Cursor, VS Code, Terminal, navegadores, Office etc.), prefira launch_desktop_app ou close_desktop_app. Para música PELO NOME use play_music_query; para embaralhar a biblioteca inteira use native_music_library_shuffle_play (sem varredura de disco); para playlist M3U por artista use play_local_music_playlist só quando fizer sentido; NUNCA use play_full_local_music_library salvo se o usuário pediu explicitamente um M3U gigante exportado (exige explicit_m3u_export_request true). Para play/pause/pular/volume do que já está tocando, prefira control_media_playback e adjust_system_volume em vez de simular teclas pelo PowerShell.",
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

    if tools_config.launch_desktop_app {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "launch_desktop_app",
                "description": "Abre um aplicativo de desktop pré-definido no Windows. Prefira isso a run_command quando pedirem para abrir Cursor, VS Code, Terminal do Windows, Chrome, Edge, Discord, OBS, Ferramenta de Captura, Groove/reprodutor de mídia, Excel, Word, PowerPoint ou Outlook. Para fechar, use close_desktop_app.",
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
                            "description": "Id do aplicativo: cursor; vscode (VS Code); terminal (Terminal do Windows); chrome; edge; discord; obs (OBS Studio); snipping_tool (captura); media_player ou groove (Groove Music); excel; word; powerpoint; outlook."
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
                "description": "Fecha (encerra) um aplicativo pré-definido no Windows parando o processo principal. Mesmos ids que launch_desktop_app. Prefira isto a run_command quando pedirem para fechar ou encerrar esses apps.",
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
                            "description": "Mesmos ids que launch_desktop_app."
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
                "description": "Controla a sessão de mídia ativa no Windows (prioriza Groove Music se houver várias). NÃO toca uma música só PELO TÍTULO — para faixa nomeada use play_music_query primeiro. Para TODAS as faixas de um artista em M3U use play_local_music_playlist (não a biblioteca inteira do PC). Para embaralhar/toda a biblioteca use só native_music_library_shuffle_play (rápido, UI do Reprodutor). play_full_local_music_library só se o usuário pedir export M3U gigante por varredura (explicit_m3u_export_request true). Fluxo: (1) Música específica → play_music_query. (2) Playlist por artista → play_local_music_playlist. (3) Biblioteca inteira → native_music_library_shuffle_play. (4) Export M3U explícito → play_full_local_music_library. (5) Senão launch_desktop_app groove/media_player ou open_url e depois control_media_playback play ou toggle. status mostra título e artista quando disponível.",
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
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "adjust_system_volume",
                "description": "Ajusta o volume principal do Windows com teclas multimídia. Use quando pedirem para aumentar, diminuir ou silenciar o volume do sistema (não controles dentro do app). Cada passo é uma pressionamento de tecla (~2% por passo em setups típicos).",
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
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "play_music_query",
                "description": "Toca uma música pelo título (artista opcional). Use SEMPRE que o usuário disser o nome da faixa — nunca open_url no YouTube para isso. Passo 1: varredura da pasta Música do Windows ([Environment]::MyMusic, pasta Música do perfil, OneDrive Music, Public Music), caminhos das Configurações do Chronos (Pastas de música), variável DEXTER_MUSIC_PATHS, até 200k entradas por raiz, casando pastas e nomes de arquivo. Passo 2: Downloads/Documentos/Desktop com limite menor. Passo 3: YouTube só se não achar local.",
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
                "description": "Grava um M3U e abre — use SÓ quando quiserem várias faixas locais por artista/pasta (ex.: «playlist do Metallica», «todas as músicas do Linkin Park»). Mesmas regras de play_music_query (palavras em caminhos/nomes). NUNCA para «biblioteca inteira do PC», «todas as minhas músicas», embaralhar tudo — isso DEVE ser native_music_library_shuffle_play (botão «Ordem aleatoria e reproduzir» no player). Frases de biblioteca inteira aqui são redirecionadas ao shuffle nativo. Não é para streaming.",
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
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "play_full_local_music_library",
                "description": "DESATIVADO salvo exportação explícita: LENTO — varredura completa do disco gerando M3U gigante. Chame SÓ se o usuário pediu literalmente criar/exportar playlist grande, M3U por varredura ou listar todos os caminhos de áudio (ex.: VLC). Para ouvir ou embaralhar a biblioteca inteira use native_music_library_shuffle_play (sem varredura). É obrigatório explicit_m3u_export_request true ou a ferramenta recusa. Limite de faixas: DEXTER_MUSIC_FULL_PLAYLIST_MAX.",
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
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "native_music_library_shuffle_play",
                "description": "RÁPIDO — preferido para tocar a biblioteca inteira: abre o Reprodutor Multimédia / Groove (sem varredura de disco, sem M3U), depois automação de UI clica em Biblioteca de músicas e no botão de embaralhar tudo com rótulo visível «Ordem aleatoria e reproduzir» (UI em português; às vezes sem acento em aleatoria). Usa a biblioteca indexada do player. Se a automação falhar, o usuário toca uma vez. NÃO é para uma música só (play_music_query), NÃO é M3U por artista (play_local_music_playlist), NÃO é export gigante — isso exige play_full_local_music_library com explicit_m3u_export_request true.",
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
