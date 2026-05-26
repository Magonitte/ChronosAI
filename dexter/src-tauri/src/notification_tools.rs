//! notification_tools.rs — Tier 1
//! Histórico de clipboard e notificações agendadas.

use chrono::{Local, NaiveTime, Timelike};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::AppHandle;

use crate::VoiceConfig;

const TOAST_APP_ID: &str = "Chronos.AI.Dexter";

/// Permite o Chronos falar o lembrete quando o timer disparar.
pub struct ReminderVoiceDelivery {
    pub app: AppHandle,
    pub config: VoiceConfig,
}

// ---------------------------------------------------------------------------
// ClipboardHistory
// ---------------------------------------------------------------------------

/// Buffer circular para histórico do clipboard (últimas N entradas).
pub struct ClipboardHistory {
    inner: Mutex<Vec<String>>,
    capacity: usize,
}

impl ClipboardHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Vec::with_capacity(capacity)),
            capacity,
        }
    }

    /// Adiciona uma entrada ao histórico. Ignora entradas vazias e duplicatas consecutivas.
    pub fn push(&self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        let mut history = self.inner.lock().unwrap();
        // Evitar duplicata consecutiva
        if history.first().map(|s: &String| s == &text).unwrap_or(false) {
            return;
        }
        history.insert(0, text);
        if history.len() > self.capacity {
            history.truncate(self.capacity);
        }
    }

    /// Retorna clone do histórico (índice 0 = mais recente).
    pub fn list(&self) -> Vec<String> {
        self.inner.lock().unwrap().clone()
    }

    /// Retorna entrada por índice (0 = mais recente).
    pub fn get(&self, index: usize) -> Option<String> {
        self.inner.lock().unwrap().get(index).cloned()
    }

    /// Número de entradas no histórico.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }
}

/// Formata a lista de histórico para o LLM narrar em voz.
pub fn format_clipboard_history(history: &[String]) -> String {
    if history.is_empty() {
        return "O histórico do clipboard está vazio.".into();
    }
    let lines: Vec<String> = history
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let preview = if text.chars().count() > 120 {
                format!("{}…", text.chars().take(120).collect::<String>())
            } else {
                text.clone()
            };
            format!("[{}] {}", i, preview)
        })
        .collect();
    format!(
        "Histórico do clipboard ({} entrada(s)):\n{}",
        history.len(),
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// schedule_notification
// ---------------------------------------------------------------------------

/// Perfil de som do lembrete (toast WinRT + WAV do sistema como reforço).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReminderSound {
    #[default]
    Reminder,
    Default,
    Alarm,
    Chime,
    Silent,
}

impl ReminderSound {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reminder => "reminder",
            Self::Default => "default",
            Self::Alarm => "alarm",
            Self::Chime => "chime",
            Self::Silent => "silent",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "silent" | "silencioso" | "mudo" => Self::Silent,
            "alarm" | "alarme" => Self::Alarm,
            "chime" | "sino" | "campainha" => Self::Chime,
            "default" | "padrao" | "normal" => Self::Default,
            _ => Self::Reminder,
        }
    }

    /// Inferido da frase de voz (fast-path).
    pub fn from_query(query: &str) -> Self {
        if query.contains("silencios") || query.contains("sem som") || query.contains("mudo") {
            Self::Silent
        } else if query.contains("alarme") {
            Self::Alarm
        } else if query.contains("sino") || query.contains("campainha") {
            Self::Chime
        } else {
            Self::Reminder
        }
    }

    /// Toast sem áudio — o som toca via WAV síncrono antes da fala do Chronos.
    fn toast_audio_xml(self) -> &'static str {
        let _ = self;
        r#"<audio silent="true"/>"#
    }

    fn wav_path(self) -> Option<&'static str> {
        match self {
            Self::Silent => None,
            Self::Alarm => Some(r"C:\Windows\Media\Alarm01.wav"),
            Self::Chime => Some(r"C:\Windows\Media\Windows Notify Messaging.wav"),
            Self::Reminder => Some(r"C:\Windows\Media\Windows Notify Calendar.wav"),
            Self::Default => Some(r"C:\Windows\Media\Windows Notify System Generic.wav"),
        }
    }

    /// Toca o WAV até o fim (bloqueia o script PowerShell).
    fn play_sound_sync_ps(self) -> String {
        let Some(path) = self.wav_path() else {
            return String::new();
        };
        format!(
            r#"
if (Test-Path '{path}') {{
    $player = New-Object System.Media.SoundPlayer '{path}'
    $player.PlaySync()
}}
"#
        )
    }
}

/// Pausa curta entre o fim do som e o XTTS do lembrete.
const PAUSE_BEFORE_REMINDER_SPEECH_MS: u64 = 600;

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

static TOAST_SETUP_DONE: AtomicBool = AtomicBool::new(false);

/// Registra AppUserModelID + atalho no Menu Iniciar (exigido pelo Windows para Toast).
fn ensure_toast_app_registered() {
    if TOAST_SETUP_DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    let exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().replace('\'', "''"))
        .unwrap_or_default();
    let app_id = TOAST_APP_ID.replace('\'', "''");
    let script = format!(
        r#"
$AppId = '{app_id}'
$Exe = '{exe}'
$reg = "HKCU:\Software\Classes\AppUserModelId\$AppId"
if (-not (Test-Path $reg)) {{
    New-Item -Path $reg -Force | Out-Null
    New-ItemProperty -Path $reg -Name DisplayName -Value 'Chronos AI' -PropertyType String -Force | Out-Null
}}
if ($Exe -and (Test-Path $Exe)) {{
    $lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Chronos AI.lnk'
    if (-not (Test-Path $lnk)) {{
        $wsh = New-Object -ComObject WScript.Shell
        $s = $wsh.CreateShortcut($lnk)
        $s.TargetPath = $Exe
        $s.WorkingDirectory = Split-Path $Exe
        $s.Description = 'Chronos AI'
        $s.Save()
    }}
}}
"#
    );
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .output();
    eprintln!("[toast] app registration attempted | app_id={TOAST_APP_ID}");
}

/// Agenda uma notificação Windows Toast.
/// Aceita `delay_seconds` OU `datetime_str` no formato "HH:MM".
pub async fn schedule_notification(
    message: &str,
    delay_seconds: Option<u64>,
    datetime_str: Option<&str>,
    sound: ReminderSound,
    voice: Option<ReminderVoiceDelivery>,
) -> Result<String, String> {
    ensure_toast_app_registered();

    let delay_secs: u64 = match (delay_seconds, datetime_str) {
        (Some(secs), _) => secs,
        (None, Some(dt)) => {
            let now = Local::now();
            let target_time = NaiveTime::parse_from_str(dt, "%H:%M")
                .or_else(|_| NaiveTime::parse_from_str(dt, "%H:%M:%S"))
                .map_err(|_| format!("Formato de hora inválido: '{}'. Use HH:MM", dt))?;

            let now_secs = now.hour() * 3600 + now.minute() * 60 + now.second();
            let tgt_secs = target_time.hour() * 3600
                + target_time.minute() * 60
                + target_time.second();

            if tgt_secs > now_secs {
                (tgt_secs - now_secs) as u64
            } else {
                // Amanhã
                (86400 - now_secs + tgt_secs) as u64
            }
        }
        (None, None) => return Err("Forneça delay_seconds ou datetime.".into()),
    };

    let msg_owned = message.to_string();
    eprintln!(
        "[toast] scheduled | delay_s={delay_secs} | sound={} | message={}",
        sound.as_str(),
        msg_owned.chars().take(80).collect::<String>()
    );
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
        eprintln!(
            "[toast] delay elapsed | firing | delay_s={delay_secs} | sound={}",
            sound.as_str()
        );
        fire_windows_toast(&msg_owned, sound);
        if let Some(delivery) = voice {
            tokio::time::sleep(std::time::Duration::from_millis(
                PAUSE_BEFORE_REMINDER_SPEECH_MS,
            ))
            .await;
            crate::voice::speak_reminder_fire(&delivery.app, &delivery.config, &msg_owned).await;
        }
    });

    let when_str = format_delay_for_speech(delay_secs);

    // Som só no toast; confirmação por voz fica curta (sem repetir o tipo de som).
    Ok(format!("Lembrete agendado {when_str}: {message}"))
}

fn format_delay_for_speech(delay_secs: u64) -> String {
    if delay_secs < 60 {
        if delay_secs == 1 {
            "em 1 segundo".to_string()
        } else {
            format!("em {delay_secs} segundos")
        }
    } else if delay_secs < 3600 {
        let mins = delay_secs / 60;
        if mins == 1 {
            "em 1 minuto".to_string()
        } else {
            format!("em {mins} minutos")
        }
    } else {
        let hours = delay_secs / 3600;
        let mins = (delay_secs % 3600) / 60;
        if mins == 0 {
            if hours == 1 {
                "em 1 hora".to_string()
            } else {
                format!("em {hours} horas")
            }
        } else if hours == 1 {
            format!("em 1 hora e {mins} minutos")
        } else {
            format!("em {hours} horas e {mins} minutos")
        }
    }
}

/// Toast visual + som WAV síncrono (termina antes da fala do Chronos).
fn fire_windows_toast(message: &str, sound: ReminderSound) {
    let body_xml = xml_escape(message);
    let audio_xml = sound.toast_audio_xml();
    let toast_xml = format!(
        r#"<toast scenario="reminder"><visual><binding template="ToastGeneric"><text>Chronos AI</text><text>Lembrete</text><text>{body_xml}</text></binding></visual>{audio_xml}</toast>"#
    );
    let toast_xml_ps = toast_xml.replace('\'', "''");
    let app_id = TOAST_APP_ID.replace('\'', "''");
    let safe_msg = message.replace('\'', "''");
    let play_sound = sound.play_sound_sync_ps();
    let script = format!(
        r#"
$AppId = '{app_id}'
$Title = 'Chronos AI'
$Body = '{safe_msg}'
$ToastXml = '{toast_xml_ps}'
$ok = $false
try {{
    [Windows.UI.Notifications.ToastNotification, Windows.UI.Notifications, ContentType=WindowsRuntime] | Out-Null
    $xml = [Windows.Data.Xml.Dom.XmlDocument,Windows.Data.Xml.Dom.XmlDocument,ContentType=WindowsRuntime]::new()
    $xml.LoadXml($ToastXml)
    $toast = [Windows.UI.Notifications.ToastNotification,Windows.UI.Notifications,ContentType=WindowsRuntime]::new($xml)
    [Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]::CreateToastNotifier($AppId).Show($toast)
    $ok = $true
}} catch {{
    $ok = $false
}}
{play_sound}
if (-not $ok) {{
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $n = New-Object System.Windows.Forms.NotifyIcon
    $n.Icon = [System.Drawing.SystemIcons]::Information
    $n.Visible = $true
    $n.ShowBalloonTip(10000, $Title, $Body, [System.Windows.Forms.ToolTipIcon]::Info)
    Start-Sleep -Seconds 3
    $n.Dispose()
}}
if ($ok) {{ 'toast_ok' }} else {{ 'balloon_ok' }}
"#
    );
    match std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .output()
    {
        Ok(out) => {
            let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
            eprintln!(
                "[toast] fired | status={} | exit={}",
                status,
                out.status.success()
            );
        }
        Err(e) => eprintln!("[toast] fire failed | err={e}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 2 — Session Notes
// ─────────────────────────────────────────────────────────────────────────────

/// Adiciona uma nota à sessão atual.
pub fn session_note_add(notes: &std::sync::Mutex<std::collections::HashMap<u64, String>>, text: &str) -> String {
    if text.trim().is_empty() {
        return "Faltou o texto da nota.".into();
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    notes.lock().unwrap().insert(ts, text.trim().to_string());
    "Nota salva.".to_string()
}

/// Lista todas as notas da sessão atual.
pub fn session_note_list(notes: &std::sync::Mutex<std::collections::HashMap<u64, String>>) -> String {
    let map = notes.lock().unwrap();
    if map.is_empty() {
        return "Sem notas na sessão atual.".into();
    }
    let mut entries: Vec<(u64, String)> = map.iter().map(|(k, v)| (*k, v.clone())).collect();
    entries.sort_by_key(|(k, _)| *k);
    let lines: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(i, (_, text))| format!("[{}] {}", i + 1, text))
        .collect();
    format!("Notas da sessão ({}):\n{}", entries.len(), lines.join("\n"))
}

/// Limpa todas as notas da sessão.
pub fn session_note_clear(notes: &std::sync::Mutex<std::collections::HashMap<u64, String>>) -> String {
    notes.lock().unwrap().clear();
    "Notas da sessão apagadas.".to_string()
}

/// Compara as duas entradas mais recentes do histórico de clipboard.
pub fn diff_clipboard(history: &[String]) -> String {
    if history.len() < 2 {
        return "Preciso de pelo menos duas entradas no histórico do clipboard.".into();
    }
    let newer = &history[0];
    let older = &history[1];
    let diff = similar::TextDiff::from_lines(older, newer);
    let mut out = String::from("Diferenças (antiga → recente):\n");
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Delete => "- ",
            similar::ChangeTag::Insert => "+ ",
            similar::ChangeTag::Equal => "  ",
        };
        out.push_str(sign);
        out.push_str(change.value());
        if !change.value().ends_with('\n') {
            out.push('\n');
        }
    }
    out
}
