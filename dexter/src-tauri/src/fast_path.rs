use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Acao que o fast-path pode executar diretamente, sem passar pelo LLM.
#[derive(Debug, Clone)]
pub struct FastAction {
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub tts_template: String,
    pub needs_vision: bool,
    pub vision_complexity: Option<VisionComplexity>,
    /// Se true, a resposta precisa de LLM para formatar o resultado da tool
    pub needs_llm_formatting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionComplexity {
    Simple,
    Complex,
}

pub enum FastPathResult {
    Hit(FastAction),
    Miss,
}

// ---------------------------------------------------------------------------
// Fast command database
// ---------------------------------------------------------------------------

type ParamExtractor = fn(&str) -> Option<HashMap<String, String>>;

struct FastCommand {
    intent: &'static str,
    patterns: &'static [&'static str],
    tool_name: &'static str,
    tts_template: &'static str,
    needs_vision: bool,
    param_extractor: ParamExtractor,
}

static FAST_COMMANDS: OnceLock<Vec<FastCommand>> = OnceLock::new();

fn fast_commands() -> &'static [FastCommand] {
    FAST_COMMANDS.get_or_init(|| {
        vec![
            // ── Tempo/Data ──
            FastCommand {
                intent: "get_time",
                patterns: &[
                    "que horas sao",
                    "qual a hora",
                    "me diga as horas",
                    "horario agora",
                    "hora atual",
                    "que horas",
                    "me fala as horas",
                ],
                tool_name: "get_current_time",
                tts_template: "Agora são {time}, {date}",
                needs_vision: false,
                param_extractor: |_| None,
            },
            FastCommand {
                intent: "get_date",
                patterns: &[
                    "qual a data",
                    "que dia e hoje",
                    "data de hoje",
                    "dia de hoje",
                    "que dia hoje",
                ],
                tool_name: "get_current_time",
                tts_template: "Hoje é {date}",
                needs_vision: false,
                param_extractor: |_| None,
            },
            // ── Volume ──
            FastCommand {
                intent: "volume_up",
                patterns: &[
                    "aumenta o volume",
                    "sobe o volume",
                    "volume mais alto",
                    "aumenta o som",
                    "sobe o som",
                    "mais volume",
                    "aumenta volume",
                    "aumentar volume",
                    "aumentar o volume",
                ],
                tool_name: "adjust_system_volume",
                tts_template: "Volume aumentado",
                needs_vision: false,
                param_extractor: |text| {
                    let steps = extract_number(text).unwrap_or(3);
                    let mut args = HashMap::new();
                    args.insert("action".into(), "up".into());
                    args.insert("steps".into(), steps.to_string());
                    Some(args)
                },
            },
            FastCommand {
                intent: "volume_down",
                patterns: &[
                    "diminui o volume",
                    "abaixa o volume",
                    "volume mais baixo",
                    "diminui o som",
                    "abaixa o som",
                    "menos volume",
                    "diminui volume",
                    "diminuir volume",
                    "diminuir o volume",
                    "abaixar o volume",
                ],
                tool_name: "adjust_system_volume",
                tts_template: "Volume reduzido",
                needs_vision: false,
                param_extractor: |text| {
                    let steps = extract_number(text).unwrap_or(3);
                    let mut args = HashMap::new();
                    args.insert("action".into(), "down".into());
                    args.insert("steps".into(), steps.to_string());
                    Some(args)
                },
            },
            FastCommand {
                intent: "volume_mute",
                patterns: &[
                    "silenciar",
                    "mudo",
                    "desmutar",
                    "tirar mudo",
                    "ligar som",
                    "desligar som",
                    "mutar",
                ],
                tool_name: "adjust_system_volume",
                tts_template: "Som alternado",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("action".into(), "mute_toggle".into());
                    Some(args)
                },
            },
            // ── Midia ──
            FastCommand {
                intent: "media_pause",
                patterns: &[
                    "pausar musica",
                    "pausa a musica",
                    "parar musica",
                    "pausar",
                    "pausa",
                ],
                tool_name: "control_media_playback",
                tts_template: "Música pausada",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("action".into(), "pause".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "media_play",
                patterns: &[
                    "tocar musica",
                    "continuar musica",
                    "play",
                    "tocar",
                    "retomar musica",
                    "despausar",
                ],
                tool_name: "control_media_playback",
                tts_template: "Reproduzindo",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("action".into(), "play".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "media_next",
                patterns: &[
                    "proxima musica",
                    "proxima faixa",
                    "pular musica",
                    "pular faixa",
                    "next",
                    "avancar",
                    "avancar musica",
                ],
                tool_name: "control_media_playback",
                tts_template: "Próxima faixa",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("action".into(), "next".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "media_previous",
                patterns: &[
                    "musica anterior",
                    "faixa anterior",
                    "voltar musica",
                    "voltar faixa",
                    "previous",
                    "voltar",
                ],
                tool_name: "control_media_playback",
                tts_template: "Faixa anterior",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("action".into(), "previous".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "media_status",
                patterns: &[
                    "que musica esta tocando",
                    "qual musica esta tocando",
                    "o que esta tocando",
                    "status da musica",
                    "musica atual",
                ],
                tool_name: "control_media_playback",
                tts_template: "{status}",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("action".into(), "status".into());
                    Some(args)
                },
            },
            // ── Apps ──
            FastCommand {
                intent: "open_chrome",
                patterns: &[
                    "abre o chrome",
                    "abri o chrome",
                    "abra o chrome",
                    "abrir chrome",
                    "abrir o chrome",
                    "abre chrome",
                    "abre o crome",
                    "abri o crome",
                    "abra o crome",
                    "abrir crome",
                    "abrir o crome",
                    "iniciar chrome",
                    "abrir google chrome",
                    "abre o google chrome",
                    "abri o google chrome",
                ],
                tool_name: "launch_desktop_app",
                tts_template: "Abrindo Chrome",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "chrome".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "open_notepad",
                patterns: &[
                    "abre o bloco de notas",
                    "abrir bloco de notas",
                    "abre bloco de notas",
                    "abra o bloco de notas",
                    "abrir o bloco de notas",
                    "abre o notepad",
                    "abrir notepad",
                ],
                tool_name: "launch_desktop_app",
                tts_template: "Abrindo o Bloco de Notas",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "notepad".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "open_vscode",
                patterns: &[
                    "abre o vs code",
                    "abrir vs code",
                    "abre o vscode",
                    "abrir vscode",
                    "iniciar vs code",
                    "abrir visual studio code",
                ],
                tool_name: "launch_desktop_app",
                tts_template: "Abrindo VS Code",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "vscode".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "open_terminal",
                patterns: &[
                    "abre o terminal",
                    "abrir terminal",
                    "abre terminal",
                    "iniciar terminal",
                    "abrir o terminal",
                ],
                tool_name: "launch_desktop_app",
                tts_template: "Abrindo Terminal",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "terminal".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "open_edge",
                patterns: &[
                    "abre o edge",
                    "abrir edge",
                    "abre edge",
                    "iniciar edge",
                    "abrir o edge",
                    "abrir microsoft edge",
                ],
                tool_name: "launch_desktop_app",
                tts_template: "Abrindo Edge",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "edge".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "open_discord",
                patterns: &[
                    "abre o discord",
                    "abrir discord",
                    "abre discord",
                    "iniciar discord",
                ],
                tool_name: "launch_desktop_app",
                tts_template: "Abrindo Discord",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "discord".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "open_media_player",
                patterns: &[
                    "abre o reprodutor de musica",
                    "abrir reprodutor de musica",
                    "abre o reprodutor",
                    "abrir reprodutor",
                    "abre o groove",
                    "abrir groove music",
                    "abre o windows media player",
                ],
                tool_name: "launch_desktop_app",
                tts_template: "Abrindo reprodutor de música",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "media_player".into());
                    Some(args)
                },
            },
            // ── Apps: fechar ──
            FastCommand {
                intent: "close_chrome",
                patterns: &[
                    "fecha o chrome",
                    "feche o chrome",
                    "fechar chrome",
                    "fechar o chrome",
                    "fecha chrome",
                ],
                tool_name: "close_desktop_app",
                tts_template: "Fechando Chrome",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "chrome".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "close_media_player",
                patterns: &[
                    "feche o player",
                    "fecha o player",
                    "fechar o player",
                    "feche o player de musica",
                    "fecha o player de musica",
                    "fechar o player de musica",
                    "feche o reprodutor",
                    "fecha o reprodutor",
                    "fechar o reprodutor",
                    "feche o reprodutor de musica",
                    "fecha o reprodutor de musica",
                ],
                tool_name: "close_desktop_app",
                tts_template: "Fechando o reprodutor de música",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "media_player".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "close_vscode",
                patterns: &[
                    "fecha o vs code",
                    "fechar vs code",
                    "fecha o vscode",
                    "fechar vscode",
                ],
                tool_name: "close_desktop_app",
                tts_template: "Fechando VS Code",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "vscode".into());
                    Some(args)
                },
            },
            FastCommand {
                intent: "close_notepad",
                patterns: &[
                    "fecha o bloco de notas",
                    "fechar bloco de notas",
                    "fecha o bloco de nota",
                    "fechar bloco de nota",
                    "fecha o notepad",
                    "fechar notepad",
                    "fechar o bloco de notas",
                ],
                tool_name: "close_desktop_app",
                tts_template: "Fechando o Bloco de Notas",
                needs_vision: false,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert("app".into(), "notepad".into());
                    Some(args)
                },
            },
            // ── Estado do Sistema ──
            FastCommand {
                intent: "list_apps",
                patterns: &[
                    "quais aplicativos estao abertos",
                    "quais apps estao abertos",
                    "o que esta aberto",
                    "quais programas estao rodando",
                    "lista os aplicativos",
                    "quais janelas estao abertas",
                    "lista de apps",
                    "lista de aplicativos",
                ],
                tool_name: "list_running_apps",
                tts_template: "Aplicativos abertos: {apps}",
                needs_vision: false,
                param_extractor: |_| None,
            },
            // ── Visao ──
            FastCommand {
                intent: "describe_screen",
                patterns: &[
                    "o que esta na tela",
                    "o que voce ve na tela",
                    "descreve a tela",
                    "o que aparece na tela",
                    "o que tem na tela",
                    "me mostra a tela",
                    "olha a tela",
                    "o que esta aparecendo",
                    "descreva a tela",
                    "descreva o que voce ve",
                    "o que voce esta vendo",
                    "o que voce ve",
                ],
                tool_name: "take_screenshot",
                tts_template: "",
                needs_vision: true,
                param_extractor: |_| None,
            },
            FastCommand {
                intent: "read_error",
                patterns: &[
                    "o que esta escrito nesse erro",
                    "leia o erro",
                    "qual e o erro",
                    "o que diz o erro",
                    "tem algum erro",
                    "leia a mensagem de erro",
                ],
                tool_name: "take_screenshot",
                tts_template: "",
                needs_vision: true,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert(
                        "question".into(),
                        "Leia e descreva apenas os erros ou mensagens de erro visíveis na tela"
                            .into(),
                    );
                    Some(args)
                },
            },
            FastCommand {
                intent: "what_apps_open_vision",
                patterns: &[
                    "quais apps estao abertos na tela",
                    "o que esta aberto na tela",
                    "quais janelas estao visiveis",
                ],
                tool_name: "take_screenshot",
                tts_template: "",
                needs_vision: true,
                param_extractor: |_| {
                    let mut args = HashMap::new();
                    args.insert(
                        "question".into(),
                        "Quais aplicativos estão abertos e visíveis? Responda em uma frase curta."
                            .into(),
                    );
                    Some(args)
                },
            },
            // ── Musica (shuffle) ──
            FastCommand {
                intent: "shuffle_all",
                patterns: &[
                    "toca musica",
                    "tocar musica",
                    "toca musica aleatoria",
                    "tocar musica aleatoria",
                    "toca alguma musica",
                    "tocar alguma musica",
                    "embaralhar musicas",
                    "tocar todas as musicas",
                    "reproduzir todas as musicas",
                    "shuffle",
                    "tocar shuffle",
                ],
                tool_name: "native_music_library_shuffle_play",
                tts_template: "Tocando todas as músicas em modo aleatório",
                needs_vision: false,
                param_extractor: |_| None,
            },
        ]
    })
}

// ---------------------------------------------------------------------------
// Normalization & helpers
// ---------------------------------------------------------------------------

fn query_has_open_verb(query: &str) -> bool {
    query.contains("abre ")
        || query.contains("abri ")
        || query.contains("abra ")
        || query.starts_with("abre ")
        || query.starts_with("abri ")
        || query.starts_with("abra ")
        || query.contains("abrir ")
        || query.contains("iniciar ")
        || query.contains("abre o ")
        || query.contains("abri o ")
        || query.contains("abra o ")
}

/// STT-tolerant open-app shortcuts (before generic pattern table).
fn keyword_open_app(query: &str) -> Option<FastAction> {
    if !query_has_open_verb(query) {
        return None;
    }
    if query.contains("chrome")
        || query.contains("crome")
        || query.contains("google chrome")
    {
        return Some(build_action(
            fast_commands()
                .iter()
                .find(|c| c.intent == "open_chrome")?,
            query,
        ));
    }
    if query.contains("bloco de notas") || query.contains("notepad") {
        return Some(build_action(
            fast_commands()
                .iter()
                .find(|c| c.intent == "open_notepad")?,
            query,
        ));
    }
    if query.contains("excel") || query.contains("exo") {
        return Some(FastAction {
            tool_name: "launch_desktop_app".into(),
            tool_args: serde_json::json!({"app": "excel"}),
            tts_template: "Abrindo Excel".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }
    if query.contains("word") || query.contains("world") {
        return Some(FastAction {
            tool_name: "launch_desktop_app".into(),
            tool_args: serde_json::json!({"app": "word"}),
            tts_template: "Abrindo Word".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }
    if query.contains("paint") || query.contains("pente") || query.contains("paete") {
        return Some(FastAction {
            tool_name: "launch_desktop_app".into(),
            tool_args: serde_json::json!({"app": "paint"}),
            tts_template: "Abrindo Paint".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }
    None
}

/// "Toque Sumirigusa da Enya no YouTube" → (title, artist).
pub fn extract_youtube_music_query(query: &str) -> Option<(String, Option<String>)> {
    let q = normalize_text(query);
    if !q.contains("youtube") {
        return None;
    }
    let is_play = q.contains("toque")
        || q.contains("toca")
        || q.contains("tocar")
        || q.contains("reproduz")
        || q.contains("coloca")
        || q.contains("ponha");
    if !is_play {
        return None;
    }

    let mut s = q.clone();
    for prefix in [
        "toque a musica",
        "toca a musica",
        "tocar a musica",
        "toque",
        "toca",
        "tocar",
        "reproduz",
        "coloca",
        "ponha",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
            break;
        }
    }
    for suffix in ["no youtube", "no yt", "youtube", "por favor"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest.trim().to_string();
        }
    }
    if s.is_empty() {
        return None;
    }

    for sep in [" da ", " do ", " de "] {
        if let Some((title, artist)) = s.split_once(sep) {
            let title = title.trim();
            let artist = artist.trim();
            if !title.is_empty() && !artist.is_empty() {
                return Some((title.to_string(), Some(artist.to_string())));
            }
        }
    }
    Some((s.to_string(), None))
}

fn fold_portuguese_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

pub fn normalize_text(text: &str) -> String {
    let normalized = fold_portuguese_accents(&text.to_lowercase());
    let re_strip = regex::Regex::new(r"[^\w\s]").unwrap();
    let cleaned = re_strip.replace_all(&normalized, " ");
    let re_space = regex::Regex::new(r"\s+").unwrap();
    re_space.replace_all(&cleaned, " ").trim().to_string()
}

/// Token delimitado por espaço (evita falso positivo "play" em "player").
fn query_has_word(query: &str, word: &str) -> bool {
    let padded = format!(" {} ", query);
    padded.contains(&format!(" {word} "))
}

fn query_mentions_reminder(query: &str) -> bool {
    query.contains("avisa")
        || query.contains("avise")
        || query.contains("avisar")
        || query.contains("lembra")
        || query.contains("lembre")
        || query.contains("lembrar")
        || query.contains("alarme")
        || query.contains("timer")
        || query.contains("lembrete")
}

/// "me avisa em 30 segundos" / "lembra em 5 minutos" → segundos até o toast.
fn parse_reminder_relative_delay(query: &str) -> Option<u64> {
    if !query_mentions_reminder(query) {
        return None;
    }
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(\d+)\s*(segundos?|seg|s|minutos?|min|m|horas?|hora|h)\b").unwrap()
    });
    let caps = re.captures(query)?;
    let n: u64 = caps[1].parse().ok()?;
    let unit = caps.get(2)?.as_str();
    if unit.starts_with('s') || unit == "seg" {
        Some(n)
    } else if unit.starts_with('m') || unit == "min" {
        n.checked_mul(60)
    } else {
        n.checked_mul(3600)
    }
}

/// Texto do lembrete após o trecho de tempo (evita confundir "da que" com "que …").
fn parse_reminder_message(query: &str) -> String {
    static RE_DELAY: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re_delay = RE_DELAY.get_or_init(|| {
        regex::Regex::new(r"\d+\s*(segundos?|seg|s|minutos?|min|m|horas?|hora|h)\b").unwrap()
    });
    if let Some(m) = re_delay.find(query) {
        if let Some(msg) = extract_reminder_message_after(&query[m.end()..]) {
            return msg;
        }
    }

    static RE_COMPACT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    if let Some(m) = RE_COMPACT
        .get_or_init(|| regex::Regex::new(r"\d{1,2}h\d{2}\b").unwrap())
        .find(query)
    {
        if let Some(msg) = extract_reminder_message_after(&query[m.end()..]) {
            return msg;
        }
    }

    static RE_CLOCK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    if let Some(m) = RE_CLOCK
        .get_or_init(|| regex::Regex::new(r"(?:as|a)\s+\d{1,2}\s+\d{2}\b").unwrap())
        .find(query)
    {
        if let Some(msg) = extract_reminder_message_after(&query[m.end()..]) {
            return msg;
        }
    }

    "Lembrete".to_string()
}

fn extract_reminder_message_after(after_time: &str) -> Option<String> {
    let mut tail = after_time.trim();
    for period in ["da manha", "da tarde", "da noite", "da madrugada"] {
        if let Some(rest) = tail.strip_prefix(period) {
            tail = rest.trim();
            break;
        }
        if let Some(pos) = tail.find(period) {
            tail = tail[pos + period.len()..].trim();
            break;
        }
    }

    for prefix in ["de ", "para ", "que "] {
        if let Some(rest) = tail.strip_prefix(prefix) {
            let msg = clean_reminder_message_text(rest);
            if is_valid_reminder_message(&msg) {
                return Some(msg);
            }
        }
    }
    if let Some(pos) = tail.find(" de ") {
        let msg = clean_reminder_message_text(tail[pos + 4..].trim());
        if is_valid_reminder_message(&msg) {
            return Some(msg);
        }
    }
    None
}

fn clean_reminder_message_text(s: &str) -> String {
    static RE_LEAD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE_LEAD.get_or_init(|| {
        regex::Regex::new(r"^\d+\s*(segundos?|seg|s|minutos?|min|m|horas?|hora|h)\s*(de\s+)?")
            .unwrap()
    });
    re.replace(s.trim().trim_end_matches('.').trim(), "")
        .trim()
        .to_string()
}

fn is_valid_reminder_message(msg: &str) -> bool {
    if msg.is_empty() || msg.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    static RE_ONLY_DELAY: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE_ONLY_DELAY
        .get_or_init(|| regex::Regex::new(r"^\d+\s*(segundos?|minutos?|horas?)").unwrap());
    !re.is_match(msg)
}

fn adjust_hour_for_day_period(query: &str, mut hour: u32) -> u32 {
    if query.contains("tarde") || query.contains("noite") {
        if hour < 12 {
            hour += 12;
        }
    } else if (query.contains("manha") || query.contains("madrugada")) && hour == 12 {
        hour = 0;
    }
    hour
}

fn validate_hhmm(hour: u32, minute: u32) -> Option<String> {
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(format!("{hour:02}:{minute:02}"))
}

/// "me lembra às 18:30" / "a 1h30 da manha" → "HH:MM"
fn parse_reminder_at_time(query: &str) -> Option<String> {
    if !query_mentions_reminder(query) {
        return None;
    }

    static RE_COMPACT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    if let Some(caps) = RE_COMPACT
        .get_or_init(|| regex::Regex::new(r"\b(\d{1,2})h(\d{2})\b").unwrap())
        .captures(query)
    {
        let hour = adjust_hour_for_day_period(query, caps[1].parse().ok()?);
        let minute: u32 = caps[2].parse().ok()?;
        return validate_hhmm(hour, minute);
    }

    static RE_CLOCK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    if let Some(caps) = RE_CLOCK
        .get_or_init(|| regex::Regex::new(r"\b(?:as|a)\s+(\d{1,2})\s+(\d{2})\b").unwrap())
        .captures(query)
    {
        let hour = adjust_hour_for_day_period(query, caps[1].parse().ok()?);
        let minute: u32 = caps[2].parse().ok()?;
        return validate_hhmm(hour, minute);
    }

    None
}

fn query_mentions_fx_rate(query: &str) -> bool {
    query.contains("pesquis")
        || query.contains("valor")
        || query.contains("cota")
        || query.contains("cotacao")
        || query.contains("cambio")
        || query.contains("quanto")
        || query.contains("busca")
        || query.contains("moeda")
}

fn detect_fx_pair(query: &str) -> Option<&'static str> {
    if query.contains("iene")
        || query.contains("ieini")
        || query.contains("iene japones")
        || query.contains("yen")
        || query.contains("jpy")
        || query.contains("iem")
        || query_has_word(query, "ien")
        || query.contains(" moeda japonesa")
        || query.contains("valor do in ")
        || query.contains("valor de in ")
        || query.contains(" do in para ")
        || ((query.contains("japones") || query.contains("moeda japonesa"))
            && (query.contains("cota") || query.contains("valor") || query.contains("cambio")))
    {
        return Some("JPY-BRL");
    }
    if query.contains("euro") || query.contains("eur") {
        return Some("EUR-BRL");
    }
    if query.contains("libra") || query.contains("gbp") {
        return Some("GBP-BRL");
    }
    if query.contains("dolar") || query.contains("usd") {
        return Some("USD-BRL");
    }
    None
}

fn query_mentions_weather(query: &str) -> bool {
    query.contains("temperatura")
        || query.contains("clima")
        || query.contains("chuva")
        || query.contains("chover")
        || query.contains("previsao")
        || query.contains("previsao do tempo")
        || query.contains("tempo hoje")
        || query.contains("como esta o tempo")
        || (query.contains("tempo")
            && (query.contains("hoje")
                || query.contains("atual")
                || query.contains("agora")
                || query.contains("amanha")))
}

fn extract_weather_location(query: &str) -> Option<String> {
    for marker in [" em ", " na ", " no "] {
        if let Some(pos) = query.rfind(marker) {
            let mut loc = query[pos + marker.len()..].trim().to_string();
            for suffix in [
                "?",
                ".",
                "!",
                " por favor",
                " hoje",
                " agora",
                " atualmente",
                " atual",
                " amanha",
                " para amanha",
                " depois de amanha",
            ] {
                if let Some(rest) = loc.strip_suffix(suffix) {
                    loc = rest.trim().to_string();
                }
            }
            if !loc.is_empty() {
                return Some(loc);
            }
        }
    }
    None
}

/// `None` = clima agora; `Some(0)` = previsão de hoje; `1` = amanhã; `2` = depois de amanhã.
pub fn extract_weather_day_offset(query: &str) -> Option<usize> {
    if query.contains("depois de amanha") {
        return Some(2);
    }
    if query.contains("amanha") {
        return Some(1);
    }
    if query.contains("previsao") || query.contains("chover") || query.contains("chuva") {
        if query.contains("hoje") {
            return Some(0);
        }
        return Some(1);
    }
    None
}

fn query_wants_close_media_player(query: &str) -> bool {
    let close_verb = query.contains("feche") || query.contains("fecha") || query.contains("fechar");
    if !close_verb {
        return false;
    }
    query.contains("player")
        || query.contains("reprodutor")
        || query.contains("play de musica")
        || (query_has_word(query, "play") && query.contains("musica"))
}

fn query_wants_native_media_player(query: &str) -> bool {
    query.contains("player de musica")
        || query.contains("reprodutor de musica")
        || query.contains("reprodutor multimidia")
        || query.contains("reprodutor midia")
        || (query.contains("player") && query.contains("musica"))
        || query.contains("groove")
        || query.contains("media player")
}

/// "Toca música November Rain" / "toque in the end do link park" (sem YouTube no texto).
fn extract_named_music_query(query: &str) -> Option<(String, Option<String>)> {
    let is_play = query.contains("toque")
        || query.contains("toca")
        || query.contains("tocar")
        || query.contains("reproduz")
        || query.contains("coloca")
        || query.contains("ponha");
    if !is_play {
        return None;
    }

    let mut s = query.to_string();
    for prefix in [
        "toque a musica",
        "toca a musica",
        "tocar a musica",
        "toque musica",
        "toca musica",
        "toque",
        "toca",
        "tocar",
        "reproduz a musica",
        "reproduz",
        "coloca",
        "ponha",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
            break;
        }
    }
    for suffix in [
        "no player de musica",
        "no reprodutor de musica",
        "no reprodutor multimidia",
        "no reprodutor midia",
        "no reprodutor",
        "no groove",
        "no media player",
        "na biblioteca de musicas",
        "na biblioteca",
        "no youtube",
        "no yt",
        "youtube",
        "por favor",
    ] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest.trim().to_string();
        }
    }
    if s.is_empty() || s == "musica" {
        return None;
    }
    const GENERIC: &[&str] = &[
        "aleatoria",
        "aleatorio",
        "alguma",
        "qualquer",
        "shuffle",
        "todas",
        "embaralhar",
    ];
    if GENERIC.iter().any(|g| s == *g) {
        return None;
    }

    for sep in [" da ", " do ", " de "] {
        if let Some((title, artist)) = s.split_once(sep) {
            let title = title.trim();
            let artist = artist.trim();
            if !title.is_empty() && !artist.is_empty() {
                return Some((title.to_string(), Some(artist.to_string())));
            }
        }
    }
    Some((s, None))
}

fn extract_number(text: &str) -> Option<u32> {
    let re = regex::Regex::new(r"(\d+)").unwrap();
    re.captures(text)
        .and_then(|cap| cap.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

// ---------------------------------------------------------------------------
// Vision complexity detection
// ---------------------------------------------------------------------------

pub fn is_complex_vision_query(text: &str) -> bool {
    let keywords = &[
        "por que",
        "explique",
        "isso esta certo",
        "tem problema",
        "analise",
        "o que devo fazer",
        "como resolver",
        "qual o problema",
        "bug",
        "erro nesse codigo",
        "faz sentido",
        "corrigir",
        "consertar",
        "porque",
        "como faz",
        "me ajuda",
    ];
    let lower = text.to_lowercase();
    keywords.iter().any(|k| lower.contains(k))
}

// ---------------------------------------------------------------------------
// Keyword-based fast matching (no HTTP, <1ms)
// ---------------------------------------------------------------------------

/// Match registered FastCommand patterns (longest win).
fn match_fast_command_patterns(query: &str) -> Option<FastAction> {
    // "toca musica november rain" não pode cair em shuffle (pattern "toca musica" é substring)
    if extract_named_music_query(query).is_some() {
        return None;
    }

    let commands = fast_commands();
    let mut best: Option<(&FastCommand, usize)> = None;

    for cmd in commands {
        for pattern in cmd.patterns {
            if query.contains(pattern) {
                let len = pattern.len();
                if best.map(|(_, l)| len > l).unwrap_or(true) {
                    best = Some((cmd, len));
                }
            }
        }
    }

    best.map(|(cmd, _)| build_action(cmd, query))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 1: Calculator fast-path (evalexpr)
// ─────────────────────────────────────────────────────────────────────────────

/// Tenta detectar e resolver expressão matemática no transcript sem chamar o LLM.
fn try_calculator_fast_path(query: &str) -> Option<FastAction> {
    use std::sync::OnceLock;
    use regex::Regex;

    static EXPR_RE: OnceLock<Regex> = OnceLock::new();
    let re = EXPR_RE.get_or_init(|| {
        // Captura sequências numéricas com operadores aritméticos
        Regex::new(r"(\d+(?:[.,]\d+)?(?:\s*[\+\-\*\/]\s*\d+(?:[.,]\d+)?)+)").unwrap()
    });

    // Só acionar se a consulta parecer matemática
    let triggers = ["quanto é", "quantos é", "quanto da", "calcula", "calculadora",
                    "quanto fica", "me diz quanto", "qual o resultado de", "resultado de",
                    "soma", "subtrai", "divide", "multiplica"];
    let has_trigger = triggers.iter().any(|t| query.contains(t));
    if !has_trigger && !query.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }

    let m = re.find(query)?;
    let raw = m.as_str();
    // Normaliza: vírgula → ponto, remove espaços internos
    let expr = raw.replace(',', ".").replace(' ', "");

    match evalexpr::eval(&expr) {
        Ok(val) => {
            let result_str = match &val {
                evalexpr::Value::Float(f) => {
                    if f.fract() == 0.0 && f.abs() < 1e15_f64 {
                        format!("{}", *f as i64)
                    } else {
                        // Remove zeros à direita
                        let s = format!("{:.10}", f);
                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                    }
                }
                evalexpr::Value::Int(i) => i.to_string(),
                _ => return None,
            };
            let tts = format!("O resultado de {} é {}", raw.trim(), result_str);
            Some(FastAction {
                tool_name: "calculator".into(),
                tool_args: serde_json::json!({
                    "expression": raw.trim(),
                    "result": result_str
                }),
                tts_template: tts,
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            })
        }
        Err(_) => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 1: extração de caminhos de arquivo (preserva extensão no transcript bruto)
// ─────────────────────────────────────────────────────────────────────────────

/// Extrai texto após a última ocorrência de `keyword` no transcript bruto (preserva `.txt`, etc.).
fn extract_path_after_keyword(raw: &str, keyword: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    let pos = lower.rfind(keyword)?;
    let after = &raw[pos + keyword.len()..];
    let cleaned = after
        .trim()
        .trim_end_matches(|c: char| "?.!".contains(c))
        .trim();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

/// Extrai o nome/pattern do arquivo a buscar a partir do transcript bruto.
fn extract_file_search_query(raw_transcript: &str) -> Option<String> {
    let query = normalize_text(raw_transcript);
    let prefixes = [
        // ── "arquivo" (existente) ──
        "acha o arquivo ",
        "acha arquivo ",
        "procura o arquivo ",
        "procura arquivo ",
        "cade o arquivo ",
        "cadê o arquivo ",
        "onde esta o arquivo ",
        "onde está o arquivo ",
        "encontra o arquivo ",
        "encontra arquivo ",
        "busca o arquivo ",
        "busca arquivo ",
        "achar o arquivo ",
        "achar arquivo ",
        "procurar o arquivo ",
        "procurar arquivo ",
        // ── "ache" + "arquivo" (novo) ──
        "ache o arquivo ",
        "ache arquivo ",
        // ── "pasta" (novo) ──
        "acha a pasta ",
        "acha pasta ",
        "ache a pasta ",
        "ache pasta ",
        "procura a pasta ",
        "procura pasta ",
        "cade a pasta ",
        "cadê a pasta ",
        "onde esta a pasta ",
        "onde está a pasta ",
        "encontra a pasta ",
        "encontra pasta ",
        "busca a pasta ",
        "busca pasta ",
        "achar a pasta ",
        "achar pasta ",
        "procurar a pasta ",
        "procurar pasta ",
    ];
    for prefix in &prefixes {
        if query.starts_with(prefix) {
            let keyword = if prefix.contains("pasta") {
                "pasta"
            } else {
                "arquivo"
            };
            return extract_path_after_keyword(raw_transcript, keyword);
        }
    }
    None
}

/// Extrai o caminho para `read_file` (ex.: "Leia o arquivo love.txt").
fn extract_read_file_path(raw_transcript: &str, normalized: &str) -> Option<String> {
    const TRIGGERS: &[&str] = &[
        "leia o arquivo",
        "ler o arquivo",
        "le o arquivo",
        "leia arquivo",
        "ler arquivo",
        "le arquivo",
        "lê o arquivo",
        "lê arquivo",
    ];
    if !TRIGGERS.iter().any(|t| normalized.contains(t)) {
        return None;
    }
    extract_path_after_keyword(raw_transcript, "arquivo")
}

/// Extrai path + conteúdo para `write_file` a partir do transcript bruto (preserva extensões).
fn extract_write_file_request(raw: &str, normalized: &str) -> Option<HashMap<String, String>> {
    let triggers = [
        "cria o arquivo",
        "criar o arquivo",
        "cria arquivo",
        "criar arquivo",
        "cria um arquivo",
        "criar um arquivo",
        "salva o arquivo",
        "salvar o arquivo",
        "salva arquivo",
        "salvar arquivo",
        "escreve no arquivo",
        "escrever no arquivo",
        "escreve o arquivo",
        "escrever o arquivo",
    ];
    if !triggers.iter().any(|t| normalized.contains(t)) && !normalized.contains("cri o arquivo") {
        return None;
    }
    if normalized.contains("abre o arquivo")
        || normalized.contains("abrir o arquivo")
        || normalized.contains("le o arquivo")
        || normalized.contains("ler o arquivo")
        || normalized.contains("leia o arquivo")
        || normalized.contains("acha o arquivo")
        || normalized.contains("procura o arquivo")
    {
        return None;
    }

    let re = regex::Regex::new(
        r"(?is)(?:cria(?:r)?|cri\s+o|salva(?:r)?|escreve(?:r)?)\s+(?:o\s+|um\s+)?arquivo\s+(\S+?)\s+com(?:\s+a\s+(?:escrita|frase)|\s+o\s+texto)?\s+(.+?)(?:\s+na\s+(?:[aá]rea\s+de\s+trabalho|desktop|documentos|downloads))?\s*[.?!]*\s*$",
    )
    .ok()?;
    let re_alt = regex::Regex::new(
        r"(?is)(?:cria(?:r)?|cri\s+o|salva(?:r)?|escreve(?:r)?)\s+(?:o\s+|um\s+)?arquivo\s+(\S+?)\s+na\s+(?:[aá]rea\s+de\s+trabalho|desktop|documentos|downloads)\s+com(?:\s+a\s+(?:escrita|frase)|\s+o\s+texto)?\s+(.+?)\s*[.?!]*\s*$",
    )
    .ok()?;
    let caps = re.captures(raw).or_else(|| re_alt.captures(raw))?;

    let filename = caps.get(1)?.as_str().trim().to_string();
    let mut content = caps.get(2)?.as_str().trim().to_string();
    if filename.is_empty() || content.is_empty() {
        return None;
    }

    for suffix in [
        " na área de trabalho",
        " na area de trabalho",
        " no desktop",
        " na desktop",
        " nos documentos",
        " na pasta documentos",
        " nos downloads",
        " na pasta downloads",
    ] {
        if let Some(pos) = content.to_lowercase().rfind(&suffix.to_lowercase()) {
            content = content[..pos].trim().to_string();
        }
    }
    content = trim_wrapping_quotes(&content);

    let on_desktop = normalized.contains("area de trabalho")
        || normalized.contains("desktop")
        || normalized.contains("na mesa");
    let on_documents = normalized.contains("documentos") || normalized.contains("documents");
    let on_downloads = normalized.contains("downloads") || normalized.contains("download");

    // Nome simples → resolve_write_path coloca no Desktop real (dirs::desktop_dir).
    let path = if on_desktop {
        filename.clone()
    } else if on_documents {
        format!("documentos/{}", filename)
    } else if on_downloads {
        format!("downloads/{}", filename)
    } else {
        filename.clone()
    };

    let mut args = HashMap::new();
    args.insert("path".into(), path);
    args.insert("content".into(), content);
    args.insert("overwrite".into(), "true".into());
    Some(args)
}

fn trim_wrapping_quotes(s: &str) -> String {
    let mut out = s.trim().to_string();
    while (out.starts_with('"') && out.ends_with('"'))
        || (out.starts_with('\'') && out.ends_with('\''))
        || (out.starts_with('“') && out.ends_with('”'))
        || (out.starts_with('«') && out.ends_with('»'))
    {
        if out.len() < 2 {
            break;
        }
        out = out[1..out.len() - 1].trim().to_string();
    }
    out
}

fn is_translate_intent(query: &str) -> bool {
    query.contains("traduz")
        || query.contains("traduza")
        || query.contains("traducao")
        || query.contains("traduzir")
        || query.contains("translate")
}

fn try_translate_fast_path(query: &str) -> Option<FastAction> {
    if !is_translate_intent(query) {
        return None;
    }
    let source = if query.contains("copi")
        || query.contains("clipboard")
        || query.contains("transferencia")
    {
        "clipboard"
    } else if query.contains("selecion") {
        "selection"
    } else {
        "auto"
    };
    let target = crate::system_tools::resolve_translate_target(None, Some(query));
    Some(FastAction {
        tool_name: "translate_selection".into(),
        tool_args: serde_json::json!({
            "source": source,
            "target_language": target.code,
        }),
        tts_template: "{result}".into(),
        needs_vision: false,
        vision_complexity: None,
        needs_llm_formatting: true,
    })
}

fn keyword_fast_match(query: &str, raw_transcript: &str) -> Option<FastAction> {
    // Tradução tem prioridade sobre "o que copiei" / read_clipboard
    if let Some(action) = try_translate_fast_path(query) {
        return Some(action);
    }

    // Cotação cambial (dólar, euro, iene, libra)
    if let Some(pair) = detect_fx_pair(query) {
        if query_mentions_fx_rate(query) {
            return Some(FastAction {
                tool_name: "fetch_fx_quote".into(),
                tool_args: serde_json::json!({"pair": pair}),
                tts_template: "{result}".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: true,
            });
        }
    }

    // Lembrete relativo: "me avisa em 30 segundos"
    if let Some(delay_secs) = parse_reminder_relative_delay(query) {
        let message = parse_reminder_message(query);
        let sound = crate::notification_tools::ReminderSound::from_query(query);
        return Some(FastAction {
            tool_name: "schedule_notification".into(),
            tool_args: serde_json::json!({
                "message": message,
                "delay_seconds": delay_secs,
                "sound": sound.as_str()
            }),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Lembrete em horário: "me lembra às 18:30" → normalize: "as 18 30"
    if let Some(datetime) = parse_reminder_at_time(query) {
        let message = parse_reminder_message(query);
        let sound = crate::notification_tools::ReminderSound::from_query(query);
        return Some(FastAction {
            tool_name: "schedule_notification".into(),
            tool_args: serde_json::json!({
                "message": message,
                "datetime": datetime,
                "sound": sound.as_str()
            }),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Clima / temperatura (Open-Meteo, sem LLM)
    if query_mentions_weather(query) {
        let mut args = serde_json::json!({});
        if let Some(loc) = extract_weather_location(query) {
            args["location"] = serde_json::Value::String(loc);
        }
        if let Some(day) = extract_weather_day_offset(query) {
            args["day_offset"] = serde_json::Value::Number(day.into());
        }
        return Some(FastAction {
            tool_name: "fetch_weather".into(),
            tool_args: args,
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // YouTube explícito (pula biblioteca local)
    if let Some((title, artist)) = extract_youtube_music_query(query) {
        let mut args = serde_json::json!({
            "query": title,
            "prefer_youtube": true,
            "prefer_native_player": false,
        });
        if let Some(a) = artist {
            args["artist"] = serde_json::Value::String(a);
        }
        return Some(FastAction {
            tool_name: "play_music_query".into(),
            tool_args: args,
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Música com título: pasta local primeiro, YouTube só se não achar
    if let Some((title, artist)) = extract_named_music_query(query) {
        let mut args = serde_json::json!({
            "query": title,
            "prefer_youtube": false,
            "prefer_native_player": query_wants_native_media_player(query),
        });
        if let Some(a) = artist {
            args["artist"] = serde_json::Value::String(a);
        }
        return Some(FastAction {
            tool_name: "play_music_query".into(),
            tool_args: args,
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    if let Some(action) = keyword_open_app(query) {
        return Some(action);
    }

    if let Some(action) = match_fast_command_patterns(query) {
        return Some(action);
    }

    // Tempo/Data
    if (query.contains("hora") || query.contains("horas"))
        && !query.contains("tela")
        && !query.contains("vendo")
    {
        return Some(FastAction {
            tool_name: "get_current_time".into(),
            tool_args: serde_json::json!({}),
            tts_template: "Agora são {time}, {date}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }
    if (query.contains("data") || query.contains("dia"))
        && query.contains("hoje")
        && !query.contains("tela")
    {
        return Some(FastAction {
            tool_name: "get_current_time".into(),
            tool_args: serde_json::json!({}),
            tts_template: "Hoje é {date}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }

    // Volume - up
    if query.contains("volume") || query.contains("som") || query.contains("silenciar") || query.contains("mudo") || query.contains("mutar") || query.contains("desmutar") {
        if query.contains("aumenta")
            || query.contains("sobe")
            || query.contains("mais")
            || query.contains("aumentar")
        {
            let steps = extract_number(query).unwrap_or(3);
            return Some(FastAction {
                tool_name: "adjust_system_volume".into(),
                tool_args: serde_json::json!({"action": "up", "steps": steps}),
                tts_template: "Volume aumentado".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("diminui")
            || query.contains("abaixa")
            || query.contains("menos")
            || query.contains("diminuir")
            || query.contains("abaixar")
        {
            let steps = extract_number(query).unwrap_or(3);
            return Some(FastAction {
                tool_name: "adjust_system_volume".into(),
                tool_args: serde_json::json!({"action": "down", "steps": steps}),
                tts_template: "Volume reduzido".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("silenciar")
            || query.contains("mudo")
            || query.contains("mutar")
            || query.contains("desmutar")
            || query.contains("desligar som")
            || query.contains("ligar som")
        {
            return Some(FastAction {
                tool_name: "adjust_system_volume".into(),
                tool_args: serde_json::json!({"action": "mute_toggle"}),
                tts_template: "Som alternado".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }

    // Midia — fechar reprodutor antes de play ("play" em "player" ou STT "feche o play")
    if query_wants_close_media_player(query) {
        return Some(FastAction {
            tool_name: "close_desktop_app".into(),
            tool_args: serde_json::json!({"app": "media_player"}),
            tts_template: "Fechando o reprodutor de música".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }
    if query.contains("pausa") || query.contains("pausar") || query.contains("parar") {
        if query.contains("musica") || query.contains("faixa") || query == "pausar" || query == "pausa"
        {
            return Some(FastAction {
                tool_name: "control_media_playback".into(),
                tool_args: serde_json::json!({"action": "pause"}),
                tts_template: "Música pausada".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }
    if !query_wants_close_media_player(query)
        && (query.contains("tocar") || query_has_word(query, "play") || query.contains("retomar"))
    {
        if (query.contains("musica") || query_has_word(query, "play") || query == "tocar")
            && !query.contains("youtube")
            && !query.contains("player")
        {
            return Some(FastAction {
                tool_name: "control_media_playback".into(),
                tool_args: serde_json::json!({"action": "play"}),
                tts_template: "Reproduzindo".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }
    if query.contains("proxima")
        || query.contains("avancar")
        || query.contains("pular")
        || query == "next"
    {
        if query.contains("musica") || query.contains("faixa") || query.contains("proxima") || query == "next"
        {
            return Some(FastAction {
                tool_name: "control_media_playback".into(),
                tool_args: serde_json::json!({"action": "next"}),
                tts_template: "Próxima faixa".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }
    if query.contains("anterior") || query.contains("voltar") || query == "previous" {
        if query.contains("musica") || query.contains("faixa") {
            return Some(FastAction {
                tool_name: "control_media_playback".into(),
                tool_args: serde_json::json!({"action": "previous"}),
                tts_template: "Faixa anterior".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }
    if query.contains("tocando")
        || (query.contains("qual")
            && query.contains("musica")
            && !query.contains("tela"))
    {
        return Some(FastAction {
            tool_name: "control_media_playback".into(),
            tool_args: serde_json::json!({"action": "status"}),
            tts_template: "{status}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Apps - abrir
    if query.contains("abre") || query.contains("abrir") || query.contains("iniciar") {
        if query.contains("chrome") {
            return Some(FastAction {
                tool_name: "launch_desktop_app".into(),
                tool_args: serde_json::json!({"app": "chrome"}),
                tts_template: "Abrindo Chrome".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("vs code") || query.contains("vscode") {
            return Some(FastAction {
                tool_name: "launch_desktop_app".into(),
                tool_args: serde_json::json!({"app": "vscode"}),
                tts_template: "Abrindo VS Code".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("terminal") {
            return Some(FastAction {
                tool_name: "launch_desktop_app".into(),
                tool_args: serde_json::json!({"app": "terminal"}),
                tts_template: "Abrindo Terminal".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("edge") {
            return Some(FastAction {
                tool_name: "launch_desktop_app".into(),
                tool_args: serde_json::json!({"app": "edge"}),
                tts_template: "Abrindo Edge".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("discord") {
            return Some(FastAction {
                tool_name: "launch_desktop_app".into(),
                tool_args: serde_json::json!({"app": "discord"}),
                tts_template: "Abrindo Discord".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("reprodutor")
            || query.contains("groove")
            || query.contains("media player")
            || query.contains("windows media")
        {
            return Some(FastAction {
                tool_name: "launch_desktop_app".into(),
                tool_args: serde_json::json!({"app": "media_player"}),
                tts_template: "Abrindo reprodutor de música".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }

    // Apps - fechar
    if query.contains("feche") || query.contains("fecha") || query.contains("fechar") {
        if query.contains("chrome") {
            return Some(FastAction {
                tool_name: "close_desktop_app".into(),
                tool_args: serde_json::json!({"app": "chrome"}),
                tts_template: "Fechando Chrome".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("vs code") || query.contains("vscode") {
            return Some(FastAction {
                tool_name: "close_desktop_app".into(),
                tool_args: serde_json::json!({"app": "vscode"}),
                tts_template: "Fechando VS Code".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
        if query.contains("bloco de notas") || query.contains("notepad") || query.contains("bloco de nota") {
            return Some(FastAction {
                tool_name: "close_desktop_app".into(),
                tool_args: serde_json::json!({"app": "notepad"}),
                tts_template: "Fechando Bloco de Notas".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }

    // Visao
    if query.contains("tela")
        || query.contains("vendo")
        || query.contains("o que vê")
        || query.contains("o que ve")
        || query.contains("descreva")
        || query.contains("descreve")
    {
        let question = "Descreva a tela de forma curta e objetiva para resposta por voz. \
                        Liste apenas os elementos principais: aplicativos abertos, textos relevantes, ações possíveis. \
                        Seja breve. Máximo 3 frases.";
        let complexity = if is_complex_vision_query(query) {
            VisionComplexity::Complex
        } else {
            VisionComplexity::Simple
        };
        return Some(FastAction {
            tool_name: "take_screenshot".into(),
            tool_args: serde_json::json!({"question": question}),
            tts_template: String::new(),
            needs_vision: true,
            vision_complexity: Some(complexity),
            needs_llm_formatting: false,
        });
    }

    // Erro
    if query.contains("erro") {
        let question =
            "Leia e descreva apenas os erros ou mensagens de erro visíveis na tela. Seja breve.";
        return Some(FastAction {
            tool_name: "take_screenshot".into(),
            tool_args: serde_json::json!({"question": question}),
            tts_template: String::new(),
            needs_vision: true,
            vision_complexity: Some(VisionComplexity::Complex),
            needs_llm_formatting: false,
        });
    }

    // Listar apps
    if query.contains("aplicativos")
        || query.contains("apps")
        || query.contains("programas")
        || query.contains("janelas")
    {
        if query.contains("abertos")
            || query.contains("rodando")
            || query.contains("executando")
        {
            return Some(FastAction {
                tool_name: "list_running_apps".into(),
                tool_args: serde_json::json!({}),
                tts_template: "Aplicativos abertos: {apps}".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: true,
            });
        }
    }

    // Shuffle all music
    if (query.contains("toca") || query.contains("tocar"))
        && (query.contains("aleatorio")
            || query.contains("aleatoria")
            || query.contains("shuffle")
            || query.contains("todas as musicas")
            || query.contains("todas as músicas"))
    {
        return Some(FastAction {
            tool_name: "native_music_library_shuffle_play".into(),
            tool_args: serde_json::json!({}),
            tts_template: "Tocando todas as músicas em modo aleatório".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }

    // ── Tier 1: fast-paths de sistema ────────────────────────────────────────

    // Calculadora (resolve antes do LLM, sem round-trip)
    if let Some(action) = try_calculator_fast_path(query) {
        return Some(action);
    }

    // Colar na janela ativa (DEVE vir antes de get_active_window para evitar colisão)
    if (query.contains("cola") || query.contains("colar") || query.contains("cole"))
        && (query.contains("janela") || query.contains("ativa"))
    {
        // Extrai texto a colar: tenta aspas primeiro, depois entre verbo e "na janela"
        let text_to_paste: String = {
            // Tenta texto entre aspas duplas ou simples
            let quoted = if let Some(start) = query.find('"') {
                query[start + 1..].find('"').map(|end| query[start + 1..start + 1 + end].to_string())
            } else if let Some(start) = query.find('\'') {
                query[start + 1..].find('\'').map(|end| query[start + 1..start + 1 + end].to_string())
            } else {
                None
            };
            if let Some(q) = quoted {
                q
            } else {
                // Tenta extrair entre o verbo e "na janela"/"na ativa"
                let mut extracted = String::new();
                for prefix in &["cole ", "cola ", "colar "] {
                    if let Some(rest) = query.strip_prefix(prefix) {
                        for suffix in &[" na janela ativa", " na janela", " na ativa", " no campo", " aqui"] {
                            if let Some(text) = rest.strip_suffix(suffix) {
                                extracted = text.trim().to_string();
                                break;
                            }
                        }
                        if extracted.is_empty() {
                            extracted = rest.trim().to_string();
                        }
                        break;
                    }
                }
                extracted
            }
        };

        if !text_to_paste.is_empty() {
            return Some(FastAction {
                tool_name: "paste_to_active_window".into(),
                tool_args: serde_json::json!({"text": text_to_paste}),
                tts_template: "Colando na janela ativa.".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }

    // Janela ativa (refinado: só pergunta, não ação)
    if ((query.starts_with("qual") || query.starts_with("que") || query.starts_with("o que"))
        && query.contains("janela")
        && (query.contains("ativa") || query.contains("foco") || query.contains("frente") || query.contains("aberta")))
        || query.contains("qual app")
        || query.contains("que app")
        || query.contains("que programa")
        || query.contains("qual programa")
    {
        return Some(FastAction {
            tool_name: "get_active_window".into(),
            tool_args: serde_json::json!({}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Informações do sistema
    if query.contains("quanto de ram")
        || query.contains("memoria livre")
        || query.contains("memória livre")
        || query.contains("uso de memoria")
        || query.contains("uso de memória")
        || query.contains("bateria")
        || (query.contains("processador") && (query.contains("modelo") || query.contains("nome") || query.contains("qual")))
        || (query.contains("disco") && (query.contains("livre") || query.contains("espaco") || query.contains("espaço")))
        || query.contains("uptime")
        || query.contains("tempo de atividade")
        || query.contains("quanto tempo ligado")
        || (query.contains("info") && query.contains("sistema"))
        || query.contains("informacoes do sistema")
        || query.contains("informações do sistema")
    {
        return Some(FastAction {
            tool_name: "system_info".into(),
            tool_args: serde_json::json!({"concise": true}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Arquivos recentes
    if query.contains("arquivos recentes")
        || query.contains("o que abri")
        || query.contains("ultimos arquivos")
        || query.contains("últimos arquivos")
        || query.contains("abertos recentemente")
        || query.contains("usados recentemente")
    {
        return Some(FastAction {
            tool_name: "get_recent_files".into(),
            tool_args: serde_json::json!({"max": 10}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Criar / salvar arquivo de texto (ex.: na área de trabalho)
    if let Some(args) = extract_write_file_request(raw_transcript, query) {
        return Some(FastAction {
            tool_name: "write_file".into(),
            tool_args: {
                let mut json_map = serde_json::Map::new();
                for (k, v) in args {
                    if k == "overwrite" {
                        json_map.insert(k, JsonValue::Bool(v == "true"));
                    } else {
                        json_map.insert(k, JsonValue::String(v));
                    }
                }
                JsonValue::Object(json_map)
            },
            tts_template: "Arquivo criado.".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }

    // Ler arquivo de texto
    if let Some(path) = extract_read_file_path(raw_transcript, query) {
        return Some(FastAction {
            tool_name: "read_file".into(),
            tool_args: serde_json::json!({"path": path}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Busca de arquivos
    if let Some(file_query) = extract_file_search_query(raw_transcript) {
        return Some(FastAction {
            tool_name: "search_files".into(),
            tool_args: serde_json::json!({"query": file_query, "max_results": 10}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // ── Tier 2: fast-paths (ferramentas sem cobertura anterior) ─────────────

    // Abrir pasta/diretório
    if let Some(folder_path) = (|| -> Option<String> {
        let prefixes = [
            "abre a pasta ",
            "abrir a pasta ",
            "abra a pasta ",
            "abre o diretório ",
            "abre o diretorio ",
            "abrir o diretório ",
            "abrir o diretorio ",
            "abra o diretório ",
            "abra o diretorio ",
            "vai para a pasta ",
            "vai pra pasta ",
            "ir para a pasta ",
        ];
        for prefix in &prefixes {
            if query.starts_with(prefix) {
                let path = query[prefix.len()..].trim().trim_end_matches('?').trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }
        None
    })() {
        return Some(FastAction {
            tool_name: "open_folder".into(),
            tool_args: serde_json::json!({"path": folder_path}),
            tts_template: "Abrindo a pasta.".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }

    // Anotar / notas de sessão — adicionar nota
    if let Some(note_text) = query
        .strip_prefix("anota ")
        .or_else(|| query.strip_prefix("anotar "))
    {
        let note_text = note_text.trim().to_string();
        if !note_text.is_empty() {
            return Some(FastAction {
                tool_name: "session_notes".into(),
                tool_args: serde_json::json!({"action": "add", "text": note_text}),
                tts_template: "Nota salva.".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }

    // Anotar / notas de sessão — listar notas
    if query.contains("lista as notas") || query.contains("listar as notas")
        || query.contains("minhas notas") || query.contains("mostra as notas")
        || query.contains("mostrar as notas") || query.contains("ver as notas")
    {
        return Some(FastAction {
            tool_name: "session_notes".into(),
            tool_args: serde_json::json!({"action": "list"}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Anotar / notas de sessão — limpar notas
    if query.contains("limpa as notas") || query.contains("limpar as notas")
        || query.contains("apaga as notas") || query.contains("apagar as notas")
    {
        return Some(FastAction {
            tool_name: "session_notes".into(),
            tool_args: serde_json::json!({"action": "clear"}),
            tts_template: "Notas apagadas.".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }

    // Pressionar tecla
    if let Some(key_name) = query
        .strip_prefix("pressiona ")
        .or_else(|| query.strip_prefix("pressione "))
        .or_else(|| query.strip_prefix("aperta "))
        .or_else(|| query.strip_prefix("aperte "))
    {
        let key_name = key_name.trim().to_string();
        if !key_name.is_empty() {
            return Some(FastAction {
                tool_name: "send_keys".into(),
                tool_args: serde_json::json!({"keys": key_name}),
                tts_template: "Tecla enviada.".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }

    // Digitar texto
    if let Some(text) = query
        .strip_prefix("digita ")
        .or_else(|| query.strip_prefix("digite "))
    {
        let text = text.trim().to_string();
        if !text.is_empty() {
            return Some(FastAction {
                tool_name: "send_keys".into(),
                tool_args: serde_json::json!({"keys": text}),
                tts_template: "Texto digitado.".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }

    // Bloquear tela
    if query.contains("bloqueia a tela") || query.contains("bloqueia tela")
        || query.contains("trava o pc") || query.contains("travar o pc")
        || query.contains("bloquear a tela") || query.contains("bloquear tela")
        || query.contains("bloqueia o pc")
    {
        return Some(FastAction {
            tool_name: "lock_screen".into(),
            tool_args: serde_json::json!({}),
            tts_template: "Bloqueando a tela.".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }

    // Modo foco / Não perturbe
    if query.contains("modo foco") || query.contains("não perturbe") || query.contains("nao perturbe")
        || query.contains("ativa o foco") || query.contains("ativar o foco")
        || query.contains("desativa o foco") || query.contains("desativar o foco")
    {
        return Some(FastAction {
            tool_name: "toggle_do_not_disturb".into(),
            tool_args: serde_json::json!({}),
            tts_template: "Modo foco alternado.".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: false,
        });
    }

    // Ler conteúdo do clipboard (não quando o pedido é traduzir)
    if !is_translate_intent(query)
        && (query.contains("o que eu copiei")
        || query.contains("o que copiei")
        || query.contains("o que tem no clipboard")
        || query.contains("o que esta no clipboard")
        || (query.contains("o que")
            && (query.contains("clipboard") || query.contains("transferencia")))
        || ((query.contains("leia") || query.contains("le o") || query.contains("ler o"))
            && (query.contains("copiei")
                || query.contains("clipboard")
                || query.contains("area de transferencia")
                || query.contains("transferencia")))
        || query.contains("leia o clipboard")
        || query.contains("leia a area de transferencia")
        || query.contains("mostra o clipboard")
        || query.contains("mostra a area de transferencia"))
    {
        return Some(FastAction {
            tool_name: "read_clipboard".into(),
            tool_args: serde_json::json!({}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // Copiar texto para clipboard
    if let Some(text_raw) = query
        .strip_prefix("copia ")
        .or_else(|| query.strip_prefix("copiar "))
    {
        let text_raw = text_raw.trim();
        if !text_raw.is_empty()
            && !text_raw.starts_with("o arquivo")
            && !text_raw.starts_with("a pasta")
            && !text_raw.starts_with("o texto")
        {
            let text_to_copy = if (text_raw.starts_with('"') && text_raw.ends_with('"'))
                || (text_raw.starts_with('\'') && text_raw.ends_with('\''))
            {
                text_raw[1..text_raw.len() - 1].to_string()
            } else {
                text_raw.to_string()
            };
            return Some(FastAction {
                tool_name: "write_clipboard".into(),
                tool_args: serde_json::json!({"text": text_to_copy}),
                tts_template: "Copiado para o clipboard.".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: false,
            });
        }
    }

    // Arquivos recentes — padrões adicionais
    if query.contains("qual arquivo abri") || query.contains("quais arquivos abri")
        || query.contains("o que eu abri") || query.contains("que arquivos eu abri")
        || query.contains("arquivo mais recente") || query.contains("ultimo arquivo")
        || query.contains("último arquivo")
    {
        return Some(FastAction {
            tool_name: "get_recent_files".into(),
            tool_args: serde_json::json!({"max": 10}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    // ── Tier 3: fast-paths ─────────────────────────────────────────────────

    // Informações de rede
    if query.contains("ip")
        || query.contains("rede")
        || query.contains("wifi")
        || query.contains("wi-fi")
        || query.contains("gateway")
        || query.contains("dns")
        || query.contains("mac")
        || (query.contains("endereço") && query.contains("rede"))
    {
        if query.contains("qual")
            || query.contains("meu")
            || query.contains("minha")
            || query.contains("informa")
            || query.contains("mostra")
            || query.contains("configura")
        {
            return Some(FastAction {
                tool_name: "get_network_info".into(),
                tool_args: serde_json::json!({}),
                tts_template: "{result}".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: true,
            });
        }
    }

    // ── Tier 4: disk_cleanup fast-path ──────────────────────────────────────
    if query.contains("limpa") || query.contains("limpar") || query.contains("limpeza")
        || query.contains("esvazia") || query.contains("esvaziar")
    {
        if query.contains("disco") || query.contains("temporário") || query.contains("temporario")
            || query.contains("temp") || query.contains("lixeira") || query.contains("cache")
            || query.contains("sistema")
        {
            return Some(FastAction {
                tool_name: "disk_cleanup".into(),
                tool_args: serde_json::json!({"action": "clean"}),
                tts_template: "{result}".into(),
                needs_vision: false,
                vision_complexity: None,
                needs_llm_formatting: true,
            });
        }
    }
    if (query.contains("quanto") || query.contains("qual"))
        && (query.contains("espaço") || query.contains("espaco") || query.contains("disco"))
        && (query.contains("livre") || query.contains("tem") || query.contains("disponível")
            || query.contains("disponivel"))
    {
        return Some(FastAction {
            tool_name: "disk_cleanup".into(),
            tool_args: serde_json::json!({"action": "analyze"}),
            tts_template: "{result}".into(),
            needs_vision: false,
            vision_complexity: None,
            needs_llm_formatting: true,
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Cosine similarity
// ---------------------------------------------------------------------------

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// ---------------------------------------------------------------------------
// Embedding via HTTP (llama-server /v1/embeddings)
// ---------------------------------------------------------------------------

fn flatten_embedding(value: &serde_json::Value) -> Result<Vec<f32>, String> {
    match value {
        serde_json::Value::Array(items) if items.first().and_then(|v| v.as_f64()).is_some() => Ok(items
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect()),
        serde_json::Value::Array(items) => items
            .first()
            .ok_or_else(|| "Invalid embedding response".to_string())
            .and_then(flatten_embedding),
        serde_json::Value::Object(map) => map
            .get("embedding")
            .ok_or_else(|| "Invalid embedding response".to_string())
            .and_then(flatten_embedding),
        _ => Err("Invalid embedding response".to_string()),
    }
}

fn parse_embedding_response(json: &serde_json::Value) -> Result<Vec<f32>, String> {
    if let Some(emb) = json.get("embedding") {
        if let Ok(v) = flatten_embedding(emb) {
            return Ok(v);
        }
    }
    if let Some(data) = json.get("data").and_then(|v| v.as_array()) {
        if let Some(first) = data.first() {
            if let Some(emb) = first.get("embedding") {
                if let Ok(v) = flatten_embedding(emb) {
                    return Ok(v);
                }
            }
        }
    }
    if let Some(arr) = json.as_array() {
        if let Some(first) = arr.first() {
            if let Some(emb) = first.get("embedding") {
                if let Ok(v) = flatten_embedding(emb) {
                    return Ok(v);
                }
            }
            if let Ok(v) = flatten_embedding(first) {
                return Ok(v);
            }
        }
    }
    flatten_embedding(json)
}

async fn get_embedding(embed_url: &str, text: &str) -> Result<Vec<f32>, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/embedding", embed_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "content": text,
    });

    let resp = client
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Embedding request failed: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Embedding API error {}: {}", status, body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Embedding parse failed: {}", e))?;

    parse_embedding_response(&json)
}

// ---------------------------------------------------------------------------
// Embedding-based pattern matching
// ---------------------------------------------------------------------------

async fn embedding_match(
    query: &str,
    embed_url: &str,
) -> Option<(usize, f32)> {
    let query_emb = match get_embedding(embed_url, query).await {
        Ok(emb) => emb,
        Err(e) => {
            eprintln!("[fast_path] Embedding HTTP failed: {e}");
            return None;
        }
    };

    let commands = fast_commands();
    let threshold: f32 = 0.82;
    let mut best_score = 0.0f32;
    let mut best_idx: Option<usize> = None;

    // For each command, compute embedding of first pattern and compare
    // In production, these would be pre-computed at startup
    for (idx, cmd) in commands.iter().enumerate() {
        // Use the first pattern as the canonical embedding
        if let Some(first_pattern) = cmd.patterns.first() {
            let cmd_emb = match get_embedding(embed_url, &normalize_text(first_pattern)).await {
                Ok(emb) => emb,
                Err(_) => continue,
            };
            let sim = cosine_similarity(&query_emb, &cmd_emb);
            if sim > best_score {
                best_score = sim;
                best_idx = Some(idx);
            }
        }
    }

    // BGE on frases curtas costuma devolver similaridade ~1.0 entre intents distintos.
    if best_score >= threshold && best_score < 0.98 {
        best_idx.map(|idx| (idx, best_score))
    } else {
        if best_score >= 0.98 {
            eprintln!(
                "[fast_path] embedding_rejected | sim={:.3} (suspeito, use DEXTER_FAST_PATH_EMBEDDING=1 para forçar)",
                best_score
            );
        }
        None
    }
}

fn embedding_fast_path_enabled() -> bool {
    std::env::var("DEXTER_FAST_PATH_EMBEDDING")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Build FastAction from FastCommand
// ---------------------------------------------------------------------------

fn build_action(cmd: &FastCommand, transcript: &str) -> FastAction {
    let args = (cmd.param_extractor)(transcript);
    let tool_args = match args {
        Some(map) => {
            let mut json_map = serde_json::Map::new();
            for (k, v) in map {
                json_map.insert(k, JsonValue::String(v));
            }
            JsonValue::Object(json_map)
        }
        None => serde_json::json!({}),
    };

    let complexity = if cmd.needs_vision {
        Some(if is_complex_vision_query(transcript) {
            VisionComplexity::Complex
        } else {
            VisionComplexity::Simple
        })
    } else {
        None
    };

    FastAction {
        tool_name: cmd.tool_name.to_string(),
        tool_args,
        tts_template: cmd.tts_template.to_string(),
        needs_vision: cmd.needs_vision,
        vision_complexity: complexity,
        needs_llm_formatting: matches!(
            cmd.intent,
            "list_apps" | "media_status" | "whats_open"
        ),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Testa todos os patterns contra o embedding do transcript.
/// Primeiro tenta keyword matching (rapido, offline).
/// Se falhar, tenta embedding-based matching (lento, HTTP).
pub async fn fast_path_match(
    transcript: &str,
    embed_url: &str,
) -> FastPathResult {
    let query = normalize_text(transcript);

    if query.is_empty() || query.len() < 3 {
        return FastPathResult::Miss;
    }

    // Step 1: Keyword matching (sub-ms, offline)
    let kw_start = std::time::Instant::now();
    if let Some(action) = keyword_fast_match(&query, transcript) {
        let elapsed_us = kw_start.elapsed().as_micros();
        eprintln!(
            "[perf] fast_path_keyword_hit | intent={} | transcript=\"{}\" | elapsed_us={}",
            action.tool_name, transcript, elapsed_us
        );
        return FastPathResult::Hit(action);
    }
    let kw_elapsed_us = kw_start.elapsed().as_micros();

    // Step 2: Embedding-based (lento; desligado por padrão — similaridade BGE em frases curtas é unreliable)
    if embed_url.is_empty() || !embedding_fast_path_enabled() {
        eprintln!(
            "[perf] fast_path_miss | transcript=\"{}\" | reason=no_match | kw_us={}",
            transcript, kw_elapsed_us
        );
        return FastPathResult::Miss;
    }

    let emb_start = std::time::Instant::now();
    match embedding_match(&query, embed_url).await {
        Some((idx, score)) => {
            let cmd = &fast_commands()[idx];
            let action = build_action(cmd, transcript);
            let elapsed_ms = emb_start.elapsed().as_millis();
            eprintln!(
                "[perf] fast_path_embedding_hit | intent={} | transcript=\"{}\" | sim={:.3} | elapsed_ms={}",
                action.tool_name, transcript, score, elapsed_ms
            );
            FastPathResult::Hit(action)
        }
        None => {
            let elapsed_ms = emb_start.elapsed().as_millis();
            eprintln!(
                "[perf] fast_path_miss | transcript=\"{}\" | reason=below_threshold | kw_us={} | emb_ms={}",
                transcript, kw_elapsed_us, elapsed_ms
            );
            FastPathResult::Miss
        }
    }
}

/// Inicializa o fast-path: pre-computa embeddings dos comandos canonicos.
/// Chamado uma vez no startup do app.
pub async fn init_fast_path(embed_url: &str) -> Result<(), String> {
    if embed_url.is_empty() {
        return Ok(()); // Nothing to pre-compute
    }

    let commands = fast_commands();
    let total = commands.len();
    eprintln!("[fast_path] Pre-computando embeddings para {} comandos...", total);

    // In a production system, we'd store these in a static OnceLock<Vec<(FastCommand, Vec<f32>)>>
    // For now, embedding_match() computes them on-the-fly (acceptable for <30 commands)
    // The keyword path handles >90% of real-world matches anyway

    eprintln!("[fast_path] init concluido (keyword path usa 0 HTTP, embedding path on-demand)");
    Ok(())
}

#[cfg(test)]
mod file_path_extraction_tests {
    use super::*;

    #[test]
    fn search_query_preserves_file_extension() {
        let raw = "Ache o arquivo love.txt";
        let q = extract_file_search_query(raw).expect("should extract");
        assert_eq!(q, "love.txt");
    }

    #[test]
    fn read_path_preserves_file_extension() {
        let raw = "Leia o arquivo love.txt";
        let norm = normalize_text(raw);
        let path = extract_read_file_path(raw, &norm).expect("should extract");
        assert_eq!(path, "love.txt");
    }

    #[test]
    fn read_path_rejects_search_phrase() {
        let raw = "Ache o arquivo love.txt";
        let norm = normalize_text(raw);
        assert!(extract_read_file_path(raw, &norm).is_none());
    }
}

#[cfg(test)]
mod translate_fast_path_tests {
    use super::*;

    #[test]
    fn traduza_o_que_copiei_hits_translate_not_clipboard() {
        let raw = "Traduza o que copiei.";
        let query = normalize_text(raw);
        let action = keyword_fast_match(&query, raw).expect("should match");
        assert_eq!(action.tool_name, "translate_selection");
        assert_eq!(
            action.tool_args.get("source").and_then(|v| v.as_str()),
            Some("clipboard")
        );
        assert_eq!(
            action.tool_args.get("target_language").and_then(|v| v.as_str()),
            Some("pt-BR")
        );
    }

    #[test]
    fn traduza_para_japones_sets_target_language() {
        let raw = "Traduza o que copiei para o japones.";
        let query = normalize_text(raw);
        let action = keyword_fast_match(&query, raw).expect("should match");
        assert_eq!(
            action.tool_args.get("target_language").and_then(|v| v.as_str()),
            Some("ja")
        );
    }

    #[test]
    fn o_que_copiei_without_traduz_hits_clipboard() {
        let raw = "O que eu copiei?";
        let query = normalize_text(raw);
        let action = keyword_fast_match(&query, raw).expect("should match");
        assert_eq!(action.tool_name, "read_clipboard");
    }
}

#[cfg(test)]
mod write_file_fast_path_tests {
    use super::*;

    #[test]
    fn extract_write_file_desktop_pt() {
        let raw = r#"Cria o arquivo oi.txt com a escrita "Olá mundo" na área de trabalho."#;
        let norm = normalize_text(raw);
        let args = extract_write_file_request(raw, &norm).expect("should parse");
        assert_eq!(args.get("path").map(String::as_str), Some("oi.txt"));
        assert_eq!(args.get("content").map(String::as_str), Some("Olá mundo"));
    }

    #[test]
    fn extract_write_file_rejects_open_file() {
        let raw = "Abre o arquivo oi.txt";
        let norm = normalize_text(raw);
        assert!(extract_write_file_request(raw, &norm).is_none());
    }

    #[test]
    fn extract_write_file_location_before_content() {
        let raw = "Cria o arquivo oi.txt na área de trabalho com a escrita Olá Mundo.";
        let norm = normalize_text(raw);
        let args = extract_write_file_request(raw, &norm).expect("should parse");
        assert_eq!(args.get("path").map(String::as_str), Some("oi.txt"));
        assert_eq!(args.get("content").map(String::as_str), Some("Olá Mundo"));
    }
}
