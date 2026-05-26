use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    webview::WebviewWindowBuilder,
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

mod context_modifier;
mod fast_path;
mod file_tools;
mod llm_ondemand;
mod media_controls;
mod notification_tools;
mod rag;
mod sandbox;
mod system_tools;
mod tools;
mod voice;

use llm_ondemand::{
    ensure_text_llm, ensure_voice_stack_ready, restore_voice_llm_after_chat, schedule_ensure_text_llm,
    LlmRuntimeMode,
};

/// Parallel HTTP TTS synthesis jobs (Coqui/Chatterbox). GPU backends stay at 1 to avoid VRAM contention.
/// Override with `DEXTER_TTS_PARALLEL` (1–8). When unset: CPU inference → 2 slots; CUDA/other → 1.
fn tts_parallel_inference_slots() -> usize {
    if let Ok(raw) = std::env::var("DEXTER_TTS_PARALLEL") {
        if let Ok(n) = raw.trim().parse::<u32>() {
            return (n as usize).clamp(1, 8);
        }
    }
    let mode = std::env::var("DEXTER_TTS_MODE").unwrap_or_default();
    if mode.eq_ignore_ascii_case("windows") {
        return 1;
    }
    let device = std::env::var("DEXTER_TTS_INFER_DEVICE").unwrap_or_default();
    if device.eq_ignore_ascii_case("cpu") {
        2
    } else {
        1
    }
}

#[derive(Clone, Serialize)]
struct ProcessingState {
    stage: String,
    text: String,
}

/// Atualiza o mini chat (bolhas) com o texto que o assistente está falando.
fn emit_voice_speaking_bubble(app: &tauri::AppHandle, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let _ = app.emit(
        "processing",
        ProcessingState {
            stage: "speaking".to_string(),
            text: trimmed.to_string(),
        },
    );
}

#[derive(Clone, Serialize)]
pub(crate) struct AudioChunk {
    index: u32,
    audio: String, // base64 WAV
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default = "current_timestamp_ms")]
    pub created_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub elapsed_ms: Option<u64>,
    /// Preserved tool_calls from assistant messages (OpenAI-compatible format).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<voice::ToolCallOut>>,
    /// Tool call ID for tool result messages (OpenAI-compatible format).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Serialize)]
struct ChatDonePayload {
    response: String,
    elapsed_ms: u64,
}

fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn chat_message(role: &str, content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: role.to_string(),
        content: content.into(),
        created_at_ms: current_timestamp_ms(),
        elapsed_ms: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

fn assistant_message_with_elapsed(content: impl Into<String>, elapsed_ms: u64) -> ChatMessage {
    ChatMessage {
        role: "assistant".to_string(),
        content: content.into(),
        created_at_ms: current_timestamp_ms(),
        elapsed_ms: Some(elapsed_ms),
        tool_calls: None,
        tool_call_id: None,
    }
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
    // Vision server on-demand: child process handle + idle timer
    vision_server_child: Mutex<Option<std::process::Child>>,
    vision_last_used: Mutex<std::time::Instant>,
    // ── LLM on-demand swap ──────────────────────────────────────
    pub voice_llm_child:   Mutex<Option<std::process::Child>>,
    pub text_llm_child:    Mutex<Option<std::process::Child>>,
    pub xtts_server_child: Mutex<Option<std::process::Child>>, // gerido pelo ciclo de swap
    pub llm_mode:          Mutex<LlmRuntimeMode>,
    pub llm_swap_lock:     tokio::sync::Mutex<()>,
    pub is_chat_streaming: std::sync::atomic::AtomicBool,
    pub warm_kill_handle:  Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// Token de geracao: incrementado em cada warm-set; tarefa atrasada
    /// so executa se o token nao mudou — evita "phantom kill".
    pub warm_kill_token:   std::sync::atomic::AtomicU64,
    pub warm_ttl_secs:     u64,
    /// Ultima vez que o chat foi usado (enviou mensagem).
    /// Permite TTL adaptativo: se nunca usado → mata Qwen imediatamente.
    pub warm_last_used:    Mutex<Option<std::time::Instant>>,
    // ── Tier 1: estado adicional ─────────────────────────────────
    pub clipboard_history: notification_tools::ClipboardHistory,
    pub session_notes: Mutex<HashMap<u64, String>>,
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
    // ── Tier 1: novas ferramentas ──────────────────────────────────
    #[serde(default = "default_true")]
    pub write_clipboard: bool,
    #[serde(default = "default_true")]
    pub get_active_window: bool,
    #[serde(default = "default_true")]
    pub system_info: bool,
    #[serde(default = "default_true")]
    pub schedule_notification: bool,
    #[serde(default = "default_true")]
    pub clipboard_history: bool,
    #[serde(default = "default_true")]
    pub search_files: bool,
    #[serde(default = "default_true")]
    pub get_recent_files: bool,
    #[serde(default = "default_true")]
    pub read_file: bool,
    #[serde(default = "default_true")]
    pub write_file: bool,
    // ── Tier 2 ──────────────────────────────────────────────────────────────
    #[serde(default = "default_true")]
    pub manage_processes: bool,
    #[serde(default = "default_true")]
    pub lock_screen: bool,
    #[serde(default = "default_true")]
    pub open_folder: bool,
    #[serde(default)]
    pub set_wallpaper: bool,
    #[serde(default = "default_true")]
    pub get_open_windows: bool,
    #[serde(default = "default_true")]
    pub read_selected_text: bool,
    #[serde(default = "default_true")]
    pub translate_selection: bool,
    #[serde(default = "default_true")]
    pub paste_to_active_window: bool,
    #[serde(default = "default_true")]
    pub toggle_do_not_disturb: bool,
    #[serde(default = "default_true")]
    pub session_notes: bool,
    #[serde(default)]
    pub diff_clipboard: bool,
    #[serde(default)]
    pub ocr_image: bool,
    #[serde(default)]
    pub transcribe_audio_file: bool,
    #[serde(default = "default_true")]
    pub audio_device_switch: bool,
    #[serde(default = "default_true")]
    pub run_powershell_script: bool,
    // ── Tier 3 ──────────────────────────────────────────────────────────────
    #[serde(default = "default_true")]
    pub get_network_info: bool,
    #[serde(default = "default_true")]
    pub take_screenshot_region: bool,
    #[serde(default = "default_true")]
    pub calendar_events: bool,
    #[serde(default)]
    pub send_email: bool,
    #[serde(default = "default_true")]
    pub send_keys: bool,
    #[serde(default)]
    pub watch_file: bool,
    #[serde(default)]
    pub snippet_library: bool,
    #[serde(default = "default_true")]
    pub set_audio_volume_app: bool,
    // ── Tier 4 ──────────────────────────────────────────────────────────────
    #[serde(default)]
    pub ui_automation: bool,
    #[serde(default)]
    pub image_generation: bool,
    #[serde(default)]
    pub disk_cleanup: bool,
}

fn default_true() -> bool {
    true
}

fn default_tts_volume() -> u8 {
    100
}
fn default_temperature() -> f32 {
    0.7
}
fn default_response_style() -> String {
    "normal".to_string()
}
fn default_personality() -> String {
    "default".to_string()
}
fn default_vision_ngl() -> u32 {
    10
}

fn default_shortcut_voice() -> String {
    "Shift+Z".to_string()
}
fn default_shortcut_hide() -> String {
    "Shift+X".to_string()
}
fn default_shortcut_clear() -> String {
    "Shift+C".to_string()
}
fn default_shortcut_chat() -> String {
    "Shift+T".to_string()
}
fn default_shortcut_settings() -> String {
    "Ctrl+Comma".to_string()
}
fn default_shortcut_stop() -> String {
    "Ctrl+5".to_string()
}

/// Cancela pipeline ativo (TTS, LLM, leitura de arquivo) e para o áudio no frontend.
fn interrupt_active_pipeline(app: &tauri::AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut cancel = state.pipeline_cancel.lock().unwrap();
        cancel.cancel();
        *cancel = CancellationToken::new();
        *state.is_recording.lock().unwrap() = false;
        state.recorded_samples.lock().unwrap().clear();
    }
    let _ = app.emit("pipeline_interrupted", ());
}

fn resolved_shortcut(user: &str, fallback: &str) -> String {
    let t = user.trim();
    if t.is_empty() {
        fallback.to_string()
    } else {
        t.to_string()
    }
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
            write_clipboard: true,
            get_active_window: true,
            system_info: true,
            schedule_notification: true,
            clipboard_history: true,
            search_files: true,
            get_recent_files: true,
            read_file: true,
            write_file: true,
            // Tier 2
            manage_processes: true,
            lock_screen: true,
            open_folder: true,
            set_wallpaper: false,
            get_open_windows: true,
            read_selected_text: true,
            translate_selection: true,
            paste_to_active_window: true,
            toggle_do_not_disturb: true,
            session_notes: true,
            diff_clipboard: false,
            ocr_image: false,
            transcribe_audio_file: false,
            audio_device_switch: true,
            run_powershell_script: true,
            // Tier 3
            get_network_info: true,
            take_screenshot_region: true,
            calendar_events: true,
            send_email: false,
            send_keys: true,
            watch_file: false,
            snippet_library: false,
            set_audio_volume_app: true,
            // Tier 4
            ui_automation: false,
            image_generation: false,
            disk_cleanup: false,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    pub whisper_model_path: String,
    #[serde(default = "default_whisper_url")]
    pub whisper_url: String,
    pub llm_url: String,
    /// URL do LLM só para o pipeline de voz (vazio = usa `llm_url`).
    #[serde(default)]
    pub llm_url_voice: String,
    /// URL do LLM só para o chat de texto (vazio = `http://127.0.0.1:8084` ou `LLM_TEXT_PORT`).
    #[serde(default)]
    pub llm_url_text: String,
    #[serde(default)]
    pub embed_url: String,
    #[serde(default = "default_vision_url")]
    pub vision_url: String,
    pub llm_model: String,
    /// Modelo só para voz (vazio = usa `llm_model`).
    #[serde(default)]
    pub llm_model_voice: String,
    /// Modelo só para chat de texto (vazio = usa `llm_model`).
    #[serde(default)]
    pub llm_model_text: String,
    pub embed_model: String,
    #[serde(default)]
    pub vision_model: String,
    #[serde(default = "default_vision_ngl")]
    pub vision_ngl: u32,
    pub chatterbox_url: String,
    pub chatterbox_voice: String,
    #[serde(default = "default_tts_volume")]
    pub tts_volume: u8,
    #[serde(default)]
    pub enable_thinking: bool,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_response_style")]
    pub response_style: String,
    pub system_prompt: String,
    #[serde(default)]
    pub system_prompt_text: String,
    #[serde(default = "default_personality")]
    pub personality: String,
    #[serde(default = "default_true")]
    pub audio_feedback: bool,
    #[serde(default = "default_shortcut_voice")]
    pub shortcut_voice: String,
    #[serde(default = "default_shortcut_hide")]
    pub shortcut_hide: String,
    #[serde(default = "default_shortcut_clear")]
    pub shortcut_clear: String,
    #[serde(default = "default_shortcut_chat")]
    pub shortcut_chat: String,
    #[serde(default = "default_shortcut_settings")]
    pub shortcut_settings: String,
    /// Parar TTS / leitura em voz / pipeline em andamento.
    #[serde(default = "default_shortcut_stop")]
    pub shortcut_stop: String,
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
fn default_vision_url() -> String {
    "http://localhost:8083".to_string()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        let default_whisper = r"J:\Modelos LLM\manifests\registry.ollama.ai\library\whisper\ggml-small.bin".to_string();
        Self {
            whisper_model_path: default_whisper,
            whisper_url: "http://localhost:8081".to_string(),
            llm_url: "http://localhost:8080".to_string(),
            llm_url_voice: String::new(),
            llm_url_text: "http://127.0.0.1:8084".to_string(),
            embed_url: "http://localhost:8082".to_string(),
            vision_url: "http://localhost:8083".to_string(),
            llm_model: "Meta-Llama-3.1-8B-Instruct-Q4_K_M".to_string(),
            llm_model_voice: String::new(),
            llm_model_text: String::new(),
            embed_model: "bge-m3-Q4_K_M".to_string(),
            vision_model: String::new(), // Use llm_model for vision if empty
            vision_ngl: 10,
            chatterbox_url: "http://localhost:8005".to_string(),
            chatterbox_voice: "dexter-ptbr".to_string(),
            tts_volume: 100,
            enable_thinking: false,
            temperature: 0.55,
            response_style: "concise".to_string(),
            system_prompt: "Você é um assistente de voz no desktop do usuário. A conversa é por microfone (Whisper) e resposta falada (TTS).\n\nIMPORTANTE: Responda SEMPRE em português do Brasil. Não misture inglês, espanhol ou outros alfabetos.\n\nMantenha 2-3 frases curtas no máximo. Sem markdown, listas ou blocos de código. Escreva como falaria em voz alta.\n\nNÃO use colchetes com sons ou direções de cena — o TTS lê isso como palavras.\n\nPara perguntas informativas simples, responda do conhecimento sem ferramentas. Quando usar uma ferramenta, diga em uma frase curta o que vai fazer antes de chamá-la.".to_string(),
            system_prompt_text: String::new(),
            personality: "default".to_string(),
            audio_feedback: true,
            shortcut_voice: "Shift+Z".to_string(),
            shortcut_hide: "Shift+X".to_string(),
            shortcut_clear: "Shift+C".to_string(),
            shortcut_chat: "Shift+T".to_string(),
            shortcut_settings: "Ctrl+Comma".to_string(),
            shortcut_stop: "Ctrl+5".to_string(),
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

    pub fn effective_llm_url_voice(&self) -> &str {
        let u = self.llm_url_voice.trim();
        if u.is_empty() {
            self.llm_url.trim()
        } else {
            u
        }
    }

    pub fn effective_llm_model_voice(&self) -> &str {
        let m = self.llm_model_voice.trim();
        if m.is_empty() {
            self.llm_model.trim()
        } else {
            m
        }
    }

    pub fn effective_llm_url_text(&self) -> &str {
        let u = self.llm_url_text.trim();
        if u.is_empty() {
            DEFAULT_TEXT_LLM_URL.as_str()
        } else {
            u
        }
    }

    pub fn effective_llm_model_text(&self) -> &str {
        self.llm_model_text.trim()
    }
}

static DEFAULT_TEXT_LLM_URL: LazyLock<String> = LazyLock::new(default_text_llm_url);

fn default_text_llm_port() -> u16 {
    std::env::var("LLM_TEXT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8084)
}

fn default_text_llm_url() -> String {
    format!("http://127.0.0.1:{}", default_text_llm_port())
}

/// Preenche URL/modelo do chat de texto antes de chamar o LLM (on-demand :8084).
async fn resolve_text_llm_config(config: &mut VoiceConfig) -> Result<(), String> {
    if config.llm_url_text.trim().is_empty() {
        config.llm_url_text = default_text_llm_url();
    }
    if config.llm_model_text.trim().is_empty() {
        let models = fetch_model_ids(&config.llm_url_text).await?;
        let id = models
            .into_iter()
            .next()
            .ok_or_else(|| "Servidor LLM de texto sem modelos em /v1/models".to_string())?;
        config.llm_model_text = id;
    }
    Ok(())
}

// ── Vision Server On-Demand ──

/// Check if the vision server HTTP endpoint is responding.
async fn is_vision_server_ready(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/v1/models", port);
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(client) => match client.get(&url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Ensure the Qwen2.5-VL vision server is running (starts it on-demand if not).
/// Uses reduced -ngl (~10) to coexist with the LLM on 8 GB VRAM.
async fn ensure_vision_server(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();

    // Extract port from vision_url (default 8083)
    let vision_port: u16 = config
        .vision_url
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8083);

    // Already running?
    if is_vision_server_ready(vision_port).await {
        *state.vision_last_used.lock().unwrap() = std::time::Instant::now();
        return Ok(());
    }

    // Kill orphaned child if any
    if let Some(mut child) = state.vision_server_child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }

    // Resolve paths from environment (set by start-all.ps1) with fallbacks
    let llama_exe = std::env::var("LLAMA_SERVER_EXE").unwrap_or_else(|_| {
        r"C:\llama.cpp\llama-cpp-turboquant\build\bin\Release\llama-server.exe".to_string()
    });
    let model_path = std::env::var("VISION_MODEL_PATH")
        .map_err(|_| "VISION_MODEL_PATH não definido (execute start-all.ps1 primeiro)".to_string())?;
    let mmproj_path = std::env::var("VISION_MMPROJ_PATH")
        .map_err(|_| "VISION_MMPROJ_PATH não definido (execute start-all.ps1 primeiro)".to_string())?;

    // -ngl: env var from start-all.ps1 overrides config. Default 0 = CPU-only (zero VRAM).
    let ngl: u32 = std::env::var("VISION_ON_DEMAND_NGL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let cpu_threads: u32 = std::env::var("VISION_CPU_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);

    let ctx: u32 = 4096;

    eprintln!(
        "[Vision] Iniciando servidor on-demand | port={} | ngl={} | cpu_threads={} | ctx={}",
        vision_port, ngl, cpu_threads, ctx
    );

    let vision_start = std::time::Instant::now();

    let child = std::process::Command::new(&llama_exe)
        .args([
            "-m", &model_path,
            "--mmproj", &mmproj_path,
            "--port", &vision_port.to_string(),
            "--host", "127.0.0.1",
            "-ngl", &ngl.to_string(),
            "-c", &ctx.to_string(),
            "-t", &cpu_threads.to_string(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Falha ao spawnar servidor de visão: {}", e))?;

    *state.vision_server_child.lock().unwrap() = Some(child);

    // Wait for the server to be ready
    let timeout = std::time::Duration::from_secs(30);
    let _vision_url = config.vision_url.clone();

    while vision_start.elapsed() < timeout {
        if is_vision_server_ready(vision_port).await {
            let elapsed = vision_start.elapsed().as_secs_f32();
            eprintln!(
                "[perf] vision_server_start | elapsed_s={:.1} | ngl={} | port={}",
                elapsed, ngl, vision_port
            );
            *state.vision_last_used.lock().unwrap() = std::time::Instant::now();
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // Timeout — kill the child we spawned
    if let Some(mut child) = state.vision_server_child.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }

    Err(format!(
        "Servidor de visão não respondeu após {}s na porta {}",
        timeout.as_secs(),
        vision_port
    ))
}

/// Kill the vision server child process (if running).
fn kill_vision_server(state: &AppState) {
    if let Some(mut child) = state.vision_server_child.lock().unwrap().take() {
        eprintln!("[Vision] Encerrando servidor de visão...");
        let _ = child.kill();
        let _ = child.wait();
    }
}

async fn fetch_model_ids(llm_url: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Erro ao criar cliente HTTP: {}", e))?;

    let resp = client
        .get(format!("{}/v1/models", llm_url.trim_end_matches('/')))
        .send()
        .await
        .map_err(|e| format!("Falha ao consultar modelos: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Servidor retornou {}", resp.status()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Resposta inválida: {}", e))?;

    let models = json["data"]
        .as_array()
        .ok_or("Formato de resposta inesperado (esperado 'data' array)")?
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .collect();

    Ok(models)
}

#[tauri::command]
async fn list_models(llm_url: String) -> Result<Vec<String>, String> {
    fetch_model_ids(&llm_url).await
}

#[tauri::command]
fn get_default_config() -> VoiceConfig {
    VoiceConfig::default()
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> VoiceConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn set_config(app: tauri::AppHandle, state: tauri::State<AppState>, config: VoiceConfig) -> Result<(), String> {
    config.save();
    *state.config.lock().unwrap() = config;
    register_global_hotkeys(&app)
}

/// Remove todos os atalhos globais (ex.: durante captura na UI).
#[tauri::command]
fn pause_global_shortcuts(app: tauri::AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| format!("{}", e))
}

/// Volta a registar atalhos conforme a config em memória.
#[tauri::command]
fn resume_global_shortcuts(app: tauri::AppHandle) -> Result<(), String> {
    register_global_hotkeys(&app)
}

#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// Repete métricas `[perf]` do frontend em stderr (mesma consola que `cargo run` / start-all).
#[tauri::command]
fn log_frontend_perf(line: String) {
    eprintln!("{}", line);
}

#[tauri::command]
fn get_messages(state: tauri::State<AppState>) -> Vec<ChatMessage> {
    state.messages.lock().unwrap().clone()
}

#[tauri::command]
fn clear_messages(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    state.messages.lock().unwrap().clear();
    let _ = save_history_internal(&state);
    let _ = app.emit("messages_cleared", ());
}

const HISTORY_FILE: &str = "history.json";

fn history_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("voice-assistant")
        .join(HISTORY_FILE)
}

fn save_history_internal(state: &AppState) -> Result<(), String> {
    let messages = state.messages.lock().unwrap().clone();
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&messages)
        .map_err(|e| format!("Falha ao serializar historico: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Falha ao salvar historico: {}", e))
}

#[tauri::command]
fn save_history(state: tauri::State<AppState>) -> Result<(), String> {
    save_history_internal(&state)
}

#[tauri::command]
fn load_history(state: tauri::State<AppState>) -> Result<Vec<ChatMessage>, String> {
    // Não substituir uma sessão já em memória pelo arquivo: evita perder mensagens recentes
    // (ex.: save falhou, ou load_history conclui depois de novas mensagens na RAM) e corrige
    // o sintoma de "o modelo não lembra do que estávamos falando".
    {
        let existing = state.messages.lock().unwrap();
        if !existing.is_empty() {
            return Ok(existing.clone());
        }
    }

    let path = history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data =
        std::fs::read_to_string(&path).map_err(|e| format!("Falha ao ler historico: {}", e))?;
    let messages: Vec<ChatMessage> = serde_json::from_str(&data)
        .map_err(|e| format!("Falha ao deserializar historico: {}", e))?;
    *state.messages.lock().unwrap() = messages.clone();
    Ok(messages)
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

fn emit_chat_token(app: &tauri::AppHandle, chunk: voice::ChatTokenChunk) {
    let app = app.clone();
    // Não emitir de forma síncrona durante send_chat_message: o invoke bloqueia o JS desta
    // webview e o emit para a mesma janela pode esperar o handler — deadlock (só "Pensando...").
    tauri::async_runtime::spawn(async move {
        if let Some(window) = app.get_webview_window("chat") {
            let _ = window.emit("chat_token", chunk);
        } else {
            let _ = app.emit("chat_token", chunk);
        }
    });
}

fn emit_chat_done(app: &tauri::AppHandle, payload: ChatDonePayload) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(window) = app.get_webview_window("chat") {
            let _ = window.emit("chat_done", payload);
        } else {
            let _ = app.emit("chat_done", payload);
        }
    });
}

/// `set_size` no Tauri/Wry usa **inner size** (área cliente), mas `set_position` usa **outer position**.
/// Com `decorations(true)`, altura interna = altura da work_area faz o contorno externo passar do fundo
/// da área útil e a barra de tarefas cobre a parte inferior — reduzimos a altura interna até caber.
fn clip_chat_window_inner_height_to_work_area(
    window: &tauri::WebviewWindow,
    wa_bottom: i32,
    win_w_px: u32,
) {
    const MIN_INNER_H: u32 = 400;
    for _ in 0..10 {
        let Ok(pos) = window.outer_position() else {
            break;
        };
        let Ok(os) = window.outer_size() else {
            break;
        };
        let bottom = pos.y.saturating_add(os.height as i32);
        if bottom <= wa_bottom {
            break;
        }
        let Ok(is) = window.inner_size() else {
            break;
        };
        let over = (bottom - wa_bottom) as u32;
        let new_h = is.height.saturating_sub(over).max(MIN_INNER_H);
        if new_h >= is.height {
            break;
        }
        let _ = window.set_size(tauri::PhysicalSize::new(win_w_px, new_h));
    }
}

fn bring_chat_to_front(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();

    let window_clone = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        let _ = window_clone.set_always_on_top(false);
    });
}

fn open_chat_window(app: &tauri::AppHandle) {
    schedule_ensure_text_llm(app);

    if let Some(window) = app.get_webview_window("chat") {
        bring_chat_to_front(&window);
        schedule_ensure_text_llm(app); // reabrir chat com QwenWarm ou apos TTL parcial deve re-disparar ensure
        return;
    }

    let url = tauri::WebviewUrl::App("index.html?view=chat".into());
    if let Ok(window) = WebviewWindowBuilder::new(app, "chat", url)
        .title("Chronos - Chat")
        .inner_size(960.0, 720.0)
        .min_inner_size(360.0, 400.0)
        .resizable(true)
        .decorations(true)
        .build()
    {
        // 34% da largura lógica do monitor, alinhado à direita da área útil (work_area):
        // topo colado ao work area; clip pós-show corrige moldura (inner vs outer).
        let mut clip_args: Option<(i32, u32)> = None;
        if let Ok(Some(monitor)) = window.current_monitor() {
            let screen = monitor.size();
            let scale = monitor.scale_factor();
            let wa = monitor.work_area();
            let wa_x = wa.position.x;
            let wa_y = wa.position.y;
            let wa_w = wa.size.width;
            let wa_h = wa.size.height;

            let logical_monitor_w = screen.width as f64 / scale;
            let mut win_w_px = (logical_monitor_w * 0.34 * scale).round() as u32;
            if win_w_px > wa_w {
                win_w_px = wa_w;
            }
            // Primeira tentativa: inner height = work_area (clip corrige título/bordas).
            let win_h_px = wa_h;
            let x_px = wa_x + (wa_w.saturating_sub(win_w_px)) as i32;
            let y_px = wa_y;
            let wa_bottom = wa_y + wa_h as i32;
            let _ = window.set_position(tauri::PhysicalPosition::new(x_px, y_px));
            let _ = window.set_size(tauri::PhysicalSize::new(win_w_px, win_h_px));
            clip_args = Some((wa_bottom, win_w_px));
        }

        bring_chat_to_front(&window);
        if let Some((wa_bottom, win_w_px)) = clip_args {
            clip_chat_window_inner_height_to_work_area(&window, wa_bottom, win_w_px);
        }

        let app_ev = app.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::Destroyed = event {
                restore_voice_llm_after_chat(app_ev.clone());
            }
        });
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
    let embed_url = if config.embed_url.is_empty() {
        &config.llm_url
    } else {
        &config.embed_url
    };
    state
        .rag_store
        .ingest(&source, &text, embed_url, &config.embed_model)
        .await
        .map_err(|e| format!("Falha ao indexar: {}", e))
}

#[tauri::command]
async fn ingest_file(app: tauri::AppHandle, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Falha ao ler arquivo: {}", e))?;
    let source = std::path::Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();
    let embed_url = if config.embed_url.is_empty() {
        &config.llm_url
    } else {
        &config.embed_url
    };
    state
        .rag_store
        .ingest(&source, &text, embed_url, &config.embed_model)
        .await
        .map_err(|e| format!("Falha ao indexar: {}", e))
}

#[tauri::command]
fn list_knowledge_sources(app: tauri::AppHandle) -> Result<Vec<(String, usize)>, String> {
    let state = app.state::<AppState>();
    state
        .rag_store
        .list_sources()
        .map_err(|e| format!("Falha ao listar: {}", e))
}

#[tauri::command]
fn delete_knowledge_source(app: tauri::AppHandle, source: String) -> Result<usize, String> {
    let state = app.state::<AppState>();
    state
        .rag_store
        .delete_source(&source)
        .map_err(|e| format!("Falha ao excluir: {}", e))
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
        return Err("Nenhum áudio gravado".to_string());
    }

    // Process in background
    let cancel_token = state.pipeline_cancel.lock().unwrap().clone();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        {
            let _ = app_handle.emit("processing", ProcessingState {
                stage: "processing".to_string(),
                text: "Preparando modo voz (Llama + XTTS)...".to_string(),
            });
            if let Err(e) = ensure_voice_stack_ready(&app_handle).await {
                let _ = app_handle.emit("processing", ProcessingState {
                    stage: "error".to_string(),
                    text: e,
                });
                return;
            }
        }

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

#[tauri::command]
async fn send_chat_message(app: tauri::AppHandle, text: String) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }

    ensure_text_llm(&app).await.map_err(|e| {
        let _ = app.emit("llm_swap_failed", e.clone()); e
    })?;

    // Registar uso para TTL adaptativo do warm cache
    *app.state::<AppState>().warm_last_used.lock().unwrap() = Some(std::time::Instant::now());

    let _ = app.emit("chat_processing_started", ());
    struct ChatProcessingEnd<'a>(&'a tauri::AppHandle);
    impl Drop for ChatProcessingEnd<'_> {
        fn drop(&mut self) {
            let _ = self.0.emit("chat_processing_ended", ());
            // Limpar flag de streaming
            self.0.state::<AppState>()
                .is_chat_streaming
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let _chat_processing_end = ChatProcessingEnd(&app);

    // Sinalizar streaming activo (bloqueia swap durante resposta)
    app.state::<AppState>()
        .is_chat_streaming
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let mut config = app.state::<AppState>().config.lock().unwrap().clone();
    resolve_text_llm_config(&mut config).await?;

    {
        let state = app.state::<AppState>();
        state.messages.lock().unwrap().push(chat_message("user", text));
    }

    let all_messages = app.state::<AppState>().messages.lock().unwrap().clone();
    let tools = voice::build_tools(&config.tools, &[]);
    let max_tool_rounds = 5;
    let response_started = std::time::Instant::now();

    let (token_tx, mut token_rx) = tokio::sync::mpsc::channel::<voice::ChatTokenChunk>(64);
    let llm_token_tx = token_tx.clone();

    let app_clone = app.clone();
    let config_clone = config.clone();

    let llm_handle = tokio::spawn(async move {
        let mut all_msgs = all_messages;

        for _round in 0..max_tool_rounds {
            let result = voice::chat_streaming_text(
                &config_clone,
                &all_msgs,
                &tools,
                &llm_token_tx,
            )
            .await
            .map_err(|e| format!("LLM: {}", e))?;

            match result {
                voice::ChatStreamResult::Content(text) => {
                    return Ok::<String, String>(text);
                }
                voice::ChatStreamResult::ToolCalls(tool_calls, preamble, xml_parsed) => {
                    if xml_parsed {
                        if !preamble.is_empty() {
                            let assistant_msg = chat_message("assistant", preamble.clone());
                            all_msgs.push(assistant_msg.clone());
                            app_clone
                                .state::<AppState>()
                                .messages
                                .lock()
                                .unwrap()
                                .push(assistant_msg);
                        }

                        let mut tool_results = String::new();
                        for tool_call in &tool_calls {
                            let _ = app_clone.emit(
                                "processing",
                                ProcessingState {
                                    stage: "tool_call".to_string(),
                                    text: tool_call.name.clone(),
                                },
                            );
                            let result_text = execute_tool(&app_clone, &config_clone, tool_call, false).await;
                            tool_results.push_str(&format!(
                                "[Resultado da ferramenta {}]: {}\n",
                                tool_call.name, result_text
                            ));
                        }

                        let follow_up = format!(
                            "Resultados das ferramentas:\n\n{}",
                            tool_results.trim()
                        );
                        let tool_summary_msg = chat_message("user", follow_up);
                        all_msgs.push(tool_summary_msg.clone());
                        app_clone
                            .state::<AppState>()
                            .messages
                            .lock()
                            .unwrap()
                            .push(tool_summary_msg);
                    } else {
                        let tool_calls_out: Vec<voice::ToolCallOut> =
                            tool_calls.iter().map(|tc| tc.to_out()).collect();
                        let assistant_msg = ChatMessage {
                            role: "assistant".to_string(),
                            content: preamble.clone(),
                            created_at_ms: current_timestamp_ms(),
                            elapsed_ms: None,
                            tool_calls: Some(tool_calls_out),
                            tool_call_id: None,
                        };
                        all_msgs.push(assistant_msg.clone());
                        app_clone
                            .state::<AppState>()
                            .messages
                            .lock()
                            .unwrap()
                            .push(assistant_msg);

                        for tool_call in &tool_calls {
                            let _ = app_clone.emit(
                                "processing",
                                ProcessingState {
                                    stage: "tool_call".to_string(),
                                    text: tool_call.name.clone(),
                                },
                            );
                            let result_text = execute_tool(&app_clone, &config_clone, tool_call, false).await;
                            let tool_msg = ChatMessage {
                                role: "tool".to_string(),
                                content: result_text,
                                created_at_ms: current_timestamp_ms(),
                                elapsed_ms: None,
                                tool_calls: None,
                                tool_call_id: Some(tool_call.id.clone()),
                            };
                            all_msgs.push(tool_msg.clone());
                            app_clone
                                .state::<AppState>()
                                .messages
                                .lock()
                                .unwrap()
                                .push(tool_msg);
                        }
                    }

                    let _ = app_clone.emit(
                        "processing",
                        ProcessingState {
                            stage: "thinking".to_string(),
                            text: "Pensando...".to_string(),
                        },
                    );
                }
            }
        }

        let result = voice::chat_streaming_text(&config_clone, &all_msgs, &[], &llm_token_tx)
            .await
            .map_err(|e| format!("LLM: {}", e))?;

        match result {
            voice::ChatStreamResult::Content(text) => Ok(text),
            voice::ChatStreamResult::ToolCalls(_, _, _) => {
                Err("Maximo de rodadas de ferramentas atingido".to_string())
            }
        }
    });

    drop(token_tx);

    while let Some(chunk) = token_rx.recv().await {
        emit_chat_token(&app, chunk);
    }

    let response = llm_handle
        .await
        .map_err(|e| format!("Task error: {}", e))?
        .map_err(|e| e)?;
    let elapsed_ms = response_started.elapsed().as_millis() as u64;

    {
        let state = app.state::<AppState>();
        state
            .messages
            .lock()
            .unwrap()
            .push(assistant_message_with_elapsed(response.clone(), elapsed_ms));
    }

    emit_chat_done(
        &app,
        ChatDonePayload {
            response,
            elapsed_ms,
        },
    );

    // Persistir historico a cada resposta
    {
        let state = app.state::<AppState>();
        let _ = save_history_internal(&state);
    }

    Ok(())
}

#[tauri::command]
fn export_conversation(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let messages = app.state::<AppState>().messages.lock().unwrap().clone();
    let mut content = String::from("# Chronos — Conversa Exportada\n\n");

    for msg in &messages {
        let role_label = match msg.role.as_str() {
            "user" => "👤 Você",
            "assistant" => "🤖 Chronos",
            "tool" => "🔧 Ferramenta",
            _ => msg.role.as_str(),
        };

        content.push_str(&format!("## {}\n\n{}\n\n---\n\n", role_label, msg.content));
    }

    std::fs::write(&path, content).map_err(|e| format!("Falha ao salvar: {}", e))?;
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
            text: "Transcrevendo...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    let whisper_url = config.whisper_url.clone();
    let stt_started = std::time::Instant::now();
    let audio_len_s = samples.len() as f32 / sample_rate as f32;
    let audio_bytes_approx = samples.len() * 2; // 16-bit mono
    let transcript = voice::transcribe_audio(&whisper_url, &samples, sample_rate).await
        .map_err(|e| format!("Falha na transcrição: {}", e))?;
    let stt_duration = stt_started.elapsed().as_secs_f32();
    eprintln!(
        "[perf] stt_done | duration_s={:.2} | audio_len_s={:.2} | rt_factor={:.2} | audio_bytes={}",
        stt_duration,
        audio_len_s,
        stt_duration / audio_len_s.max(0.01),
        audio_bytes_approx
    );

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
        return Err("Nenhuma fala detectada".to_string());
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
        app.state::<AppState>()
            .messages
            .lock()
            .unwrap()
            .push(chat_message("user", transcript.clone()));
    }

    // ── Fast-Path Layer: commands simples bypass LLM ──
    let mut embed_url = config.embed_url.clone();
    if embed_url.is_empty() {
        embed_url = config.llm_url.clone();
    }

    let fast_start = std::time::Instant::now();
    let fast_result = fast_path::fast_path_match(&transcript, &embed_url).await;

    match fast_result {
        fast_path::FastPathResult::Hit(action) => {
            let tool_name = action.tool_name.clone();
            eprintln!(
                "[perf] fast_path_hit | tool={} | vision={} | transcript=\"{}\"",
                tool_name, action.needs_vision, transcript
            );

            if action.needs_vision {
                // ── Fast-path visao: screenshot → VLM direto (sem LLM) ──
                // Spawn screenshot em paralelo com ensure_vision_server
                let vision_url = if config.vision_url.is_empty() {
                    config.llm_url.clone()
                } else {
                    config.vision_url.clone()
                };
                let vision_model = if config.vision_model.is_empty() {
                    "qwen2.5-vl-3b-instruct".to_string()
                } else {
                    config.vision_model.clone()
                };

                let question = action
                    .tool_args
                    .get("question")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Descreva a tela de forma curta e objetiva.")
                    .to_string();

                let screenshot_b64 = tools::take_screenshot(None)
                    .map_err(|e| format!("Falha ao capturar tela: {}", e))?;

                if let Err(e) = ensure_vision_server(&app).await {
                    return Err(format!("Falha ao iniciar servidor de visao: {}", e));
                }

                let vision_intent = tools::classify_vision_intent(&question);
                let max_tokens = tools::max_tokens_for_intent(vision_intent);
                let description = tools::describe_screenshot(
                    &vision_url,
                    &vision_model,
                    &screenshot_b64,
                    &question,
                    max_tokens,
                )
                .await
                .map_err(|e| format!("Falha na descricao da tela: {}", e))?;

                // Atualizar timestamp do servidor de visao
                *app.state::<AppState>().vision_last_used.lock().unwrap() =
                    std::time::Instant::now();

                let is_complex = fast_path::is_complex_vision_query(&transcript);
                if is_complex {
                    // Query complexa: VLM descreveu → envia para LLM raciocinar (sem tools)
                    let follow_up = format!(
                        "O usuario perguntou: \"{}\"\n\nA tela mostra: {}\n\nResponda de forma concisa em portugues.",
                        transcript, description
                    );
                    {
                        app.state::<AppState>()
                            .messages
                            .lock()
                            .unwrap()
                            .push(chat_message("user", follow_up.clone()));
                    }
                    let msgs = app.state::<AppState>().messages.lock().unwrap().clone();
                    let (sentence_tx, mut sentence_rx) =
                        tokio::sync::mpsc::channel::<String>(6);
                    let _app_clone = app.clone();
                    let config_clone = config.clone();
                    let _cancel_llm = cancel.clone();
                    let sentence_tx_clone = sentence_tx.clone();

                    let llm_task = tokio::spawn(async move {
                        voice::chat_streaming(&config_clone, &msgs, &[], &sentence_tx_clone, 0)
                            .await
                            .map_err(|e| format!("Falha no LLM: {}", e))
                    });
                    drop(sentence_tx);

                    let tts_semaphore = Arc::new(Semaphore::new(tts_parallel_inference_slots()));
                    let mut sentence_index: u32 = 0;
                    let mut full_response = String::new();

                    while let Some(sentence) = sentence_rx.recv().await {
                        if cancel.is_cancelled() {
                            break;
                        }
                        full_response.push_str(&sentence);
                        full_response.push(' ');
                        emit_voice_speaking_bubble(&app, full_response.trim());
                        let idx = sentence_index;
                        sentence_index += 1;
                        let permit = tts_semaphore.clone().acquire_owned().await.unwrap();
                        let app_c = app.clone();
                        let cfg_c = config.clone();
                        let cancel_c = cancel.clone();
                        tokio::spawn(async move {
                            let _p = permit;
                            if cancel_c.is_cancelled() {
                                return;
                            }
                            if let Ok(audio) = voice::synthesize(&cfg_c, &sentence, idx).await {
                                app_c
                                    .emit("play_audio_chunk", AudioChunk { index: idx, audio })
                                    .ok();
                            }
                        });
                    }

                    if cancel.is_cancelled() {
                        llm_task.abort();
                        return Err("interrupted".to_string());
                    }
                    match llm_task.await {
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => return Err(e),
                        Err(_) => return Err("LLM task failed".to_string()),
                    }

                    app.emit("play_audio_done", sentence_index)
                        .map_err(|e: tauri::Error| e.to_string())?;
                    app.state::<AppState>()
                        .messages
                        .lock()
                        .unwrap()
                        .push(chat_message("assistant", full_response));
                    {
                        let s = app.state::<AppState>();
                        let _ = save_history_internal(&s);
                    }
                } else {
                    // Query simples: VLM respondeu direto → TTS
                    let cleaned = voice::strip_paralinguistic_brackets(&description);
                    let tts_text = if cleaned.trim().is_empty() {
                        "Nao foi possivel descrever a tela.".to_string()
                    } else {
                        cleaned
                    };

                    emit_voice_speaking_bubble(&app, &tts_text);

                    // Chunk long TTS text to avoid Chatterbox timeout
                    {
                        let mut chunk_idx: u32 = 0;
                        let mut remaining: &str = &tts_text;
                        while !remaining.is_empty() {
                            let chunk = if let Some(pos) = voice::find_tts_chunk_end(remaining) {
                                let c = remaining[..pos].trim().to_string();
                                remaining = &remaining[pos..];
                                c
                            } else {
                                let c = remaining.trim().to_string();
                                remaining = "";
                                c
                            };
                            if !chunk.is_empty() {
                                match voice::synthesize(&config, &chunk, chunk_idx).await {
                                    Ok(audio) => {
                                        app.emit("play_audio_chunk", AudioChunk { index: chunk_idx, audio })
                                            .map_err(|e: tauri::Error| e.to_string())?;
                                    }
                                    Err(e) => {
                                        eprintln!("TTS fast-path vision chunk {} failed: {}", chunk_idx, e);
                                    }
                                }
                                chunk_idx += 1;
                            }
                        }
                        app.emit("play_audio_done", chunk_idx)
                            .map_err(|e: tauri::Error| e.to_string())?;
                    }

                    app.state::<AppState>()
                        .messages
                        .lock()
                        .unwrap()
                        .push(chat_message("assistant", tts_text));
                    {
                        let s = app.state::<AppState>();
                        let _ = save_history_internal(&s);
                    }
                }

                let fast_elapsed = fast_start.elapsed().as_millis();
                eprintln!(
                    "[perf] fast_path_vision_done | elapsed_ms={} | tool={} | complex={}",
                    fast_elapsed, tool_name, is_complex
                );
                return Ok(());
            } else {
                // ── Fast-path comando simples: executar tool + TTS ──
                app.emit(
                    "processing",
                    ProcessingState {
                        stage: "executing".to_string(),
                        text: format!("Executando: {}", tool_name),
                    },
                )
                .map_err(|e: tauri::Error| e.to_string())?;

                let (tts_text, raw_tool_result) = if action.needs_llm_formatting {
                    // Executar tool e usar resultado bruto no TTS
                    let tool_call = voice::ToolCall {
                        id: "fp_0".to_string(),
                        name: tool_name.clone(),
                        arguments: {
                            let mut m = std::collections::HashMap::new();
                            if let Some(obj) = action.tool_args.as_object() {
                                for (k, v) in obj {
                                    if v.is_string() || v.is_boolean() || v.is_number() {
                                        m.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                            m
                        },
                    };
                    let file_path_arg = action
                        .tool_args
                        .get("path")
                        .and_then(|v| v.as_str());
                    let result = execute_tool(&app, &config, &tool_call, true).await;
                    let spoken = if tool_name == "write_file" {
                        voice::spoken_write_file_result(&tool_name, &result)
                    } else if tool_name == "read_file" {
                        voice::spoken_read_file_result(&tool_name, &result, file_path_arg)
                    } else if tool_name == "search_files" {
                        voice::spoken_search_files_result(&result)
                    } else if tool_name == "translate_selection" {
                        let target_lang = action
                            .tool_args
                            .get("target_language")
                            .and_then(|v| v.as_str());
                        let target =
                            system_tools::resolve_translate_target(target_lang, None);
                        voice::spoken_translate_tts(&result, &target)
                    } else if result.starts_with("Não foi possível")
                        || result.starts_with("Faltou")
                        || result.starts_with("Erro")
                        || result.starts_with("Não encontrei")
                    {
                        result.clone()
                    } else {
                        action
                            .tts_template
                            .replace("{result}", &result)
                            .replace("{apps}", &result)
                            .replace("{status}", &result)
                            .replace("{time}", &result)
                            .replace("{date}", &result)
                            .replace("{state}", &result)
                    };
                    (spoken, Some(result))
                } else {
                    // Template with optional placeholders (e.g. {time}/{date} for get_current_time)
                    let tool_call = voice::ToolCall {
                        id: "fp_0".to_string(),
                        name: tool_name.clone(),
                        arguments: {
                            let mut m = std::collections::HashMap::new();
                            if let Some(obj) = action.tool_args.as_object() {
                                for (k, v) in obj {
                                    if v.is_string() || v.is_boolean() || v.is_number() {
                                        m.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                            m
                        },
                    };
                    let tool_result = execute_tool(&app, &config, &tool_call, true).await;
                    let spoken = if tool_name == "get_current_time" {
                        let (time_part, date_part) =
                            tools::split_datetime_for_templates(&tool_result);
                        action
                            .tts_template
                            .replace("{time}", &time_part)
                            .replace("{date}", &date_part)
                    } else if tool_name == "write_file" {
                        voice::spoken_write_file_result(&tool_name, &tool_result)
                    } else {
                        action.tts_template.clone()
                    };
                    (spoken, None)
                };

                let cleaned = voice::strip_paralinguistic_brackets(&tts_text);
                let bubble_text = if tool_name == "translate_selection" {
                    if let Some(ref result) = raw_tool_result {
                        if result.starts_with("translate_selection:")
                            || result.starts_with("Erro")
                            || result.starts_with("Faltou")
                            || result.contains("Nenhum texto")
                        {
                            cleaned.clone()
                        } else {
                            voice::read_aloud_history_preview(&voice::translation_body(result))
                        }
                    } else {
                        cleaned.clone()
                    }
                } else if tool_name == "read_file" && !cleaned.starts_with("Não") {
                    voice::read_aloud_history_preview(&cleaned)
                } else {
                    cleaned.clone()
                };
                emit_voice_speaking_bubble(&app, &bubble_text);
                let read_aloud = if tool_name == "read_file" {
                    true
                } else if tool_name == "translate_selection" {
                    let target_lang = action
                        .tool_args
                        .get("target_language")
                        .and_then(|v| v.as_str());
                    system_tools::resolve_translate_target(target_lang, None)
                        .voice_reads_translation_aloud()
                } else {
                    false
                };
                voice::emit_chunked_tts_cancellable(&app, &config, &cancel, &cleaned, read_aloud)
                    .await?;

                if cancel.is_cancelled() {
                    return Err("interrupted".to_string());
                }

                app.state::<AppState>()
                    .messages
                    .lock()
                    .unwrap()
                    .push(chat_message("assistant", &bubble_text));
                {
                    let s = app.state::<AppState>();
                    let _ = save_history_internal(&s);
                }

                let fast_elapsed = fast_start.elapsed().as_millis();
                eprintln!(
                    "[perf] fast_path_done | elapsed_ms={} | tool={} | tts=\"{}\"",
                    fast_elapsed, tool_name,
                    &cleaned.chars().take(60).collect::<String>()
                );
                return Ok(());
            }
        }
        fast_path::FastPathResult::Miss => {
            eprintln!(
                "[perf] fast_path_miss | transcript=\"{}\" | elapsed_us={}",
                transcript,
                fast_start.elapsed().as_micros()
            );
        }
    }

    // Stage 2: LLM with tool calling → streaming TTS
    app.emit(
        "processing",
        ProcessingState {
            stage: "thinking".to_string(),
            text: "Pensando...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    // ── ContextModifier: inject extra instructions based on transcript + clipboard ──
    let mut config = config;
    {
        let needs_clip = {
            let t = transcript.to_lowercase();
            t.contains("erro") || t.contains("error") || t.contains("código")
                || t.contains("codigo") || t.contains("debug") || t.contains("explica")
                || t.contains("corrig")
        };
        let clipboard_ctx = if needs_clip {
            tools::read_clipboard().unwrap_or_default()
        } else {
            String::new()
        };
        let ctx_modifiers = context_modifier::detect_modifiers(&transcript, &clipboard_ctx);
        for m in &ctx_modifiers {
            let extra = context_modifier::modifier_to_prompt(m);
            if !extra.is_empty() {
                config.system_prompt.push('\n');
                config.system_prompt.push('\n');
                config.system_prompt.push_str(extra);
            }
        }
    }

    // ── ToolCategory routing: seleciona tools relevantes para o transcript ────
    let tool_categories = voice::detect_tool_categories(&transcript);
    let all_tools = voice::build_tools(&config.tools, &tool_categories);
    let attach_tools = voice::should_attach_voice_tools(&transcript);
    let all_messages = if attach_tools {
        eprintln!(
            "[voice] tools_context=fresh | transcript=\"{}\"",
            transcript.chars().take(80).collect::<String>()
        );
        vec![chat_message("user", transcript.clone())]
    } else {
        app.state::<AppState>().messages.lock().unwrap().clone()
    };
    let voice_tools: Vec<serde_json::Value> = if attach_tools {
        all_tools
    } else {
        eprintln!(
            "[voice] tools=off (resposta direta) | transcript=\"{}\"",
            transcript.chars().take(100).collect::<String>()
        );
        Vec::new()
    };
    // Voice: 2 rounds is enough (tool batch → short confirmation). More rounds made the LLM
    // call play_music_query / open_url repeatedly and restart playback each time.
    let max_tool_rounds = if voice_tools.is_empty() { 1 } else { 2 };

    // Single streaming loop: stream with tools → if model returns tool calls,
    // execute them and stream again. If it returns content, sentences flow to TTS.
    let (sentence_tx, mut sentence_rx) = tokio::sync::mpsc::channel::<String>(6);
    let mut sentence_index: u32 = 0;
    let mut full_text = String::new();
    let llm_started = std::time::Instant::now();

    let app_clone = app.clone();
    let config_clone = config.clone();
    let cancel_llm = cancel.clone();

    let llm_handle = {
        let voice_tools = voice_tools.clone();
        let sentence_tx = sentence_tx.clone();
        let app = app_clone.clone();
        let config = config_clone.clone();

        tokio::spawn(async move {
            let mut all_msgs = all_messages;
            let mut executed_voice_tools: HashMap<String, String> = HashMap::new();

            for _round in 0..max_tool_rounds {
                if cancel_llm.is_cancelled() { return Err("interrupted".to_string()); }

                let result = tokio::select! {
                    _ = cancel_llm.cancelled() => { return Err("interrupted".to_string()); }
                    r = voice::chat_streaming(&config, &all_msgs, &voice_tools, &sentence_tx, _round) => {
                        r.map_err(|e| format!("Falha no LLM: {}", e))?
                    }
                };

                match result {
                    voice::StreamResult::Content(text) => {
                        return Ok::<String, String>(text);
                    }
                    voice::StreamResult::ToolCalls(tool_calls, preamble, xml_parsed) => {
                        if cancel_llm.is_cancelled() { return Err("interrupted".to_string()); }

                        let mut playback_started = false;

                        if xml_parsed {
                            // XML-parsed tool calls: model emitted XML as text.
                            if !preamble.is_empty() {
                                let m = chat_message("assistant", preamble.clone());
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

                                let result_text = execute_voice_tool_deduped(
                                    &mut executed_voice_tools,
                                    &app,
                                    &config,
                                    tool_call,
                                    _round,
                                )
                                .await;
                                if voice_playback_tool_succeeded(&tool_call.name, &tool_call.arguments, &result_text) {
                                    playback_started = true;
                                }
                                tool_results.push_str(&format!(
                                    "[Resultado da ferramenta {}]: {}\n",
                                    tool_call.name, result_text
                                ));
                            }

                            let follow_up = format!(
                                "Resultados das ferramentas para esta resposta:\n\n{}",
                                tool_results.trim()
                            );
                            let um = chat_message("user", follow_up);
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
                                created_at_ms: current_timestamp_ms(),
                                elapsed_ms: None,
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

                                let result_text = execute_voice_tool_deduped(
                                    &mut executed_voice_tools,
                                    &app,
                                    &config,
                                    tool_call,
                                    _round,
                                )
                                .await;
                                if voice_playback_tool_succeeded(&tool_call.name, &tool_call.arguments, &result_text) {
                                    playback_started = true;
                                }

                                let tool_msg = ChatMessage {
                                    role: "tool".to_string(),
                                    content: result_text,
                                    created_at_ms: current_timestamp_ms(),
                                    elapsed_ms: None,
                                    tool_calls: None,
                                    tool_call_id: Some(tool_call.id.clone()),
                                };
                                all_msgs.push(tool_msg.clone());
                                app.state::<AppState>().messages.lock().unwrap().push(tool_msg);
                            }
                        }

                        if playback_started {
                            eprintln!(
                                "[voice] tool_round_stop | reason=playback_started | round={}",
                                _round
                            );
                            break;
                        }

                        let _ = app.emit(
                            "processing",
                            ProcessingState {
                                stage: "thinking".to_string(),
                                text: "Pensando...".to_string(),
                            },
                        );
                    }
                }
            }

            // Hit max rounds — do one final stream without tools
            if cancel_llm.is_cancelled() { return Err("interrupted".to_string()); }

            let result = voice::chat_streaming(&config, &all_msgs, &[], &sentence_tx, max_tool_rounds)
                .await
                        .map_err(|e| format!("Falha no LLM: {}", e))?;

            match result {
                voice::StreamResult::Content(text) => Ok(text),
                voice::StreamResult::ToolCalls(_, _, _) => Err("O modelo solicitou ferramentas após o número máximo de rodadas".to_string()),
            }
        })
    };

    // Drop our copy of sentence_tx so the channel closes when the spawned task finishes
    drop(sentence_tx);

    // Process sentences as they arrive from the stream → TTS → audio
    // Semaphore limits concurrent HTTP TTS calls (GPU: 1; CPU: 2 by default — see tts_parallel_inference_slots).
    let tts_semaphore = Arc::new(Semaphore::new(tts_parallel_inference_slots()));

    while let Some(sentence) = sentence_rx.recv().await {
        if cancel.is_cancelled() { break; }

        eprintln!(
            "[perf] tts_dequeue | seq={} | elapsed_ms={}",
            sentence_index,
            pipeline_started.elapsed().as_millis()
        );

        full_text.push_str(&sentence);
        full_text.push(' ');

        emit_voice_speaking_bubble(&app, full_text.trim());

        let current_index = sentence_index;
        sentence_index += 1;

        let app = app.clone();
        let config = config.clone();
        let cancel = cancel.clone();
        let sem = tts_semaphore.clone();

        // Acquire a TTS slot before spawning — back‑pressure + bounded parallelism
        // (see tts_parallel_inference_slots).
        let permit = tokio::select! {
            _ = cancel.cancelled() => { break; }
            p = sem.acquire_owned() => p.unwrap()
        };

        tokio::spawn(async move {
            let _permit = permit; // held until synthesis completes

            if cancel.is_cancelled() { return; }

            let tts_started = std::time::Instant::now();
            let tts_result = tokio::select! {
                _ = cancel.cancelled() => { return; }
                r = voice::synthesize(&config, &sentence, current_index) => r
            };

            match tts_result {
                Ok(audio_base64) => {
                    eprintln!(
                        "[perf] TTS chunk {} finished in {:.2}s",
                        current_index,
                        tts_started.elapsed().as_secs_f32()
                    );
                    // Note: first_audio event is logged by the frontend (TTFS).
                    if cancel.is_cancelled() { return; }
                    app.emit("play_audio_chunk", AudioChunk {
                        index: current_index,
                        audio: audio_base64,
                    }).ok();
                }
                Err(e) => {
                    eprintln!("TTS failed for chunk {}: {}", current_index, e);
                }
            }
        });
    }

    if cancel.is_cancelled() {
        llm_handle.abort(); // kill the LLM task
        return Err("interrupted".to_string());
    }

    let full_response = llm_handle
        .await
        .map_err(|e| format!("Falha na tarefa do LLM: {}", e))?
        .map_err(|e| e)?;
    eprintln!("[perf] LLM stream finished in {:.2}s", llm_started.elapsed().as_secs_f32());

    app.emit("play_audio_done", sentence_index)
        .map_err(|e: tauri::Error| e.to_string())?;

    // Add assistant message to history
    app.state::<AppState>()
        .messages
        .lock()
        .unwrap()
        .push(chat_message("assistant", full_response));

    // Persistir historico a cada resposta (modo voz)
    {
        let state = app.state::<AppState>();
        let _ = save_history_internal(&state);
    }

    Ok(())
}

/// Execute a single tool call and return the result text.
fn json_opt_u64(
    arguments: &std::collections::HashMap<String, serde_json::Value>,
    key: &str,
) -> Option<u64> {
    match arguments.get(key)? {
        serde_json::Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok())),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

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

fn voice_tool_dedup_key(tool_call: &voice::ToolCall) -> String {
    let mut pairs: Vec<(String, serde_json::Value)> = tool_call
        .arguments
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let args = serde_json::to_string(&pairs).unwrap_or_default();
    format!("{}:{}", tool_call.name, args)
}

fn voice_tool_result_failed(result: &str) -> bool {
    let r = result.trim();
    r.is_empty()
        || r.starts_with("Faltou")
        || r.starts_with("Não foi possível")
        || r.starts_with("Não encontrei")
        || r.starts_with("Erro")
        || r.contains("ferramenta desconhecida")
}

fn voice_playback_tool_succeeded(
    name: &str,
    args: &std::collections::HashMap<String, serde_json::Value>,
    result: &str,
) -> bool {
    if voice_tool_result_failed(result) {
        return false;
    }
    match name {
        "play_music_query" | "play_local_music_playlist" => true,
        "open_url" => {
            result.contains("YouTube")
                || result.contains("youtube")
                || result.contains("Tocando")
                || result.contains("Abri o")
                || result.contains("Abri ")
        }
        "control_media_playback" => args
            .get("action")
            .and_then(|v| v.as_str())
            .map(|a| a.eq_ignore_ascii_case("play"))
            .unwrap_or(false),
        _ => false,
    }
}

async fn execute_voice_tool_deduped(
    executed: &mut HashMap<String, String>,
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    tool_call: &voice::ToolCall,
    round: usize,
) -> String {
    let key = voice_tool_dedup_key(tool_call);
    if let Some(cached) = executed.get(&key) {
        eprintln!(
            "[voice] tool_dedup_skip | round={} | name={}",
            round, tool_call.name
        );
        return cached.clone();
    }
    eprintln!(
        "[voice] tool_exec | round={} | name={}",
        round, tool_call.name
    );
    let result = execute_tool(app, config, tool_call, true).await;
    let preview: String = result.chars().take(160).collect();
    eprintln!(
        "[voice] tool_result | round={} | name={} | ok={} | result=\"{}\"",
        round,
        tool_call.name,
        !voice_tool_result_failed(&result),
        preview
    );
    executed.insert(key, result.clone());
    result
}

async fn execute_tool(
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    tool_call: &voice::ToolCall,
    voice_compact: bool,
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

            let embed_url = if config.embed_url.is_empty() {
                &config.llm_url
            } else {
                &config.embed_url
            };
            let results = rag_store
                .search(&query, embed_url, &config.embed_model, 5)
                .await
                .unwrap_or_default();

            if results.is_empty() {
                "Nenhum resultado relevante na base de conhecimento.".to_string()
            } else {
                results.iter().enumerate()
                    .map(|(i, r)| format!("[{}] (fonte: {}, relevância: {:.2})\n{}", i + 1, r.source, r.score, r.text))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        }
        "take_screenshot" => {
            let question = tool_call.arguments.get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("Descreva em detalhe o que você vê nesta tela.")
                .to_string();
            let monitor = tool_call.arguments.get("monitor")
                .and_then(|v| v.as_u64()).map(|n| n as u32);

            let _ = app.emit("processing", ProcessingState {
                stage: "thinking".to_string(),
                text: "Olhando sua tela...".to_string(),
            });

            match tools::take_screenshot_region(monitor, None, None, None, None, Some(&question)) {
                Ok(image_b64) => {
                    // Garantir que o servidor de visão está rodando (modo on-demand)
                    if let Err(e) = ensure_vision_server(app).await {
                        return format!("Erro ao iniciar servidor de visão: {}", e);
                    }
                    let vision_url = if config.vision_url.is_empty() {
                        &config.llm_url
                    } else {
                        &config.vision_url
                    };
                    let vision_model = if config.vision_model.is_empty() {
                        "qwen2.5-vl-3b-instruct"
                    } else {
                        &config.vision_model
                    };
                    let vision_intent = tools::classify_vision_intent(&question);
                    let max_tokens = tools::max_tokens_for_intent(vision_intent);
                    match tools::describe_screenshot(vision_url, vision_model, &image_b64, &question, max_tokens).await {
                        Ok(desc) => {
                            // Atualizar timestamp de uso do servidor de visão
                            *app.state::<AppState>().vision_last_used.lock().unwrap() =
                                std::time::Instant::now();
                            desc
                        }
                        Err(e) => format!("Captura feita, mas o modelo de visão falhou: {}. Confirme se o servidor de visão está rodando e o modelo aceita imagens (multimodal).", e),
                    }
                }
                Err(e) => format!("Falha ao capturar a tela: {}", e),
            }
        }
        "read_clipboard" => match tools::read_clipboard() {
            Ok(text) => {
                if text.trim().is_empty() {
                    "A área de transferência está vazia.".to_string()
                } else {
                    // Registrar no histórico do clipboard
                    app.state::<AppState>().clipboard_history.push(text.clone());
                    format!("Conteúdo da área de transferência:\n{}", text)
                }
            }
            Err(e) => format!("Falha ao ler a área de transferência: {}", e),
        },
        "open_url" => {
            let url = tool_call.arguments.get("url")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if url.is_empty() {
                "Nenhuma URL informada.".to_string()
            } else if let Some(search_q) = tools::youtube_search_query_from_open_url(&url) {
                eprintln!(
                    "[voice] open_url_redirect | youtube_search -> play_music_query | q=\"{}\"",
                    search_q.chars().take(80).collect::<String>()
                );
                let _ = app.emit(
                    "processing",
                    ProcessingState {
                        stage: "thinking".to_string(),
                        text: format!("Procurando no YouTube: {}", search_q),
                    },
                );
                match tools::play_music_query(&search_q, None, music_paths_opt, true, false).await {
                    Ok(msg) => msg,
                    Err(e) => format!("Não foi possível abrir a música: {}", e),
                }
            } else {
                match tools::open_url(&url) {
                    Ok(msg) => msg,
                    Err(e) => format!("Falha ao abrir URL: {}", e),
                }
            }
        }
        "get_current_time" => tools::get_current_time(),
        "list_running_apps" => match tools::list_running_apps() {
            Ok(apps) => format!("Aplicativos em execução no momento:\n{}", apps),
            Err(e) => format!("Falha ao listar aplicativos: {}", e),
        },
        "fetch_fx_quote" => {
            let pair = tool_call
                .arguments
                .get("pair")
                .and_then(|v| v.as_str())
                .unwrap_or("USD-BRL");
            if voice_compact {
                match tools::fetch_fx_quote_voice(pair).await {
                    Ok(msg) => msg,
                    Err(e) => format!("Não foi possível obter cotação: {}", e),
                }
            } else {
                match tools::fetch_fx_quote(pair).await {
                    Ok(msg) => msg,
                    Err(e) => format!("Não foi possível obter cotação: {}", e),
                }
            }
        }
        "fetch_weather" => {
            let location = tool_call
                .arguments
                .get("location")
                .and_then(|v| v.as_str());
            let day_offset = tool_call
                .arguments
                .get("day_offset")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            if voice_compact {
                match tools::fetch_weather_voice(location, day_offset).await {
                    Ok(msg) => msg,
                    Err(e) => format!("Não foi possível obter o clima: {}", e),
                }
            } else {
                match tools::fetch_weather(location, day_offset).await {
                    Ok(msg) => msg,
                    Err(e) => format!("Não foi possível obter o clima: {}", e),
                }
            }
        }
        "web_fetch" => {
            let url = tool_call.arguments.get("url")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if url.is_empty() {
                "Nenhuma URL informada.".to_string()
            } else {
                let lower = url.to_lowercase();
                if lower.contains("dolar")
                    || lower.contains("dólar")
                    || lower.contains("usd-brl")
                    || lower.contains("iene")
                    || lower.contains("yen")
                    || lower.contains("jpy")
                    || lower.contains("euro")
                    || lower.contains("eur-brl")
                    || lower.contains("cotacao")
                    || lower.contains("cotação")
                {
                    let pair = if lower.contains("iene") || lower.contains("yen") || lower.contains("jpy") {
                        "JPY-BRL"
                    } else if lower.contains("euro") || lower.contains("eur") {
                        "EUR-BRL"
                    } else {
                        "USD-BRL"
                    };
                    eprintln!("[voice] web_fetch_redirect | fx_quote | url=\"{}\"", url);
                    match tools::fetch_fx_quote(pair).await {
                        Ok(msg) => msg,
                        Err(e) => format!("Não foi possível obter cotação: {}", e),
                    }
                } else {
                    match tools::web_fetch(&url).await {
                        Ok(text) => text,
                        Err(e) => format!("Falha ao buscar {}: {}", url, e),
                    }
                }
            }
        }
        "run_command" => {
            let command = tool_call.arguments.get("command")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if command.is_empty() {
                "Nenhum comando informado.".to_string()
            } else {
                let _ = app.emit("processing", ProcessingState {
                    stage: "thinking".to_string(),
                    text: format!("Executando: {}", command),
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
                "Informe o id do app. Use: cursor, vscode, terminal, chrome, edge, discord, obs, snipping_tool, media_player, groove, excel, word, powerpoint, outlook.".to_string()
            } else {
                let _ = app.emit("processing", ProcessingState {
                    stage: "thinking".to_string(),
                    text: format!("Abrindo {}", app_id),
                });
                match tools::launch_desktop_app(app_id) {
                    Ok(msg) => msg,
                    Err(e) => format!("Falha ao abrir o aplicativo: {}", e),
                }
            }
        }
        "close_desktop_app" => {
            let app_id = tool_call.arguments.get("app")
                .and_then(|v| v.as_str()).unwrap_or("").trim();
            if app_id.is_empty() {
                "Informe o id do app (os mesmos de launch_desktop_app).".to_string()
            } else {
                let _ = app.emit("processing", ProcessingState {
                    stage: "thinking".to_string(),
                    text: format!("Fechando {}", app_id),
                });
                match tools::close_desktop_app(app_id) {
                    Ok(msg) => msg,
                    Err(e) => format!("Falha ao fechar o aplicativo: {}", e),
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
            let prefer_youtube = tool_call
                .arguments
                .get("prefer_youtube")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let prefer_native_player = tool_call
                .arguments
                .get("prefer_native_player")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
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
                match tools::play_music_query(
                    &query,
                    artist_ref,
                    music_paths_opt,
                    prefer_youtube,
                    prefer_native_player,
                )
                .await
                {
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
        // ── Tier 1: System Tools ─────────────────────────────────────────────
        "write_clipboard" => {
            let text = tool_call.arguments.get("text")
                .and_then(|v| v.as_str()).unwrap_or("");
            if text.is_empty() {
                "Faltou o texto a copiar.".to_string()
            } else {
                match system_tools::write_clipboard(text) {
                    Ok(msg) => msg,
                    Err(e) => format!("Falha ao escrever no clipboard: {}", e),
                }
            }
        }
        "get_active_window" => match system_tools::get_active_window() {
            Ok(info) => info,
            Err(e) => format!("Falha ao obter janela ativa: {}", e),
        },
        "system_info" => {
            let concise = tool_call.arguments.get("concise")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match system_tools::system_info(concise) {
                Ok(info) => info,
                Err(e) => format!("Falha ao obter informações do sistema: {}", e),
            }
        },

        // ── Tier 1: Notification Tools ───────────────────────────────────────
        "schedule_notification" => {
            let message = tool_call
                .arguments
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Lembrete".to_string());
            let delay = json_opt_u64(&tool_call.arguments, "delay_seconds");
            let dt_str = tool_call.arguments.get("datetime")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let sound = tool_call
                .arguments
                .get("sound")
                .and_then(|v| v.as_str())
                .map(notification_tools::ReminderSound::parse)
                .unwrap_or_default();
            let voice_delivery = Some(notification_tools::ReminderVoiceDelivery {
                app: app.clone(),
                config: config.clone(),
            });
            match notification_tools::schedule_notification(
                &message,
                delay,
                dt_str.as_deref(),
                sound,
                voice_delivery,
            )
            .await
            {
                Ok(msg) => msg,
                Err(e) => format!("Falha ao agendar lembrete: {}", e),
            }
        }
        "clipboard_history" => {
            let action = tool_call.arguments.get("action")
                .and_then(|v| v.as_str()).unwrap_or("list");
            let history_ref = &app.state::<AppState>().clipboard_history;
            match action {
                "get" => {
                    let idx = tool_call.arguments.get("index")
                        .and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    match history_ref.get(idx) {
                        Some(text) => format!("Clipboard [{}]: {}", idx, text),
                        None => format!("Índice {} fora do intervalo. O histórico tem {} entrada(s).", idx, history_ref.len()),
                    }
                }
                _ => {
                    let list = history_ref.list();
                    notification_tools::format_clipboard_history(&list)
                }
            }
        }

        // ── Tier 1: File Tools ───────────────────────────────────────────────
        "search_files" => {
            let query = tool_call.arguments.get("query")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            let max = tool_call.arguments.get("max_results")
                .and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            if query.is_empty() {
                "Faltou a consulta de busca.".to_string()
            } else {
                let _ = app.emit("processing", ProcessingState {
                    stage: "thinking".to_string(),
                    text: format!("Buscando arquivos: {}", query),
                });
                let sandbox = config.sandbox.clone();
                match file_tools::search_files(&query, max, &sandbox) {
                    Ok(result) => result,
                    Err(e) => format!("Erro na busca de arquivos: {}", e),
                }
            }
        }
        "get_recent_files" => {
            let max = tool_call.arguments.get("max")
                .and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            match file_tools::get_recent_files(max) {
                Ok(result) => result,
                Err(e) => format!("Erro ao obter arquivos recentes: {}", e),
            }
        }
        "read_file" => {
            let path = tool_call.arguments.get("path")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if path.is_empty() {
                "Faltou o caminho do arquivo.".to_string()
            } else {
                let sandbox = config.sandbox.clone();
                match file_tools::read_file(&path, &sandbox) {
                    Ok(content) => {
                        if voice_compact {
                            voice::prepare_read_aloud_for_tts(&content, Some(&path))
                        } else {
                            content
                        }
                    }
                    Err(e) => format!("Erro ao ler arquivo: {}", e),
                }
            }
        }
        "write_file" => {
            let path = tool_call.arguments.get("path")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            let content = tool_call.arguments.get("content")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            let overwrite = tool_call.arguments.get("overwrite")
                .and_then(|v| v.as_bool()).unwrap_or(false);
            if path.is_empty() {
                "Faltou o caminho do arquivo.".to_string()
            } else {
                let sandbox = config.sandbox.clone();
                match file_tools::write_file(&path, &content, overwrite, &sandbox) {
                    Ok(msg) => msg,
                    Err(e) => format!("Erro ao escrever arquivo: {}", e),
                }
            }
        }
        // calculator é resolvido no fast_path; este arm serve como fallback
        "calculator" => {
            tool_call.arguments.get("result")
                .and_then(|v| v.as_str())
                .map(|r| r.to_string())
                .unwrap_or_else(|| "Resultado não disponível.".to_string())
        }

        // ── Tier 2: System Tools ─────────────────────────────────────────────
        "manage_processes" => {
            let action = tool_call.arguments.get("action")
                .and_then(|v| v.as_str()).unwrap_or("list");
            let name = tool_call.arguments.get("process_name")
                .and_then(|v| v.as_str());
            match system_tools::manage_processes(action, name) {
                Ok(r) => r,
                Err(e) => format!("manage_processes: {}", e),
            }
        }
        "lock_screen" => match system_tools::lock_screen() {
            Ok(r) => r,
            Err(e) => format!("lock_screen: {}", e),
        },
        "open_folder" => {
            let path = tool_call.arguments.get("path")
                .and_then(|v| v.as_str()).unwrap_or("~");
            match system_tools::open_folder(path) {
                Ok(r) => r,
                Err(e) => format!("open_folder: {}", e),
            }
        }
        "set_wallpaper" => {
            let path = tool_call.arguments.get("path")
                .and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                "Faltou o caminho da imagem.".to_string()
            } else {
                match system_tools::set_wallpaper(path) {
                    Ok(r) => r,
                    Err(e) => format!("set_wallpaper: {}", e),
                }
            }
        }
        "get_open_windows" => match system_tools::get_open_windows() {
            Ok(r) => r,
            Err(e) => format!("get_open_windows: {}", e),
        },
        "toggle_do_not_disturb" => match system_tools::toggle_do_not_disturb() {
            Ok(r) => r,
            Err(e) => format!("toggle_do_not_disturb: {}", e),
        },
        "read_selected_text" => {
            let result = system_tools::read_selected_text();
            match result {
                Ok(text) => {
                    // Empurra o texto selecionado para o histórico do clipboard
                    app.state::<AppState>().clipboard_history.push(text.clone());
                    text
                }
                Err(e) => format!("read_selected_text: {}", e),
            }
        }
        "translate_selection" => {
            let source = tool_call
                .arguments
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("auto");
            let target_language = tool_call
                .arguments
                .get("target_language")
                .and_then(|v| v.as_str());
            let llm_url = config.effective_llm_url_voice();
            let llm_model = config.effective_llm_model_voice();
            let target = system_tools::resolve_translate_target(target_language, None);
            match system_tools::translate_selection(
                llm_url,
                llm_model,
                source,
                target_language,
                None,
            )
            .await
            {
                Ok(translated) => {
                    let _ = system_tools::write_clipboard(&translated);
                    app.state::<AppState>()
                        .clipboard_history
                        .push(translated.clone());
                    system_tools::format_translation_result(&target, &translated)
                }
                Err(e) => format!("translate_selection: {}", e),
            }
        }
        "paste_to_active_window" => {
            let text = tool_call.arguments.get("text")
                .and_then(|v| v.as_str()).unwrap_or("");
            if text.is_empty() {
                "Faltou o texto a colar.".to_string()
            } else {
                match system_tools::paste_to_active_window(text) {
                    Ok(r) => r,
                    Err(e) => format!("paste_to_active_window: {}", e),
                }
            }
        }

        // ── Tier 2: Session Notes ────────────────────────────────────────────
        "session_notes" => {
            let action = tool_call.arguments.get("action")
                .and_then(|v| v.as_str()).unwrap_or("list");
            let notes_ref = &app.state::<AppState>().session_notes;
            match action {
                "add" => {
                    let text = tool_call.arguments.get("text")
                        .and_then(|v| v.as_str()).unwrap_or("");
                    notification_tools::session_note_add(notes_ref, text)
                }
                "clear" => notification_tools::session_note_clear(notes_ref),
                _ => notification_tools::session_note_list(notes_ref),
            }
        }

        // ── Tier 2: Diff Clipboard ───────────────────────────────────────────
        "diff_clipboard" => {
            let hist = app.state::<AppState>().clipboard_history.list();
            notification_tools::diff_clipboard(&hist)
        }

        // ── Tier 2: OCR Image ────────────────────────────────────────────────
        "ocr_image" => {
            // Captura a tela e usa o vision endpoint para extrair texto (OCR)
            let _ = app.emit("processing", ProcessingState {
                stage: "thinking".to_string(),
                text: "Lendo texto na tela...".to_string(),
            });
            let ocr_prompt = "Extraia TODO o texto visível nesta imagem, linha por linha, preservando a estrutura. Não descreva a imagem — apenas transcreva o texto.";
            match tools::take_screenshot_region(None, None, None, None, None, Some(ocr_prompt)) {
                Ok(image_b64) => {
                    if let Err(e) = ensure_vision_server(app).await {
                        return format!("Erro ao iniciar servidor de visão: {}", e);
                    }
                    let vision_url = if config.vision_url.is_empty() {
                        &config.llm_url
                    } else {
                        &config.vision_url
                    };
                    let vision_model = if config.vision_model.is_empty() {
                        "qwen2.5-vl-3b-instruct"
                    } else {
                        &config.vision_model
                    };
                    match tools::describe_screenshot(vision_url, vision_model, &image_b64, ocr_prompt, 2048).await {
                        Ok(text) => {
                            *app.state::<AppState>().vision_last_used.lock().unwrap() =
                                std::time::Instant::now();
                            text
                        }
                        Err(e) => format!("OCR falhou: {}", e),
                    }
                }
                Err(e) => format!("Falha ao capturar tela para OCR: {}", e),
            }
        }

        // ── Tier 2: Transcribe Audio File ────────────────────────────────────
        "transcribe_audio_file" => {
            let path = tool_call.arguments.get("path")
                .and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                "Faltou o caminho do arquivo de áudio.".to_string()
            } else {
                let whisper_url = config.whisper_url.clone();
                let sandbox = config.sandbox.clone();
                match file_tools::transcribe_audio_file(path, &whisper_url, &sandbox).await {
                    Ok(text) => text,
                    Err(e) => format!("Transcrição falhou: {}", e),
                }
            }
        }

        // ── Tier 2: Audio Device Switch ──────────────────────────────────────
        "audio_device_switch" => {
            let action = tool_call.arguments.get("action")
                .and_then(|v| v.as_str()).unwrap_or("list");
            match action {
                "switch" => {
                    let device = tool_call.arguments.get("device_name")
                        .and_then(|v| v.as_str()).unwrap_or("");
                    if device.is_empty() {
                        "Faltou device_name para switch.".to_string()
                    } else {
                        match media_controls::switch_audio_device(device) {
                            Ok(r) => r,
                            Err(e) => format!("audio_device_switch: {}", e),
                        }
                    }
                }
                _ => match media_controls::list_audio_devices() {
                    Ok(r) => r,
                    Err(e) => format!("audio_device_switch (list): {}", e),
                },
            }
        }

        // ── Tier 2: Run PowerShell Script ────────────────────────────────────
        "run_powershell_script" => {
            let path = tool_call.arguments.get("path")
                .and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                "Faltou o caminho do script .ps1.".to_string()
            } else {
                let sandbox = config.sandbox.clone();
                let audit_ref = &app.state::<AppState>().audit_log;
                match file_tools::run_powershell_script(path, &sandbox, audit_ref) {
                    Ok(r) => r,
                    Err(e) => format!("run_powershell_script: {}", e),
                }
            }
        }

        // ── Tier 3: Network Info ────────────────────────────────────────────
        "get_network_info" => match system_tools::get_network_info() {
            Ok(r) => r,
            Err(e) => format!("get_network_info: {}", e),
        },

        // ── Tier 3: take_screenshot_region (com coordenadas) ────────────────
        "take_screenshot_region" => {
            let x = tool_call.arguments.get("x").and_then(|v| v.as_u64()).map(|n| n as u32);
            let y = tool_call.arguments.get("y").and_then(|v| v.as_u64()).map(|n| n as u32);
            let w = tool_call.arguments.get("width").and_then(|v| v.as_u64()).map(|n| n as u32);
            let h = tool_call.arguments.get("height").and_then(|v| v.as_u64()).map(|n| n as u32);
            let question = tool_call.arguments.get("question").and_then(|v| v.as_str());
            let _ = app.emit("processing", ProcessingState {
                stage: "thinking".to_string(),
                text: "Capturando região da tela...".to_string(),
            });
            match tools::take_screenshot_region(None, x, y, w, h, question) {
                Ok(image_b64) => {
                    if let Err(e) = ensure_vision_server(app).await {
                        return format!("Erro ao iniciar servidor de visão: {}", e);
                    }
                    let vision_url = if config.vision_url.is_empty() {
                        &config.llm_url
                    } else {
                        &config.vision_url
                    };
                    let vision_model = if config.vision_model.is_empty() {
                        "qwen2.5-vl-3b-instruct"
                    } else {
                        &config.vision_model
                    };
                    let desc_question = question.unwrap_or("Descreva o que você vê nesta região da tela.");
                    match tools::describe_screenshot(vision_url, vision_model, &image_b64, desc_question, 1024).await {
                        Ok(desc) => {
                            *app.state::<AppState>().vision_last_used.lock().unwrap() =
                                std::time::Instant::now();
                            desc
                        }
                        Err(e) => format!("Captura feita, mas visão falhou: {}", e),
                    }
                }
                Err(e) => format!("Falha ao capturar região da tela: {}", e),
            }
        }

        // ── Tier 3: Calendar Events (Outlook) ───────────────────────────────
        "calendar_events" => {
            let days = tool_call.arguments.get("days_ahead")
                .and_then(|v| v.as_u64()).map(|n| n as u32);
            match system_tools::calendar_events(days) {
                Ok(r) => r,
                Err(e) => format!("calendar_events: {}", e),
            }
        }

        // ── Tier 3: Send Email (Outlook) ────────────────────────────────────
        "send_email" => {
            let to = tool_call.arguments.get("to")
                .and_then(|v| v.as_str()).unwrap_or("");
            let subject = tool_call.arguments.get("subject")
                .and_then(|v| v.as_str()).unwrap_or("");
            let body = tool_call.arguments.get("body")
                .and_then(|v| v.as_str()).unwrap_or("");
            if to.is_empty() {
                "Faltou o destinatário (to).".to_string()
            } else {
                match system_tools::send_email(to, subject, body) {
                    Ok(r) => r,
                    Err(e) => format!("send_email: {}", e),
                }
            }
        }

        // ── Tier 3: Send Keys ───────────────────────────────────────────────
        "send_keys" => {
            let keys = tool_call.arguments.get("keys")
                .and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if keys.is_empty() {
                "Faltou a tecla ou texto (keys).".to_string()
            } else {
                match system_tools::send_keys(&keys) {
                    Ok(r) => r,
                    Err(e) => format!("send_keys: {}", e),
                }
            }
        }

        // ── Tier 3: Watch File ──────────────────────────────────────────────
        "watch_file" => {
            let path = tool_call.arguments.get("path")
                .and_then(|v| v.as_str()).unwrap_or("");
            let duration = tool_call.arguments.get("duration_seconds")
                .and_then(|v| v.as_u64()).unwrap_or(60);
            let on_change = tool_call.arguments.get("on_change")
                .and_then(|v| v.as_str()).map(|s| s.to_string());
            if path.is_empty() {
                "Faltou o caminho do arquivo (path).".to_string()
            } else {
                let sandbox = config.sandbox.clone();
                match file_tools::watch_file(path, duration, &sandbox, on_change).await {
                    Ok(r) => r,
                    Err(e) => format!("watch_file: {}", e),
                }
            }
        }

        // ── Tier 3: Snippet Library ─────────────────────────────────────────
        "snippet_library" => {
            let action = tool_call.arguments.get("action")
                .and_then(|v| v.as_str()).unwrap_or("list");
            let rag = &app.state::<AppState>().rag_store;
            match action {
                "save" => {
                    let name = tool_call.arguments.get("name")
                        .and_then(|v| v.as_str()).unwrap_or("");
                    let content = tool_call.arguments.get("content")
                        .and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() || content.is_empty() {
                        "Faltou name ou content para salvar o snippet.".to_string()
                    } else {
                        match rag.save_snippet(name, content) {
                            Ok(()) => format!("Snippet '{}' salvo.", name),
                            Err(e) => format!("Erro ao salvar snippet: {}", e),
                        }
                    }
                }
                "get" => {
                    let name = tool_call.arguments.get("name")
                        .and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() {
                        "Faltou o nome do snippet.".to_string()
                    } else {
                        match rag.get_snippet(name) {
                            Ok(Some(content)) => content,
                            Ok(None) => format!("Snippet '{}' não encontrado.", name),
                            Err(e) => format!("Erro ao obter snippet: {}", e),
                        }
                    }
                }
                "delete" => {
                    let name = tool_call.arguments.get("name")
                        .and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() {
                        "Faltou o nome do snippet.".to_string()
                    } else {
                        match rag.delete_snippet(name) {
                            Ok(n) => format!("{} snippet(s) removido(s).", n),
                            Err(e) => format!("Erro ao remover snippet: {}", e),
                        }
                    }
                }
                _ => {
                    match rag.list_snippets() {
                        Ok(snippets) => {
                            if snippets.is_empty() {
                                "Nenhum snippet salvo.".to_string()
                            } else {
                                let lines: Vec<String> = snippets.iter()
                                    .map(|(name, updated)| format!("• {} (atualizado: {})", name, updated))
                                    .collect();
                                format!("Snippets ({}):\n{}", snippets.len(), lines.join("\n"))
                            }
                        }
                        Err(e) => format!("Erro ao listar snippets: {}", e),
                    }
                }
            }
        }

        // ── Tier 3: set_audio_volume_app ─────────────────────────────────────
        "set_audio_volume_app" => {
            let app_name = tool_call.arguments.get("app_name")
                .and_then(|v| v.as_str()).unwrap_or("");
            let volume = tool_call.arguments.get("volume")
                .and_then(|v| v.as_u64()).unwrap_or(50) as u32;
            if app_name.is_empty() {
                "Faltou o nome do aplicativo (app_name).".to_string()
            } else {
                let app = app_name.to_string();
                match tokio::task::spawn_blocking(move || {
                    media_controls::set_audio_volume_app(&app, volume)
                }).await {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => format!("set_audio_volume_app: {}", e),
                    Err(e) => format!("Erro ao ajustar volume do app: {}", e),
                }
            }
        }
        // ── Tier 4 ──────────────────────────────────────────────────────────
        "disk_cleanup" => {
            let action = tool_call.arguments.get("action")
                .and_then(|v| v.as_str()).unwrap_or("analyze");
            let drive = tool_call.arguments.get("drive")
                .and_then(|v| v.as_str());
            match system_tools::disk_cleanup(action, drive) {
                Ok(r) => r,
                Err(e) => format!("disk_cleanup: {}", e),
            }
        }
        "ui_automation" => {
            let action = tool_call.arguments.get("action")
                .and_then(|v| v.as_str()).unwrap_or("click");
            let x = tool_call.arguments.get("x").and_then(|v| v.as_u64()).map(|n| n as u32);
            let y = tool_call.arguments.get("y").and_then(|v| v.as_u64()).map(|n| n as u32);
            let text = tool_call.arguments.get("text").and_then(|v| v.as_str());
            let direction = tool_call.arguments.get("direction").and_then(|v| v.as_str());
            let amount = tool_call.arguments.get("amount").and_then(|v| v.as_u64()).map(|n| n as u32);
            match system_tools::ui_automation(action, x, y, text, direction, amount) {
                Ok(r) => r,
                Err(e) => format!("ui_automation: {}", e),
            }
        }
        "image_generation" => {
            let prompt = tool_call.arguments.get("prompt")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if prompt.is_empty() {
                "Faltou o prompt para geração de imagem.".to_string()
            } else {
                let neg = tool_call.arguments.get("negative_prompt")
                    .and_then(|v| v.as_str()).map(|s| s.to_string());
                let w = tool_call.arguments.get("width").and_then(|v| v.as_u64()).map(|n| n as u32);
                let h = tool_call.arguments.get("height").and_then(|v| v.as_u64()).map(|n| n as u32);
                let st = tool_call.arguments.get("steps").and_then(|v| v.as_u64()).map(|n| n as u32);
                let url = tool_call.arguments.get("sd_url").and_then(|v| v.as_str());
                let _ = app.emit("processing", ProcessingState {
                    stage: "thinking".to_string(),
                    text: format!("Gerando imagem: {}...", &prompt[..prompt.len().min(60)]),
                });
                match tools::image_generation(&prompt, neg.as_deref(), w, h, st, url).await {
                    Ok(msg) => msg,
                    Err(e) => format!("image_generation: {}", e),
                }
            }
        }

        unknown => format!("Ferramenta desconhecida: {}", unknown),
    }
}

// ────────────────────────────────────────────────────────────────────────────

/// Atualiza atalhos globais conforme `AppState.config` (chamar após startup ou `set_config`).
fn register_global_hotkeys(app: &tauri::AppHandle) -> Result<(), String> {
    let gs = app.global_shortcut();
    gs.unregister_all()
        .map_err(|e| format!("Atalhos: falha ao remover registros antigos: {}", e))?;

    let ks = app.state::<AppState>().config.lock().unwrap().clone();
    let k_voice = resolved_shortcut(&ks.shortcut_voice, "Shift+Z");
    let k_hide = resolved_shortcut(&ks.shortcut_hide, "Shift+X");
    let k_clear = resolved_shortcut(&ks.shortcut_clear, "Shift+C");
    let k_chat = resolved_shortcut(&ks.shortcut_chat, "Shift+T");
    let k_settings = resolved_shortcut(&ks.shortcut_settings, "Ctrl+Comma");
    let k_stop = resolved_shortcut(&ks.shortcut_stop, "Ctrl+5");

    gs.on_shortcut(k_stop.as_str(), |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            interrupt_active_pipeline(app);
        }
    })
    .map_err(|e| format!("Atalho parar: {}", e))?;

    gs.on_shortcut(k_voice.as_str(), |app, _shortcut, event| {
        match event.state {
            ShortcutState::Pressed => {
                if let Some(window) = app.get_webview_window("main") {
                    if let Ok(Some(monitor)) = window.current_monitor() {
                        let screen = monitor.size();
                        let scale = monitor.scale_factor();
                        let win_w = 320.0;
                        let win_h = 400.0;
                        let padding = 20.0;
                        let dock_offset = 60.0;
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

                {
                    let state = app.state::<AppState>();
                    let cfg = state.config.lock().unwrap().clone();
                    voice::play_mic_beep(&cfg);
                }

                {
                    let state = app.state::<AppState>();
                    let config = state.config.lock().unwrap();
                    let tts_mode = std::env::var("DEXTER_TTS_MODE")
                        .unwrap_or_else(|_| "chatterbox".to_string());
                    let tts_slots = tts_parallel_inference_slots();
                    eprintln!(
                        "[perf] pipeline_start | tts_mode={} | tts_parallel_slots={} | tts_max_chars={} | tts_split_comma={} | llm_model={}",
                        tts_mode,
                        tts_slots,
                        voice::tts_max_chunk_chars(),
                        voice::tts_split_on_commas(),
                        config.llm_model
                    );
                }

                let state = app.state::<AppState>();
                *state.is_recording.lock().unwrap() = false;

                let cancel_token = state.pipeline_cancel.lock().unwrap().clone();

                let app_clone = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    let state = app_clone.state::<AppState>();
                    let samples = state.recorded_samples.lock().unwrap().clone();
                    let sample_rate = *state.recording_sample_rate.lock().unwrap();
                    let config = state.config.lock().unwrap().clone();

                    if samples.is_empty() {
                        let _ = app_clone.emit(
                            "processing",
                            ProcessingState {
                                stage: "error".to_string(),
                                text: "Nenhum áudio gravado".to_string(),
                            },
                        );
                        return;
                    }

                    tauri::async_runtime::spawn(async move {
                        let _ = app_clone.emit("processing", ProcessingState {
                            stage: "processing".to_string(),
                            text: "Preparando modo voz (Llama + XTTS)...".to_string(),
                        });
                        if let Err(e) = ensure_voice_stack_ready(&app_clone).await {
                            let _ = app_clone.emit("processing", ProcessingState {
                                stage: "error".to_string(),
                                text: e,
                            });
                            return;
                        }
                        if let Err(e) = process_pipeline(
                            app_clone.clone(),
                            samples,
                            sample_rate,
                            config,
                            cancel_token,
                        )
                        .await
                        {
                            if e != "interrupted" {
                                eprintln!("Pipeline error: {}", e);
                                let _ = app_clone.emit(
                                    "processing",
                                    ProcessingState {
                                        stage: "error".to_string(),
                                        text: e,
                                    },
                                );
                            }
                        }
                    });
                });
            }
        }
    })
    .map_err(|e| format!("Atalho voz: {}", e))?;

    gs.on_shortcut(k_hide.as_str(), |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
        }
    })
    .map_err(|e| format!("Atalho esconder: {}", e))?;

    gs.on_shortcut(k_clear.as_str(), |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            let state = app.state::<AppState>();
            state.messages.lock().unwrap().clear();
            let _ = app.emit("messages_cleared", ());
        }
    })
    .map_err(|e| format!("Atalho limpar: {}", e))?;

    gs.on_shortcut(k_chat.as_str(), |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            open_chat_window(app);
        }
    })
    .map_err(|e| format!("Atalho chat: {}", e))?;

    gs.on_shortcut(k_settings.as_str(), |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            if let Some(window) = app.get_webview_window("settings") {
                let _ = window.show();
                let _ = window.set_focus();
            } else {
                let url = tauri::WebviewUrl::App("index.html?view=settings".into());
                let _ = WebviewWindowBuilder::new(app, "settings", url)
                    .title("Chronos — Configuracoes")
                    .inner_size(720.0, 700.0)
                    .min_inner_size(600.0, 500.0)
                    .resizable(true)
                    .build();
            }
        }
    })
    .map_err(|e| format!("Atalho configuracoes: {}", e))?;

    Ok(())
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
            vision_server_child: Mutex::new(None),
            vision_last_used: Mutex::new(std::time::Instant::now()),
            voice_llm_child:    Mutex::new(None),
            text_llm_child:     Mutex::new(None),
            xtts_server_child:  Mutex::new(None),
            llm_mode:           Mutex::new(LlmRuntimeMode::VoiceReady),
            llm_swap_lock:      tokio::sync::Mutex::new(()),
            is_chat_streaming:  std::sync::atomic::AtomicBool::new(false),
            warm_kill_handle:   Mutex::new(None),
            warm_kill_token:    std::sync::atomic::AtomicU64::new(0),
            warm_ttl_secs:      300,
            warm_last_used:     Mutex::new(None),
            clipboard_history:  notification_tools::ClipboardHistory::new(20),
            session_notes:      Mutex::new(HashMap::new()),
        })
        .setup(|app| {
            let ks = app.state::<AppState>().config.lock().unwrap().clone();
            let k_voice = resolved_shortcut(&ks.shortcut_voice, "Shift+Z");

            // Build tray menu
            let show_item =
                MenuItemBuilder::with_id("show", "Mostrar janela").build(app)?;
            let settings_item =
                MenuItemBuilder::with_id("settings", "Configurações").build(app)?;
            let clear_item =
                MenuItemBuilder::with_id("clear", "Limpar conversa").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Sair").build(app)?;

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
                .tooltip(format!("Chronos — segure {} para falar", k_voice))
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
                                .title("Chronos — Configurações")
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

            register_global_hotkeys(app.handle())?;

            // Vision server idle timeout: desliga após 5 min sem uso
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

                    let state = app_handle.state::<AppState>();
                    let last_used = *state.vision_last_used.lock().unwrap();
                    let idle_secs = last_used.elapsed().as_secs();

                    if idle_secs > 300 {
                        // 5 minutos ocioso → desligar
                        if state.vision_server_child.lock().unwrap().is_some() {
                            eprintln!(
                                "[Vision] Desligando servidor apos {}s de inatividade",
                                idle_secs
                            );
                            kill_vision_server(&state);
                        }
                    }
                }
            });

            // Make webview background transparent and hide on launch
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
                let _ = window.hide();

                // Salvar historico ao fechar a janela principal
                let app_clone = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Destroyed = event {
                        let state = app_clone.state::<AppState>();
                        // Salvar historico
                        let messages = state.messages.lock().unwrap().clone();
                        let path = history_path();
                        if let Some(parent) = path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Ok(json) = serde_json::to_string_pretty(&messages) {
                            let _ = std::fs::write(&path, json);
                        }
                        // Encerrar servidor de visão se estiver rodando
                        kill_vision_server(&state);
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            pause_global_shortcuts,
            resume_global_shortcuts,
            restart_app,
            log_frontend_perf,
            get_default_config,
            list_models,
            send_chat_message,
            export_conversation,
            get_messages,
            clear_messages,
            save_history,
            load_history,
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
