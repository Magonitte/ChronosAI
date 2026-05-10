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
