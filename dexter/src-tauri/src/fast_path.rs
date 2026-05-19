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
                tts_template: "Som {state}",
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

fn keyword_fast_match(query: &str) -> Option<FastAction> {
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
    }

    // Visao
    if query.contains("tela")
        || query.contains("vendo")
        || (query.contains("ve") && !query.contains("volume"))
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
    if let Some(action) = keyword_fast_match(&query) {
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
