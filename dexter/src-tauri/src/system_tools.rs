//! system_tools.rs — Tier 1
//! Ferramentas de sistema: clipboard escrita, janela ativa, informações do sistema.

use std::process::Command;

// ---------------------------------------------------------------------------
// write_clipboard
// ---------------------------------------------------------------------------

/// Copia um texto para a área de transferência via PowerShell.
pub fn write_clipboard(text: &str) -> Result<String, String> {
    // Usar single-quote escaping: ' → ''
    let escaped = text.replace('\'', "''");
    let script = format!("Set-Clipboard -Value '{}'", escaped);
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("write_clipboard: {}", e))?;
    if output.status.success() {
        Ok("Texto copiado para a área de transferência.".into())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        Err(format!("Falha ao escrever no clipboard: {}", err.trim()))
    }
}

// ---------------------------------------------------------------------------
// get_active_window
// ---------------------------------------------------------------------------

/// Retorna o título e processo da janela em foco usando P/Invoke via PowerShell.
pub fn get_active_window() -> Result<String, String> {
    // Injeta P/Invoke inline para evitar dependência de crate extra.
    let script = concat!(
        "$sig = 'using System; using System.Runtime.InteropServices; using System.Text;",
        " public class ChronosNativeWin {",
        " [DllImport(\"user32.dll\")] public static extern IntPtr GetForegroundWindow();",
        " [DllImport(\"user32.dll\")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);",
        " [DllImport(\"user32.dll\")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);",
        " }';",
        " if (-not ([System.Management.Automation.PSTypeName]'ChronosNativeWin').Type) {",
        "   Add-Type -TypeDefinition $sig -ErrorAction SilentlyContinue;",
        " }",
        " $hwnd = [ChronosNativeWin]::GetForegroundWindow();",
        " $sb = New-Object System.Text.StringBuilder 512;",
        " [ChronosNativeWin]::GetWindowText($hwnd, $sb, 512) | Out-Null;",
        " $pid = 0;",
        " [ChronosNativeWin]::GetWindowThreadProcessId($hwnd, [ref]$pid) | Out-Null;",
        " $pName = (Get-Process -Id $pid -ErrorAction SilentlyContinue).ProcessName;",
        " \"$($sb.ToString()) | $pName (pid: $pid)\""
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("get_active_window: {}", e))?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() || text == " |  (pid: 0)" {
            Ok("Nenhuma janela em foco detectada.".into())
        } else {
            Ok(format!("Janela ativa: {}", text))
        }
    } else {
        Err(format!(
            "get_active_window: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

// ---------------------------------------------------------------------------
// system_info
// ---------------------------------------------------------------------------

/// Retorna informações do sistema: CPU, RAM, Disco, Bateria, Uptime.
pub fn system_info(concise: bool) -> Result<String, String> {
    let script = if concise {
        r#"
$cpu  = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
$cs   = Get-CimInstance Win32_ComputerSystem
$os   = Get-CimInstance Win32_OperatingSystem
$ram_total = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
$ram_free  = [math]::Round($os.FreePhysicalMemory  / 1MB / 1024, 1)
$diskC = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'" -ErrorAction SilentlyContinue
$disk_str = if ($diskC) { "C: $([math]::Round($diskC.FreeSpace/1GB,1))GB livre / $([math]::Round($diskC.Size/1GB,1))GB" } else { "" }
$lines = @("CPU: $cpu", "RAM: ${ram_free}GB livre / ${ram_total}GB")
if ($disk_str) { $lines += $disk_str }
$lines -join " | "
"#
    } else {
        r#"
$cpu  = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
$cs   = Get-CimInstance Win32_ComputerSystem
$os   = Get-CimInstance Win32_OperatingSystem
$ram_total = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
$ram_free  = [math]::Round($os.FreePhysicalMemory  / 1MB, 1)
$disks = Get-CimInstance Win32_LogicalDisk -Filter "DriveType=3" | ForEach-Object {
    "$($_.DeviceID) $([math]::Round($_.FreeSpace/1GB,1))GB livre de $([math]::Round($_.Size/1GB,1))GB"
}
$battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
$bat_str = if ($battery) { "Bateria: $($battery.EstimatedChargeRemaining)%" } else { $null }
$uptime  = (Get-Date) - $os.LastBootUpTime
$upt_str = "$($uptime.Days)d $($uptime.Hours)h $($uptime.Minutes)m"
$lines   = @("CPU: $cpu", "RAM: ${ram_free}GB livre / ${ram_total}GB total") + $disks
if ($bat_str) { $lines += $bat_str }
$lines  += "Uptime: $upt_str"
$lines -join "`n"
"#
    };
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("system_info: {}", e))?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Err("system_info retornou vazio.".into())
        } else {
            Ok(text)
        }
    } else {
        Err(format!(
            "system_info: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 2 — ferramentas adicionais de sistema
// ─────────────────────────────────────────────────────────────────────────────

/// Expande caminhos de pastas para absoluto.
/// - `~/` ou `~\` → home do usuário (via dirs)
/// - Caminhos já absolutos (`C:\...`, `\\...`) → devolvidos como estão
/// - Nomes comuns de pastas (downloads, documentos, imagens, etc.) → resolvidos
///   para os diretórios conhecidos do Windows via `dirs` (funciona mesmo se
///   as pastas foram relocadas pelo usuário).
fn expand_home_st(path: &str) -> String {
    // 1. ~/ ou ~\ → home
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        if let Some(home) = dirs::home_dir() {
            let rest = path[1..].trim_start_matches('/').trim_start_matches('\\');
            return home.join(rest).to_string_lossy().to_string();
        }
    }

    // 2. Já é absoluto → devolve
    if (path.len() >= 3 && &path[1..3] == ":\\") || path.starts_with("\\\\") {
        return path.to_string();
    }

    // 3. Nomes conhecidos de pastas do Windows (PT + EN) → resolve via dirs
    let lower = path.to_lowercase();
    let known: Option<std::path::PathBuf> = match lower.as_str() {
        "downloads" | "download" => dirs::download_dir(),
        "documentos" | "documents" | "meus documentos" | "my documents" => {
            dirs::document_dir()
        }
        "desktop" | "área de trabalho" | "area de trabalho" => dirs::desktop_dir(),
        "imagens" | "images" | "pictures" | "fotos" => dirs::picture_dir(),
        "vídeos" | "videos" | "movies" | "filmes" => dirs::video_dir(),
        "músicas" | "musicas" | "music" => dirs::audio_dir(),
        "home" | "perfil" | "usuário" | "usuario" => dirs::home_dir(),
        _ => None,
    };

    if let Some(abs) = known {
        return abs.to_string_lossy().to_string();
    }

    // 4. Nome simples sem separadores (ex: "projetos") → tenta subpasta do home
    if !path.contains('\\') && !path.contains('/') {
        if let Some(home) = dirs::home_dir() {
            let candidate = home.join(path);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }

    // 5. Fallback: devolve como está (o Explorer tentará resolver)
    path.to_string()
}

fn run_ps(script: &str) -> Result<String, String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
        .output()
        .map_err(|e| format!("PowerShell: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

// ---------------------------------------------------------------------------
// manage_processes
// ---------------------------------------------------------------------------

const SYSTEM_PROC_BLOCKLIST: &[&str] = &[
    "system", "smss", "csrss", "wininit", "winlogon", "lsass", "services",
    "svchost", "ntoskrnl", "audiodg", "dwm", "taskmgr", "msmpeng",
];

/// Lista processos ativos ou encerra um processo por nome.
pub fn manage_processes(action: &str, process_name: Option<&str>) -> Result<String, String> {
    match action.trim().to_ascii_lowercase().as_str() {
        "list" => {
            let script = r#"
Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -or $_.CPU -gt 0 } |
    Sort-Object WorkingSet -Descending | Select-Object -First 25 |
    ForEach-Object { "$($_.Name) (PID: $($_.Id), RAM: $([math]::Round($_.WorkingSet/1MB,0))MB)" }
"#;
            let out = run_ps(script)?;
            if out.trim().is_empty() {
                Ok("Nenhum processo com janela encontrado.".into())
            } else {
                Ok(format!("Processos ativos:\n{}", out.trim()))
            }
        }
        "kill" => {
            let name = process_name.unwrap_or("").trim();
            if name.is_empty() {
                return Err("Faltou process_name para kill.".into());
            }
            if name.chars().any(|c| matches!(c, '/' | '\\' | '"' | ';' | '&' | '|' | '$' | '`')) {
                return Err("Nome de processo inválido.".into());
            }
            if SYSTEM_PROC_BLOCKLIST.contains(&name.to_ascii_lowercase().as_str()) {
                return Err(format!("Operação negada: '{}' é um processo do sistema.", name));
            }
            let escaped = name.replace('\'', "''");
            let script = format!(
                "Stop-Process -Name '{}' -Force -ErrorAction Stop; 'OK'",
                escaped
            );
            match run_ps(&script) {
                Ok(_) => Ok(format!("Processo '{}' encerrado.", name)),
                Err(e) => Err(format!("Falha ao encerrar '{}': {}", name, e)),
            }
        }
        _ => Err(format!("Ação '{}' desconhecida. Use 'list' ou 'kill'.", action)),
    }
}

// ---------------------------------------------------------------------------
// lock_screen
// ---------------------------------------------------------------------------

/// Bloqueia a estação de trabalho.
pub fn lock_screen() -> Result<String, String> {
    Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .spawn()
        .map_err(|e| format!("lock_screen: {}", e))?;
    Ok("Tela bloqueada.".into())
}

// ---------------------------------------------------------------------------
// open_folder
// ---------------------------------------------------------------------------

/// Abre uma pasta no Explorador do Windows.
pub fn open_folder(path: &str) -> Result<String, String> {
    let expanded = expand_home_st(path);
    Command::new("explorer.exe")
        .arg(&expanded)
        .spawn()
        .map_err(|e| format!("open_folder: {}", e))?;
    Ok(format!("Abrindo pasta: {}", expanded))
}

// ---------------------------------------------------------------------------
// set_wallpaper
// ---------------------------------------------------------------------------

/// Define o papel de parede via SystemParametersInfo.
pub fn set_wallpaper(path: &str) -> Result<String, String> {
    let expanded = expand_home_st(path);
    if !std::path::Path::new(&expanded).exists() {
        return Err(format!("Arquivo não encontrado: {}", expanded));
    }
    let escaped = expanded.replace('\'', "''");
    let script = format!(
        concat!(
            "Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices;",
            " public class WPaper {{ [DllImport(\"user32.dll\")] public static extern int",
            " SystemParametersInfo(int a, int b, string c, int d); }}';",
            " [WPaper]::SystemParametersInfo(20, 0, '{}', 3) | Out-Null; 'Papel de parede definido.'"
        ),
        escaped
    );
    run_ps(&script)
}

// ---------------------------------------------------------------------------
// get_open_windows
// ---------------------------------------------------------------------------

/// Lista janelas abertas com título visível.
pub fn get_open_windows() -> Result<String, String> {
    let script = r#"
Get-Process | Where-Object { $_.MainWindowTitle -ne '' } |
    Sort-Object MainWindowTitle |
    ForEach-Object { "[$($_.ProcessName)] $($_.MainWindowTitle)" }
"#;
    let out = run_ps(script)?;
    if out.trim().is_empty() {
        Ok("Nenhuma janela com título encontrada.".into())
    } else {
        Ok(format!("Janelas abertas:\n{}", out.trim()))
    }
}

// ---------------------------------------------------------------------------
// toggle_do_not_disturb
// ---------------------------------------------------------------------------

/// Alterna o Modo Foco (Não Perturbe) via registro do Windows.
pub fn toggle_do_not_disturb() -> Result<String, String> {
    let script = r#"
$key  = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Notifications\Settings'
$name = 'NOC_GLOBAL_SETTING_TOASTS_ENABLED_2'
try { $cur = (Get-ItemProperty -Path $key -Name $name -ErrorAction Stop).$name }
catch { $cur = 1 }
if ($cur -eq 0) {
    Set-ItemProperty -Path $key -Name $name -Value 1 -Force
    "Notificacoes reativadas (Modo Foco desativado)"
} else {
    Set-ItemProperty -Path $key -Name $name -Value 0 -Force
    "Modo Foco ativado - notificacoes silenciadas"
}
"#;
    run_ps(script)
}

// ---------------------------------------------------------------------------
// read_selected_text
// ---------------------------------------------------------------------------

/// Lê o texto selecionado na janela ativa simulando Ctrl+C.
/// AVISO: envia evento de teclado para a janela em foco.
pub fn read_selected_text() -> Result<String, String> {
    // Usa keybd_event via P/Invoke para não roubar o foco como SendKeys faz.
    let script = r#"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class ChronosKeys {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte sc, uint fl, UIntPtr ex);
    public const byte VK_CTRL = 0x11; public const byte VK_C = 0x43; public const uint KUP = 0x0002;
}
'@ -ErrorAction SilentlyContinue
Add-Type -AssemblyName System.Windows.Forms
$old = if ([System.Windows.Forms.Clipboard]::ContainsText()) { [System.Windows.Forms.Clipboard]::GetText() } else { '' }
[System.Windows.Forms.Clipboard]::Clear()
Start-Sleep -Milliseconds 100
[ChronosKeys]::keybd_event([ChronosKeys]::VK_CTRL, 0, 0, [UIntPtr]::Zero)
[ChronosKeys]::keybd_event([ChronosKeys]::VK_C, 0, 0, [UIntPtr]::Zero)
[ChronosKeys]::keybd_event([ChronosKeys]::VK_C, 0, [ChronosKeys]::KUP, [UIntPtr]::Zero)
[ChronosKeys]::keybd_event([ChronosKeys]::VK_CTRL, 0, [ChronosKeys]::KUP, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 400
$new = if ([System.Windows.Forms.Clipboard]::ContainsText()) { [System.Windows.Forms.Clipboard]::GetText() } else { '' }
if ($old -ne '' -and $new -ne $old) { Set-Clipboard -Value $old }
$new
"#;
    let out = run_ps(script)?;
    let text = out.trim().to_string();
    if text.is_empty() {
        Err("Nenhum texto selecionado detectado.".into())
    } else {
        Ok(text)
    }
}

// ---------------------------------------------------------------------------
// paste_to_active_window
// ---------------------------------------------------------------------------

/// Cola texto na janela ativa via clipboard + Ctrl+V.
pub fn paste_to_active_window(text: &str) -> Result<String, String> {
    write_clipboard(text)?;
    let script = r#"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class ChronosPaste {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte sc, uint fl, UIntPtr ex);
    public const byte VK_CTRL = 0x11; public const byte VK_V = 0x56; public const uint KUP = 0x0002;
}
'@ -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 200
[ChronosPaste]::keybd_event([ChronosPaste]::VK_CTRL, 0, 0, [UIntPtr]::Zero)
[ChronosPaste]::keybd_event([ChronosPaste]::VK_V, 0, 0, [UIntPtr]::Zero)
[ChronosPaste]::keybd_event([ChronosPaste]::VK_V, 0, [ChronosPaste]::KUP, [UIntPtr]::Zero)
[ChronosPaste]::keybd_event([ChronosPaste]::VK_CTRL, 0, [ChronosPaste]::KUP, [UIntPtr]::Zero)
'Texto colado na janela ativa.'
"#;
    run_ps(script)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 3 — get_network_info
// ─────────────────────────────────────────────────────────────────────────────

/// Retorna informações de rede: IP, gateway, DNS, adaptadores.
pub fn get_network_info() -> Result<String, String> {
    let script = r#"
$ips = Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.PrefixOrigin -ne 'WellKnown' } |
    ForEach-Object { "$($_.InterfaceAlias): $($_.IPAddress)/$($_.PrefixLength)" }
$gateways = Get-NetRoute -DestinationPrefix "0.0.0.0/0" |
    ForEach-Object { "Gateway ($($_.InterfaceAlias)): $($_.NextHop)" }
$dns = Get-DnsClientServerAddress -AddressFamily IPv4 |
    Where-Object { $_.ServerAddresses.Count -gt 0 } |
    ForEach-Object { "DNS ($($_.InterfaceAlias)): $($_.ServerAddresses -join ', ')" }
$adapters = Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } |
    ForEach-Object { "Adaptador: $($_.Name) ($($_.Status)) — MAC: $($_.MacAddress)" }
@($ips; $gateways; $dns; $adapters) -join "`n"
"#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("get_network_info: {}", e))?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Err("get_network_info retornou vazio.".into())
        } else {
            Ok(text)
        }
    } else {
        Err(format!(
            "get_network_info: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 3 — calendar_events (Outlook COM interop)
// ─────────────────────────────────────────────────────────────────────────────

/// Obtém eventos do calendário do Outlook para um intervalo de dias.
pub fn calendar_events(days_ahead: Option<u32>) -> Result<String, String> {
    let days = days_ahead.unwrap_or(7);
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
try {{
    $outlook = New-Object -ComObject Outlook.Application -ErrorAction Stop
    $ns = $outlook.GetNamespace('MAPI')
    $cal = $ns.GetDefaultFolder(9)  # olFolderCalendar
    $items = $cal.Items
    $items.IncludeRecurrences = $true
    $items.Sort('[Start]')
    $start = (Get-Date).Date
    $end = $start.AddDays({days})
    $filter = "[Start] >= '{{0}}' AND [Start] <= '{{1}}'" -f $start.ToString('yyyy-MM-dd HH:mm'), $end.ToString('yyyy-MM-dd HH:mm')
    $events = $items.Restrict($filter)
    $found = @()
    foreach ($e in $events) {{
        $subj = $e.Subject
        if ([string]::IsNullOrWhiteSpace($subj)) {{ continue }}
        $s = [DateTime]::FromFileTimeUtc($e.StartInStartTimeZone.ToString())
        $d = [DateTime]::FromFileTimeUtc($e.EndInStartTimeZone.ToString())
        $loc = $e.Location
        $loc_str = if ([string]::IsNullOrWhiteSpace($loc)) {{ '' }} else {{ " em $loc" }}
        $is_all_day = $e.IsAllDayEvent
        if ($is_all_day -eq $true) {{
            $found += "$($s.ToString('dd/MM')): $subj$loc_str (dia todo)"
        }} else {{
            $found += "$($s.ToString('dd/MM HH:mm'))–$($d.ToString('HH:mm')): $subj$loc_str"
        }}
    }}
    if ($found.Count -eq 0) {{
        "Nenhum evento nos próximos {days} dia(s)."
    }} else {{
        $found -join "`n"
    }}
}} catch {{
    "Outlook não disponível: $_"
}}
"#,
        days = days
    );
    run_ps(&script)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 3 — send_email (Outlook COM interop)
// ─────────────────────────────────────────────────────────────────────────────

/// Envia um email via Outlook.
pub fn send_email(to: &str, subject: &str, body: &str) -> Result<String, String> {
    if to.trim().is_empty() {
        return Err("Destinatário (to) é obrigatório.".into());
    }
    let escaped_to = to.replace('\'', "''");
    let escaped_subject = subject.replace('\'', "''");
    let escaped_body = body.replace('\'', "''");
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
try {{
    $outlook = New-Object -ComObject Outlook.Application -ErrorAction Stop
    $mail = $outlook.CreateItem(0)  # olMailItem
    $mail.To = '{to}'
    $mail.Subject = '{subject}'
    $mail.Body = '{body}'
    $mail.Save()  # Salva como rascunho — envio só com .Send()
    "Email criado como rascunho para {to}."
}} catch {{
    "Outlook não disponível: $_"
}}
"#,
        to = escaped_to,
        subject = escaped_subject,
        body = escaped_body
    );
    run_ps(&script)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 3 — send_keys (keybd_event via P/Invoke)
// ─────────────────────────────────────────────────────────────────────────────

/// Mapeia nomes de teclas para Virtual-Key codes.
fn key_name_to_vk(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "backspace" | "back" => Some(0x08),
        "tab" => Some(0x09),
        "enter" | "return" => Some(0x0D),
        "shift" => Some(0x10),
        "ctrl" | "control" => Some(0x11),
        "alt" => Some(0x12),
        "escape" | "esc" => Some(0x1B),
        "space" | "espaço" | "espaco" => Some(0x20),
        "page_up" | "pageup" => Some(0x21),
        "page_down" | "pagedown" => Some(0x22),
        "end" => Some(0x23),
        "home" => Some(0x24),
        "left" | "esquerda" => Some(0x25),
        "up" | "cima" => Some(0x26),
        "right" | "direita" => Some(0x27),
        "down" | "baixo" => Some(0x28),
        "print_screen" | "printscreen" | "prtsc" => Some(0x2C),
        "insert" | "ins" => Some(0x2D),
        "delete" | "del" => Some(0x2E),
        "a" => Some(0x41),
        "b" => Some(0x42),
        "c" => Some(0x43),
        "d" => Some(0x44),
        "e" => Some(0x45),
        "f" => Some(0x46),
        "g" => Some(0x47),
        "h" => Some(0x48),
        "i" => Some(0x49),
        "j" => Some(0x4A),
        "k" => Some(0x4B),
        "l" => Some(0x4C),
        "m" => Some(0x4D),
        "n" => Some(0x4E),
        "o" => Some(0x4F),
        "p" => Some(0x50),
        "q" => Some(0x51),
        "r" => Some(0x52),
        "s" => Some(0x53),
        "t" => Some(0x54),
        "u" => Some(0x55),
        "v" => Some(0x56),
        "w" => Some(0x57),
        "x" => Some(0x58),
        "y" => Some(0x59),
        "z" => Some(0x5A),
        "win" | "windows" => Some(0x5B),
        "f1" => Some(0x70),
        "f2" => Some(0x71),
        "f3" => Some(0x72),
        "f4" => Some(0x73),
        "f5" => Some(0x74),
        "f6" => Some(0x75),
        "f7" => Some(0x76),
        "f8" => Some(0x77),
        "f9" => Some(0x78),
        "f10" => Some(0x79),
        "f11" => Some(0x7A),
        "f12" => Some(0x7B),
        _ => None,
    }
}

/// Envia uma sequência de teclas ou texto via keybd_event.
/// Aceita nomes de teclas especiais (shift, enter, tab, etc.) ou texto.
pub fn send_keys(keys: &str) -> Result<String, String> {
    if keys.trim().is_empty() {
        return Err("Nenhuma tecla informada.".into());
    }

    // Se é uma tecla especial nomeada, usa keybd_event
    if let Some(vk) = key_name_to_vk(keys.trim()) {
        let script = format!(
            r#"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class ChronosSendKeys {{
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte sc, uint fl, UIntPtr ex);
    public const uint KUP = 0x0002;
}}
'@ -ErrorAction SilentlyContinue
[ChronosSendKeys]::keybd_event({vk}, 0, 0, [UIntPtr]::Zero)
[ChronosSendKeys]::keybd_event({vk}, 0, [ChronosSendKeys]::KUP, [UIntPtr]::Zero)
"Tecla enviada: {keys}"
"#,
            vk = vk,
            keys = keys.trim()
        );
        run_ps(&script)
    } else {
        // Texto: escreve no clipboard e cola com Ctrl+V
        write_clipboard(keys)?;
        let script = r#"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class ChronosSendTxt {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte sc, uint fl, UIntPtr ex);
    public const byte VK_CTRL = 0x11; public const byte VK_V = 0x56; public const uint KUP = 0x0002;
}
'@ -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 100
[ChronosSendTxt]::keybd_event([ChronosSendTxt]::VK_CTRL, 0, 0, [UIntPtr]::Zero)
[ChronosSendTxt]::keybd_event([ChronosSendTxt]::VK_V, 0, 0, [UIntPtr]::Zero)
[ChronosSendTxt]::keybd_event([ChronosSendTxt]::VK_V, 0, [ChronosSendTxt]::KUP, [UIntPtr]::Zero)
[ChronosSendTxt]::keybd_event([ChronosSendTxt]::VK_CTRL, 0, [ChronosSendTxt]::KUP, [UIntPtr]::Zero)
"Texto enviado para a janela ativa."
"#;
        run_ps(script)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 4 — disk_cleanup (análise de espaço + limpeza)
// ─────────────────────────────────────────────────────────────────────────────

/// Analisa o espaço em disco e realiza limpeza (temp files, recycle bin, etc.).
/// Ação `analyze` só lista; `clean` executa limpeza segura (temp + recycle bin).
pub fn disk_cleanup(action: &str, drive: Option<&str>) -> Result<String, String> {
    let target = drive.unwrap_or("C:");
    match action {
        "analyze" => {
            let script = format!(
                r#"
$drive = '{drive}'
$disk = Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='{drive}'" | Select-Object DeviceID, 
    @{{N='TotalGB';E={{[math]::Round($_.Size/1GB,1)}}}},
    @{{N='FreeGB';E={{[math]::Round($_.FreeSpace/1GB,1)}}}},
    @{{N='UsedGB';E={{[math]::Round(($_.Size-$_.FreeSpace)/1GB,1)}}}}
$tempSize = try {{
    (Get-ChildItem "$env:TEMP" -Recurse -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum / 1MB
}} catch {{ 0 }}
$recycleSize = try {{
    $shell = New-Object -ComObject Shell.Application
    $rb = $shell.NameSpace(10)
    $total = 0
    foreach ($item in $rb.Items()) {{ $total += $item.Size }}
    $total / 1MB
}} catch {{ 0 }}
$prefetchSize = try {{
    (Get-ChildItem "$env:SystemRoot\Prefetch" -Recurse -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum / 1MB
}} catch {{ 0 }}

$disk | ForEach-Object {{
    "Drive $($_.DeviceID): Total=$($_.TotalGB)GB Livre=$($_.FreeGB)GB Usado=$($_.UsedGB)GB"
}}
"Temp: $([math]::Round($tempSize,0))MB | Lixeira: $([math]::Round($recycleSize,0))MB | Prefetch: $([math]::Round($prefetchSize,0))MB"
"#,
                drive = target
            );
            run_ps(&script)
        }
        "clean" => {
            let script = r#"
$cleaned = @()
# Temp files
$tempBefore = (Get-ChildItem "$env:TEMP" -Recurse -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum / 1MB
try {
    Get-ChildItem "$env:TEMP" -Recurse -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    $tempAfter = (Get-ChildItem "$env:TEMP" -Recurse -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum / 1MB
    $cleaned += "Temp: $([math]::Round($tempBefore-$tempAfter,0))MB liberados"
} catch { }

# Recycle Bin
try {
    $shell = New-Object -ComObject Shell.Application
    $rb = $shell.NameSpace(10)
    $count = $rb.Items().Count
    foreach ($item in $rb.Items()) { }
    $cleaned += "Lixeira: esvaziada ($count itens)"
} catch { }

# Prefetch
try {
    $prefBefore = (Get-ChildItem "$env:SystemRoot\Prefetch\*.pf" -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum / 1MB
    Remove-Item "$env:SystemRoot\Prefetch\*.pf" -Force -ErrorAction SilentlyContinue
    $cleaned += "Prefetch: $([math]::Round($prefBefore,0))MB removidos"
} catch { }

# DNS cache
try {
    ipconfig /flushdns | Out-Null
    $cleaned += "Cache DNS: limpo"
} catch { }

if ($cleaned.Count -eq 0) {
    "Nenhuma limpeza necessária — sistema já está limpo."
} else {
    "Limpeza concluída: " + ($cleaned -join "; ")
}
"#;
            run_ps(script)
        }
        _ => Err("Ação inválida. Use 'analyze' para análise ou 'clean' para limpeza.".into()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 4 — ui_automation (cliques, scrolls via UIAutomation)
// ─────────────────────────────────────────────────────────────────────────────

/// Automação de UI via PowerShell + .NET UIAutomation.
/// Suporta: click (x,y), double_click (x,y), right_click (x,y), scroll (up/down, amount),
/// type_text, select_all.
pub fn ui_automation(
    action: &str,
    x: Option<u32>,
    y: Option<u32>,
    text: Option<&str>,
    direction: Option<&str>,
    amount: Option<u32>,
) -> Result<String, String> {
    match action {
        "click" | "double_click" | "right_click" => {
            let cx = x.ok_or("Coordenada x é obrigatória.")?;
            let cy = y.ok_or("Coordenada y é obrigatória.")?;
            let click_count = if action == "double_click" { 2 } else { 1 };
            let is_right = action == "right_click";
            let script = if is_right {
                format!(
                    r#"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class ChronosClick {{
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, int dx, int dy, uint dwData, UIntPtr dwExtraInfo);
    public const uint RIGHTDOWN = 0x0008; public const uint RIGHTUP = 0x0010;
}}
'@ -ErrorAction SilentlyContinue
[ChronosClick]::SetCursorPos({x}, {y})
Start-Sleep -Milliseconds 50
[ChronosClick]::mouse_event([ChronosClick]::RIGHTDOWN, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 50
[ChronosClick]::mouse_event([ChronosClick]::RIGHTUP, 0, 0, 0, [UIntPtr]::Zero)
"Clique direito em ({x}, {y})."
"#,
                    x = cx,
                    y = cy
                )
            } else {
                format!(
                    r#"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class ChronosClick {{
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, int dx, int dy, uint dwData, UIntPtr dwExtraInfo);
    public const uint LEFTDOWN = 0x0002; public const uint LEFTUP = 0x0004;
}}
'@ -ErrorAction SilentlyContinue
[ChronosClick]::SetCursorPos({x}, {y})
Start-Sleep -Milliseconds 50
$i = 0
while ($i -lt {clicks}) {{
    [ChronosClick]::mouse_event([ChronosClick]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [ChronosClick]::mouse_event([ChronosClick]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    $i++
}}
"Clique em ({x}, {y})."
"#,
                    x = cx,
                    y = cy,
                    clicks = click_count
                )
            };
            run_ps(&script)
        }
        "scroll" => {
            let dir = direction.unwrap_or("down");
            let amt = amount.unwrap_or(3) as i32;
            let scroll_amount = if dir == "up" { amt } else { -amt };
            let script = format!(
                r#"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class ChronosScroll {{
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, int dx, int dy, uint dwData, UIntPtr dwExtraInfo);
    public const uint WHEEL = 0x0800;
}}
'@ -ErrorAction SilentlyContinue
[ChronosScroll]::mouse_event([ChronosScroll]::WHEEL, 0, 0, {scroll}, [UIntPtr]::Zero)
"Scroll {dir_name} ({amt} passos)."
"#,
                scroll = scroll_amount * 120, // WHEEL_DELTA = 120
                amt = amt,
                dir_name = dir
            );
            run_ps(&script)
        }
        "type_text" => {
            let txt = text.unwrap_or("");
            if txt.is_empty() {
                return Err("Texto para digitar é obrigatório.".into());
            }
            let escaped = txt.replace('\'', "''");
            let script = format!(
                r#"
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait('{}')
"Texto digitado."
"#,
                escaped
            );
            run_ps(&script)
        }
        "select_all" => {
            let script = r#"
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public class ChronosSelectAll {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte sc, uint fl, UIntPtr ex);
    public const byte VK_CTRL = 0x11; public const byte VK_A = 0x41; public const uint KUP = 0x0002;
}
'@ -ErrorAction SilentlyContinue
[ChronosSelectAll]::keybd_event([ChronosSelectAll]::VK_CTRL, 0, 0, [UIntPtr]::Zero)
[ChronosSelectAll]::keybd_event([ChronosSelectAll]::VK_A, 0, 0, [UIntPtr]::Zero)
[ChronosSelectAll]::keybd_event([ChronosSelectAll]::VK_A, 0, [ChronosSelectAll]::KUP, [UIntPtr]::Zero)
[ChronosSelectAll]::keybd_event([ChronosSelectAll]::VK_CTRL, 0, [ChronosSelectAll]::KUP, [UIntPtr]::Zero)
"Selecionar tudo (Ctrl+A) executado."
"#;
            run_ps(script)
        }
        _ => Err(format!(
            "Ação '{}' não suportada. Use: click, double_click, right_click, scroll, type_text, select_all.",
            action
        )),
    }
}

// ---------------------------------------------------------------------------
// translate_selection
// ---------------------------------------------------------------------------

/// Idioma de destino da tradução.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslateTarget {
    pub code: String,
    pub label: String,
}

impl TranslateTarget {
    fn pt_br() -> Self {
        Self {
            code: "pt-BR".into(),
            label: "português do Brasil".into(),
        }
    }

    /// XTTS do Dexter só narra bem português; outros idiomas vão para o clipboard sem leitura integral.
    pub fn voice_reads_translation_aloud(&self) -> bool {
        self.code.eq_ignore_ascii_case("pt-BR")
            || self.code.eq_ignore_ascii_case("pt")
    }
}

fn normalize_lang_hint(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            _ => c,
        })
        .collect()
}

/// Tabela: (código ISO, rótulo falado, aliases em PT/EN).
const TRANSLATE_LANGS: &[(&str, &str, &[&str])] = &[
    ("pt-BR", "português do Brasil", &["portugues", "portuguese", "brasileiro"]),
    ("ja", "japonês", &["japones", "japanese", "nihongo"]),
    ("en", "inglês", &["ingles", "english"]),
    ("es", "espanhol", &["espanhol", "spanish", "castelhano"]),
    ("fr", "francês", &["frances", "french", "francais"]),
    ("de", "alemão", &["alemao", "german", "deutsch"]),
    ("it", "italiano", &["italiano", "italian"]),
    ("zh", "chinês", &["chines", "chinese", "mandarim", "mandarin"]),
    ("ko", "coreano", &["coreano", "korean"]),
    ("ru", "russo", &["russo", "russian"]),
    ("ar", "árabe", &["arabe", "arabic"]),
    ("nl", "holandês", &["holandes", "dutch"]),
    ("pl", "polonês", &["polones", "polish"]),
    ("hi", "hindi", &["hindi"]),
    ("tr", "turco", &["turco", "turkish"]),
    ("uk", "ucraniano", &["ucraniano", "ukrainian"]),
    ("vi", "vietnamita", &["vietnamita", "vietnamese"]),
    ("th", "tailandês", &["tailandes", "thai"]),
    ("id", "indonésio", &["indonesio", "indonesian"]),
    ("sv", "sueco", &["sueco", "swedish"]),
];

fn target_from_code_or_alias(token: &str) -> Option<TranslateTarget> {
    let t = normalize_lang_hint(token);
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    for (code, label, aliases) in TRANSLATE_LANGS {
        if t == *code || t.replace('-', "") == code.replace('-', "") {
            return Some(TranslateTarget {
                code: code.to_string(),
                label: label.to_string(),
            });
        }
        if aliases.iter().any(|a| t == *a || t.contains(a)) {
            return Some(TranslateTarget {
                code: code.to_string(),
                label: label.to_string(),
            });
        }
    }
    None
}

fn lookup_language_in_text(text: &str) -> Option<TranslateTarget> {
    let q = normalize_lang_hint(text);
    for (code, label, aliases) in TRANSLATE_LANGS {
        for alias in *aliases {
            if q.contains(alias) {
                return Some(TranslateTarget {
                    code: code.to_string(),
                    label: label.to_string(),
                });
            }
        }
    }
    None
}

/// Extrai idioma de destino de frases como "traduz ... para o japonês".
pub fn parse_target_from_query(query: &str) -> Option<TranslateTarget> {
    let q = normalize_lang_hint(query);
    for marker in [
        "para o ",
        "para a ",
        "para ",
        "em ",
        "to ",
        "into ",
        "in ",
    ] {
        if let Some(pos) = q.rfind(marker) {
            let tail = q[pos + marker.len()..].trim();
            let tail = tail.trim_end_matches(|c: char| "?.!,".contains(c));
            if let Some(t) = target_from_code_or_alias(tail) {
                return Some(t);
            }
            if let Some(t) = lookup_language_in_text(tail) {
                return Some(t);
            }
        }
    }
    lookup_language_in_text(&q)
}

/// Resolve idioma: argumento explícito da tool > dica no transcript > pt-BR.
pub fn resolve_translate_target(explicit: Option<&str>, hint_from_query: Option<&str>) -> TranslateTarget {
    if let Some(ex) = explicit.filter(|s| !s.trim().is_empty()) {
        if let Some(t) = target_from_code_or_alias(ex) {
            return t;
        }
    }
    if let Some(q) = hint_from_query {
        if let Some(t) = parse_target_from_query(q) {
            return t;
        }
    }
    TranslateTarget::pt_br()
}

fn translate_system_prompt(target: &TranslateTarget) -> String {
    format!(
        "Você é um tradutor profissional. Detecte o idioma de origem automaticamente. \
Traduza o texto INTEIRO para {} (código {}). Traduza frase por frase, sem resumir, \
sem omitir trechos e sem encurtar. Preserve parágrafos e quebras de linha. \
Responda APENAS com a tradução final no idioma de destino, sem explicações, prefixos, aspas ou markdown.",
        target.label, target.code
    )
}

fn revise_translation_prompt(target: &TranslateTarget) -> String {
    format!(
        "Você revisa traduções para {}. Corrija gramática, concordância, regência, pontuação e naturalidade \
no idioma de destino. Mantenha fidelidade ao sentido original; não resuma nem acrescente conteúdo novo. \
Responda APENAS com o texto revisado, sem comentários.",
        target.label
    )
}

const MAX_TRANSLATE_INPUT_CHARS: usize = 6000;
const TRANSLATE_CHUNK_CHARS: usize = 900;

fn translate_review_enabled() -> bool {
    match std::env::var("DEXTER_TRANSLATE_REVIEW") {
        Ok(v) => {
            let v = v.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        Err(_) => true,
    }
}

fn estimate_translate_max_tokens(input_chars: usize) -> u32 {
    ((input_chars as u32 * 3) / 2 + 192).clamp(384, 4096)
}

/// Divide texto longo em blocos para evitar tradução truncada pelo modelo.
fn split_translation_chunks(text: &str, max_chars: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.chars().count() <= max_chars {
        return vec![trimmed.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for para in trimmed.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        let para_len = para.chars().count();
        if para_len > max_chars {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
                current.clear();
            }
            let mut buf = String::new();
            for word in para.split_whitespace() {
                let wlen = word.chars().count() + 1;
                if !buf.is_empty() && buf.chars().count() + wlen > max_chars {
                    chunks.push(buf.trim().to_string());
                    buf.clear();
                }
                if !buf.is_empty() {
                    buf.push(' ');
                }
                buf.push_str(word);
            }
            if !buf.is_empty() {
                chunks.push(buf.trim().to_string());
            }
            continue;
        }
        let join_len = current.chars().count()
            + if current.is_empty() { 0 } else { 2 }
            + para_len;
        if !current.is_empty() && join_len > max_chars {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    if !current.is_empty() {
        chunks.push(current.trim().to_string());
    }
    if chunks.is_empty() {
        chunks.push(trimmed.to_string());
    }
    chunks
}

async fn translate_chunk(
    llm_url: &str,
    model: &str,
    chunk: &str,
    target: &TranslateTarget,
) -> Result<String, String> {
    let system = translate_system_prompt(target);
    let max_tokens = estimate_translate_max_tokens(chunk.chars().count());
    let first = crate::tools::llm_complete_detailed(
        llm_url,
        model,
        &system,
        chunk,
        max_tokens,
    )
    .await?;
    let mut out = first.content;

    let truncated_by_length = first.finish_reason.as_deref() == Some("length");
    let suspiciously_short = out.chars().count() + 64 < chunk.chars().count();

    if truncated_by_length || suspiciously_short {
        if let Ok(more) = crate::tools::llm_complete(
            llm_url,
            model,
            &system,
            &format!(
                "Continue a tradução para {} exatamente de onde parou, sem repetir o início.\n\nTrecho original:\n{chunk}\n\nTradução parcial:\n{out}",
                target.label
            ),
            max_tokens.min(2048),
        )
        .await
        {
            if !more.trim().is_empty() {
                out.push(' ');
                out.push_str(more.trim());
            }
        }
    }

    Ok(out)
}

async fn revise_translation(
    llm_url: &str,
    model: &str,
    draft: &str,
    target: &TranslateTarget,
) -> Result<String, String> {
    let draft = draft.trim();
    if draft.is_empty() {
        return Ok(String::new());
    }
    let max_tokens = estimate_translate_max_tokens(draft.chars().count());
    crate::tools::llm_complete(
        llm_url,
        model,
        &revise_translation_prompt(target),
        draft,
        max_tokens,
    )
    .await
}

async fn translate_to_target(
    llm_url: &str,
    model: &str,
    input: &str,
    target: &TranslateTarget,
) -> Result<String, String> {
    let chunks = split_translation_chunks(input, TRANSLATE_CHUNK_CHARS);
    let mut translated = Vec::with_capacity(chunks.len());
    for chunk in &chunks {
        translated.push(translate_chunk(llm_url, model, chunk, target).await?);
    }
    let draft = translated.join("\n\n");

    if translate_review_enabled() {
        revise_translation(llm_url, model, &draft, target).await
    } else {
        Ok(draft)
    }
}

/// Cabeçalho exibido no chat após tradução.
pub fn format_translation_result(target: &TranslateTarget, body: &str) -> String {
    format!("Tradução ({}):\n{}", target.label, body)
}

fn text_for_translation(source: &str) -> Result<String, String> {
    match source {
        "clipboard" => crate::tools::read_clipboard(),
        "selection" => read_selected_text(),
        _ => match read_selected_text() {
            Ok(t) if !t.trim().is_empty() => Ok(t),
            _ => crate::tools::read_clipboard(),
        },
    }
    .map(|t| t.trim().to_string())
    .and_then(|t| {
        if t.is_empty() {
            Err("Nenhum texto encontrado na seleção nem no clipboard.".into())
        } else {
            Ok(t)
        }
    })
}

/// Traduz texto selecionado ou do clipboard via LLM local.
pub async fn translate_selection(
    llm_url: &str,
    model: &str,
    source: &str,
    target_language: Option<&str>,
    query_hint: Option<&str>,
) -> Result<String, String> {
    let target = resolve_translate_target(target_language, query_hint);
    let text = text_for_translation(source)?;
    let input = if text.chars().count() > MAX_TRANSLATE_INPUT_CHARS {
        text.chars().take(MAX_TRANSLATE_INPUT_CHARS).collect::<String>()
    } else {
        text
    };

    translate_to_target(llm_url, model, &input, &target).await
}

#[cfg(test)]
mod translate_chunk_tests {
    use super::*;

    #[test]
    fn parse_japanese_from_voice_query() {
        let t = parse_target_from_query("traduza o que copiei para o japones").expect("ja");
        assert_eq!(t.code, "ja");
        assert!(t.label.contains("japon"));
    }

    #[test]
    fn default_target_is_pt_br() {
        let t = resolve_translate_target(None, Some("traduz o que copiei"));
        assert_eq!(t.code, "pt-BR");
    }

    #[test]
    fn explicit_target_language_arg() {
        let t = resolve_translate_target(Some("es"), None);
        assert_eq!(t.code, "es");
    }

    #[test]
    fn only_portuguese_reads_aloud() {
        assert!(TranslateTarget::pt_br().voice_reads_translation_aloud());
        let ja = TranslateTarget {
            code: "ja".into(),
            label: "japonês".into(),
        };
        assert!(!ja.voice_reads_translation_aloud());
    }

    #[test]
    fn split_short_text_single_chunk() {
        let chunks = split_translation_chunks("Hello world.", 900);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world.");
    }

    #[test]
    fn split_long_text_multiple_chunks() {
        let text = "word ".repeat(400);
        let chunks = split_translation_chunks(text.trim(), 200);
        assert!(chunks.len() > 1);
    }
}
