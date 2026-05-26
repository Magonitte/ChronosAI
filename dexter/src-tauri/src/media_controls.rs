//! Windows: control active media session (Spotify, browsers, Groove, etc.) via SMTC,
//! and system volume via multimedia keys.

#[cfg(target_os = "windows")]
mod win {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    };
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    fn ensure_sta() -> Result<(), String> {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_ok() || hr.0 == windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
                Ok(())
            } else {
                Err(format!("CoInitializeEx failed: {:?}", hr))
            }
        }
    }

    fn press_vkey(vk: u16) {
        unsafe {
            let down = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: 0,
                        dwFlags: KEYEVENTF_EXTENDEDKEY,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            let up = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// VK_VOLUME_MUTE / VK_VOLUME_DOWN / VK_VOLUME_UP
    pub fn send_volume_key(vk: u16, steps: u32) {
        for _ in 0..steps.max(1) {
            press_vkey(vk);
        }
    }

    fn session_manager() -> Result<GlobalSystemMediaTransportControlsSessionManager, String> {
        ensure_sta()?;
        let op = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map_err(|e| format!("RequestAsync failed: {:?}", e))?;
        op.get()
            .map_err(|e| format!("Could not get media session manager: {:?}", e))
    }

    fn session_control_score(
        session: &GlobalSystemMediaTransportControlsSession,
    ) -> Option<(i32, GlobalSystemMediaTransportControlsSessionPlaybackStatus)> {
        let info = session.GetPlaybackInfo().ok()?;
        let controls = info.Controls().ok()?;
        let status = info.PlaybackStatus().ok()?;
        use GlobalSystemMediaTransportControlsSessionPlaybackStatus as P;
        let mut score: i32 = match status {
            P::Playing => 100,
            P::Paused => 70,
            P::Opened => 50,
            P::Changing => 45,
            P::Stopped => 25,
            P::Closed => 5,
            _ => 15,
        };
        if controls.IsPlayPauseToggleEnabled().unwrap_or(false) {
            score += 30;
        }
        if controls.IsPlayEnabled().unwrap_or(false) {
            score += 20;
        }
        if controls.IsPauseEnabled().unwrap_or(false) {
            score += 10;
        }
        Some((score, status))
    }

    /// Prefer Groove Music (app ID Microsoft.ZuneMusic…) when several sessions compete (ex.: Edge + Groove).
    fn groove_session_bonus(session: &GlobalSystemMediaTransportControlsSession) -> i32 {
        let Ok(id) = session.SourceAppUserModelId() else {
            return 0;
        };
        let id = id.to_string().to_ascii_lowercase();
        if id.contains("zunemusic") || id.contains("groove") {
            50
        } else {
            0
        }
    }

    /// Prefer the session that actually exposes controls (Groove vs random app with empty SMTC).
    fn pick_session(
        manager: &GlobalSystemMediaTransportControlsSessionManager,
    ) -> Result<GlobalSystemMediaTransportControlsSession, String> {
        let sessions = manager
            .GetSessions()
            .map_err(|e| format!("GetSessions failed: {:?}", e))?;
        let n = sessions
            .Size()
            .map_err(|e| format!("Sessions.Size failed: {:?}", e))?;
        if n == 0 {
            return Err(
                "Nenhuma sessão de mídia no Windows. Abra Spotify, YouTube no navegador, Groove ou outro player e tente de novo. Para só abrir o app de música use launch_desktop_app com media_player.".into(),
            );
        }

        let mut best_i: u32 = 0;
        let mut best_score: i32 = -1;

        for i in 0..n {
            let s = sessions
                .GetAt(i)
                .map_err(|e| format!("GetAt failed: {:?}", e))?;
            if let Some((mut sc, _)) = session_control_score(&s) {
                sc += groove_session_bonus(&s);
                if sc > best_score {
                    best_score = sc;
                    best_i = i;
                }
            }
        }

        if let Ok(cur) = manager.GetCurrentSession() {
            if let Some((mut cur_sc, _)) = session_control_score(&cur) {
                cur_sc += groove_session_bonus(&cur);
                // Current session wins if it is close — avoids switching away from obvious focus.
                if cur_sc + 15 >= best_score {
                    return Ok(cur);
                }
            }
        }

        sessions
            .GetAt(best_i)
            .map_err(|e| format!("GetAt(best) failed: {:?}", e))
    }

    /// Resume/start playback: many apps report IsPlayEnabled=false when paused but accept toggle.
    fn try_play_or_resume(
        session: &GlobalSystemMediaTransportControlsSession,
    ) -> Result<bool, String> {
        let playback_info = session
            .GetPlaybackInfo()
            .map_err(|e| format!("GetPlaybackInfo: {:?}", e))?;
        let controls = playback_info
            .Controls()
            .map_err(|e| format!("PlaybackInfo.Controls: {:?}", e))?;

        if controls.IsPlayEnabled().unwrap_or(false) {
            let ok = session
                .TryPlayAsync()
                .map_err(|e| format!("TryPlayAsync: {:?}", e))?
                .get()
                .map_err(|e| format!("TryPlayAsync result: {:?}", e))?;
            if ok {
                return Ok(true);
            }
        }

        if controls.IsPlayPauseToggleEnabled().unwrap_or(false) {
            let ok = session
                .TryTogglePlayPauseAsync()
                .map_err(|e| format!("TryTogglePlayPauseAsync: {:?}", e))?
                .get()
                .map_err(|e| format!("TryTogglePlayPauseAsync result: {:?}", e))?;
            if ok {
                return Ok(true);
            }
        }

        // Apps às vezes mentem nos flags — tenta na ordem play → toggle.
        if let Ok(op) = session.TryPlayAsync() {
            if let Ok(true) = op.get() {
                return Ok(true);
            }
        }
        if let Ok(op) = session.TryTogglePlayPauseAsync() {
            if let Ok(true) = op.get() {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn playback_status_name(
        s: GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    ) -> &'static str {
        use GlobalSystemMediaTransportControlsSessionPlaybackStatus as P;
        match s {
            P::Playing => "playing",
            P::Paused => "paused",
            P::Stopped => "stopped",
            P::Closed => "closed",
            _ => "unknown",
        }
    }

    pub fn control_playback(action: &str) -> Result<String, String> {
        let manager = session_manager()?;
        let session = pick_session(&manager)?;

        let playback_info = session
            .GetPlaybackInfo()
            .map_err(|e| format!("GetPlaybackInfo: {:?}", e))?;
        let controls = playback_info
            .Controls()
            .map_err(|e| format!("PlaybackInfo.Controls: {:?}", e))?;

        let action_norm = action.trim().to_ascii_lowercase();

        if action_norm == "status" || action_norm == "now_playing" {
            let status = playback_info
                .PlaybackStatus()
                .map_err(|e| format!("PlaybackStatus: {:?}", e))?;

            let mut lines = vec![format!(
                "Estado: {}",
                playback_status_name(status)
            )];

            if let Ok(op) = session.TryGetMediaPropertiesAsync() {
                if let Ok(props) = op.get() {
                    let title = props.Title().unwrap_or_default().to_string();
                    let artist = props.Artist().unwrap_or_default().to_string();
                    let album = props.AlbumTitle().unwrap_or_default().to_string();
                    if !title.is_empty() {
                        lines.push(format!("Faixa: {}", title));
                    }
                    if !artist.is_empty() {
                        lines.push(format!("Artista: {}", artist));
                    }
                    if !album.is_empty() {
                        lines.push(format!("Álbum: {}", album));
                    }
                }
            }

            lines.push(format!(
                "Controles: play={}, pause={}, próxima={}, anterior={}, toggle={}",
                controls.IsPlayEnabled().unwrap_or(false),
                controls.IsPauseEnabled().unwrap_or(false),
                controls.IsNextEnabled().unwrap_or(false),
                controls.IsPreviousEnabled().unwrap_or(false),
                controls.IsPlayPauseToggleEnabled().unwrap_or(false),
            ));

            return Ok(lines.join("\n"));
        }

        let ok = match action_norm.as_str() {
            "play" => {
                let worked = try_play_or_resume(&session)?;
                if !worked {
                    return Err(
                        "Não consegui iniciar a reprodução nesta sessão. Abra Spotify, YouTube no Edge ou Chrome, ou o Groove, comece uma faixa ou deixe algo na fila; você pode pedir para abrir o reprodutor com launch_desktop_app e media_player. Para tocar algo novo pela web use open_url com um link do YouTube ou Spotify.".into(),
                    );
                }
                true
            }
            "pause" => {
                if !controls.IsPauseEnabled().unwrap_or(false) {
                    return Err("Pausar não está disponível para esta sessão.".into());
                }
                session
                    .TryPauseAsync()
                    .map_err(|e| format!("TryPauseAsync: {:?}", e))?
                    .get()
                    .map_err(|e| format!("TryPauseAsync result: {:?}", e))?
            }
            "toggle" | "play_pause" => {
                if !controls.IsPlayPauseToggleEnabled().unwrap_or(false) {
                    return Err("Alternar play/pause não está disponível.".into());
                }
                session
                    .TryTogglePlayPauseAsync()
                    .map_err(|e| format!("TryTogglePlayPauseAsync: {:?}", e))?
                    .get()
                    .map_err(|e| format!("TryTogglePlayPauseAsync result: {:?}", e))?
            }
            "next" | "skip_next" => {
                if !controls.IsNextEnabled().unwrap_or(false) {
                    return Err("Próxima faixa não está disponível.".into());
                }
                session
                    .TrySkipNextAsync()
                    .map_err(|e| format!("TrySkipNextAsync: {:?}", e))?
                    .get()
                    .map_err(|e| format!("TrySkipNextAsync result: {:?}", e))?
            }
            "previous" | "prev" | "skip_previous" => {
                if !controls.IsPreviousEnabled().unwrap_or(false) {
                    return Err("Faixa anterior não está disponível.".into());
                }
                session
                    .TrySkipPreviousAsync()
                    .map_err(|e| format!("TrySkipPreviousAsync: {:?}", e))?
                    .get()
                    .map_err(|e| format!("TrySkipPreviousAsync result: {:?}", e))?
            }
            "stop" => {
                if !controls.IsStopEnabled().unwrap_or(false) {
                    return Err("Parar não está disponível para esta sessão.".into());
                }
                session
                    .TryStopAsync()
                    .map_err(|e| format!("TryStopAsync: {:?}", e))?
                    .get()
                    .map_err(|e| format!("TryStopAsync result: {:?}", e))?
            }
            _ => {
                return Err(format!(
                    "Ação inválida: {:?}. Use: play, pause, toggle, next, previous, stop, status.",
                    action
                ));
            }
        };

        if ok {
            Ok(format!("Comando de mídia {:?} enviado com sucesso.", action_norm))
        } else {
            Err(format!(
                "O aplicativo de mídia não aceitou o comando {:?}.",
                action_norm
            ))
        }
    }
}

#[cfg(target_os = "windows")]
pub fn control_playback(action: &str) -> Result<String, String> {
    win::control_playback(action)
}

#[cfg(target_os = "windows")]
pub fn adjust_volume(action: &str, steps: u32) -> Result<String, String> {
    const VK_VOLUME_MUTE: u16 = 0xAD;
    const VK_VOLUME_DOWN: u16 = 0xAE;
    const VK_VOLUME_UP: u16 = 0xAF;

    let a = action.trim().to_ascii_lowercase();
    match a.as_str() {
        "mute_toggle" | "mute" => {
            win::send_volume_key(VK_VOLUME_MUTE, 1);
            Ok("Tecla de silenciar alternada (mute).".into())
        }
        "up" | "volume_up" => {
            let n = steps.max(1).min(50);
            win::send_volume_key(VK_VOLUME_UP, n);
            Ok(format!(
                "Volume aumentado (~{} passos da tecla multimedia).",
                n
            ))
        }
        "down" | "volume_down" => {
            let n = steps.max(1).min(50);
            win::send_volume_key(VK_VOLUME_DOWN, n);
            Ok(format!(
                "Volume diminuído (~{} passos da tecla multimedia).",
                n
            ))
        }
        _ => Err(format!(
            "Ação de volume inválida: {:?}. Use: up, down, mute_toggle.",
            action
        )),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn control_playback(_action: &str) -> Result<String, String> {
    Err("Controle de mídia só é suportado no Windows.".into())
}

#[cfg(not(target_os = "windows"))]
pub fn adjust_volume(_action: &str, _steps: u32) -> Result<String, String> {
    Err("Ajuste de volume só é suportado no Windows.".into())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 2 — Audio device management
// ─────────────────────────────────────────────────────────────────────────────

fn run_ps_media(script: &str) -> Result<String, String> {
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
        .output()
        .map_err(|e| format!("PowerShell: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Lista dispositivos de áudio disponíveis via WMI.
pub fn list_audio_devices() -> Result<String, String> {
    let script = r#"
Get-WmiObject Win32_SoundDevice |
    Where-Object { $_.Status -eq 'OK' } |
    Select-Object -ExpandProperty Name
"#;
    let out = run_ps_media(script)?;
    if out.trim().is_empty() {
        Ok("Nenhum dispositivo de áudio encontrado.".into())
    } else {
        Ok(format!("Dispositivos de áudio disponíveis:\n{}", out.trim()))
    }
}

/// Troca o dispositivo de áudio padrão pelo nome (requer módulo AudioDeviceCmdlets).
pub fn switch_audio_device(device_name: &str) -> Result<String, String> {
    if device_name.trim().is_empty() {
        return Err("Nome do dispositivo não informado.".into());
    }
    let escaped = device_name.replace('\'', "''");
    let script = format!(
        r#"
if (-not (Get-Module -ListAvailable -Name AudioDeviceCmdlets)) {{
    "INSTALAR_MODULO: Execute no PowerShell: Install-Module AudioDeviceCmdlets -Scope CurrentUser -Force"
}} else {{
    Import-Module AudioDeviceCmdlets
    $dev = Get-AudioDevice -List | Where-Object {{ $_.Name -like '*{}*' -and $_.Type -eq 'Playback' }} | Select-Object -First 1
    if ($null -eq $dev) {{
        "Dispositivo nao encontrado: {}"
    }} else {{
        Set-AudioDevice -Index $dev.Index | Out-Null
        "Dispositivo ativo: $($dev.Name)"
    }}
}}
"#,
        escaped, escaped
    );
    run_ps_media(&script)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier 3 — set_audio_volume_app (IAudioSessionManager2)
// ─────────────────────────────────────────────────────────────────────────────

/// Ajusta o volume de um aplicativo específico pelo nome.
pub fn set_audio_volume_app(app_name: &str, volume: u32) -> Result<String, String> {
    if app_name.trim().is_empty() {
        return Err("Nome do aplicativo não informado.".into());
    }
    let vol = volume.min(100);
    let escaped = app_name.replace('\'', "''");
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
try {{
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
public class _MMDeviceEnumerator {{ }}
public enum EDataFlow {{ eRender = 0, eCapture = 1, eAll = 2 }}
public enum ERole {{ eConsole = 0, eMultimedia = 1, eCommunications = 2 }}
[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDeviceEnumerator {{
    int EnumAudioEndpoints(EDataFlow dataFlow, uint stateMask, out IntPtr devices);
    int GetDefaultAudioEndpoint(EDataFlow dataFlow, ERole role, out IntPtr endpoint);
    int GetDevice(string id, out IntPtr device);
    int RegisterEndpointNotificationCallback(IntPtr client);
    int UnregisterEndpointNotificationCallback(IntPtr client);
}}
[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDevice {{
    int Activate(ref Guid id, int clsCtx, IntPtr activationParams, out IntPtr iface);
    int OpenPropertyStore(int stgm, out IntPtr propStore);
    int GetId(out IntPtr idStr);
    int GetState(out uint state);
}}
[Guid("77AA99A0-1BD6-484F-8BC7-2C654C9A9B6F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IAudioSessionManager2 {{
    int GetAudioSessionControl(IntPtr sessionGuid, uint streamFlags, out IntPtr sessionControl);
    int GetSimpleAudioVolume(IntPtr sessionGuid, uint streamFlags, out IntPtr simpleVolume);
    int GetSessionEnumerator(out IntPtr sessionEnum);
    int RegisterSessionNotification(IntPtr sessionNotification);
    int UnregisterSessionNotification(IntPtr sessionNotification);
    int RegisterDuckNotification(string sessionId, IntPtr duckNotification);
    int UnregisterDuckNotification(IntPtr duckNotification);
}}
[Guid("E2F5BB11-0570-40CA-ACDD-3AA01277DEE8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IAudioSessionEnumerator {{
    int GetCount(out int count);
    int GetSession(int index, out IntPtr sessionControl);
}}
[Guid("F4B1A599-7266-431E-A8CA-E70ACB11E8CD"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IAudioSessionControl {{
    int GetState(out uint state);
    int GetDisplayName(out IntPtr displayName);
    int SetDisplayName(string name, ref Guid eventContext);
    int GetIconPath(out IntPtr iconPath);
    int GetGroupingParam(out Guid groupingId, out int idx);
    int SetGroupingParam(ref Guid groupingId, ref Guid context);
    int RegisterAudioSessionNotification(IntPtr notification);
    int UnregisterAudioSessionNotification(IntPtr notification);
}}
[Guid("87CE5498-68D6-44E5-9215-6DA47EF883D8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface ISimpleAudioVolume {{
    int SetMasterVolume(float level, ref Guid context);
    int GetMasterVolume(out float level);
    int SetMute(bool mute, ref Guid context);
    int GetMute(out bool mute);
}}
'@ -ErrorAction SilentlyContinue

    $devEnum = New-Object _MMDeviceEnumerator
    $devEnumPtr = [System.Runtime.InteropServices.Marshal]::GetComInterfaceForObject($devEnum, [IMMDeviceEnumerator])
    $devEnum2 = [System.Runtime.InteropServices.Marshal]::GetTypedObjectForIUnknown($devEnumPtr, [IMMDeviceEnumerator])

    $endpoint = $null
    $devEnum2.GetDefaultAudioEndpoint([EDataFlow]::eRender, [ERole]::eConsole, [ref]$endpoint) | Out-Null
    $endpointPtr = [System.Runtime.InteropServices.Marshal]::GetComInterfaceForObject($endpoint, [IMMDevice])

    $iidAsm2 = [Guid]::New('77AA99A0-1BD6-484F-8BC7-2C654C9A9B6F')
    $asm2Ptr = [System.IntPtr]::Zero
    $endpoint.Activate([ref]$iidAsm2, 0, [System.IntPtr]::Zero, [ref]$asm2Ptr) | Out-Null
    $asm2 = [System.Runtime.InteropServices.Marshal]::GetTypedObjectForIUnknown($asm2Ptr, [IAudioSessionManager2])

    $sessionEnumPtr = [System.IntPtr]::Zero
    $asm2.GetSessionEnumerator([ref]$sessionEnumPtr) | Out-Null
    $sessionEnum = [System.Runtime.InteropServices.Marshal]::GetTypedObjectForIUnknown($sessionEnumPtr, [IAudioSessionEnumerator])

    $count = 0
    $sessionEnum.GetCount([ref]$count) | Out-Null
    $found = $false

    for ($i = 0; $i -lt $count; $i++) {{
        $sessionPtr = [System.IntPtr]::Zero
        $sessionEnum.GetSession($i, [ref]$sessionPtr) | Out-Null
        $session = [System.Runtime.InteropServices.Marshal]::GetTypedObjectForIUnknown($sessionPtr, [IAudioSessionControl])

        $dispNamePtr = [System.IntPtr]::Zero
        $session.GetDisplayName([ref]$dispNamePtr) | Out-Null
        $dispName = [System.Runtime.InteropServices.Marshal]::PtrToStringUni($dispNamePtr)
        if ([string]::IsNullOrWhiteSpace($dispName)) {{
            continue
        }}

        if ($dispName -like '*{escaped}*') {{
            $iidSsv = [Guid]::New('87CE5498-68D6-44E5-9215-6DA47EF883D8')
            $ssvPtr = [System.IntPtr]::Zero
            $asm2.GetSimpleAudioVolume($sessionPtr, 0, [ref]$ssvPtr) | Out-Null
            $ssv = [System.Runtime.InteropServices.Marshal]::GetTypedObjectForIUnknown($ssvPtr, [ISimpleAudioVolume])
            $context = [Guid]::NewGuid()
            $ssv.SetMasterVolume($({vol}f / 100.0), [ref]$context) | Out-Null
            $found = $true
            "Volume de '$dispName' ajustado para {vol}%."
            break
        }}
    }}

    if (-not $found) {{
        "App '{app_name}' não encontrado entre as sessões de áudio."
    }}
}} catch {{
    "Erro ao ajustar volume do app: $_"
}}
"#,
        escaped = escaped,
        app_name = app_name,
        vol = vol
    );
    run_ps_media(&script)
}
