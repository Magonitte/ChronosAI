//! file_tools.rs — Tier 1
//! Ferramentas de arquivo: busca, recentes, leitura e escrita com validação sandbox.

use std::path::{Path, PathBuf};
use crate::sandbox::SandboxConfig;

// ---------------------------------------------------------------------------
// Helpers de caminho
// ---------------------------------------------------------------------------

/// Expande `~` para o diretório home. Pastas conhecidas usam `dirs::` (Desktop real no Windows/OneDrive).
fn expand_home(path: &str) -> String {
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        if let Some(home) = dirs::home_dir() {
            let rest = &path[1..].trim_start_matches('/').trim_start_matches('\\');
            if let Some(resolved) = resolve_special_home_subpath(rest) {
                return resolved;
            }
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

/// `~/Desktop/arquivo.txt` → pasta Desktop do sistema (não só `home/Desktop`).
fn resolve_special_home_subpath(rest: &str) -> Option<String> {
    let norm = rest.replace('\\', "/");
    let (head, tail) = match norm.split_once('/') {
        Some((h, t)) => (h.to_lowercase(), Some(t)),
        None => (norm.to_lowercase(), None),
    };
    let with_tail = |base: PathBuf| -> String {
        match tail {
            Some(t) if !t.is_empty() => base.join(t).to_string_lossy().to_string(),
            _ => base.to_string_lossy().to_string(),
        }
    };
    match head.as_str() {
        "desktop" => dirs::desktop_dir().map(with_tail),
        "documents" | "documentos" => dirs::document_dir().map(with_tail),
        "downloads" | "download" => dirs::download_dir().map(with_tail),
        "pictures" | "imagens" | "fotos" => dirs::picture_dir().map(with_tail),
        _ => None,
    }
}

/// Canonicaliza caminho; se o arquivo ainda não existe, canonicaliza o diretório pai.
fn canonicalize_path_best_effort(path: &Path) -> PathBuf {
    if path.exists() {
        return path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    }
    if let Some(parent) = path.parent() {
        if parent.exists() {
            if let Ok(can_parent) = parent.canonicalize() {
                if let Some(name) = path.file_name() {
                    return can_parent.join(name);
                }
            }
        }
    }
    path.to_path_buf()
}

/// Raízes permitidas para um item de `readable_paths` (inclui aliases do sistema).
fn expand_allowed_roots(allowed: &str) -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(expand_home(allowed))];
    let lower = allowed.to_lowercase();
    if lower.contains("desktop") {
        if let Some(d) = dirs::desktop_dir() {
            roots.push(d);
        }
    }
    if lower.contains("document") {
        if let Some(d) = dirs::document_dir() {
            roots.push(d);
        }
    }
    if lower.contains("download") {
        if let Some(d) = dirs::download_dir() {
            roots.push(d);
        }
    }
    if lower.contains("picture") || lower.contains("imagen") || lower.contains("foto") {
        if let Some(d) = dirs::picture_dir() {
            roots.push(d);
        }
    }
    roots
}

fn fold_portuguese_accents(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

/// Chave alfanumérica para comparar nomes (tolera STT: `1.nf1.md` ≈ `1 Néfi 1.md`).
fn normalize_filename_key(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(fold_portuguese_accents)
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn search_query_variants(query: &str) -> Vec<String> {
    let q = query.trim();
    let mut variants = vec![q.to_string()];
    let dots_as_spaces = q.replace('.', " ");
    if dots_as_spaces != q {
        variants.push(dots_as_spaces);
    }
    let lower = q.to_lowercase();
    for prefix in ["primeiro", "primeira"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let tail = rest.trim_start_matches(|c: char| !c.is_alphanumeric());
            if !tail.is_empty() && tail != lower {
                variants.push(tail.to_string());
            }
        }
    }
    variants.sort();
    variants.dedup();
    variants
}

/// Pontuação 0..1 entre consulta (voz/STT) e nome real do arquivo.
pub fn fuzzy_filename_score(query: &str, candidate_name: &str) -> f64 {
    let mut best = 0.0_f64;
    for q in search_query_variants(query) {
        let qk = normalize_filename_key(&q);
        let ck = normalize_filename_key(candidate_name);
        if qk.is_empty() || ck.is_empty() {
            continue;
        }
        let score = if qk == ck {
            1.0
        } else if ck.contains(&qk) || qk.contains(&ck) {
            0.92
        } else {
            strsim::jaro_winkler(&qk, &ck)
        };
        best = best.max(score);
    }
    best
}

const FUZZY_MATCH_THRESHOLD: f64 = 0.72;

/// Pastas do usuário onde buscamos arquivos (sem exigir caminho completo).
fn collect_search_roots(sandbox: &SandboxConfig) -> Vec<PathBuf> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    let mut push = |p: PathBuf| {
        if p.exists() && seen.insert(p.clone()) {
            roots.push(p);
        }
    };
    for allowed in &sandbox.readable_paths {
        for r in expand_allowed_roots(allowed) {
            push(r);
        }
    }
    if let Some(d) = dirs::desktop_dir() {
        push(d);
    }
    if let Some(d) = dirs::document_dir() {
        push(d);
    }
    if let Some(d) = dirs::download_dir() {
        push(d);
    }
    if let Some(d) = dirs::picture_dir() {
        push(d);
    }
    roots
}

/// Rótulo curto da pasta (Área de trabalho, Documentos, …) — sem caminho completo.
pub fn friendly_location_label(path: &Path) -> String {
    let check = |dir: Option<PathBuf>, label: &str| -> Option<String> {
        let d = dir?;
        let dir_c = canonicalize_path_best_effort(&d);
        let path_c = canonicalize_path_best_effort(path);
        if path_c.starts_with(&dir_c) {
            Some(label.to_string())
        } else {
            None
        }
    };
    check(dirs::desktop_dir(), "Área de trabalho")
        .or_else(|| check(dirs::document_dir(), "Documentos"))
        .or_else(|| check(dirs::download_dir(), "Downloads"))
        .or_else(|| check(dirs::picture_dir(), "Imagens"))
        .unwrap_or_else(|| "Pasta do usuário".to_string())
}

fn format_file_hit_line(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    format!("• {} — {}", name, friendly_location_label(path))
}

/// Localiza arquivo por nome em Desktop, Documentos, Downloads, Imagens, etc.
pub fn locate_file_in_sandbox(query: &str, sandbox: &SandboxConfig) -> Option<PathBuf> {
    use walkdir::WalkDir;

    let direct = PathBuf::from(resolve_write_path(query));
    if direct.is_file()
        && direct.exists()
        && is_path_allowed(&direct, sandbox)
    {
        return Some(direct);
    }

    let mut best: Option<(PathBuf, f64)> = None;
    for root in collect_search_roots(sandbox) {
        for entry in WalkDir::new(&root)
            .max_depth(8)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let name = entry.file_name().to_string_lossy();
            let score = fuzzy_filename_score(query, &name);
            if score >= FUZZY_MATCH_THRESHOLD {
                let replace = best
                    .as_ref()
                    .map(|(_, s)| score > *s)
                    .unwrap_or(true);
                if replace {
                    best = Some((entry.path().to_path_buf(), score));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

fn path_under_allowed_root(target: &Path, root: &Path) -> bool {
    let target_c = canonicalize_path_best_effort(target);
    let root_c = canonicalize_path_best_effort(root);
    let target_str = target_c.to_string_lossy().to_lowercase();
    let root_str = root_c.to_string_lossy().to_lowercase();
    if target_str.starts_with(&root_str) {
        return true;
    }
    // Fallback textual (paths ainda não criados)
    target
        .to_string_lossy()
        .to_lowercase()
        .starts_with(&root.to_string_lossy().to_lowercase())
}

/// Resolve aliases de pasta (PT/EN), `~` e nomes simples (ex.: `oi.txt` → Desktop).
pub fn resolve_write_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }

    if (trimmed.len() >= 3 && trimmed.as_bytes().get(1) == Some(&b':'))
        || trimmed.starts_with("\\\\")
    {
        return trimmed.to_string();
    }

    let expanded = expand_home(trimmed);
    if expanded != trimmed {
        return expanded;
    }

    let lower = trimmed.to_lowercase();
    let folder_aliases: &[(&str, fn() -> Option<PathBuf>)] = &[
        ("area de trabalho/", dirs::desktop_dir),
        ("área de trabalho/", dirs::desktop_dir),
        ("desktop/", dirs::desktop_dir),
        ("documentos/", dirs::document_dir),
        ("documents/", dirs::document_dir),
        ("downloads/", dirs::download_dir),
        ("download/", dirs::download_dir),
    ];
    for (prefix, dir_fn) in folder_aliases {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if let Some(dir) = dir_fn() {
                return dir.join(rest).to_string_lossy().to_string();
            }
        }
    }

    // Nome simples (ex.: "oi.txt") → Área de trabalho (comportamento esperado por voz).
    if !trimmed.contains('\\') && !trimmed.contains('/') {
        if let Some(desktop) = dirs::desktop_dir() {
            return desktop.join(trimmed).to_string_lossy().to_string();
        }
    }

    trimmed.to_string()
}

/// Verifica se `path` está dentro de uma das `readable_paths` do sandbox (ou do workspace).
pub fn is_path_allowed(path: &Path, sandbox: &SandboxConfig) -> bool {
    for allowed in &sandbox.readable_paths {
        for root in expand_allowed_roots(allowed) {
            if path_under_allowed_root(path, &root) {
                return true;
            }
        }
    }

    let ws = PathBuf::from(&sandbox.workspace);
    path_under_allowed_root(path, &ws)
}

// ---------------------------------------------------------------------------
// search_files
// ---------------------------------------------------------------------------

/// Busca arquivos por nome nas pastas do usuário (fuzzy; retorna pasta, não caminho completo).
pub fn search_files(query: &str, max_results: usize, sandbox: &SandboxConfig) -> Result<String, String> {
    use walkdir::WalkDir;

    if query.trim().is_empty() {
        return Err("Consulta de busca vazia.".into());
    }

    let query_lower = query.to_lowercase();
    let mut scored: Vec<(PathBuf, f64)> = Vec::new();

    for root in collect_search_roots(sandbox) {
        for entry in WalkDir::new(&root)
            .max_depth(8)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let name = entry.file_name().to_string_lossy();
            let name_lower = name.to_lowercase();
            let fuzzy = fuzzy_filename_score(query, &name);
            let substring = name_lower.contains(&query_lower);
            if fuzzy >= FUZZY_MATCH_THRESHOLD || substring {
                let score = if fuzzy >= FUZZY_MATCH_THRESHOLD {
                    fuzzy
                } else {
                    0.75
                };
                scored.push((entry.path().to_path_buf(), score));
            }
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.dedup_by(|a, b| a.0 == b.0);
    let found: Vec<PathBuf> = scored
        .into_iter()
        .take(max_results)
        .map(|(p, _)| p)
        .collect();

    if found.is_empty() {
        Ok(format!("Nenhum arquivo encontrado para '{}'.", query))
    } else {
        let count = found.len();
        let list = found
            .iter()
            .map(|p| format_file_hit_line(p.as_path()))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!("Encontrei {} arquivo(s) para '{}':\n{}", count, query, list))
    }
}

// ---------------------------------------------------------------------------
// get_recent_files
// ---------------------------------------------------------------------------

/// Retorna os arquivos mais recentemente acessados no Windows (pasta Recent).
pub fn get_recent_files(max: usize) -> Result<String, String> {
    let script = format!(
        r#"Get-ChildItem "$env:APPDATA\Microsoft\Windows\Recent" -ErrorAction SilentlyContinue |
  Where-Object {{ $_.Extension -eq '.lnk' }} |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First {max} |
  ForEach-Object {{ $_.Name -replace '\.lnk$','' }}"#,
        max = max
    );

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("get_recent_files: {}", e))?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Ok("Nenhum arquivo recente encontrado.".into())
        } else {
            Ok(format!("Arquivos recentes:\n{}", text))
        }
    } else {
        Err(format!(
            "get_recent_files: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

// ---------------------------------------------------------------------------
// read_file
// ---------------------------------------------------------------------------

/// Lê o conteúdo de um arquivo de texto, validando que está dentro do sandbox.
pub fn read_file(path: &str, sandbox: &SandboxConfig) -> Result<String, String> {
    let file_path = locate_file_in_sandbox(path, sandbox).ok_or_else(|| {
        format!(
            "Arquivo não encontrado: '{}' (busquei em Área de trabalho, Documentos, Downloads e Imagens).",
            path
        )
    })?;

    if !is_path_allowed(&file_path, sandbox) {
        return Err(format!(
            "Acesso negado: '{}' está fora das pastas permitidas (sandbox).",
            path
        ));
    }

    if file_path.is_dir() {
        return Err(format!("'{}' é um diretório, não um arquivo.", file_path.display()));
    }

    // Limita a 512 KB para não sobrecarregar o contexto do LLM
    let metadata = std::fs::metadata(&file_path).map_err(|e| format!("Metadados: {}", e))?;
    const MAX_BYTES: u64 = 512 * 1024;
    if metadata.len() > MAX_BYTES {
        return Err(format!(
            "Arquivo muito grande ({:.1}KB). Máximo permitido: 512KB.",
            metadata.len() as f64 / 1024.0
        ));
    }

    std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Erro ao ler '{}': {}", path, e))
}

// ---------------------------------------------------------------------------
// write_file
// ---------------------------------------------------------------------------

/// Escreve conteúdo em um arquivo, validando que está dentro do sandbox.
pub fn write_file(
    path: &str,
    content: &str,
    overwrite: bool,
    sandbox: &SandboxConfig,
) -> Result<String, String> {
    let expanded = resolve_write_path(path);
    let file_path = Path::new(&expanded);

    if !is_path_allowed(file_path, sandbox) {
        return Err(format!(
            "Acesso negado: '{}' está fora das pastas permitidas (sandbox). Caminho resolvido: '{}'.",
            path,
            file_path.display()
        ));
    }

    if file_path.exists() && !overwrite {
        return Err(format!(
            "Arquivo '{}' já existe. Use overwrite: true para sobrescrever.",
            path
        ));
    }

    if let Some(parent) = file_path.parent() {
        if !parent.exists() {
            return Err(format!(
                "Diretório pai não existe: '{}'",
                parent.display()
            ));
        }
    }

    std::fs::write(file_path, content)
        .map_err(|e| format!("Erro ao escrever '{}': {}", path, e))?;

    Ok(format!("Arquivo salvo: {}", file_path.display()))
}

#[cfg(test)]
mod resolve_write_path_tests {
    use super::{expand_home, is_path_allowed, resolve_write_path};
    use crate::sandbox::SandboxConfig;

    #[test]
    fn expand_home_desktop_uses_system_folder() {
        let expanded = expand_home("~/Desktop/teste.txt");
        if let Some(desktop) = dirs::desktop_dir() {
            let expected = desktop.join("teste.txt");
            assert_eq!(
                std::path::Path::new(&expanded),
                expected.as_path(),
                "expand_home should match dirs::desktop_dir"
            );
        }
    }

    #[test]
    fn sandbox_allows_desktop_write_before_file_exists() {
        let sandbox = SandboxConfig::default();
        if let Some(desktop) = dirs::desktop_dir() {
            let target = desktop.join("dexter_sandbox_test_8.txt");
            assert!(
                is_path_allowed(&target, &sandbox),
                "desktop write path should be allowed before file exists: {}",
                target.display()
            );
        }
    }

    #[test]
    fn bare_filename_goes_to_desktop() {
        let resolved = resolve_write_path("oi.txt");
        let lower = resolved.to_lowercase();
        assert!(
            lower.contains("desktop") || lower.contains("área de trabalho"),
            "expected desktop path, got {resolved}"
        );
        assert!(lower.ends_with("oi.txt") || lower.ends_with("oi.txt\\"));
    }

    #[test]
    fn desktop_alias_expands() {
        let resolved = resolve_write_path("~/Desktop/teste.txt");
        assert!(resolved.contains("teste.txt"));
    }

    #[test]
    fn bare_filename_allowed_for_read() {
        let resolved = resolve_write_path("love.txt");
        let sandbox = SandboxConfig::default();
        assert!(
            is_path_allowed(std::path::Path::new(&resolved), &sandbox),
            "read path should be sandbox-allowed: {resolved}"
        );
    }
}

#[cfg(test)]
mod fuzzy_filename_tests {
    use super::fuzzy_filename_score;

    #[test]
    fn stt_dot_typo_matches_nefi() {
        let score = fuzzy_filename_score("1.nf1.md", "1 Néfi 1.md");
        assert!(
            score >= 0.72,
            "expected fuzzy match for STT typo, got {score}"
        );
    }

    #[test]
    fn exact_name_scores_high() {
        assert!(fuzzy_filename_score("love.txt", "love.txt") >= 0.99);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 2 — transcribe_audio_file
// ─────────────────────────────────────────────────────────────────────────────

/// Transcreve um arquivo de áudio local usando o servidor Whisper HTTP.
pub async fn transcribe_audio_file(
    path: &str,
    whisper_url: &str,
    sandbox: &SandboxConfig,
) -> Result<String, String> {
    let resolved = expand_home(path);
    let resolved_path = Path::new(&resolved);

    if !is_path_allowed(resolved_path, sandbox) {
        return Err(format!("Acesso negado: '{}' não está nas pastas permitidas.", resolved));
    }
    if !resolved_path.exists() {
        return Err(format!("Arquivo não encontrado: {}", resolved));
    }

    let bytes = std::fs::read(resolved_path)
        .map_err(|e| format!("Erro ao ler arquivo de áudio: {}", e))?;
    if bytes.is_empty() {
        return Err("Arquivo de áudio vazio.".into());
    }

    let file_name = resolved_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();
    let mime = if file_name.ends_with(".mp3") {
        "audio/mpeg"
    } else if file_name.ends_with(".m4a") {
        "audio/mp4"
    } else if file_name.ends_with(".ogg") {
        "audio/ogg"
    } else {
        "audio/wav"
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(mime)
        .map_err(|e| format!("MIME: {}", e))?;
    let form = reqwest::multipart::Form::new()
        .text("model", "whisper")
        .text("language", "pt")
        .part("file", part);

    let url = format!("{}/v1/audio/transcriptions", whisper_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Whisper request: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Whisper error: {}", body));
    }

    #[derive(serde::Deserialize)]
    struct Resp {
        text: String,
    }
    let result: Resp = resp.json().await.map_err(|e| format!("JSON parse: {}", e))?;
    Ok(result.text.trim().to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 2 — run_powershell_script
// ─────────────────────────────────────────────────────────────────────────────

/// Executa um script PowerShell (.ps1) dentro do sandbox.
pub fn run_powershell_script(
    path: &str,
    sandbox: &SandboxConfig,
    audit: &std::sync::Mutex<crate::sandbox::AuditLog>,
) -> Result<String, String> {
    let resolved = expand_home(path);
    let resolved_path = Path::new(&resolved);

    if !is_path_allowed(resolved_path, sandbox) {
        return Err(format!("Acesso negado: '{}' não está nas pastas permitidas.", resolved));
    }
    if !resolved_path.exists() {
        return Err(format!("Script não encontrado: {}", resolved));
    }
    let ext = resolved_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "ps1" {
        return Err("Apenas scripts .ps1 são suportados.".into());
    }

    let command = format!(
        "powershell.exe -ExecutionPolicy Bypass -File \"{}\"",
        resolved
    );
    crate::sandbox::execute(&command, sandbox, audit)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 3 — watch_file (notify crate + schedule_notification)
// ─────────────────────────────────────────────────────────────────────────────
use tokio::sync::mpsc;

/// Vigia um arquivo por mudanças durante N segundos.
/// Quando o arquivo é modificado, dispara uma notificação via callback.
pub async fn watch_file(
    path: &str,
    duration_seconds: u64,
    sandbox: &SandboxConfig,
    on_change: Option<String>,
) -> Result<String, String> {
    use notify::{EventKind, RecursiveMode, Watcher};

    let resolved = expand_home(path);
    let resolved_path = Path::new(&resolved);

    if !is_path_allowed(resolved_path, sandbox) {
        return Err(format!("Acesso negado: '{}' não está nas pastas permitidas.", resolved));
    }
    if !resolved_path.exists() && !resolved_path.parent().map(|p| p.exists()).unwrap_or(false) {
        return Err(format!("Arquivo ou diretório não encontrado: {}", resolved));
    }

    let (tx, mut rx) = mpsc::channel::<notify::Result<notify::Event>>(16);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let _ = tx.blocking_send(res);
    })
    .map_err(|e| format!("Watcher: {}", e))?;

    let watch_target: PathBuf = if resolved_path.exists() && resolved_path.is_file() {
        resolved_path.parent().unwrap_or(&resolved_path).to_path_buf()
    } else if resolved_path.exists() && resolved_path.is_dir() {
        resolved_path.to_path_buf()
    } else {
        // Arquivo não existe ainda — vigia o diretório pai
        resolved_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| resolved_path.to_path_buf())
    };

    watcher
        .watch(&watch_target, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Watch: {}", e))?;

    let change_msg = on_change.unwrap_or_else(|| format!("Arquivo {} foi modificado.", path));
    let path_owned = path.to_string();

    let (result_tx, mut result_rx) = mpsc::channel::<String>(1);

    tokio::spawn(async move {
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(duration_seconds));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(Ok(event)) => {
                            let relevant = matches!(
                                event.kind,
                                EventKind::Modify(_) | EventKind::Create(_)
                            );
                            if relevant {
                                let name = event.paths.first()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                // Verifica se o path contém o nome do arquivo alvo
                                if name.contains(&path_owned) || path_owned.contains(&name) || path_owned == "any" {
                                    let _ = result_tx.send(format!("Mudança detectada: {}", change_msg)).await;
                                    break;
                                }
                            }
                        }
                        Some(Err(_)) | None => break,
                    }
                }
                _ = &mut timeout => {
                    let _ = result_tx.send(format!("Timeout: nenhuma mudança em {} após {}s.", path_owned, duration_seconds)).await;
                    break;
                }
            }
        }
        // Watcher é droppado ao sair do escopo
        drop(watcher);
    });

    match result_rx.recv().await {
        Some(msg) => Ok(msg),
        None => Ok(format!("Vigilância de {} encerrada.", path)),
    }
}
