use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{Datelike, Timelike, Weekday};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Crop strategies + Vision intent classification
// ---------------------------------------------------------------------------

pub enum CropStrategy {
    Full,
    Center,
    TextRegion,
    Custom(u32, u32, u32, u32),
}

/// Classificacao semantica da pergunta de visao (para resolucao + max_tokens).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionIntent {
    /// "o que esta na tela" — contexto geral, resolucao baixa
    General,
    /// "o que esta escrito", "leia o texto" — OCR focado
    Focused,
    /// "analise esse codigo", "tem bug?" — precisa de detalhe
    Detailed,
}

/// Classifica a intencao da pergunta de visao.
pub fn classify_vision_intent(text: &str) -> VisionIntent {
    let q = text.to_lowercase();

    // Detailed: perguntas complexas que precisam de raciocinio
    let complex_keywords = &[
        "por que", "explique", "analise", "isso esta certo",
        "tem problema", "o que devo fazer", "como resolver",
        "qual o problema", "bug", "erro nesse codigo",
        "faz sentido", "corrigir", "consertar", "porque",
        "como faz", "me ajuda",
    ];
    if complex_keywords.iter().any(|k| q.contains(k)) {
        return VisionIntent::Detailed;
    }

    // Focused: queries de leitura de texto/OCR
    let focused_keywords = &[
        "texto", "escrito", "ocr", "leia", "mensagem",
        "erro", "código", "codigo",
    ];
    if focused_keywords.iter().any(|k| q.contains(k)) {
        return VisionIntent::Focused;
    }

    VisionIntent::General
}

/// Resolucao maxima baseada na intencao (dimensao maior).
fn resolution_for_intent(intent: VisionIntent) -> u32 {
    match intent {
        VisionIntent::General => 512,
        VisionIntent::Focused => 768,
        VisionIntent::Detailed => 1024,
    }
}

/// max_tokens baseado na intencao (evita respostas longas desnecessarias).
pub fn max_tokens_for_intent(intent: VisionIntent) -> u32 {
    match intent {
        VisionIntent::General => 80,
        VisionIntent::Focused => 120,
        VisionIntent::Detailed => 150,
    }
}

/// Detecta a melhor estrategia de crop com base na pergunta do usuario.
fn detect_crop_strategy(question: &str) -> CropStrategy {
    let q = question.to_lowercase();

    if q.contains("erro") || q.contains("texto") || q.contains("escrito") || q.contains("ocr") {
        return CropStrategy::TextRegion;
    }

    if q.contains("icone") || q.contains("botao") || q.contains("menu") || q.contains("barra") {
        return CropStrategy::Center;
    }

    CropStrategy::Full
}

/// Valida se a area do crop nao e suspeita (< 20% da imagem original).
fn is_crop_valid(crop_w: u32, crop_h: u32, full_w: u32, full_h: u32) -> bool {
    let crop_area = (crop_w as f32) * (crop_h as f32);
    let full_area = (full_w as f32) * (full_h as f32);
    if full_area == 0.0 {
        return false;
    }
    crop_area / full_area > 0.20
}

/// Calcula variancia de luminosidade (proxy para densidade de texto).
fn image_variance(img: &image::DynamicImage) -> f64 {
    let gray = img.to_luma8();
    let pixels: Vec<f64> = gray.pixels().map(|p| p.0[0] as f64).collect();
    let mean = pixels.iter().sum::<f64>() / pixels.len() as f64;
    let variance =
        pixels.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / pixels.len() as f64;
    variance
}

/// Aplica crop strategy a imagem, com fallback para full se area suspeita.
fn apply_crop(img: &image::DynamicImage, strategy: &CropStrategy) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());

    match strategy {
        CropStrategy::Full => img.clone(),
        CropStrategy::Center => {
            let cx = w / 4;
            let cy = h / 4;
            let cw = w / 2;
            let ch = h / 2;
            // Center padrao sempre e > 20% (50% da tela)
            img.crop_imm(cx, cy, cw, ch)
        }
        CropStrategy::TextRegion => {
            let cell_w = w / 3;
            let cell_h = h / 3;
            let mut best_score = 0.0f64;
            let mut best_x = 0u32;
            let mut best_y = 0u32;

            for row in 0..3 {
                for col in 0..3 {
                    let x = col * cell_w;
                    let y = row * cell_h;
                    let cw = cell_w.min(w - x);
                    let ch = cell_h.min(h - y);
                    if cw == 0 || ch == 0 {
                        continue;
                    }
                    let sub = img.crop_imm(x, y, cw, ch);
                    let score = image_variance(&sub);
                    if score > best_score {
                        best_score = score;
                        best_x = x;
                        best_y = y;
                    }
                }
            }

            let margin = (cell_w / 4).min(cell_h / 4);
            let cx = best_x.saturating_sub(margin);
            let cy = best_y.saturating_sub(margin);
            let cw = (cell_w + 2 * margin).min(w - cx);
            let ch = (cell_h + 2 * margin).min(h - cy);

            if cw == 0 || ch == 0 || !is_crop_valid(cw, ch, w, h) {
                eprintln!(
                    "[vision] crop_fallback | reason=area_suspeita | crop={}x{} | full={}x{} | ratio={:.2}",
                    cw, ch, w, h,
                    (cw * ch) as f32 / (w * h) as f32
                );
                return img.clone();
            }
            img.crop_imm(cx, cy, cw, ch)
        }
        CropStrategy::Custom(x, y, cw, ch) => {
            let cx = (*x).min(w.saturating_sub(1));
            let cy = (*y).min(h.saturating_sub(1));
            let ccw = (*cw).min(w.saturating_sub(cx));
            let cch = (*ch).min(h.saturating_sub(cy));
            if ccw > 0 && cch > 0 && is_crop_valid(ccw, cch, w, h) {
                img.crop_imm(cx, cy, ccw, cch)
            } else {
                img.clone()
            }
        }
    }
}

/// Capture the screen on Windows using PowerShell.
/// Returns the screenshot as a base64-encoded JPEG (resized dynamically, JPEG quality 70).
pub fn take_screenshot(monitor: Option<u32>) -> Result<String, String> {
    take_screenshot_region(monitor, None, None, None, None, None)
}

/// Capture the screen or a region on Windows using PowerShell.
/// Now supports intelligent crop strategy and dynamic downscale.
pub fn take_screenshot_region(
    _monitor: Option<u32>,
    region_x: Option<u32>,
    region_y: Option<u32>,
    region_w: Option<u32>,
    region_h: Option<u32>,
    question: Option<&str>,
) -> Result<String, String> {
    let tmp_raw = std::env::temp_dir().join("voice-assistant-screenshot-raw.png");
    let tmp_jpeg = std::env::temp_dir().join("voice-assistant-screenshot.jpg");
    let tmp_crop = std::env::temp_dir().join("voice-assistant-screenshot-crop.jpg");
    let raw_str = tmp_raw.to_string_lossy().replace('\\', "\\\\");
    let _jpeg_str = tmp_jpeg.to_string_lossy().replace('\\', "\\\\");

    // PowerShell script to capture screen and save as PNG
    let ps_script = format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$screen = [System.Windows.Forms.Screen]::PrimaryScreen
$bitmap = New-Object System.Drawing.Bitmap($screen.Bounds.Width, $screen.Bounds.Height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($screen.Bounds.X, $screen.Bounds.Y, 0, 0, $screen.Bounds.Size)
$bitmap.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
"#,
        raw_str
    );

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_script,
        ])
        .status()
        .map_err(|e| format!("Falha ao executar captura de tela: {}", e))?;

    if !status.success() {
        return Err("Falha ao capturar a tela.".to_string());
    }

    let img = image::open(&tmp_raw)
        .map_err(|e| format!("Falha ao abrir a captura: {}", e))?;

    // Determine crop strategy + vision intent
    let vision_intent = if let Some(q) = question {
        classify_vision_intent(q)
    } else {
        VisionIntent::General
    };

    let crop_strategy = if let (Some(x), Some(y), Some(w), Some(h)) =
        (region_x, region_y, region_w, region_h)
    {
        CropStrategy::Custom(x, y, w, h)
    } else if question.is_some() {
        detect_crop_strategy(question.unwrap())
    } else {
        CropStrategy::Full
    };

    // Apply crop (with fallback for suspicious areas)
    let cropped = apply_crop(&img, &crop_strategy);

    // Dynamic downscale based on vision intent (not crop strategy)
    let (w, h) = (cropped.width(), cropped.height());
    let max_dim: u32 = resolution_for_intent(vision_intent);

    let resized = if w > max_dim || h > max_dim {
        if w > h {
            let new_h = (h as f64 * max_dim as f64 / w as f64) as u32;
            cropped.resize(max_dim, new_h, image::imageops::FilterType::Lanczos3)
        } else {
            let new_w = (w as f64 * max_dim as f64 / h as f64) as u32;
            cropped.resize(new_w, max_dim, image::imageops::FilterType::Lanczos3)
        }
    } else {
        cropped
    };

    // Salvar JPEG com qualidade reduzida (70 = ~2x menor que padrao 85%, ainda legivel)
    let mut output_bytes: Vec<u8> = Vec::new();
    {
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output_bytes, 70);
        encoder
            .encode(
                resized.as_bytes(),
                resized.width(),
                resized.height(),
                resized.color(),
            )
            .map_err(|e| format!("Falha ao codificar JPEG: {}", e))?;
    }
    std::fs::write(&tmp_jpeg, &output_bytes)
        .map_err(|e| format!("Falha ao salvar JPEG: {}", e))?;

    let _ = std::fs::remove_file(&tmp_raw);
    let _ = std::fs::remove_file(&tmp_crop);

    let bytes = std::fs::read(&tmp_jpeg)
        .map_err(|e| format!("Falha ao ler JPEG da captura: {}", e))?;
    let _ = std::fs::remove_file(&tmp_jpeg);

    Ok(STANDARD.encode(&bytes))
}

/// Prompt otimizado para respostas de voz curtas do VLM.
pub const VISION_PROMPT_FAST: &str =
    "Descreva a tela de forma curta e objetiva para resposta por voz. \
     Liste apenas os elementos principais: aplicativos abertos, textos relevantes, acoes possiveis. \
     Seja breve. Maximo 3 frases.";

/// Describe a screenshot image using llama.cpp's OpenAI-compatible vision API (Qwen2.5-VL dedicated server).
/// Uses optimized prompt + max_tokens for faster responses.
pub async fn describe_screenshot(
    vision_url: &str,
    model: &str,
    image_b64: &str,
    question: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // OpenAI-compatible vision format with dynamic max_tokens based on intent
    let body = serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "text",
                    "text": question
                },
                {
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:image/jpeg;base64,{}", image_b64)
                    }
                }
            ]
        }],
        "stream": false,
        "max_tokens": max_tokens,
        "temperature": 0.3
    });

    let resp = client
        .post(format!("{}/v1/chat/completions", vision_url))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Falha na requisição de visão: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Erro na API de visão {}: {}", status, text));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Falha ao interpretar resposta de visão: {}", e))?;

    Ok(json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("Não foi possível descrever a captura de tela.")
        .to_string())
}

/// Resultado de uma completion não-streaming.
pub struct LlmCompletion {
    pub content: String,
    pub finish_reason: Option<String>,
}

/// Completions de texto (não-streaming) via API OpenAI-compatível do llama-server.
pub async fn llm_complete(
    llm_url: &str,
    model: &str,
    system_prompt: &str,
    user_message: &str,
    max_tokens: u32,
) -> Result<String, String> {
    llm_complete_detailed(llm_url, model, system_prompt, user_message, max_tokens)
        .await
        .map(|r| r.content)
}

pub async fn llm_complete_detailed(
    llm_url: &str,
    model: &str,
    system_prompt: &str,
    user_message: &str,
    max_tokens: u32,
) -> Result<LlmCompletion, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_message }
        ],
        "stream": false,
        "max_tokens": max_tokens,
        "temperature": 0.2
    });

    let resp = client
        .post(format!("{}/v1/chat/completions", llm_url.trim_end_matches('/')))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Falha na requisição ao LLM: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Erro na API do LLM {}: {}", status, text));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Falha ao interpretar resposta do LLM: {}", e))?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Resposta vazia do modelo.".to_string())?;
    let finish_reason = json["choices"][0]["finish_reason"]
        .as_str()
        .map(str::to_string);

    Ok(LlmCompletion {
        content,
        finish_reason,
    })
}

/// Read the system clipboard text on Windows using PowerShell.
pub fn read_clipboard() -> Result<String, String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "Get-Clipboard -Format Text"])
        .output()
        .map_err(|e| format!("Falha ao ler a área de transferência: {}", e))?;

    if !output.status.success() {
        return Err("Não foi possível ler a área de transferência.".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Área de transferência não é UTF-8 válido: {}", e))
}

fn normalize_search_tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter_map(|w| {
            let t = w.trim_matches(|c: char| !c.is_alphanumeric());
            if t.len() >= 2 {
                Some(t.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn alphanumeric_compact(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Relative path from `root` as one lowercase string (folders + file stem useful when track number only in filename).
fn relative_path_blob_lower(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(os) => os.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Score when this file matches the search; higher wins. Uses filename, full relative path, and compact forms.
fn local_audio_match_score(
    path: &Path,
    root: &Path,
    filename: &str,
    tokens: &[String],
    compact_q: &str,
) -> Option<i32> {
    if tokens.is_empty() && compact_q.len() < 6 {
        return None;
    }

    let fname_lower = filename.to_lowercase();
    let blob = relative_path_blob_lower(path, root);
    let blob_c = alphanumeric_compact(&blob);
    let fname_c = alphanumeric_compact(filename);

    let mut best: i32 = -1;

    if !tokens.is_empty() && tokens.iter().all(|t| fname_lower.contains(t.as_str())) {
        let sc = 850 - (filename.len() as i32 / 35).min(20);
        best = best.max(sc);
    }

    if !tokens.is_empty() && tokens.iter().all(|t| blob.contains(t.as_str())) {
        let sc = 820 - (blob.len() as i32 / 60).min(25);
        best = best.max(sc);
    }

    if compact_q.len() >= 6 {
        if fname_c.contains(compact_q) {
            best = best.max(780);
        }
        if blob_c.contains(compact_q) {
            best = best.max(760);
        }
    }

    if tokens.len() == 1 {
        let t = &tokens[0];
        if t.len() >= 5 && fname_lower.contains(t.as_str()) {
            best = best.max(650);
        } else if t.len() >= 5 && blob.contains(t.as_str()) {
            best = best.max(620);
        }
    }

    if best < 0 {
        None
    } else {
        Some(best)
    }
}

fn dedup_existing_dirs(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.retain(|p| p.is_dir());
    paths.sort();
    let mut seen = std::collections::HashSet::<String>::new();
    paths
        .into_iter()
        .filter(|p| {
            let key = p.to_string_lossy().to_ascii_lowercase();
            seen.insert(key)
        })
        .collect()
}

fn push_music_paths_from_raw(paths: &mut Vec<PathBuf>, raw: &str) {
    for part in raw.split(|c| c == ';' || c == '|' || c == '\n' || c == '\r') {
        let t = part.trim().trim_matches('"').trim_matches('\'');
        if !t.is_empty() {
            paths.push(PathBuf::from(t));
        }
    }
}

/// Pastas de biblioteca de música (Shell do Windows + perfil). Varredura prioritária antes de Downloads etc.
/// `settings_extra_paths`: texto das Configurações do app (mesmo formato que DEXTER_MUSIC_PATHS).
fn collect_music_library_roots(settings_extra_paths: Option<&str>) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let ps_read_folder = |special: &str| -> Option<PathBuf> {
        let script = format!(
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); Write-Output ([Environment]::GetFolderPath([Environment+SpecialFolder]::{special}))"
        );
        let output = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() {
            return None;
        }
        let p = PathBuf::from(s);
        if p.is_dir() {
            Some(p)
        } else {
            None
        }
    };

    if let Some(p) = ps_read_folder("MyMusic") {
        paths.push(p);
    }
    if let Some(p) = ps_read_folder("CommonMusic") {
        paths.push(p);
    }

    if let Ok(profile) = std::env::var("USERPROFILE") {
        let base = PathBuf::from(&profile);
        paths.push(base.join("Music"));
        paths.push(base.join("Música"));
        if let Ok(rd) = std::fs::read_dir(&base) {
            for entry in rd.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("OneDrive") {
                    let od = entry.path();
                    paths.push(od.join("Music"));
                    paths.push(od.join("Música"));
                }
            }
        }
    }

    if let Some(p) = dirs::audio_dir() {
        paths.push(p);
    }

    if let Some(h) = dirs::home_dir() {
        paths.push(h.join("Music"));
        paths.push(h.join("Música"));
        paths.push(h.join("Documents").join("Music"));
        paths.push(h.join("Documents").join("Música"));
        paths.push(h.join("OneDrive").join("Music"));
        paths.push(h.join("OneDrive").join("Música"));
    }

    paths.push(PathBuf::from(r"C:\Users\Public\Music"));

    if let Ok(extra) = std::env::var("DEXTER_MUSIC_PATHS") {
        push_music_paths_from_raw(&mut paths, &extra);
    }
    if let Some(raw) = settings_extra_paths {
        push_music_paths_from_raw(&mut paths, raw);
    }

    dedup_existing_dirs(paths)
}

fn collect_secondary_audio_roots() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for d in [
        dirs::download_dir(),
        dirs::video_dir(),
        dirs::document_dir(),
    ] {
        if let Some(p) = d {
            paths.push(p);
        }
    }
    if let Some(h) = dirs::home_dir() {
        paths.push(h.join("Desktop"));
        paths.push(h.join("OneDrive").join("Documents"));
        if let Ok(rd) = std::fs::read_dir(&h) {
            for entry in rd.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("OneDrive") {
                    paths.push(entry.path().join("Documents"));
                }
            }
        }
    }
    dedup_existing_dirs(paths)
}

fn walk_roots_for_best_audio_match(
    roots: &[PathBuf],
    tokens: &[String],
    compact_q: &str,
    allowed_ext: &std::collections::HashSet<&str>,
    max_depth: usize,
    max_entries_per_root: u32,
    mut best: Option<(i32, PathBuf)>,
) -> Option<(i32, PathBuf)> {
    'roots: for root in roots {
        let mut entries: u32 = 0;
        for entry in walkdir::WalkDir::new(root)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            entries += 1;
            if entries > max_entries_per_root {
                continue 'roots;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            if !allowed_ext.contains(ext.as_str()) {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            let Some(sc) = local_audio_match_score(path, root, name, tokens, compact_q) else {
                continue;
            };
            if sc > best.as_ref().map(|(s, _)| *s).unwrap_or(-1) {
                best = Some((sc, path.to_path_buf()));
            }
        }
    }
    best
}

/// Search typical user folders for an audio file whose name matches all words in `search`.
fn find_best_local_audio_file(search: &str, settings_extra_paths: Option<&str>) -> Option<PathBuf> {
    let tokens = normalize_search_tokens(search);
    let compact_q = alphanumeric_compact(search.trim());
    if tokens.is_empty() && compact_q.len() < 6 {
        return None;
    }

    let music_roots = collect_music_library_roots(settings_extra_paths);
    let other_roots = collect_secondary_audio_roots();

    let allowed_ext: std::collections::HashSet<&str> = [
        "mp3", "flac", "wav", "m4a", "aac", "ogg", "wma", "opus",
    ]
    .into_iter()
    .collect();

    // 1) Varredura pesada só na biblioteca de música (pasta Música do Windows e equivalentes).
    let best = walk_roots_for_best_audio_match(
        &music_roots,
        &tokens,
        &compact_q,
        &allowed_ext,
        32,
        200_000,
        None,
    );

    // 2) Se não achou, resto dos locais com limite menor.
    let best = walk_roots_for_best_audio_match(
        &other_roots,
        &tokens,
        &compact_q,
        &allowed_ext,
        24,
        45_000,
        best,
    );

    best.map(|(_, p)| p)
}

fn append_playlist_matches_from_roots(
    roots: &[PathBuf],
    tokens: &[String],
    compact_q: &str,
    allowed_ext: &HashSet<&str>,
    max_depth: usize,
    max_entries_per_root: u32,
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    max_total_tracks: usize,
) {
    'roots: for root in roots {
        if out.len() >= max_total_tracks {
            break;
        }
        let mut entries: u32 = 0;
        for entry in walkdir::WalkDir::new(root)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if out.len() >= max_total_tracks {
                break 'roots;
            }
            entries += 1;
            if entries > max_entries_per_root {
                continue 'roots;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            if !allowed_ext.contains(ext.as_str()) {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if local_audio_match_score(path, root, name, tokens, compact_q).is_none() {
                continue;
            }
            let key = path.to_string_lossy().to_ascii_lowercase();
            if seen.insert(key) {
                out.push(path.to_path_buf());
            }
        }
    }
}

fn collect_playlist_audio_files(
    artist_or_keyword: &str,
    max_tracks: usize,
    settings_extra_paths: Option<&str>,
) -> Vec<PathBuf> {
    let tokens = normalize_search_tokens(artist_or_keyword);
    let compact_q = alphanumeric_compact(artist_or_keyword.trim());
    if tokens.is_empty() && compact_q.len() < 6 {
        return Vec::new();
    }

    let allowed_ext: HashSet<&str> = [
        "mp3", "flac", "wav", "m4a", "aac", "ogg", "wma", "opus",
    ]
    .into_iter()
    .collect();

    let mut out = Vec::new();
    let mut seen = HashSet::<String>::new();

    let music_roots = collect_music_library_roots(settings_extra_paths);
    append_playlist_matches_from_roots(
        &music_roots,
        &tokens,
        &compact_q,
        &allowed_ext,
        32,
        200_000,
        &mut out,
        &mut seen,
        max_tracks,
    );

    if out.len() < max_tracks {
        let secondary = collect_secondary_audio_roots();
        append_playlist_matches_from_roots(
            &secondary,
            &tokens,
            &compact_q,
            &allowed_ext,
            24,
            45_000,
            &mut out,
            &mut seen,
            max_tracks,
        );
    }

    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    out
}

fn allowed_audio_extensions() -> HashSet<&'static str> {
    [
        "mp3", "flac", "wav", "m4a", "aac", "ogg", "wma", "opus",
    ]
    .into_iter()
    .collect()
}

fn max_full_library_tracks() -> usize {
    std::env::var("DEXTER_MUSIC_FULL_PLAYLIST_MAX")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0 && n <= 50_000)
        .unwrap_or(12_000)
}

/// Todas as faixas de áudio sob `roots`, até `max_total_tracks`, sem filtro por nome.
fn append_all_audio_from_roots(
    roots: &[PathBuf],
    allowed_ext: &HashSet<&str>,
    max_depth: usize,
    max_entries_per_root: u32,
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    max_total_tracks: usize,
) {
    'roots: for root in roots {
        if out.len() >= max_total_tracks {
            break;
        }
        let mut entries: u32 = 0;
        for entry in walkdir::WalkDir::new(root)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if out.len() >= max_total_tracks {
                break 'roots;
            }
            entries += 1;
            if entries > max_entries_per_root {
                continue 'roots;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            if !allowed_ext.contains(ext.as_str()) {
                continue;
            }
            let key = path.to_string_lossy().to_ascii_lowercase();
            if seen.insert(key) {
                out.push(path.to_path_buf());
            }
        }
    }
}

fn collect_all_library_audio_files(
    max_tracks: usize,
    include_secondary_folders: bool,
    settings_extra_paths: Option<&str>,
) -> Vec<PathBuf> {
    let allowed_ext = allowed_audio_extensions();
    let mut out = Vec::new();
    let mut seen = HashSet::<String>::new();

    let music_roots = collect_music_library_roots(settings_extra_paths);
    // Biblioteca completa: orçamento por pasta — um único contador global esgotava antes das outras raízes.
    append_all_audio_from_roots(
        &music_roots,
        &allowed_ext,
        32,
        1_000_000,
        &mut out,
        &mut seen,
        max_tracks,
    );

    if include_secondary_folders && out.len() < max_tracks {
        let secondary = collect_secondary_audio_roots();
        append_all_audio_from_roots(
            &secondary,
            &allowed_ext,
            24,
            600_000,
            &mut out,
            &mut seen,
            max_tracks,
        );
    }

    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    out
}

/// Remove prefixo `\\?\` do Windows — Groove e outros players não resolvem bem paths verbatim dentro de M3U.
fn strip_windows_verbatim_prefix(path_str: &str) -> String {
    let Some(rest) = path_str.strip_prefix(r"\\?\") else {
        return path_str.to_string();
    };
    if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case("UNC\\") {
        format!(r"\\{}", &rest[4..])
    } else {
        rest.to_string()
    }
}

/// Caminho absoluto legível por reprodutores (sem `\\?\`).
fn path_line_for_m3u(audio_file: &Path) -> String {
    let resolved = std::fs::canonicalize(audio_file).unwrap_or_else(|_| audio_file.to_path_buf());
    strip_windows_verbatim_prefix(&resolved.to_string_lossy())
}

fn write_m3u_playlist(paths: &[PathBuf], file_stem: &str) -> Result<PathBuf, String> {
    let mut content = String::from('\u{FEFF}');
    content.push_str("#EXTM3U\r\n");
    for p in paths {
        let line = path_line_for_m3u(p);
        content.push_str(&line);
        content.push_str("\r\n");
    }
    let ms = chrono::Utc::now().timestamp_millis();
    let base = dirs::audio_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Music")))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| std::env::temp_dir());
    let stem = file_stem.trim().trim_matches(|c| c == '.' || c == '/' || c == '\\');
    let stem = if stem.is_empty() {
        "dexter-playlist"
    } else {
        stem
    };
    let out = base.join(format!("{stem}-{ms}.m3u"));
    std::fs::write(&out, content.as_bytes()).map_err(|e| format!("Gravar M3U: {}", e))?;
    Ok(out)
}

fn open_path_default_program(path: &Path) -> Result<(), String> {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let p = strip_windows_verbatim_prefix(&resolved.to_string_lossy());
    let status = Command::new("cmd")
        .args(["/c", "start", "", &p])
        .status()
        .map_err(|e| format!("Falha ao abrir arquivo: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err("Não foi possível abrir o arquivo com o programa padrão.".into())
    }
}

/// Quando o modelo chama `play_local_music_playlist` com frases de “toda a biblioteca”, usa o reprodutor nativo (sem M3U).
fn is_entire_local_library_request(artist: &str) -> bool {
    let t = artist.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "*"
            | "__full__"
            | "__full_library__"
            | "__all__"
            | "all"
            | "tudo"
            | "todas"
            | "library"
            | "biblioteca"
            | "full"
            | "completo"
            | "completa"
    ) {
        return true;
    }
    const PHRASES: &[&str] = &[
        "biblioteca inteira",
        "biblioteca completa",
        "todas as músicas",
        "todas as musicas",
        "todas minhas músicas",
        "todas minhas musicas",
        "músicas do pc",
        "musicas do pc",
        "full library",
        "whole library",
        "entire library",
        "player com todas",
        "todas as faixas",
        "coleção completa",
        "colecao completa",
        "shuffle tudo",
        "tocar tudo",
        "minha biblioteca",
        "músicas locais todas",
        "musicas locais todas",
        "reproduzir tudo local",
        "ordem aleatória e reproduzir",
        "ordem aleatoria e reproduzir",
    ];
    PHRASES.iter().any(|p| lower.contains(p))
}

/// Todas as faixas locais que combinam com artista/pasta; abre playlist M3U no reprodutor padrão.
pub async fn play_local_music_playlist(
    artist: &str,
    settings_music_paths: Option<&str>,
) -> Result<String, String> {
    let artist = artist.trim();
    if artist.is_empty() {
        return Err("Nome do artista ou pasta vazio.".into());
    }
    if is_entire_local_library_request(artist) {
        return native_music_library_shuffle_play();
    }
    let artist_owned = artist.to_string();
    let settings_owned = settings_music_paths.map(|s| s.to_string());
    const MAX_TRACKS: usize = 4000;
    tokio::task::spawn_blocking(move || {
        let paths = collect_playlist_audio_files(
            &artist_owned,
            MAX_TRACKS,
            settings_owned.as_deref(),
        );
        if paths.is_empty() {
            return Err(format!(
                "Nenhuma faixa local encontrada para {:?}. Pastas ou nomes de arquivo devem conter as mesmas palavras do pedido (ex.: linkin e park). Configure pastas em Configurações do Chronos ou use DEXTER_MUSIC_PATHS.",
                artist_owned
            ));
        }
        let n = paths.len();
        let m3u_path = write_m3u_playlist(&paths, "dexter-playlist")?;
        open_path_default_program(&m3u_path)?;
        Ok(format!(
            "Playlist com {} faixas para {:?}. Abri no reprodutor padrão. Caminho da lista: {}",
            n,
            artist_owned,
            m3u_path.display()
        ))
    })
    .await
    .map_err(|e| format!("Playlist: {}", e))?
}

/// Varredura completa do disco e M3U gigante — só deve ser invocada com `explicit_m3u_export_request` no executor (pedido explícito do utilizador).
pub async fn play_full_local_music_library(
    include_downloads_documents: bool,
    settings_music_paths: Option<&str>,
) -> Result<String, String> {
    let include_secondary = include_downloads_documents;
    let settings_owned = settings_music_paths.map(|s| s.to_string());
    tokio::task::spawn_blocking(move || {
        let cap = max_full_library_tracks();
        let paths = collect_all_library_audio_files(
            cap,
            include_secondary,
            settings_owned.as_deref(),
        );
        if paths.is_empty() {
            let music_roots = collect_music_library_roots(settings_owned.as_deref());
            let sec_roots = collect_secondary_audio_roots();
            let mut msg = String::from(
                "Nenhum arquivo de áudio encontrado nas pastas pesquisadas. ",
            );
            if music_roots.is_empty() {
                msg.push_str(
                    "Não há pastas Music/Música/MyMusic visíveis neste perfil. ",
                );
            } else {
                msg.push_str(&format!(
                    "{} pasta(s) de biblioteca (Music/Música/OneDrive/etc.): ",
                    music_roots.len()
                ));
                for r in music_roots.iter().take(10) {
                    msg.push_str(&format!("{} — ", r.display()));
                }
            }
            if include_secondary {
                msg.push_str(&format!(
                    "Pastas secundárias (Downloads etc.): {}. ",
                    sec_roots.len()
                ));
            }
            msg.push_str(
                "Em Configurações do Chronos adicione \"Pastas de música\", ou use DEXTER_MUSIC_PATHS.",
            );
            return Err(msg);
        }
        let n = paths.len();
        let capped = n >= cap;
        let m3u_path = write_m3u_playlist(&paths, "dexter-library")?;
        open_path_default_program(&m3u_path)?;
        let tail = if capped {
            format!(
                " Limite de {} faixas (ajuste com DEXTER_MUSIC_FULL_PLAYLIST_MAX se precisar).",
                cap
            )
        } else {
            String::new()
        };
        Ok(format!(
            "Biblioteca com {} faixas locais. Abri no reprodutor padrão.{tail} Lista em {}",
            n,
            m3u_path.display()
        ))
    })
    .await
    .map_err(|e| format!("Biblioteca local: {}", e))?
}

fn youtube_watch_with_autoplay(watch_base: &str) -> String {
    let mut s = watch_base
        .split('&')
        .next()
        .unwrap_or(watch_base)
        .to_string();
    if s.contains("autoplay=1") || s.contains("autoplay=true") {
        return s;
    }
    if s.contains('?') {
        s.push_str("&autoplay=1");
    } else {
        s.push_str("?autoplay=1");
    }
    s
}

/// If the URL is a YouTube search/results page, returns the query text to resolve via `play_music_query`.
pub fn youtube_search_query_from_open_url(url: &str) -> Option<String> {
    let lower = url.to_lowercase();
    if !lower.contains("youtube.com") && !lower.contains("youtu.be") {
        return None;
    }
    if lower.contains("/watch") || lower.contains("youtu.be/") {
        return None;
    }
    if !lower.contains("/results") && !lower.contains("search_query=") {
        return None;
    }
    let marker = "search_query=";
    let idx = lower.find(marker)?;
    let rest = &url[idx + marker.len()..];
    let end = rest.find('&').unwrap_or(rest.len());
    let encoded = &rest[..end];
    urlencoding::decode(encoded)
        .ok()
        .map(|s| s.into_owned())
        .filter(|s| !s.trim().is_empty())
}

/// Resolve a song title (and optional artist) to a YouTube watch URL via public Piped/Invidious APIs, then open it.
pub async fn play_music_query(
    title: &str,
    artist: Option<&str>,
    settings_music_paths: Option<&str>,
    prefer_youtube: bool,
    prefer_native_player: bool,
) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Título da música vazio.".into());
    }
    let q = match artist.map(|a| a.trim()).filter(|a| !a.is_empty()) {
        Some(a) => format!("{} {}", title, a),
        None => title.to_string(),
    };

    if !prefer_youtube {
        let q_local = q.clone();
        let settings_owned = settings_music_paths.map(|s| s.to_string());
        match tokio::task::spawn_blocking(move || {
            find_best_local_audio_file(&q_local, settings_owned.as_deref())
        })
        .await
        {
            Ok(Some(path)) => {
                if prefer_native_player {
                    let _ = launch_desktop_app("media_player");
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                }
                open_path_default_program(&path)?;
                let where_spoken = if prefer_native_player {
                    "no reprodutor de música"
                } else {
                    "da sua biblioteca local"
                };
                return Ok(format!("Tocando {:?} {}.", q, where_spoken));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("Busca local: {}", e)),
        }
    }

    let local_only = std::env::var("DEXTER_MUSIC_LOCAL_ONLY")
        .map(|v| {
            v == "1"
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false);
    if local_only {
        return Err(
            "Não encontrei essa faixa nas pastas locais pesquisadas. Nas Configurações do Chronos defina \"Pastas de música\", ou DEXTER_MUSIC_PATHS, \
             ou mova os arquivos para Música / Documentos. O assistente compara o título com nomes de pastas e de arquivos."
                .into(),
        );
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(14))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;

    let enc = urlencoding::encode(&q);

    let piped_bases = [
        "https://pipedapi.kavin.rocks",
        "https://pipedapi.in.projectsegfau.lt",
        "https://pipedapi.leptons.xyz",
        "https://api.piped.private.coffee",
    ];
    let piped_filters = ["music_songs", "music_videos", "videos", "all"];

    for base in piped_bases {
        for filter in piped_filters {
            let api = format!("{}/search?q={}&filter={}", base, enc, filter);
            let Ok(resp) = client.get(api).send().await else {
                continue;
            };
            let Ok(json) = resp.json::<serde_json::Value>().await else {
                continue;
            };
            let Some(items) = json.get("items").and_then(|x| x.as_array()) else {
                continue;
            };
            for item in items {
                let Some(u) = item.get("url").and_then(|x| x.as_str()) else {
                    continue;
                };
                if !u.contains("/watch?v=") && !u.starts_with("/watch?v=") {
                    continue;
                }
                let watch = if u.starts_with("http") {
                    u.to_string()
                } else {
                    format!("https://www.youtube.com{}", u)
                };
                let watch = youtube_watch_with_autoplay(&watch);
                open_url(&watch)?;
                return Ok(format!(
                    "Tocando {:?} no YouTube.",
                    q
                ));
            }
        }
    }

    let inv_url = format!(
        "https://vid.puffyan.us/api/v1/search?q={}&type=video",
        enc
    );
    if let Ok(resp) = client.get(inv_url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json.as_array() {
                for item in arr {
                    if let Some(id) = item.get("videoId").and_then(|x| x.as_str()) {
                        if id.is_empty() {
                            continue;
                        }
                        let watch = format!("https://www.youtube.com/watch?v={}", id);
                        let watch = youtube_watch_with_autoplay(&watch);
                        open_url(&watch)?;
                        return Ok(format!(
                            "Tocando {:?} no YouTube.",
                            q
                        ));
                    }
                }
            }
        }
    }

    Err(format!(
        "Não encontrei o vídeo no YouTube para {:?}. Fale de novo com o nome da música e do artista.",
        q
    ))
}

/// Open a URL in the default browser on Windows.
pub fn open_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("URL vazia.".to_string());
    }
    #[cfg(windows)]
    {
        // `cmd start` breaks on `&` in query strings (e.g. watch?v=…&autoplay=1).
        // FileProtocolHandler passes the URL intact to the default browser.
        let status = Command::new("rundll32")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .status()
            .map_err(|e| format!("Falha ao abrir URL: {}", e))?;

        if status.success() {
            Ok(format!("Abri {} no navegador padrão.", url))
        } else {
            Err("Falha ao abrir URL.".to_string())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("open_url is only supported on Windows.".to_string())
    }
}

fn normalize_fx_pair(pair: &str) -> Result<&'static str, String> {
    let pair_key = pair.trim().to_uppercase().replace('/', "-");
    match pair_key.as_str() {
        "USD-BRL" | "USD" | "DOLAR" | "DÓLAR" => Ok("USD-BRL"),
        "EUR-BRL" | "EUR" | "EURO" => Ok("EUR-BRL"),
        "JPY-BRL" | "JPY" | "IENE" | "YEN" => Ok("JPY-BRL"),
        "GBP-BRL" | "GBP" | "LIBRA" => Ok("GBP-BRL"),
        _ => Err(format!(
            "Par de moedas não suportado: {:?}. Use USD-BRL, EUR-BRL, JPY-BRL ou GBP-BRL.",
            pair
        )),
    }
}

fn fx_label_pt(api_path: &str) -> &'static str {
    match api_path {
        "EUR-BRL" => "euro",
        "JPY-BRL" => "iene",
        "GBP-BRL" => "libra esterlina",
        _ => "dólar",
    }
}

fn format_fx_quote_text(label: &str, bid: &str, pct: &str, compact: bool) -> String {
    let pct_f: f64 = pct.parse().unwrap_or(0.0);
    if compact {
        let bid_short = bid
            .parse::<f64>()
            .map(|v| {
                let s = format!("{:.2}", v);
                s.trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            })
            .unwrap_or_else(|_| bid.to_string());
        let pct_short = if pct_f >= 0.0 {
            format!("alta de {:.0} por cento", pct_f.round())
        } else {
            format!("queda de {:.0} por cento", pct_f.abs().round())
        };
        return format!("{} a {} reais, {}.", label, bid_short, pct_short);
    }
    let pct_spoken = if pct_f >= 0.0 {
        format!("alta de {:.1} por cento", pct_f)
    } else {
        format!("queda de {:.1} por cento", pct_f.abs())
    };
    format!(
        "O {} está a {} reais, com {} nas últimas 24 horas.",
        label, bid, pct_spoken
    )
}

/// Cotação FX em tempo real (AwesomeAPI).
pub async fn fetch_fx_quote(pair: &str) -> Result<String, String> {
    fetch_fx_quote_internal(pair, false).await
}

/// Cotação FX em uma frase curta para TTS por voz (fast-path).
pub async fn fetch_fx_quote_voice(pair: &str) -> Result<String, String> {
    fetch_fx_quote_internal(pair, true).await
}

async fn fetch_fx_quote_internal(pair: &str, compact: bool) -> Result<String, String> {
    let api_path = normalize_fx_pair(pair)?;
    let url = format!("https://economia.awesomeapi.com.br/last/{}", api_path);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Chronos-Dexter/1.0")
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;

    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Falha ao buscar cotação: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Falha ao interpretar cotação: {}", e))?;

    let node_key = api_path.replace('-', "");
    let node = json
        .get(&node_key)
        .or_else(|| json.get(api_path))
        .ok_or_else(|| format!("Resposta inesperada da API: {}", json))?;

    let bid = node
        .get("bid")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let pct = node
        .get("pctChange")
        .and_then(|v| v.as_str())
        .unwrap_or("0");

    let label = fx_label_pt(api_path);
    Ok(format_fx_quote_text(label, bid, pct, compact))
}

fn wmo_weather_label_pt(code: i64) -> &'static str {
    match code {
        0 => "céu limpo",
        1 | 2 | 3 => "parcialmente nublado",
        45 | 48 => "neblina",
        51 | 53 | 55 => "garoa",
        56 | 57 => "garoa congelante",
        61 | 63 | 65 => "chuva",
        66 | 67 => "chuva congelante",
        71 | 73 | 75 => "neve",
        77 => "grãos de neve",
        80 | 81 | 82 => "pancadas de chuva",
        85 | 86 => "pancadas de neve",
        95 => "tempestade",
        96 | 99 => "tempestade com granizo",
        _ => "condição variável",
    }
}

async fn geocode_place(client: &reqwest::Client, name: &str) -> Result<(f64, f64, String), String> {
    let aliases: &[(&str, &str)] = &[
        ("jv", "Juiz de Fora"),
        ("jf", "Juiz de Fora"),
        ("rj", "Rio de Janeiro"),
        ("sp", "São Paulo"),
        ("bh", "Belo Horizonte"),
        ("bsb", "Brasília"),
        ("poa", "Porto Alegre"),
        ("cwb", "Curitiba"),
    ];
    let key = name.trim().to_lowercase();
    let search = aliases
        .iter()
        .find(|(abbr, _)| *abbr == key.as_str())
        .map(|(_, full)| *full)
        .unwrap_or(name.trim());

    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=pt&format=json",
        urlencoding::encode(search)
    );
    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Falha ao localizar cidade: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Falha ao interpretar geocoding: {}", e))?;

    let results = json
        .get("results")
        .and_then(|v| v.as_array())
        .filter(|a| !a.is_empty())
        .ok_or_else(|| format!("Cidade não encontrada: {:?}", search))?;

    let first = &results[0];
    let lat = first
        .get("latitude")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "latitude ausente".to_string())?;
    let lon = first
        .get("longitude")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "longitude ausente".to_string())?;
    let label = first
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(search);
    let admin1 = first.get("admin1").and_then(|v| v.as_str()).unwrap_or("");
    let country = first.get("country").and_then(|v| v.as_str()).unwrap_or("");
    let place = if admin1.is_empty() {
        format!("{}, {}", label, country)
    } else {
        format!("{}, {}, {}", label, admin1, country)
    };
    Ok((lat, lon, place))
}

async fn coords_from_ip(client: &reqwest::Client) -> Result<(f64, f64, String), String> {
    let json: serde_json::Value = client
        .get("http://ip-api.com/json/?fields=status,message,lat,lon,city,regionName,country")
        .send()
        .await
        .map_err(|e| format!("Falha na geolocalização por IP: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Falha ao interpretar geolocalização: {}", e))?;

    if json.get("status").and_then(|v| v.as_str()) != Some("success") {
        return Err(json
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("geolocalização indisponível")
            .to_string());
    }
    let lat = json.get("lat").and_then(|v| v.as_f64()).ok_or("lat ausente")?;
    let lon = json.get("lon").and_then(|v| v.as_f64()).ok_or("lon ausente")?;
    let city = json.get("city").and_then(|v| v.as_str()).unwrap_or("sua região");
    let region = json
        .get("regionName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let place = if region.is_empty() {
        city.to_string()
    } else {
        format!("{}, {}", city, region)
    };
    Ok((lat, lon, place))
}

fn weather_day_label_pt(offset: usize) -> &'static str {
    match offset {
        0 => "Hoje",
        1 => "Amanhã",
        2 => "Depois de amanhã",
        _ => "Nesse dia",
    }
}

/// Clima via Open-Meteo (sem API key). `day_offset`: None = agora; 0 = previsão de hoje; 1 = amanhã; etc.
pub async fn fetch_weather(
    location: Option<&str>,
    day_offset: Option<usize>,
) -> Result<String, String> {
    fetch_weather_internal(location, day_offset, false).await
}

/// Clima em frase curta para TTS por voz (fast-path).
pub async fn fetch_weather_voice(
    location: Option<&str>,
    day_offset: Option<usize>,
) -> Result<String, String> {
    fetch_weather_internal(location, day_offset, true).await
}

async fn fetch_weather_internal(
    location: Option<&str>,
    day_offset: Option<usize>,
    compact: bool,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent("Chronos-Dexter/1.0")
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;

    let loc_hint = location.map(str::trim).filter(|s| !s.is_empty());
    let (lat, lon, place) = if let Some(name) = loc_hint {
        geocode_place(&client, name).await?
    } else if let Ok(city) = std::env::var("DEXTER_WEATHER_CITY") {
        let c = city.trim();
        if c.is_empty() {
            coords_from_ip(&client).await?
        } else {
            geocode_place(&client, c).await?
        }
    } else {
        coords_from_ip(&client).await?
    };

    let forecast_days = day_offset.map(|d| d + 1).unwrap_or(1).max(3).min(7);

    let url = if day_offset.is_some() {
        format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
             &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max\
             &timezone=auto&forecast_days={}",
            lat, lon, forecast_days
        )
    } else {
        format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
             &current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code\
             &timezone=auto",
            lat, lon
        )
    };

    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Falha ao buscar clima: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Falha ao interpretar clima: {}", e))?;

    if let Some(day) = day_offset {
        let daily = json
            .get("daily")
            .ok_or_else(|| "Resposta de clima sem previsão diária".to_string())?;
        let codes = daily
            .get("weather_code")
            .and_then(|v| v.as_array())
            .ok_or("weather_code ausente")?;
        let maxs = daily
            .get("temperature_2m_max")
            .and_then(|v| v.as_array())
            .ok_or("temperature_2m_max ausente")?;
        let mins = daily
            .get("temperature_2m_min")
            .and_then(|v| v.as_array())
            .ok_or("temperature_2m_min ausente")?;
        let rain = daily
            .get("precipitation_probability_max")
            .and_then(|v| v.as_array());
        if day >= codes.len() {
            return Err("Previsão não disponível para esse dia.".into());
        }
        let code = codes[day].as_i64().unwrap_or(-1);
        let tmax = maxs[day].as_f64().unwrap_or(0.0);
        let tmin = mins[day].as_f64().unwrap_or(0.0);
        let rain_pct = rain
            .and_then(|a| a.get(day))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let condition = wmo_weather_label_pt(code);
        let label = weather_day_label_pt(day);
        let place_short = place.split(',').next().unwrap_or(&place).trim();
        if compact {
            return Ok(format!(
                "{} em {}: máxima {:.0}, mínima {:.0}, {}.",
                label, place_short, tmax, tmin, condition
            ));
        }
        return Ok(format!(
            "{} em {}: máxima {:.0} graus, mínima {:.0}, {}, chance de chuva {:.0}%.",
            label, place, tmax, tmin, condition, rain_pct
        ));
    }

    let current = json
        .get("current")
        .ok_or_else(|| "Resposta de clima sem campo current".to_string())?;

    let temp = current
        .get("temperature_2m")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "temperatura ausente".to_string())?;
    let feels = current
        .get("apparent_temperature")
        .and_then(|v| v.as_f64())
        .unwrap_or(temp);
    let humidity = current
        .get("relative_humidity_2m")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let code = current
        .get("weather_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let condition = wmo_weather_label_pt(code);
    let place_short = place.split(',').next().unwrap_or(&place).trim();

    if compact {
        Ok(format!(
            "Em {} agora, {:.0} graus, {}.",
            place_short, temp, condition
        ))
    } else {
        Ok(format!(
            "Em {} agora: {:.0} graus, sensação de {:.0}, umidade {}%, {}.",
            place, temp, feels, humidity, condition
        ))
    }
}

/// Data, hora e dia da semana em português do Brasil.
pub fn get_current_time() -> String {
    let now = chrono::Local::now();
    let dia_semana = match now.weekday() {
        Weekday::Mon => "segunda-feira",
        Weekday::Tue => "terça-feira",
        Weekday::Wed => "quarta-feira",
        Weekday::Thu => "quinta-feira",
        Weekday::Fri => "sexta-feira",
        Weekday::Sat => "sábado",
        Weekday::Sun => "domingo",
    };
    let mes = match now.month() {
        1 => "janeiro",
        2 => "fevereiro",
        3 => "março",
        4 => "abril",
        5 => "maio",
        6 => "junho",
        7 => "julho",
        8 => "agosto",
        9 => "setembro",
        10 => "outubro",
        11 => "novembro",
        _ => "dezembro",
    };
    format!(
        "{}, {} de {} de {} — {:02}:{:02}:{:02}",
        dia_semana,
        now.day(),
        mes,
        now.year(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

/// Splits the output of `get_current_time()` into `(time_str, date_str)` usable
/// for replacing `{time}` and `{date}` in fast-path TTS templates.
///
/// `get_current_time()` returns something like:
///   "segunda-feira, 26 de maio de 2025 — 18:45:30"
///
/// This extracts:
/// - date: "segunda-feira, 26 de maio de 2025"
/// - time: "18:45" (drops seconds for cleaner speech)
pub fn split_datetime_for_templates(full: &str) -> (String, String) {
    if let Some((date_part, time_part)) = full.split_once(" — ") {
        let date_str = date_part.trim().to_string();
        // "18:45:30" → "18:45"
        let time_clean = time_part
            .trim()
            .split(':')
            .take(2)
            .collect::<Vec<_>>()
            .join(":");
        (time_clean, date_str)
    } else {
        // Fallback: if format unexpectedly changes, return full string for both
        (full.to_string(), full.to_string())
    }
}

/// Fetch a URL and return its text content (HTML stripped to readable text).
pub async fn web_fetch(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("Erro no cliente HTTP: {}", e))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Falha ao buscar URL: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status));
    }

    let html = resp
        .text()
        .await
        .map_err(|e| format!("Falha ao ler o corpo da resposta: {}", e))?;

    // Strip HTML to plain text
    let text = strip_html(&html);

    // Truncate to avoid flooding context
    let max_len = 6000;
    if text.len() > max_len {
        Ok(format!("{}...\n(truncado, {} caracteres no total)", &text[..max_len], text.len()))
    } else {
        Ok(text)
    }
}

/// Naive HTML-to-text: strip tags, decode common entities, collapse whitespace.
fn strip_html(html: &str) -> String {
    // Remove script and style blocks entirely
    let mut s = html.to_string();
    for tag in &["script", "style", "noscript", "svg"] {
        loop {
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            if let Some(start) = s.to_lowercase().find(&open) {
                if let Some(end) = s.to_lowercase()[start..].find(&close) {
                    s = format!("{}{}", &s[..start], &s[start + end + close.len()..]);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // Replace block elements with newlines
    let block_tags = ["</p>", "</div>", "</li>", "</h1>", "</h2>", "</h3>", "</h4>", "</h5>", "</h6>", "<br>", "<br/>", "<br />", "</tr>", "</blockquote>"];
    for tag in block_tags {
        s = s.replace(tag, "\n");
    }

    // Strip remaining tags
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }

    // Decode common entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/");

    // Collapse whitespace: multiple spaces -> one, multiple newlines -> two
    let mut cleaned = String::with_capacity(result.len());
    let mut prev_newline = 0;
    let mut prev_space = false;
    for ch in result.chars() {
        if ch == '\n' || ch == '\r' {
            prev_newline += 1;
            prev_space = false;
            if prev_newline <= 2 {
                cleaned.push('\n');
            }
        } else if ch.is_whitespace() {
            prev_newline = 0;
            if !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            prev_newline = 0;
            prev_space = false;
            cleaned.push(ch);
        }
    }

    cleaned.trim().to_string()
}

/// List running applications on Windows using PowerShell.
pub fn list_running_apps() -> Result<String, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command",
            "Get-Process | Where-Object {$_.MainWindowTitle -ne ''} | Select-Object -ExpandProperty MainWindowTitle | Sort-Object"
        ])
        .output()
        .map_err(|e| format!("Falha ao listar aplicativos: {}", e))?;

    if !output.status.success() {
        return Err("Não foi possível listar os aplicativos em execução.".to_string());
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Saída não é UTF-8 válido: {}", e))
}

/// Executa script UI Automation para «Biblioteca de músicas» + «Ordem aleatória e reproduzir».
#[cfg(windows)]
fn run_media_player_shuffle_automation() -> bool {
    const SCRIPT: &str = include_str!("../scripts/media-player-library-shuffle.ps1");
    let tmp = std::env::temp_dir().join(format!(
        "dexter-media-shuffle-{}.ps1",
        std::process::id()
    ));
    if std::fs::write(&tmp, SCRIPT).is_err() {
        return false;
    }
    let ok = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            tmp.to_string_lossy().as_ref(),
        ])
        .output()
        .map(|o| o.status.code() == Some(0))
        .unwrap_or(false);
    let _ = std::fs::remove_file(&tmp);
    ok
}

/// Abre o Reprodutor Multimédia do Windows e tenta acionar «Biblioteca de músicas» + «Ordem aleatória e reproduzir» via UI Automation.
/// Não varre disco nem gera M3U.
pub fn native_music_library_shuffle_play() -> Result<String, String> {
    launch_desktop_app("media_player")?;
    #[cfg(windows)]
    {
        std::thread::sleep(std::time::Duration::from_millis(4500));
        for attempt in 0..5u32 {
            if run_media_player_shuffle_automation() {
                return Ok(
                    "Abri o Reprodutor Multimédia e acionei a biblioteca com «Ordem aleatoria e reproduzir». \
                     Se não começar, confirma que a pasta está em «Adicionar uma pasta» neste app."
                        .into(),
                );
            }
            if attempt < 4 {
                std::thread::sleep(std::time::Duration::from_millis(2800));
            }
        }
        return Ok(
            "Abri o Reprodutor Multimédia. A automação ainda não encontrou os botões nesta versão da app — \
             escolhe «Biblioteca de músicas» e «Ordem aleatória e reproduzir» manualmente."
                .into(),
        );
    }
    #[cfg(not(windows))]
    {
        Ok(String::new())
    }
}

/// Launch a predefined desktop application (whitelist only). Windows-only.
pub fn launch_desktop_app(app: &str) -> Result<String, String> {
    #[cfg(not(windows))]
    {
        let _ = app;
        return Err("launch_desktop_app is only supported on Windows.".to_string());
    }
    #[cfg(windows)]
    {
        launch_desktop_app_windows(app)
    }
}

#[cfg(windows)]
fn launch_desktop_app_windows(app: &str) -> Result<String, String> {
    let mut key = app.trim().to_lowercase().replace('-', "_");
    if key.contains("bloco") && key.contains("notas") {
        key = "notepad".to_string();
    } else if key == "crome" || key == "google_crome" {
        key = "chrome".to_string();
    } else if key.contains("notepad") {
        key = "notepad".to_string();
    }

    let label = match key.as_str() {
        "cursor" => {
            launch_cursor()?;
            "Cursor"
        }
        "vscode" | "vs_code" | "code" | "visual_studio_code" => {
            launch_vscode()?;
            "Visual Studio Code"
        }
        "terminal" | "windows_terminal" | "wt" => {
            launch_terminal()?;
            "Windows Terminal"
        }
        "chrome" | "google_chrome" => {
            launch_chrome()?;
            "Google Chrome"
        }
        "notepad" | "bloco_de_notas" | "bloco de notas" => {
            launch_notepad()?;
            "Bloco de Notas"
        }
        "edge" | "microsoft_edge" | "msedge" => {
            launch_edge()?;
            "Microsoft Edge"
        }
        "discord" => {
            launch_discord()?;
            "Discord"
        }
        "obs" | "obs_studio" => {
            launch_obs()?;
            "OBS Studio"
        }
        "snipping_tool" | "snipping" | "capture" | "screen_capture" => {
            launch_snipping_tool()?;
            "Snipping Tool"
        }
        "media_player" | "groove" | "music" | "zune" => {
            launch_shell_apps_folder(
                r"shell:AppsFolder\Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic",
            )?;
            "Windows Media Player (Groove)"
        }
        "excel" => {
            launch_office_exe("EXCEL.EXE")?;
            "Microsoft Excel"
        }
        "word" => {
            launch_office_exe("WINWORD.EXE")?;
            "Microsoft Word"
        }
        "powerpoint" | "ppt" => {
            launch_office_exe("POWERPNT.EXE")?;
            "Microsoft PowerPoint"
        }
        "outlook" => {
            launch_office_exe("OUTLOOK.EXE")?;
            "Microsoft Outlook"
        }
        "paint" | "mspaint" => {
            launch_mspaint()?;
            "Paint"
        }
        _ => {
            return Err(format!(
                "App desconhecido {:?}. Permitidos: cursor, vscode, terminal, chrome, edge, notepad, discord, obs, snipping_tool, media_player, excel, word, powerpoint, outlook, paint.",
                app
            ));
        }
    };

    Ok(format!("{} foi aberto.", label))
}

/// GUI apps inherit a minimal PATH from the parent process; use absolute paths and `cmd /c start`.
#[cfg(windows)]
fn windows_gui_spawn(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Não encontrado: {}", path.display()));
    }
    // `start "" <path>` works for .exe, .cmd, paths with spaces; avoids broken PATH lookups.
    Command::new("cmd.exe")
        .args(["/c", "start", ""])
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}", e))
}

#[cfg(windows)]
fn try_spawn_paths(paths: &[PathBuf], label: &str) -> Result<(), String> {
    let mut last = String::from("(no path matched)");
    for p in paths {
        if !p.exists() {
            continue;
        }
        match windows_gui_spawn(p) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(format!("{} — {}", label, last))
}

#[cfg(windows)]
fn program_files() -> PathBuf {
    std::env::var("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\Program Files"))
}

#[cfg(windows)]
fn program_files_x86() -> PathBuf {
    std::env::var("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\Program Files (x86)"))
}

#[cfg(windows)]
fn local_app_data() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(windows)]
fn launch_cursor() -> Result<(), String> {
    let pf = program_files();
    let paths = vec![
        pf.join("cursor").join("Cursor.exe"),
        pf.join("cursor")
            .join("resources")
            .join("app")
            .join("bin")
            .join("cursor.cmd"),
        pf.join("cursor")
            .join("resources")
            .join("app")
            .join("bin")
            .join("cursor.exe"),
    ];
    try_spawn_paths(&paths, "Cursor").or_else(|_| {
        spawn_simple(&["cursor"], "Cursor").map_err(|e| format!("Cursor: {}", e))
    })
}

#[cfg(windows)]
fn launch_vscode() -> Result<(), String> {
    let local = local_app_data();
    let pf = program_files();
    let paths = vec![
        local
            .join("Programs")
            .join("Microsoft VS Code")
            .join("Code.exe"),
        local
            .join("Programs")
            .join("Microsoft VS Code")
            .join("bin")
            .join("code.cmd"),
        pf.join("Microsoft VS Code").join("Code.exe"),
    ];
    try_spawn_paths(&paths, "Visual Studio Code").or_else(|_| {
        spawn_simple(&["code"], "Visual Studio Code").map_err(|e| format!("VS Code: {}", e))
    })
}

#[cfg(windows)]
fn launch_terminal() -> Result<(), String> {
    let local = local_app_data();
    let paths = vec![
        local
            .join("Microsoft")
            .join("WindowsApps")
            .join("wt.exe"),
        PathBuf::from(r"C:\Program Files\WindowsApps\wt.exe"),
    ];
    try_spawn_paths(&paths, "Windows Terminal")
        .or_else(|_| spawn_simple(&["wt"], "Windows Terminal").map_err(|e| format!("wt: {}", e)))
}

#[cfg(windows)]
fn spawn_simple(candidates: &[&str], desc: &str) -> Result<(), String> {
    let mut last_err: Option<String> = None;
    for cmd in candidates {
        match Command::new(cmd).spawn() {
            Ok(_) => return Ok(()),
            Err(e) => last_err = Some(e.to_string()),
        }
    }
    Err(format!(
        "Não foi possível iniciar {} (tentou {:?}): {}",
        desc,
        candidates,
        last_err.unwrap_or_else(|| "erro desconhecido".to_string())
    ))
}

#[cfg(windows)]
fn launch_notepad() -> Result<(), String> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    let notepad = Path::new(&system_root).join("System32").join("notepad.exe");
    if notepad.exists() {
        windows_gui_spawn(&notepad)
    } else {
        spawn_simple(&["notepad"], "Bloco de Notas")
    }
}

#[cfg(windows)]
fn launch_chrome() -> Result<(), String> {
    let pf = program_files();
    let pf86 = program_files_x86();
    let local = local_app_data();
    let paths = vec![
        pf.join("Google")
            .join("Chrome")
            .join("Application")
            .join("chrome.exe"),
        pf86
            .join("Google")
            .join("Chrome")
            .join("Application")
            .join("chrome.exe"),
        local
            .join("Google")
            .join("Chrome")
            .join("Application")
            .join("chrome.exe"),
    ];
    try_spawn_paths(&paths, "Chrome").or_else(|_| {
        spawn_simple(&["chrome"], "Chrome").map_err(|e| format!("Chrome: {}", e))
    })
}

#[cfg(windows)]
fn launch_edge() -> Result<(), String> {
    let pf = program_files();
    let pf86 = program_files_x86();
    let paths = vec![
        pf86
            .join("Microsoft")
            .join("Edge")
            .join("Application")
            .join("msedge.exe"),
        pf.join("Microsoft")
            .join("Edge")
            .join("Application")
            .join("msedge.exe"),
    ];
    try_spawn_paths(&paths, "Edge").or_else(|_| {
        spawn_simple(&["msedge"], "Edge").map_err(|e| format!("Edge: {}", e))
    })
}

#[cfg(windows)]
fn launch_discord() -> Result<(), String> {
    let local = local_app_data();
    let update = local.join("Discord").join("Update.exe");
    if update.exists() {
        return Command::new(&update)
            .args(["--processStart", "Discord.exe"])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Discord: {}", e));
    }
    spawn_simple(&["discord"], "Discord").map_err(|e| format!("Discord: {}", e))
}

#[cfg(windows)]
fn launch_obs() -> Result<(), String> {
    let pf = program_files();
    let pf86 = program_files_x86();
    let local = local_app_data();
    let candidates = vec![
        pf.join("obs-studio").join("bin").join("64bit").join("obs64.exe"),
        pf86
            .join("obs-studio")
            .join("bin")
            .join("64bit")
            .join("obs64.exe"),
        local.join("Programs").join("obs-studio").join("bin").join("64bit").join("obs64.exe"),
    ];
    try_spawn_paths(&candidates, "OBS").or_else(|_| {
        spawn_simple(&["obs64"], "OBS").map_err(|e| format!("OBS: {}", e))
    })
}

#[cfg(windows)]
fn launch_mspaint() -> Result<(), String> {
    let paths = vec![PathBuf::from(r"C:\Windows\System32\mspaint.exe")];
    try_spawn_paths(&paths, "Paint")
        .or_else(|_| spawn_simple(&["mspaint"], "Paint").map_err(|e| format!("{}", e)))
}

#[cfg(windows)]
fn launch_snipping_tool() -> Result<(), String> {
    let local = local_app_data();
    let paths = vec![
        local
            .join("Microsoft")
            .join("WindowsApps")
            .join("SnippingTool.exe"),
        PathBuf::from(r"C:\Program Files\WindowsApps\SnippingTool.exe"),
    ];
    try_spawn_paths(&paths, "Snipping Tool")
        .or_else(|_| spawn_simple(&["SnippingTool"], "Snipping Tool").map_err(|e| format!("{}", e)))
}

#[cfg(windows)]
fn office_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let bases = [program_files(), program_files_x86()];
    for base in bases {
        let mo = base.join("Microsoft Office");
        for rel in [
            mo.join("root").join("Office16"),
            mo.join("root").join("Office15"),
            mo.join("Office16"),
            mo.join("Office15"),
        ] {
            if rel.is_dir() {
                roots.push(rel);
            }
        }
    }
    roots
}

#[cfg(windows)]
fn find_office_exe(exe_name: &str) -> Option<PathBuf> {
    office_roots()
        .into_iter()
        .map(|r| r.join(exe_name))
        .find(|p| p.exists())
}

#[cfg(windows)]
fn launch_office_exe(exe_name: &str) -> Result<(), String> {
    let exe = find_office_exe(exe_name).ok_or_else(|| {
        format!(
            "Microsoft Office executable {} not found (searched standard Office16/Office15 under Program Files).",
            exe_name
        )
    })?;
    windows_gui_spawn(&exe).map_err(|e| format!("Office ({}): {}", exe_name, e))
}

#[cfg(windows)]
fn launch_shell_apps_folder(uri: &str) -> Result<(), String> {
    // explorer + shell:AppsFolder is reliable when spawned with ArgumentList from some hosts (e.g. Tauri).
    let esc = uri.replace('\'', "''");
    let ps_cmd = format!(
        "Start-Process -FilePath explorer.exe -ArgumentList '{}'",
        esc
    );
    if Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &ps_cmd,
        ])
        .spawn()
        .is_ok()
    {
        return Ok(());
    }
    Command::new("explorer.exe")
        .arg(uri)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("explorer: {}", e))
}

/// Close predefined desktop apps by process image name (whitelist only). Windows-only.
/// Uses `taskkill /IM … /T /F` — same allow-list as launch_desktop_app.
pub fn close_desktop_app(app: &str) -> Result<String, String> {
    #[cfg(not(windows))]
    {
        let _ = app;
        return Err("close_desktop_app is only supported on Windows.".to_string());
    }
    #[cfg(windows)]
    {
        close_desktop_app_windows(app)
    }
}

#[cfg(windows)]
fn close_desktop_app_windows(app: &str) -> Result<String, String> {
    let mut key = app.trim().to_lowercase().replace('-', "_");
    if key.contains("bloco") && key.contains("notas") {
        key = "notepad".to_string();
    } else if key.contains("notepad") {
        key = "notepad".to_string();
    }

    let (label, exe_names): (&str, &[&str]) = match key.as_str() {
        "cursor" => ("Cursor", &["Cursor.exe"]),
        "vscode" | "vs_code" | "code" | "visual_studio_code" => (
            "Visual Studio Code",
            &["Code.exe", "Code - Insiders.exe"],
        ),
        "terminal" | "windows_terminal" | "wt" => (
            "Windows Terminal",
            &[
                "WindowsTerminal.exe",
                "WindowsTerminalPreview.exe",
                "wt.exe",
            ],
        ),
        "chrome" | "google_chrome" => ("Google Chrome", &["chrome.exe"]),
        "notepad" | "bloco_de_notas" | "bloco de notas" => {
            ("Bloco de Notas", &["notepad.exe"])
        }
        "edge" | "microsoft_edge" | "msedge" => ("Microsoft Edge", &["msedge.exe"]),
        "discord" => (
            "Discord",
            &[
                "Discord.exe",
                "DiscordPTB.exe",
                "DiscordCanary.exe",
                "DiscordDevelopment.exe",
            ],
        ),
        "obs" | "obs_studio" => ("OBS Studio", &["obs64.exe", "obs32.exe", "obs.exe"]),
        "snipping_tool" | "snipping" | "capture" | "screen_capture" => (
            "Snipping Tool",
            &["SnippingTool.exe", "ScreenSketch.exe"],
        ),
        "media_player" | "groove" | "music" | "zune" => (
            "Windows media player / Groove",
            &[
                "GrooveMusic.exe",
                "Microsoft.Media.Player.exe",
                "Music.UI.exe",
            ],
        ),
        "excel" => ("Microsoft Excel", &["EXCEL.EXE", "excel.exe"]),
        "word" => ("Microsoft Word", &["WINWORD.EXE", "winword.exe"]),
        "powerpoint" | "ppt" => ("Microsoft PowerPoint", &["POWERPNT.EXE", "powerpnt.exe"]),
        "outlook" => ("Microsoft Outlook", &["OUTLOOK.EXE", "outlook.exe"]),
        "paint" | "mspaint" => ("Paint", &["mspaint.exe", "MSPaint.exe"]),
        _ => {
            return Err(format!(
                "Unknown app {:?}. Same ids as launch_desktop_app.",
                app
            ));
        }
    };

    let media_extra = matches!(
        key.as_str(),
        "media_player" | "groove" | "music" | "zune"
    );

    taskkill_any(exe_names, label, media_extra)
}

/// Try `taskkill`, then PowerShell `Stop-Process` by base name (handles edge cases taskkill misses).
/// If `try_media_path_kill`, run an extra script that matches Store/UWP installs under WindowsApps.
#[cfg(windows)]
fn taskkill_any(exe_names: &[&str], friendly_label: &str, try_media_path_kill: bool) -> Result<String, String> {
    let mut killed: Vec<String> = Vec::new();

    for im in exe_names {
        match Command::new("taskkill")
            .args(["/IM", im, "/T", "/F"])
            .output()
        {
            Ok(out) => {
                if out.status.success() {
                    killed.push((*im).to_string());
                }
            }
            Err(_) => {}
        }
    }

    if !killed.is_empty() {
        return Ok(format!(
            "{} fechado (encerrado: {}).",
            friendly_label,
            killed.join(", ")
        ));
    }

    // Phase 2: Stop-Process — works when Image name differs slightly or taskkill fails on hosted shells.
    let bases: Vec<String> = exe_names.iter().map(|e| exe_base_name(e)).collect();
    if let Ok(true) = powershell_stop_named_processes(&bases) {
        return Ok(format!(
            "{} fechado (via Stop-Process: {}).",
            friendly_label,
            bases.join(", ")
        ));
    }

    // Phase 3: Groove / Store media — paths under WindowsApps or Zune package.
    if try_media_path_kill {
        if let Ok(true) = powershell_stop_media_by_path() {
            return Ok(format!(
                "{} fechado (caminho do processo em Groove / Reprodutor / Store).",
                friendly_label
            ));
        }
    }

    Err(format!(
        "Não foi possível fechar {} — nenhum processo correspondente (tentou: {}). Se for app da Microsoft Store, feche manualmente ou confira o nome exato no Gerenciador de Tarefas.",
        friendly_label,
        exe_names.join(", ")
    ))
}

#[cfg(windows)]
fn exe_base_name(im: &str) -> String {
    let s = im.trim();
    let lower = s.to_lowercase();
    if lower.ends_with(".exe") {
        s[..s.len() - 4].to_string()
    } else {
        s.to_string()
    }
}

/// Returns Ok(true) if at least one process was found and Stop-Process ran.
#[cfg(windows)]
fn powershell_stop_named_processes(base_names: &[String]) -> Result<bool, String> {
    if base_names.is_empty() {
        return Ok(false);
    }
    let quoted: Vec<String> = base_names
        .iter()
        .map(|b| format!("'{}'", b.replace('\'', "''")))
        .collect();
    let arr = quoted.join(",");
    let script = format!(
        r#"$stopped = $false
foreach ($name in @({0})) {{
  $procs = Get-Process -Name $name -ErrorAction SilentlyContinue
  if ($null -ne $procs) {{
    $procs | Stop-Process -Force -ErrorAction SilentlyContinue
    $stopped = $true
  }}
}}
if ($stopped) {{ exit 0 }} else {{ exit 1 }}"#,
        arr
    );

    let out = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(out.status.success())
}

/// Kill processes whose main module path looks like Groove / Zune / modern Media Player (WindowsApps).
#[cfg(windows)]
fn powershell_stop_media_by_path() -> Result<bool, String> {
    let script = r#"
$stopped = $false
Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
  try {
    $path = $_.Path
    if (-not $path) { return }
    $p = $path.ToLowerInvariant()
    if ($p -like '*\windowsapps\*' -and ($p -like '*zunemusic*' -or $p -like '*groove*' -or $p -like '*media.player*' -or $p -like '*music.ui*')) {
      Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
      $stopped = $true
    }
  } catch {}
}
if ($stopped) { exit 0 } else { exit 1 }
"#;

    let out = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(out.status.success())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 4 — image_generation (Stable Diffusion local)
// ─────────────────────────────────────────────────────────────────────────────

/// Gera uma imagem via Stable Diffusion local (Automatic1111 WebUI API por padrão).
pub async fn image_generation(
    prompt: &str,
    negative_prompt: Option<&str>,
    width: Option<u32>,
    height: Option<u32>,
    steps: Option<u32>,
    sd_url: Option<&str>,
) -> Result<String, String> {
    let base_url = sd_url.unwrap_or("http://127.0.0.1:7860");
    let url = format!("{}/sdapi/v1/txt2img", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "prompt": prompt,
        "negative_prompt": negative_prompt.unwrap_or(""),
        "steps": steps.unwrap_or(20),
        "width": width.unwrap_or(512),
        "height": height.unwrap_or(512),
        "cfg_scale": 7.0,
        "sampler_name": "Euler a",
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("Stable Diffusion não acessível em {}: {}", base_url, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Stable Diffusion API retornou erro {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Resposta inválida do SD: {}", e))?;

    let images = json["images"]
        .as_array()
        .ok_or("Resposta do SD não contém 'images'.")?;

    if images.is_empty() {
        return Err("Nenhuma imagem foi gerada.".into());
    }

    let b64_str = images[0].as_str().ok_or("Formato de imagem inválido.")?;

    let image_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_str)
        .map_err(|e| format!("Falha ao decodificar base64: {}", e))?;

    let out_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Chronos")
        .join("generated");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Falha ao criar diretório de saída: {}", e))?;

    let filename = format!("sd_{}.png", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    let out_path = out_dir.join(&filename);

    std::fs::write(&out_path, &image_bytes)
        .map_err(|e| format!("Falha ao salvar imagem: {}", e))?;

    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", &out_path.to_string_lossy()])
        .spawn();

    Ok(format!("Imagem gerada e salva como {}. Prompt: \"{}\"", out_path.display(), prompt))
}
