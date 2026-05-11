# Voice Assistant - Launcher (Windows)
# Inicia todos os servidores + o assistente com um unico comando.
#
# Uso: .\start-all.ps1 [-Profile voice-fast|balanced|quality|voice-chatterbox|voice-chatterbox-cpu] [-ForceRestartServices] [-NoWhisper] [-WhisperTiny] [-NoTts] [-NoEmbedding]
#  Padrao: voice-chatterbox (menos camadas GPU no LLM -ngl 28 + Chatterbox TTS em CUDA / clone de voz)
#  voice-chatterbox-cpu: Chatterbox em CPU (sem contencao VRAM), LLM com -ngl 99 e contexto 8192
# Para encerrar: Ctrl+C (mata todos os processos automaticamente)

param(
    [ValidateSet("voice-fast", "balanced", "quality", "voice-chatterbox", "voice-chatterbox-cpu")]
    [string]$Profile = "voice-chatterbox",
    [switch]$ForceRestartServices,
    [switch]$KeepStaleFrontend,
    [switch]$NoWhisper,
    [switch]$WhisperTiny,
    [switch]$NoTts,
    [switch]$NoEmbedding
)

$ErrorActionPreference = "Stop"

# ═══════════════════════════════════════════════════
# CONFIGURACAO — ajuste os caminhos conforme seu ambiente
# ═══════════════════════════════════════════════════

# LLM (llama.cpp)
$LLAMA_SERVER = "C:\llama.cpp\llama-cpp-turboquant\build\bin\Release\llama-server.exe"
$LLM_MODEL    = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Gemma4-26B-A4B\gemma-4-26B-A4B-it-UD-Q4_K_M.gguf"
$LLM_MMPROJ   = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Gemma4-26B-A4B\mmproj-F16.gguf"
$LLM_PORT     = 8080
$LLM_NGL      = 99
$LLM_CPU_MOE  = 33

# Whisper STT
$WHISPER_EXE   = "..\build\bin\Release\whisper-server.exe"
$WHISPER_MODEL = "J:\Modelos LLM\manifests\registry.ollama.ai\library\whisper\ggml-small.bin"
$WHISPER_MODEL_TINY = "J:\Modelos LLM\manifests\registry.ollama.ai\library\whisper\ggml-tiny.bin"
$WHISPER_PORT  = 8081
$WHISPER_THREADS = 4

# Embedding (BGE-M3)
$EMBED_MODEL = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Embadding\bge-m3-Q4_K_M.gguf"
$EMBED_PORT = 8082
$EMBED_THREADS = 4

# Chatterbox TTS (chatterbox-tts-api com modelo multilingual PT-BR)
$CHATTERBOX_PORT = 8005
$CHATTERBOX_VOICE = "dexter-ptbr"
$CHATTERBOX_DIR = Join-Path $PSScriptRoot "chatterbox-tts-api"
$CHATTERBOX_USE_UV = $true  # uv e mais rapido; mude para $false se usar pip/venv
# Probe HTTP para /voices: com 2s o check falha sob carga (llama carregando + CUDA), gerando falso "120s".
$TTS_HTTP_PROBE_TIMEOUT_SEC = 25

# Voice Assistant
$APP_DIR = "."  # diretorio do projeto (dexter/)
$VITE_PORT = 1420

# Perfis de performance. Padrao voice-chatterbox; use voice-fast para mais folga VRAM no LLM e TTS Windows nativo.
$LLM_CONTEXT = 4096
$LLM_THREADS = 8
$LLM_USE_MMPROJ = $false
$LLM_USE_MLOCK = $false
$LLM_USE_NO_MMAP = $false
$CHATTERBOX_DEVICE = "cpu"
$TTS_MODE = "windows"

switch ($Profile) {
    "balanced" {
        $LLM_CONTEXT = 8192
        $LLM_THREADS = 8
        $CHATTERBOX_DEVICE = "cpu"
        $TTS_MODE = "windows"
    }
    "quality" {
        $LLM_CONTEXT = 16384
        $LLM_THREADS = 6
        $LLM_USE_MMPROJ = $true
        $LLM_USE_MLOCK = $true
        $LLM_USE_NO_MMAP = $true
        $CHATTERBOX_DEVICE = "cuda"
        $TTS_MODE = "chatterbox"
    }
    "voice-chatterbox" {
        # Preset para TTS com clone (Chatterbox na GPU): reduz -ngl no LLM e libera VRAM.
        # P4 — contexto reduzido 16384→8192 (voice nao precisa de 16k tokens).
        $LLM_NGL = 28
        $LLM_CONTEXT = 8192
        $LLM_THREADS = 8
        $LLM_USE_MMPROJ = $true
        $LLM_USE_MLOCK = $true
        $LLM_USE_NO_MMAP = $true
        $CHATTERBOX_DEVICE = "cuda"
        $TTS_MODE = "chatterbox"
    }
    "voice-chatterbox-cpu" {
        # P2+P4 — Chatterbox em CPU elimina contencao de VRAM com LLM.
        # LLM recebe -ngl 99 (max GPU layers) + contexto 8192 (menos VRAM).
        # Chatterbox CPU ~2-3x mais lento que GPU, mas sem picos de 19.6s.
        $LLM_NGL = 99
        $LLM_CONTEXT = 8192
        $LLM_THREADS = 8
        $LLM_USE_MMPROJ = $true
        $LLM_USE_MLOCK = $true
        $LLM_USE_NO_MMAP = $true
        $CHATTERBOX_DEVICE = "cpu"
        $TTS_MODE = "chatterbox"
    }
}

# ═══════════════════════════════════════════════════
# FUNCOES
# ═══════════════════════════════════════════════════

$script:processes = @()

function Get-PortListeners {
    param([int]$Port)

    @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue |
        Sort-Object OwningProcess -Unique)
}

function Test-NetstatListening {
    param([int]$Port)

    $match = netstat -ano 2>$null | Select-String ":$Port\s+.*LISTENING"
    return $null -ne $match
}

function Get-ProcessSummary {
    param([int]$ProcessId)

    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($proc) {
        return "$($proc.ProcessName) (PID $ProcessId)"
    }
    return "PID $ProcessId"
}

function Test-PortListening {
    param([int]$Port)
    return ((Get-PortListeners -Port $Port).Count -gt 0) -or (Test-NetstatListening -Port $Port)
}

function Test-HttpReady {
    param(
        [string]$Url,
        [int]$TimeoutSec = 2
    )

    try {
        $resp = Invoke-WebRequest -Uri $Url -Method GET -TimeoutSec $TimeoutSec -UseBasicParsing -ErrorAction Stop
        return $resp.StatusCode -ge 200 -and $resp.StatusCode -lt 500
    } catch {
        return $false
    }
}

function Get-ChatterboxProcesses {
    $dirPattern = [regex]::Escape($CHATTERBOX_DIR)
    @(Get-CimInstance Win32_Process -Filter "Name = 'python.exe'" -ErrorAction SilentlyContinue |
        Where-Object {
            $_.CommandLine -and (
                $_.CommandLine -match $dirPattern -or
                ($_.CommandLine -match "main\.py" -and $_.CommandLine -match "chatterbox-tts-api")
            )
        })
}

function Stop-ChatterboxProcesses {
    $procs = Get-ChatterboxProcesses
    foreach ($proc in $procs) {
        Write-Host "[TTS] Encerrando Chatterbox antigo/em inicializacao (PID $($proc.ProcessId))..." -ForegroundColor Yellow
        Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
    }
    if ($procs.Count -gt 0) {
        Start-Sleep -Seconds 1
    }
}

function Set-DotEnvValue {
    param(
        [string]$Path,
        [string]$Key,
        [string]$Value
    )

    if (-not (Test-Path $Path)) { return }

    $lines = Get-Content -Path $Path
    $updated = $false
    $newLines = $lines | ForEach-Object {
        if ($_ -match "^\s*$([regex]::Escape($Key))=") {
            $updated = $true
            "$Key=$Value"
        } else {
            $_
        }
    }

    if (-not $updated) {
        $newLines += "$Key=$Value"
    }

    Set-Content -Path $Path -Value $newLines -Encoding UTF8
}

function Stop-PortListeners {
    param(
        [int]$Port,
        [string]$Name,
        [switch]$OnlyNode
    )

    $listeners = Get-PortListeners -Port $Port
    foreach ($listener in $listeners) {
        $proc = Get-Process -Id $listener.OwningProcess -ErrorAction SilentlyContinue
        if (-not $proc) { continue }

        if ($OnlyNode -and $proc.ProcessName -ne "node") {
            Write-Host "[$Name] Porta $Port ocupada por $($proc.ProcessName) (PID $($proc.Id)); nao vou matar automaticamente." -ForegroundColor Yellow
            continue
        }

        Write-Host "[$Name] Liberando porta ${Port}: matando $($proc.ProcessName) (PID $($proc.Id))..." -ForegroundColor Yellow
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    }

    Start-Sleep -Milliseconds 500
}

function Wait-Port {
    param(
        [string]$Name,
        [int]$Port,
        [int]$TimeoutSeconds = 30
    )

    $elapsed = 0.0
    while ($elapsed -lt $TimeoutSeconds) {
        if (Test-PortListening -Port $Port) {
            Write-Host "[$Name] Pronto na porta $Port." -ForegroundColor Green
            return $true
        }
        Start-Sleep -Milliseconds 500
        $elapsed += 0.5
    }

    Write-Host "[$Name] AVISO: nao detectado na porta $Port apos ${TimeoutSeconds}s" -ForegroundColor Yellow
    return $false
}

function Start-Server {
    param(
        [string]$Name,
        [string]$Exe,
        [string]$ServerArgs,
        [int]$Port,
        [string]$Priority = "Normal"
    )

    $listeners = Get-PortListeners -Port $Port
    if ($listeners.Count -gt 0) {
        if ($ForceRestartServices) {
            Stop-PortListeners -Port $Port -Name $Name
        } else {
            $owners = $listeners | ForEach-Object { Get-ProcessSummary -ProcessId $_.OwningProcess }
            Write-Host "[$Name] Porta $Port ja esta em uso por: $($owners -join ', ')" -ForegroundColor Yellow
            Write-Host "[$Name] Reutilizando servico existente. Use -ForceRestartServices para reiniciar." -ForegroundColor Gray
            return $null
        }
    }

    Write-Host "[$Name] Iniciando na porta $Port..." -ForegroundColor Cyan

    $proc = Start-Process -FilePath $Exe -ArgumentList $ServerArgs -NoNewWindow -PassThru
    $script:processes += $proc

    try {
        $proc.PriorityClass = $Priority
    } catch {
        Write-Host "[$Name] Aviso: nao foi possivel definir prioridade $Priority ($($_.Exception.Message))" -ForegroundColor Yellow
    }

    Write-Host "[$Name] PID $($proc.Id) - aguardando..." -ForegroundColor Gray
    Wait-Port -Name $Name -Port $Port -TimeoutSeconds 45 | Out-Null
    return $proc
}

function Stop-AllServers {
    Write-Host "`nEncerrando servidores..." -ForegroundColor Yellow
    foreach ($proc in $script:processes) {
        if ($proc -and !$proc.HasExited) {
            Write-Host "  Matando $($proc.ProcessName) (PID $($proc.Id))..." -ForegroundColor Gray
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        }
    }
    Write-Host "Todos encerrados." -ForegroundColor Green
}

# Limpeza ao sair (Ctrl+C ou erro)
$null = Register-EngineEvent -SourceIdentifier PowerShell.Exiting -Action { Stop-AllServers }

# ═══════════════════════════════════════════════════
# INICIO
# ═══════════════════════════════════════════════════

Clear-Host
Write-Host @"

╔════════════════════════════════════════════════╗
║      Voice Assistant — Inicializando...        ║
╚════════════════════════════════════════════════╝

"@ -ForegroundColor Magenta
Write-Host "[Config] Perfil: $Profile | contexto LLM: $LLM_CONTEXT | threads: $LLM_THREADS | ngl: $LLM_NGL | mmproj: $LLM_USE_MMPROJ | TTS: $TTS_MODE" -ForegroundColor Gray
$env:DEXTER_TTS_MODE = $TTS_MODE

# Verificar pre-requisitos
$errors = @()

if (-not (Get-Command "node" -ErrorAction SilentlyContinue)) { $errors += "Node.js nao encontrado" }
if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) { $errors += "Cargo/Rust nao encontrado" }

if ($errors.Count -gt 0) {
    Write-Host "ERRO: Pre-requisitos faltando:" -ForegroundColor Red
    $errors | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}

# 1. LLM
$llamaOk = $true
if (-not (Test-Path $LLAMA_SERVER)) {
    Write-Host "[LLM] llama-server.exe nao encontrado em: $LLAMA_SERVER" -ForegroundColor Red
    $llamaOk = $false
}
if (-not (Test-Path $LLM_MODEL)) {
    Write-Host "[LLM] Modelo nao encontrado em: $LLM_MODEL" -ForegroundColor Yellow
    $llamaOk = $false
}
if ($LLM_USE_MMPROJ -and -not (Test-Path $LLM_MMPROJ)) {
    Write-Host "[LLM] mmproj nao encontrado em: $LLM_MMPROJ" -ForegroundColor Yellow
    $llamaOk = $false
}

if ($llamaOk) {
    $llmParts = @(
        "--embedding",
        "-m `"$LLM_MODEL`"",
        "-ngl $LLM_NGL",
        "--n-cpu-moe $LLM_CPU_MOE",
        "-ctk turbo4",
        "-ctv turbo3",
        "-c $LLM_CONTEXT",
        "--flash-attn on",
        "-t $LLM_THREADS",
        "--host 0.0.0.0",
        "--port $LLM_PORT"
    )

    if ($LLM_USE_MMPROJ) { $llmParts += "--mmproj `"$LLM_MMPROJ`"" }
    if ($LLM_USE_NO_MMAP) { $llmParts += "--no-mmap" }
    if ($LLM_USE_MLOCK) { $llmParts += "--mlock" }

    $llmArgs = $llmParts -join " "
    Start-Server -Name "LLM" -Exe $LLAMA_SERVER -ServerArgs $llmArgs -Port $LLM_PORT -Priority "High"
} else {
    Write-Host "[LLM] Assumindo que o llama-server ja esta rodando na porta $LLM_PORT..." -ForegroundColor Gray
}

# 2. Whisper STT
if ($NoWhisper) {
    Write-Host "[Whisper] Ignorado por parametro -NoWhisper" -ForegroundColor Yellow
} elseif (Test-Path $WHISPER_EXE) {
    $whisperModelPath = if ($WhisperTiny) { $WHISPER_MODEL_TINY } else { $WHISPER_MODEL }
    $whisperModelName = if ($WhisperTiny) { "tiny" } else { "small" }
    if (Test-Path $whisperModelPath) {
        Write-Host "[Whisper] Modelo: $whisperModelName | threads: $WHISPER_THREADS" -ForegroundColor Gray
        Start-Server -Name "Whisper" -Exe $WHISPER_EXE -ServerArgs "--model `"$whisperModelPath`" --host 127.0.0.1 --port $WHISPER_PORT --request-path `"/v1/audio`" --inference-path `"/transcriptions`" -t $WHISPER_THREADS" -Port $WHISPER_PORT -Priority "BelowNormal"
    } else {
        Write-Host "[Whisper] Modelo nao encontrado em: $whisperModelPath" -ForegroundColor Red
        Write-Host "[Whisper] Baixe de: https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-${whisperModelName}.bin" -ForegroundColor Gray
    }
} else {
    Write-Host "[Whisper] whisper-server.exe nao encontrado em: $WHISPER_EXE" -ForegroundColor Yellow
    Write-Host "[Whisper] Compile com: git clone https://github.com/ggerganov/whisper.cpp && cd whisper.cpp && cmake -B build && cmake --build build --config Release" -ForegroundColor Gray
}

# 3. Embedding (BGE-M3)
if ($NoEmbedding) {
    Write-Host "[Embedding] Ignorado por parametro -NoEmbedding" -ForegroundColor Yellow
} elseif (Test-Path $EMBED_MODEL) {
    $embedArgs = @(
        "-m `"$EMBED_MODEL`"",
        "--embeddings",
        "--port $EMBED_PORT",
        "--host 127.0.0.1",
        "-ngl 0",
        "-c 512",
        "-t $EMBED_THREADS"
    ) -join " "
    Start-Server -Name "Embedding" -Exe $LLAMA_SERVER -ServerArgs $embedArgs -Port $EMBED_PORT -Priority "BelowNormal"
} else {
    Write-Host "[Embedding] Modelo BGE-M3 nao encontrado em: $EMBED_MODEL" -ForegroundColor Yellow
    Write-Host "[Embedding] Baixe com .\download-bge-m3.ps1" -ForegroundColor Gray
    Write-Host "[Embedding] O RAG usara o LLM principal como fallback." -ForegroundColor Gray
}

# 4. Chatterbox TTS (chatterbox-tts-api multilingual)
if ($NoTts) {
    Write-Host "[TTS] Ignorado por parametro -NoTts" -ForegroundColor Yellow
} elseif ($TTS_MODE -eq "windows") {
    Write-Host "[TTS] Usando Windows TTS nativo (perfil rapido). Chatterbox nao sera iniciado." -ForegroundColor Green
    if ($ForceRestartServices) {
        Stop-PortListeners -Port $CHATTERBOX_PORT -Name "TTS"
        Stop-ChatterboxProcesses
    }
} elseif (Test-Path $CHATTERBOX_DIR) {
    $chatterboxEnvPath = Join-Path $CHATTERBOX_DIR ".env"
    Set-DotEnvValue -Path $chatterboxEnvPath -Key "DEVICE" -Value $CHATTERBOX_DEVICE
    $env:DEVICE = $CHATTERBOX_DEVICE
    Write-Host "[TTS] Device configurado: $CHATTERBOX_DEVICE" -ForegroundColor Gray

    if ($ForceRestartServices) {
        Stop-PortListeners -Port $CHATTERBOX_PORT -Name "TTS"
        Stop-ChatterboxProcesses

        $cleanupElapsed = 0
        while ($cleanupElapsed -lt 10 -and ((Get-PortListeners -Port $CHATTERBOX_PORT).Count -gt 0 -or (Get-ChatterboxProcesses).Count -gt 0)) {
            Start-Sleep -Milliseconds 500
            $cleanupElapsed += 0.5
        }
    }

    $ttsReadyUrl = "http://localhost:$CHATTERBOX_PORT/voices"
    $existingTtsProcesses = Get-ChatterboxProcesses

    if (Test-HttpReady -Url $ttsReadyUrl -TimeoutSec $TTS_HTTP_PROBE_TIMEOUT_SEC) {
        Write-Host "[TTS] Chatterbox ja esta rodando na porta $CHATTERBOX_PORT" -ForegroundColor Green
    } elseif ($existingTtsProcesses.Count -gt 0 -or (Get-PortListeners -Port $CHATTERBOX_PORT).Count -gt 0) {
        $owners = $existingTtsProcesses | ForEach-Object { "PID $($_.ProcessId)" }
        if ($owners.Count -gt 0) {
            Write-Host "[TTS] Chatterbox ja esta iniciando ($($owners -join ', ')); aguardando ficar pronto..." -ForegroundColor Yellow
        } else {
            Write-Host "[TTS] Porta $CHATTERBOX_PORT ocupada, aguardando endpoint /voices responder..." -ForegroundColor Yellow
        }

        $ttsTimeout = 60
        $ttsElapsed = 0
        while ($ttsElapsed -lt $ttsTimeout -and -not (Test-HttpReady -Url $ttsReadyUrl -TimeoutSec $TTS_HTTP_PROBE_TIMEOUT_SEC)) {
            Start-Sleep -Seconds 2
            $ttsElapsed += 2
            if ($ttsElapsed % 10 -eq 0) {
                Write-Host "[TTS] Aguardando processo existente... ($ttsElapsed/${ttsTimeout}s)" -ForegroundColor Gray
            }
        }

        if (Test-HttpReady -Url $ttsReadyUrl -TimeoutSec $TTS_HTTP_PROBE_TIMEOUT_SEC) {
            Write-Host "[TTS] Chatterbox pronto na porta $CHATTERBOX_PORT" -ForegroundColor Green
        } else {
            Write-Host "[TTS] AVISO: processo/porta existente nao respondeu em ${ttsTimeout}s." -ForegroundColor Yellow
            if ($ForceRestartServices) {
                Write-Host "[TTS] Forcando nova tentativa de inicializacao..." -ForegroundColor Yellow
                Stop-PortListeners -Port $CHATTERBOX_PORT -Name "TTS"
                Stop-ChatterboxProcesses
                $existingTtsProcesses = @()
            } else {
                Write-Host "[TTS] Use -ForceRestartServices para limpar a porta e reiniciar." -ForegroundColor Gray
            }
        }
    }

    if (-not (Test-HttpReady -Url $ttsReadyUrl -TimeoutSec $TTS_HTTP_PROBE_TIMEOUT_SEC) -and $existingTtsProcesses.Count -eq 0) {
        Write-Host "[TTS] Iniciando Chatterbox TTS (multilingual PT-BR)..." -ForegroundColor Cyan
        
        $env:PYTHONIOENCODING = "utf-8"
        $venvPython = Join-Path $CHATTERBOX_DIR ".venv\Scripts\python.exe"
        if (Test-Path $venvPython) {
            $ttsProc = Start-Process -FilePath $venvPython `
                -ArgumentList "main.py" `
                -WorkingDirectory $CHATTERBOX_DIR `
                -NoNewWindow -PassThru
            try { $ttsProc.PriorityClass = "High" } catch { }
        } else {
            Write-Host "[TTS] venv nao encontrado. Execute .\setup-tts.ps1 primeiro." -ForegroundColor Red
            $ttsProc = $null
        }
        
        if ($ttsProc) {
            $script:processes += $ttsProc
            Write-Host "[TTS] PID $($ttsProc.Id) - carregando modelo (primeira vez demora mais)..." -ForegroundColor Gray

            # Aguardar TTS ficar pronto (modelo multilingual pode demorar)
            $ttsTimeout = 120
            $ttsElapsed = 0
            while ($ttsElapsed -lt $ttsTimeout) {
                if (Test-HttpReady -Url $ttsReadyUrl -TimeoutSec $TTS_HTTP_PROBE_TIMEOUT_SEC) {
                    Write-Host "[TTS] Chatterbox pronto na porta $CHATTERBOX_PORT" -ForegroundColor Green
                    break
                }
                Start-Sleep -Seconds 2
                $ttsElapsed += 2
                if ($ttsElapsed % 10 -eq 0) {
                    Write-Host "[TTS] Aguardando... ($ttsElapsed/${ttsTimeout}s)" -ForegroundColor Gray
                }
            }

            if ($ttsElapsed -ge $ttsTimeout) {
                Write-Host "[TTS] AVISO: Chatterbox nao respondeu apos ${ttsTimeout}s - pode ainda estar carregando o modelo" -ForegroundColor Yellow
            }

            # Registrar voz clonada se ainda nao existe
            $voiceFile = Join-Path $PSScriptRoot "Clone_voz.mp3"
            if (Test-Path $voiceFile) {
                try {
                    $voicesResp = Invoke-RestMethod -Uri "http://localhost:$CHATTERBOX_PORT/voices" -Method GET -TimeoutSec 5
                    $hasVoice = $voicesResp.voices | Where-Object { $_.name -eq $CHATTERBOX_VOICE }
                    if (-not $hasVoice) {
                        Write-Host "[TTS] Registrando voz clonada '$CHATTERBOX_VOICE'..." -ForegroundColor Cyan
                        & (Join-Path $PSScriptRoot "register-voice.ps1") -Port $CHATTERBOX_PORT -VoiceName $CHATTERBOX_VOICE -VoiceFile $voiceFile -MaxRetries 5
                    } else {
                        Write-Host "[TTS] Voz '$CHATTERBOX_VOICE' ja registrada" -ForegroundColor Green
                    }
                } catch {
                    Write-Host "[TTS] Aviso: nao foi possivel verificar/registrar voz ($($_.Exception.Message))" -ForegroundColor Yellow
                    Write-Host "[TTS] Execute .\register-voice.ps1 manualmente apos o servidor iniciar" -ForegroundColor Gray
                }
            } else {
                Write-Host "[TTS] Clone_voz.mp3 nao encontrado - usando voz padrao" -ForegroundColor Yellow
            }
        }
    }
} else {
    Write-Host "[TTS] chatterbox-tts-api nao encontrado em: $CHATTERBOX_DIR" -ForegroundColor Yellow
    Write-Host "[TTS] Execute .\setup-tts.ps1 para instalar" -ForegroundColor Gray
    Write-Host "[TTS] Assumindo que TTS esta rodando externamente na porta $CHATTERBOX_PORT" -ForegroundColor Gray
}

# 5. Voice Assistant
if (-not $KeepStaleFrontend -and (Test-PortListening -Port $VITE_PORT)) {
    Stop-PortListeners -Port $VITE_PORT -Name "App/Vite" -OnlyNode
}

if (Test-PortListening -Port $VITE_PORT) {
    $owners = Get-PortListeners -Port $VITE_PORT | ForEach-Object { Get-ProcessSummary -ProcessId $_.OwningProcess }
    Write-Host "[App] ERRO: porta $VITE_PORT ainda ocupada por: $($owners -join ', ')" -ForegroundColor Red
    Write-Host "[App] Encerre o processo acima ou rode sem -KeepStaleFrontend para liberar node.exe antigo." -ForegroundColor Gray
    Stop-AllServers
    exit 1
}

Write-Host "[App] Iniciando Voice Assistant..." -ForegroundColor Cyan
Write-Host "[App] Segure Shift+Z para falar, Shift+X para fechar." -ForegroundColor Gray
Write-Host "[App] Pressione Ctrl+C para encerrar tudo.`n" -ForegroundColor Gray

# Tauri `AppHandle::restart()` termina o binario com RESTART_EXIT_CODE (= [int32]::MaxValue).
# Sem este ciclo, o `finally` abaixo corria na 1ª saida e `Stop-AllServers` matava LLM/Whisper/Vite.
$TauriRestartExitCode = [int32]::MaxValue

Push-Location $APP_DIR
try {
    do {
        npx tauri dev
        $tauriDevExit = $LASTEXITCODE
        if ($tauriDevExit -eq $TauriRestartExitCode) {
            Write-Host "`n[App] Reinicio do Chronos (atalhos/config) — a reabrir o assistente sem encerrar os servidores (LLM, Whisper, TTS...)." -ForegroundColor Cyan
            Start-Sleep -Seconds 1
        }
    } while ($tauriDevExit -eq $TauriRestartExitCode)
} finally {
    Pop-Location
    Stop-AllServers
}
