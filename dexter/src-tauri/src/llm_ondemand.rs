use crate::AppState;
use tauri::{Emitter, Manager};
use std::sync::Mutex;

// ── Runtime Mode Enum ──

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmRuntimeMode {
    VoiceReady,      // Llama 8B em :8080 activo — estado normal apos boot
    SwappingToText,  // A matar Llama e a subir Qwen 35B em :8084
    TextReady,       // Qwen 35B em :8084 activo — janela chat aberta
    QwenWarm,        // Chat fechado mas Qwen em memoria (warm TTL a contar)
    SwappingToVoice, // A matar Qwen e a repor Llama em :8080
}

impl LlmRuntimeMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::VoiceReady      => "Modo voz (Llama ativo)",
            Self::SwappingToText  => "Carregando modelo de chat...",
            Self::TextReady       => "Modo chat (Qwen carregado)",
            Self::QwenWarm        => "Modo chat (modelo em cache)",
            Self::SwappingToVoice => "Restaurando voz...",
        }
    }
}

// ── Spawn Parameters ──

pub struct LlmSpawnParams {
    pub model_path: String,
    pub port: u16,
    pub ngl: u32,
    pub ctx: u32,
    pub threads: u32,
    pub mlock: bool,
    pub no_mmap: bool,
    pub cpu_moe: u32,              // 0 = omitir --n-cpu-moe
    pub ctx_checkpoints_zero: bool,// true = --ctx-checkpoints 0
    pub flash_attn: bool,
    pub jinja: bool,
    pub host: String,
    pub extra_args: Vec<String>,   // args verbatim extra (ex. KV: ["-ctk","turbo4",…])
}

// ── Server Ready Checks ──

pub async fn is_llm_server_ready(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/v1/models", port);
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(c) => c.get(&url).send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false),
        Err(_) => false,
    }
}

pub async fn is_xtts_server_ready() -> bool {
    let port: u16 = std::env::var("XTTS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8005);
    let url = format!("http://127.0.0.1:{}/health", port);
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(c) => c
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false),
        Err(_) => false,
    }
}

pub async fn is_voice_stack_ready() -> bool {
    is_llm_server_ready(8080).await && is_xtts_server_ready().await
}

fn xtts_startup_timeout() -> std::time::Duration {
    let secs = std::env::var("XTTS_STARTUP_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    std::time::Duration::from_secs(secs)
}

// ── Port Listening Checks ──

/// Verifica se um serviço ainda responde na porta (para kill/wait).
/// XTTS (:8005) usa /health; llama-server usa /v1/models.
async fn is_port_listening(port: u16) -> bool {
    match port {
        8005 => is_xtts_server_ready().await,
        8080 | 8084 => is_llm_server_ready(port).await,
        _ => is_llm_server_ready(port).await,
    }
}

async fn wait_until_port_closed(port: u16, timeout: std::time::Duration) {
    let t0 = std::time::Instant::now();
    while t0.elapsed() < timeout {
        if !is_port_listening(port).await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    eprintln!("[LLM] timeout aguardando porta :{} fechar", port);
}

// ── Kill Process Listening On Port (Windows) ──

const LLM_KILL_PORTS: [u16; 3] = [8080, 8084, 8005]; // nunca 8081/8082/8083

fn kill_process_listening_on_port(port: u16) -> Result<(), String> {
    if !LLM_KILL_PORTS.contains(&port) {
        return Err(format!("Porta :{} nao esta na lista de portas permitidas para kill", port));
    }

    eprintln!("[LLM] kill por porta :{}", port);

    // Use netstat to find PID listening on the port, then taskkill
    let output = std::process::Command::new("cmd")
        .args(["/C", &format!("netstat -ano | findstr :{} | findstr LISTENING", port)])
        .output()
        .map_err(|e| format!("Falha ao executar netstat: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut killed = false;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(pid_str) = parts.last() {
            if let Ok(pid) = pid_str.parse::<u32>() {
                if pid == 0 || pid == 4 {
                    // System / kernel — skip
                    continue;
                }
                eprintln!("[LLM] taskkill /PID {} /F /T (escuta na porta :{})", pid, port);
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F", "/T"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                killed = true;
            }
        }
    }

    if killed {
        // Brief wait for the kill to take effect
        std::thread::sleep(std::time::Duration::from_millis(500));
    } else {
        eprintln!("[LLM] Nenhum PID encontrado escutando na porta :{}", port);
    }

    Ok(())
}

// ── Kill Service Helpers (Child + Fallback por Porta) ──

async fn kill_service_on_port(state: &AppState, port: u16, child_slot: fn(&AppState) -> &Mutex<Option<std::process::Child>>) {
    // 1) Child Rust
    if let Some(mut c) = child_slot(state).lock().unwrap().take() {
        eprintln!("[LLM] Encerrando (child) :{}", port);
        let _ = c.kill();
        let _ = c.wait();
    }
    // 2) Fallback: processo iniciado pelo PS1 (boot)
    if is_port_listening(port).await {
        if let Err(e) = kill_process_listening_on_port(port) {
            eprintln!("[LLM] kill por porta :{} falhou: {}", port, e);
        }
        wait_until_port_closed(port, std::time::Duration::from_secs(10)).await;
    }
}

pub async fn kill_voice_llm_async(state: &AppState) {
    kill_service_on_port(state, 8080, |s| &s.voice_llm_child).await;
}

pub async fn kill_text_llm_async(state: &AppState) {
    kill_service_on_port(state, 8084, |s| &s.text_llm_child).await;
}

pub async fn kill_xtts_server_async(state: &AppState) {
    kill_service_on_port(state, 8005, |s| &s.xtts_server_child).await;
}

// ── Spawn LLM Server ──

fn spawn_llm_server(llama_exe: &str, p: &LlmSpawnParams) -> Result<std::process::Child, String> {
    let mut args: Vec<String> = vec![
        "-m".into(), p.model_path.clone(),
        "--port".into(), p.port.to_string(),
        "--host".into(), p.host.clone(),
        "-ngl".into(), p.ngl.to_string(),
        "-c".into(), p.ctx.to_string(),
        "-t".into(), p.threads.to_string(),
    ];
    if p.mlock  { args.push("--mlock".into()); }
    if p.no_mmap{ args.push("--no-mmap".into()); }
    if p.cpu_moe > 0 {
        args.extend(["--n-cpu-moe".into(), p.cpu_moe.to_string()]);
    }
    if p.ctx_checkpoints_zero {
        args.extend(["--ctx-checkpoints".into(), "0".into()]);
    }
    if p.flash_attn { args.push("--flash-attn".into()); args.push("on".into()); }
    if p.jinja      { args.push("--jinja".into()); }
    args.extend(p.extra_args.iter().cloned());

    std::process::Command::new(llama_exe)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Falha spawn llama-server :{}: {}", p.port, e))
}

// ── Ensure XTTS Server ──

pub async fn ensure_xtts_server(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    // Se ja esta a responder, reutilizar
    if is_xtts_server_ready().await {
        return Ok(());
    }

    let xtts_port: u16 = std::env::var("XTTS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8005);

    // Limpar child orfao
    if let Some(mut c) = state.xtts_server_child.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }

    // Porta ocupada mas /health falha (processo PS1 zumbi ou crash)
    if is_port_listening(xtts_port).await {
        eprintln!("[LLM] Porta :{} ocupada sem /health — libertando...", xtts_port);
        if let Err(e) = kill_process_listening_on_port(xtts_port) {
            eprintln!("[LLM] kill por porta :{} falhou: {}", xtts_port, e);
        }
        wait_until_port_closed(xtts_port, std::time::Duration::from_secs(15)).await;
    }

    let xtts_path = std::env::var("XTTS_SERVER_PATH")
        .unwrap_or_else(|_| r"C:\llama.cpp\xtts-api-server\main.py".into());
    let xtts_python = std::env::var("XTTS_PYTHON_EXE")
        .unwrap_or_else(|_| "python".into());
    let xtts_device = std::env::var("DEXTER_TTS_INFER_DEVICE")
        .unwrap_or_else(|_| "cuda".into());

    eprintln!("[LLM] Subindo XTTS :{} | device={}", xtts_port, xtts_device);
    let child = std::process::Command::new(&xtts_python)
        .args([&xtts_path, "--port", &xtts_port.to_string(), "--device", &xtts_device])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Falha spawn XTTS :{}: {}", xtts_port, e))?;

    *state.xtts_server_child.lock().unwrap() = Some(child);

    // Aguardar ate responder (primeira carga CUDA pode levar 1–3 min)
    let t0 = std::time::Instant::now();
    let timeout = xtts_startup_timeout();
    while t0.elapsed() < timeout {
        if is_xtts_server_ready().await {
            eprintln!("[perf] xtts_start | elapsed_s={:.1}", t0.elapsed().as_secs_f32());
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    kill_xtts_server_async(&state).await;
    Err(format!(
        "XTTS nao respondeu apos {}s em :{}",
        timeout.as_secs(),
        xtts_port
    ))
}

// ── Ensure Voice LLM (Llama 8B :8080) ──

fn cancel_warm_kill_timer(state: &AppState) {
    state.warm_kill_token.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(h) = state.warm_kill_handle.lock().unwrap().take() {
        h.abort();
    }
}

async fn spawn_voice_llm_server(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    let llama_exe = std::env::var("LLAMA_SERVER_EXE").unwrap_or_else(|_| {
        r"C:\llama.cpp\llama-cpp-turboquant\build\bin\Release\llama-server.exe".into()
    });
    let model_path = std::env::var("LLM_VOICE_MODEL_PATH")
        .map_err(|_| "LLM_VOICE_MODEL_PATH nao definido (execute teste.ps1 primeiro)".to_string())?;

    let ngl: u32 = std::env::var("LLM_VOICE_NGL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(28);
    let ctx: u32 = std::env::var("LLM_VOICE_CTX")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8192);
    let threads: u32 = std::env::var("LLM_VOICE_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let mlock = std::env::var("LLM_VOICE_MLOCK")
        .map(|v| v == "1")
        .unwrap_or(true);
    let no_mmap = std::env::var("LLM_VOICE_NO_MMAP")
        .map(|v| v == "1")
        .unwrap_or(true);

    let p = LlmSpawnParams {
        model_path,
        port: 8080,
        ngl,
        ctx,
        threads,
        mlock,
        no_mmap,
        cpu_moe: 0,
        ctx_checkpoints_zero: false,
        flash_attn: false,
        jinja: false,
        host: "127.0.0.1".into(),
        extra_args: vec![],
    };
    eprintln!("[LLM] Subindo Llama :8080 | ngl={} ctx={} t={}", ngl, ctx, threads);
    let t0 = std::time::Instant::now();
    let child = spawn_llm_server(&llama_exe, &p)?;
    *state.voice_llm_child.lock().unwrap() = Some(child);

    let timeout = std::time::Duration::from_secs(120);
    while t0.elapsed() < timeout {
        if is_llm_server_ready(8080).await {
            eprintln!(
                "[perf] llm_voice_start | elapsed_s={:.1}",
                t0.elapsed().as_secs_f32()
            );
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    kill_voice_llm_async(&state).await;
    Err("Llama nao respondeu apos 120s em :8080".into())
}

/// Mata Qwen, repoe XTTS + Llama e emite eventos de swap para a UI.
pub async fn restore_voice_stack(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _swap_guard = state.llm_swap_lock.lock().await;

    cancel_warm_kill_timer(&state);

    if is_llm_server_ready(8080).await && is_xtts_server_ready().await {
        *state.llm_mode.lock().unwrap() = LlmRuntimeMode::VoiceReady;
        let _ = app.emit("llm_mode_changed", LlmRuntimeMode::VoiceReady.label());
        let _ = app.emit("llm_swap_done", ());
        return Ok(());
    }

    *state.llm_mode.lock().unwrap() = LlmRuntimeMode::SwappingToVoice;
    let _ = app.emit(
        "llm_mode_changed",
        LlmRuntimeMode::SwappingToVoice.label(),
    );
    let _ = app.emit("llm_swap_started", ());
    let t_swap = std::time::Instant::now();
    eprintln!("[perf] llm_swap_start | target=voice");

    kill_text_llm_async(&state).await;
    if let Err(e) = ensure_xtts_server(app).await {
        let _ = app.emit("llm_swap_failed", e.clone());
        return Err(e);
    }
    spawn_voice_llm_server(app).await.map_err(|e| {
        let _ = app.emit("llm_swap_failed", e.clone());
        e
    })?;

    *state.llm_mode.lock().unwrap() = LlmRuntimeMode::VoiceReady;
    let _ = app.emit("llm_mode_changed", LlmRuntimeMode::VoiceReady.label());
    let _ = app.emit("llm_swap_done", ());
    eprintln!(
        "[perf] llm_swap_ready | target=voice duration_ms={}",
        t_swap.elapsed().as_millis()
    );
    Ok(())
}

pub async fn ensure_voice_llm(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    cancel_warm_kill_timer(&state);

    if is_voice_stack_ready().await {
        *state.llm_mode.lock().unwrap() = LlmRuntimeMode::VoiceReady;
        let _ = app.emit("llm_mode_changed", LlmRuntimeMode::VoiceReady.label());
        return Ok(());
    }

    // Qwen ainda na VRAM → swap completo (mata :8084, repoe XTTS + Llama)
    if is_llm_server_ready(8084).await {
        return restore_voice_stack(app).await;
    }

    let mode = state.llm_mode.lock().unwrap().clone();
    if matches!(
        mode,
        LlmRuntimeMode::TextReady
            | LlmRuntimeMode::QwenWarm
            | LlmRuntimeMode::SwappingToText
            | LlmRuntimeMode::SwappingToVoice
    ) {
        return restore_voice_stack(app).await;
    }

    // Llama morto mas sem Qwen (ex.: apos falha parcial) — repor XTTS + Llama com UI
    restore_voice_stack(app).await
}

/// Garante Llama :8080 + XTTS :8005 antes do pipeline de voz (atalho e stop_recording).
pub async fn ensure_voice_stack_ready(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mode = state.llm_mode.lock().unwrap().clone();
    if mode == LlmRuntimeMode::TextReady {
        return Err("Modo chat ativo — feche o chat para usar a voz.".into());
    }
    if is_voice_stack_ready().await {
        *state.llm_mode.lock().unwrap() = LlmRuntimeMode::VoiceReady;
        let _ = app.emit("llm_mode_changed", LlmRuntimeMode::VoiceReady.label());
        return Ok(());
    }
    ensure_voice_llm(app).await
}

// ── Ensure Text LLM (Qwen 35B :8084) ──

pub async fn ensure_text_llm(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _swap_guard = state.llm_swap_lock.lock().await;

    if state.is_chat_streaming.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Aguarde a resposta terminar antes de trocar de modo.".into());
    }

    // QwenWarm: cancelar timer e reutilizar processo
    let is_warm = *state.llm_mode.lock().unwrap() == LlmRuntimeMode::QwenWarm;
    if is_warm {
        if let Some(h) = state.warm_kill_handle.lock().unwrap().take() { h.abort(); }
        if is_llm_server_ready(8084).await {
            *state.llm_mode.lock().unwrap() = LlmRuntimeMode::TextReady;
            let _ = app.emit("llm_mode_changed", LlmRuntimeMode::TextReady.label());
            return Ok(());
        }
    }

    if is_llm_server_ready(8084).await {
        *state.llm_mode.lock().unwrap() = LlmRuntimeMode::TextReady;
        let _ = app.emit("llm_mode_changed", LlmRuntimeMode::TextReady.label());
        return Ok(());
    }

    *state.llm_mode.lock().unwrap() = LlmRuntimeMode::SwappingToText;
    let _ = app.emit("llm_mode_changed", LlmRuntimeMode::SwappingToText.label());
    let _ = app.emit("llm_swap_started", ());
    let t_swap = std::time::Instant::now(); // metrica de swap
    eprintln!("[perf] llm_swap_start | target=text");

    // Abortar pipeline de voz antes de libertar VRAM
    state.pipeline_cancel.lock().unwrap().cancel();
    *state.pipeline_cancel.lock().unwrap() = tokio_util::sync::CancellationToken::new();

    kill_voice_llm_async(&state).await; // exclusao mutua de VRAM (Child ou PS1)
    kill_xtts_server_async(&state).await; // libertar VRAM do XTTS para o Qwen

    let llama_exe = std::env::var("LLAMA_SERVER_EXE").unwrap_or_else(|_|
        r"C:\llama.cpp\llama-cpp-turboquant\build\bin\Release\llama-server.exe".into());
    let model_path = std::env::var("LLM_TEXT_MODEL_PATH")
        .map_err(|_| "LLM_TEXT_MODEL_PATH nao definido (execute teste.ps1 primeiro)".to_string())?;

    let ngl:u32      = std::env::var("LLM_TEXT_NGL").ok().and_then(|s| s.parse().ok()).unwrap_or(99);
    let ctx:u32      = std::env::var("LLM_TEXT_CTX").ok().and_then(|s| s.parse().ok()).unwrap_or(16384);
    let threads:u32  = std::env::var("LLM_TEXT_THREADS").ok().and_then(|s| s.parse().ok()).unwrap_or(6);
    let mlock        = std::env::var("LLM_TEXT_MLOCK").map(|v| v == "1").unwrap_or(true);
    let no_mmap      = std::env::var("LLM_TEXT_NO_MMAP").map(|v| v == "1").unwrap_or(true);
    let cpu_moe:u32  = std::env::var("LLM_CPU_MOE").ok().and_then(|s| s.parse().ok()).unwrap_or(33);
    let ctx_ckpts    = std::env::var("LLM_TEXT_CTX_CHECKPOINTS").map(|v| v == "0").unwrap_or(true);

    let p = LlmSpawnParams {
        model_path, port: 8084, ngl, ctx, threads, mlock, no_mmap,
        cpu_moe, ctx_checkpoints_zero: ctx_ckpts, flash_attn: true, jinja: true,
        host: "127.0.0.1".into(),
        extra_args: vec!["-ctk".into(),"turbo4".into(),"-ctv".into(),"turbo3".into()],
    };
    eprintln!("[LLM] Subindo Qwen :8084 | ngl={} ctx={} t={} cpu_moe={}", ngl, ctx, threads, cpu_moe);
    let t0 = std::time::Instant::now();

    let child = spawn_llm_server(&llama_exe, &p).map_err(|e| {
        let _ = app.emit("llm_swap_failed", e.clone()); e
    })?;
    eprintln!("[LLM] Qwen spawn OK | pid={}", child.id());
    *state.text_llm_child.lock().unwrap() = Some(child);

    let timeout = std::time::Duration::from_secs(120);
    while t0.elapsed() < timeout {
        if is_llm_server_ready(8084).await {
            let elapsed_ms = t_swap.elapsed().as_millis();
            eprintln!("[perf] llm_swap_ready | target=text duration_ms={}", elapsed_ms);
            *state.llm_mode.lock().unwrap() = LlmRuntimeMode::TextReady;
            let _ = app.emit("llm_mode_changed", LlmRuntimeMode::TextReady.label());
            let _ = app.emit("llm_swap_done", ());
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    // ── Recovery garantido: nao deixar maquina sem voz nem chat ──
    kill_text_llm_async(&state).await;
    *state.llm_mode.lock().unwrap() = LlmRuntimeMode::SwappingToVoice;
    let _ = app.emit("llm_mode_changed", LlmRuntimeMode::SwappingToVoice.label());
    // Repor stack de voz (XTTS → Llama); erros so logam
    if let Err(e) = ensure_xtts_server(app).await {
        eprintln!("[LLM] recovery pos-timeout Qwen: XTTS falhou: {}", e);
    }
    if let Err(e) = ensure_voice_llm(app).await {
        eprintln!("[LLM] recovery pos-timeout Qwen: Llama falhou: {}", e);
    }
    let e = "Qwen nao respondeu apos 120s em :8084".to_string();
    let _ = app.emit("llm_swap_failed", e.clone());
    Err(e)
}

// ── Restore Voice LLM After Chat ──

pub fn restore_voice_llm_after_chat(app: tauri::AppHandle) {
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = restore_voice_stack(&app2).await {
            eprintln!("[LLM] restore apos fechar chat: {}", e);
        }
    });
}

// ── Schedule Helper for open_chat_window ──

pub fn schedule_ensure_text_llm(app: &tauri::AppHandle) {
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = ensure_text_llm(&app2).await {
            eprintln!("[LLM] ensure_text_llm falhou: {}", e);
            let _ = app2.emit("llm_swap_failed", e);
        }
    });
}
