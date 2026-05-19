# Voice Assistant - Launcher SANDBOX (experimentos / regressao rapida)
# Inicializador oficial (producao): .\start-all.ps1 — perfil padrao voice-xtts-cuda-partial + LLM on-demand.
# Use este script para validar mudancas antes de promover para start-all.ps1.
#
# LLM voz: Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf
# Uso recomendado:
#   Baseline CPU:  .\teste.ps1 -Profile voice-xtts-cpu
#   XTTS CUDA:     .\teste.ps1 -Profile voice-xtts-cuda
#   CUDA parcial:  .\teste.ps1 -Profile voice-xtts-cuda-partial   (alias: voice-xtts) — recomendado RTX 3070
# Apos validar, promover alteracoes para start-all.ps1 apenas com revisao explicita.
#
# Inicia todos os servidores + o assistente com um unico comando.
#
# Uso: .\teste.ps1 [-Profile ...] ...  |  Sem -Profile: menu interativo (terminal com stdin).
#  Perfis XTTS (Fase 0):
#    voice-xtts-cpu          — XTTS CPU; Llama -ngl 99 (GPU cheia no LLM; baseline DADOS-PERF)
#    voice-xtts-cuda         — XTTS CUDA; Llama -ngl 20 (prioriza VRAM para o TTS na RTX 8 GB)
#    voice-xtts-cuda-partial — XTTS CUDA; Llama -ngl 28 (partilha VRAM LLM+TTS; plano Fase 6)
#    voice-xtts              — igual a voice-xtts-cuda-partial (compatibilidade)
#    voice-xtts-safe         — XTTS CPU + contexto 4096; se o PC travar
#  Se o Windows travar ao subir CUDA: -Profile voice-xtts-safe -EasyOnRam
# Para encerrar: Ctrl+C (mata todos os processos automaticamente)

param(
    [ValidateSet(
        "voice-fast", "balanced", "quality",
        "voice-chatterbox", "voice-chatterbox-cpu", "voice-chatterbox-safe",
        "voice-xtts", "voice-xtts-cpu", "voice-xtts-cuda", "voice-xtts-cuda-partial", "voice-xtts-safe"
    )]
    [string]$Profile = "voice-xtts-cuda-partial",
    [switch]$WithTextLlm,
    [switch]$ForceRestartServices,
    [switch]$KeepStaleFrontend,
    [switch]$NoWhisper,
    [switch]$WhisperTiny,
    [switch]$NoTts,
    [switch]$NoEmbedding,
    [switch]$EasyOnRam,
    [int]$StartupStaggerSec = 0
)

$ErrorActionPreference = "Stop"

# Catálogo único: ajuda na tela + menu interativo (quando -Profile nao e passado).
$script:StartAllProfileCatalog = @(
  # --- Plano voz: Llama 8B + XTTS (sandbox teste.ps1) ---
    @{ Id = "voice-xtts-cpu";          Desc = "[sandbox] XTTS CPU; Llama -ngl 99 (33/33 GPU). Baseline DADOS." },
    @{ Id = "voice-xtts-cuda";         Desc = "[sandbox] XTTS CUDA; Llama -ngl 20 (~20/33 GPU) — mais VRAM para TTS." },
    @{ Id = "voice-xtts-cuda-partial"; Desc = "[sandbox] XTTS CUDA; Llama -ngl 28 (~28/33 GPU) — recomendado RTX 3070." },
    @{ Id = "voice-xtts";              Desc = "[sandbox] Alias de voice-xtts-cuda-partial (-ngl 28 + XTTS CUDA)." },
    @{ Id = "voice-xtts-safe";         Desc = "[sandbox] XTTS CPU, contexto 4096, sem mlock; se travar com CUDA." },
  # --- Outros (mesmo launcher, modelo Llama 8B) ---
    @{ Id = "voice-fast";              Desc = "TTS Windows nativo; Llama -ngl 99; sem servidor Python TTS." },
    @{ Id = "balanced";                Desc = "Contexto 8192; TTS Windows." },
    @{ Id = "quality";                 Desc = "Contexto 16k; Chatterbox GPU (nao XTTS)." },
    @{ Id = "voice-chatterbox";        Desc = "Chatterbox CUDA; Llama -ngl 28." },
    @{ Id = "voice-chatterbox-cpu";    Desc = "Chatterbox CPU; Llama -ngl 99." },
    @{ Id = "voice-chatterbox-safe";   Desc = "Chatterbox CPU safe; contexto 4096." }
)

if (-not $PSBoundParameters.ContainsKey('Profile')) {
    $canMenu = $false
    if ([Environment]::UserInteractive) {
        try {
            $canMenu = -not [Console]::IsInputRedirected
        } catch {
            $canMenu = $false
        }
    }
    if ($canMenu) {
        Clear-Host
        Write-Host ""
        Write-Host "  Escolha o perfil (voce nao passou -Profile). Digite o numero e Enter." -ForegroundColor Cyan
        Write-Host ""
        for ($i = 0; $i -lt $script:StartAllProfileCatalog.Count; $i++) {
            $row = $script:StartAllProfileCatalog[$i]
            Write-Host ("  [{0,2}]  {1,-24} {2}" -f ($i + 1), $row.Id, $row.Desc) -ForegroundColor Gray
        }
        Write-Host ""
        $defaultIdx = [array]::IndexOf(
            ($script:StartAllProfileCatalog | ForEach-Object { $_.Id }),
            "voice-xtts-cuda-partial"
        )
        if ($defaultIdx -lt 0) { $defaultIdx = 0 }
        $defaultId = $script:StartAllProfileCatalog[$defaultIdx].Id
        $max = $script:StartAllProfileCatalog.Count
        $pickedOk = $false
        while (-not $pickedOk) {
            $raw = Read-Host "  Numero de 1 a $max [Enter = $($defaultIdx + 1) $defaultId]"
            if ([string]::IsNullOrWhiteSpace($raw)) {
                $Profile = $defaultId
                $pickedOk = $true
                break
            }
            $num = 0
            if ([int]::TryParse($raw.Trim(), [ref]$num)) {
                if ($num -ge 1 -and $num -le $max) {
                    $Profile = $script:StartAllProfileCatalog[$num - 1].Id
                    $pickedOk = $true
                }
            }
            if (-not $pickedOk) {
                Write-Host "  Valor invalido. Digite um numero entre 1 e $max." -ForegroundColor Yellow
            }
        }
        Write-Host ""
        Write-Host "  Perfil selecionado: $Profile" -ForegroundColor Green
        Write-Host ""
        Start-Sleep -Milliseconds 600
    }
}

# ═══════════════════════════════════════════════════
# CONFIGURACAO — ajuste os caminhos conforme seu ambiente
# ═══════════════════════════════════════════════════

# LLM (llama.cpp)
$LLAMA_SERVER = "C:\llama.cpp\llama-cpp-turboquant\build\bin\Release\llama-server.exe"
# Fase 0 — modelo voz (Llama 3.1 8B Instruct)
$LLM_MODEL    = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Llama-3.1-8B\Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf"
# Fase 4 — chat texto (Qwen 35B); sobe na 8084 com -WithTextLlm
$LLM_MODEL_TEXT    = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Qwen3.6-35B-A3B\Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q4_K_P.gguf"
$LLM_PORT_TEXT     = 8084
$LLM_TEXT_NGL      = 99    # NGL max; Llama e morto antes — ~5-6 GB VRAM livres
$LLM_TEXT_CTX_SIZE = 16384 # dobro do contexto de voz; seguro com turbo KV
$LLM_TEXT_TCOUNT   = 6     # seguro com --flash-attn (issue #139: threads<default+flash-attn bug)
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

# Vision (Qwen2.5-VL 3B)
$VISION_MODEL = "J:\Modelos LLM\manifests\registry.ollama.ai\library\qwen2.5vl\Qwen2.5-VL-3B-Instruct-q4_k_m.gguf"
$VISION_MMPROJ = "J:\Modelos LLM\manifests\registry.ollama.ai\library\qwen2.5vl\Qwen2.5-VL-3B-Instruct-mmproj-f16.gguf"
$VISION_PORT = 8083
$VISION_NGL = 99
$VISION_CONTEXT = 4096

# Chatterbox TTS (chatterbox-tts-api com modelo multilingual PT-BR)
$CHATTERBOX_PORT = 8005
$CHATTERBOX_VOICE = "dexter-ptbr"
$CHATTERBOX_DIR = Join-Path $PSScriptRoot "chatterbox-tts-api"
$CHATTERBOX_USE_UV = $true  # uv e mais rapido; mude para $false se usar pip/venv
# XTTS v2 TTS (xtts-api-server com Coqui XTTS v2 PT-BR)
$XTTS_DIR = Join-Path $PSScriptRoot "xtts-api-server"
$XTTS_DEVICE = "cuda"
$XTTS_USE_UV = $true
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
    "voice-fast" {
        # TTS Windows nativo + LLM com max GPU; sem servidor Chatterbox/XTTS.
        $LLM_NGL = 99
        $LLM_CONTEXT = 4096
        $LLM_THREADS = 8
        $LLM_USE_MMPROJ = $false
        $LLM_USE_MLOCK = $false
        $LLM_USE_NO_MMAP = $false
        $CHATTERBOX_DEVICE = "cpu"
        $TTS_MODE = "windows"
    }
    "balanced" {
        $LLM_CONTEXT = 8192
        $LLM_THREADS = 8
        $CHATTERBOX_DEVICE = "cpu"
        $TTS_MODE = "windows"
    }
    "quality" {
        $LLM_CONTEXT = 16384
        $LLM_THREADS = 6
        $LLM_USE_MMPROJ = $false   # mmproj removido — visao usa servidor dedicado on-demand (Qwen2.5-VL)
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
        $LLM_USE_MMPROJ = $false   # mmproj removido — visao usa servidor dedicado on-demand (Qwen2.5-VL)
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
        $LLM_USE_MMPROJ = $false   # mmproj removido — visao usa servidor dedicado on-demand (Qwen2.5-VL)
        $LLM_USE_MLOCK = $true
        $LLM_USE_NO_MMAP = $true
        $CHATTERBOX_DEVICE = "cpu"
        $TTS_MODE = "chatterbox"
    }
    "voice-chatterbox-safe" {
        # RTX 8 GB / PC trava: Chatterbox em CPU + LLM sem travar RAM (sem mlock) + contexto menor.
        $LLM_NGL = 99
        $LLM_CONTEXT = 4096
        $LLM_THREADS = 6
        $LLM_USE_MMPROJ = $false
        $LLM_USE_MLOCK = $false
        $LLM_USE_NO_MMAP = $false
        $CHATTERBOX_DEVICE = "cpu"
        $TTS_MODE = "chatterbox"
    }
    "voice-xtts-cuda" {
        # XTTS na GPU com prioridade de VRAM: menos camadas do Llama na GPU (RTX 8 GB).
        $LLM_NGL = 20
        $LLM_CONTEXT = 8192
        $LLM_THREADS = 8
        $LLM_USE_MMPROJ = $false
        $LLM_USE_MLOCK = $true
        $LLM_USE_NO_MMAP = $true
        $XTTS_DEVICE = "cuda"
        $TTS_MODE = "xtts"
        $script:UseXtts = $true
        $env:DEXTER_TTS_SPLIT_COMMA = "0"
        $env:DEXTER_TTS_MAX_CHUNK_CHARS = "260"
    }
    "voice-xtts-cuda-partial" {
        # XTTS CUDA + Llama parcial na GPU (partilha VRAM; referencia PLANO-VOZ Fase 6).
        $LLM_NGL = 28
        $LLM_CONTEXT = 8192
        $LLM_THREADS = 8
        $LLM_USE_MMPROJ = $false
        $LLM_USE_MLOCK = $true
        $LLM_USE_NO_MMAP = $true
        $XTTS_DEVICE = "cuda"
        $TTS_MODE = "xtts"
        $script:UseXtts = $true
        $env:DEXTER_TTS_SPLIT_COMMA = "0"
        $env:DEXTER_TTS_MAX_CHUNK_CHARS = "260"
    }
    "voice-xtts" {
        # Alias de voice-xtts-cuda-partial (compatibilidade com start-all.ps1).
        $LLM_NGL = 28
        $LLM_CONTEXT = 8192
        $LLM_THREADS = 8
        $LLM_USE_MMPROJ = $false
        $LLM_USE_MLOCK = $true
        $LLM_USE_NO_MMAP = $true
        $XTTS_DEVICE = "cuda"
        $TTS_MODE = "xtts"
        $script:UseXtts = $true
        $env:DEXTER_TTS_SPLIT_COMMA = "0"
        $env:DEXTER_TTS_MAX_CHUNK_CHARS = "260"
    }
    "voice-xtts-cpu" {
        # XTTS v2 em CPU: LLM recebe GPU completa.
        # Sem contencao de VRAM. XTTS em CPU e mais lento que GPU, mas funcional.
        $LLM_NGL = 99
        $LLM_CONTEXT = 8192
        $LLM_THREADS = 8
        $LLM_USE_MMPROJ = $false
        $LLM_USE_MLOCK = $true
        $LLM_USE_NO_MMAP = $true
        $XTTS_DEVICE = "cpu"
        $TTS_MODE = "xtts"
        $script:UseXtts = $true
        $env:DEXTER_TTS_SPLIT_COMMA = "0"
        $env:DEXTER_TTS_MAX_CHUNK_CHARS = "260"
    }
    "voice-xtts-safe" {
        # RTX 8 GB / sistema trava: XTTS em CPU + LLM com GPU max + sem mlock/no-mmap + contexto 4096.
        $LLM_NGL = 99
        $LLM_CONTEXT = 4096
        $LLM_THREADS = 6
        $LLM_USE_MMPROJ = $false
        $LLM_USE_MLOCK = $false
        $LLM_USE_NO_MMAP = $false
        $XTTS_DEVICE = "cpu"
        $TTS_MODE = "xtts"
        $script:UseXtts = $true
        $env:DEXTER_TTS_SPLIT_COMMA = "0"
        $env:DEXTER_TTS_MAX_CHUNK_CHARS = "260"
    }
}

if ($EasyOnRam) {
    Write-Host "[Config] EasyOnRam: --mlock e --no-mmap desligados no LLM (menos risco de travar o Windows)." -ForegroundColor Yellow
    $LLM_USE_MLOCK = $false
    $LLM_USE_NO_MMAP = $false
}

if ($Profile -in @("voice-xtts-cuda", "voice-xtts-cuda-partial", "voice-xtts") -and -not $PSBoundParameters.ContainsKey("StartupStaggerSec")) {
    $StartupStaggerSec = 5
    Write-Host "[Config] Perfil '$Profile' (XTTS CUDA): pausa de ${StartupStaggerSec}s entre servicos (carga GPU/VRAM). -StartupStaggerSec 0 para desativar." -ForegroundColor Gray
}

if ($Profile -in @("voice-xtts-safe", "voice-chatterbox-safe") -and -not $PSBoundParameters.ContainsKey("StartupStaggerSec")) {
    $StartupStaggerSec = 10
    Write-Host "[Config] Perfil '$Profile': pausa de ${StartupStaggerSec}s entre servicos (evita pico CPU/RAM). Passe -StartupStaggerSec 0 para desativar." -ForegroundColor Gray
}

function Invoke-StartupStagger {
    param([string]$Etapa)
    if ($StartupStaggerSec -gt 0) {
        Write-Host "[Config] Pausa ${StartupStaggerSec}s apos $Etapa (aliviar disco/GPU)..." -ForegroundColor DarkGray
        Start-Sleep -Seconds $StartupStaggerSec
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

Write-Host "  Perfis disponiveis (-Profile <nome>):" -ForegroundColor Cyan
foreach ($row in $script:StartAllProfileCatalog) {
    $line = "  {0,-24} {1}" -f $row.Id, $row.Desc
    if ($row.Id -eq $Profile) {
        Write-Host $line -ForegroundColor Green
    } else {
        Write-Host $line -ForegroundColor Gray
    }
}
Write-Host ""
Write-Host "  Outras opcoes: -EasyOnRam  -StartupStaggerSec N  -ForceRestartServices  -NoWhisper  -WhisperTiny  -NoTts  -NoEmbedding" -ForegroundColor DarkGray
Write-Host "  Encerrar tudo: Ctrl+C nesta janela." -ForegroundColor DarkGray
Write-Host ""

Write-Host "[Config] SANDBOX teste.ps1 (plano voz) | LLM: $(Split-Path -Leaf $LLM_MODEL)" -ForegroundColor Cyan
$xttsDevLine = if ($script:UseXtts) { " | XTTS device: $XTTS_DEVICE | chunk_chars: $($env:DEXTER_TTS_MAX_CHUNK_CHARS) | split_comma: $($env:DEXTER_TTS_SPLIT_COMMA)" } else { "" }
Write-Host "[Config] Perfil: $Profile | contexto LLM: $LLM_CONTEXT | threads: $LLM_THREADS | ngl: $LLM_NGL | mmproj: $LLM_USE_MMPROJ | mlock: $LLM_USE_MLOCK | TTS: $TTS_MODE$xttsDevLine | stagger: ${StartupStaggerSec}s | EasyOnRam: $EasyOnRam" -ForegroundColor Gray
if ($Profile -in @("voice-xtts-cuda", "voice-xtts-cuda-partial", "voice-xtts")) {
    Write-Host "[Config] Apos subir: GET http://127.0.0.1:8005/health -> device deve ser 'cuda'. Llama log: offloaded N/33 layers (N ~= ngl $LLM_NGL)." -ForegroundColor DarkGray
}
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
        "--cache-type-k turbo3",
        "--cache-type-v turbo3",
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

Invoke-StartupStagger "LLM"

# 1b. LLM texto (Qwen 35B) — DEPRECADO: carregado on-demand pelo Rust ao abrir chat
if ($WithTextLlm) {
    Write-Warning "[-WithTextLlm DEPRECADO] O Qwen e carregado automaticamente ao abrir o chat (Shift+T). Flag ignorada."
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
    Write-Host '[Whisper] Compile com: git clone https://github.com/ggerganov/whisper.cpp; cd whisper.cpp; cmake -B build; cmake --build build --config Release' -ForegroundColor Gray
}

Invoke-StartupStagger "Whisper"

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

Invoke-StartupStagger "Embedding"

# 3.5. Vision (Qwen2.5-VL 3B) — modo on-demand
# O servidor de visao NAO e iniciado no boot para evitar contencao de VRAM (RTX 3070 8 GB).
# Ele e iniciado sob demanda pelo backend Rust na primeira screenshot e desligado apos 5 min de inatividade.
# Usa CPU-only (-ngl 0) para zero contencao de VRAM com LLM. Performance via threads.
$VISION_ON_DEMAND_NGL = 0   # CPU-only: zero VRAM, performance via VISION_CPU_THREADS
$VISION_CPU_THREADS = 8     # threads para CPU (testar 6, 8, 12 no benchmark)
$env:VISION_MODEL_PATH = $VISION_MODEL
$env:VISION_MMPROJ_PATH = $VISION_MMPROJ
$env:VISION_ON_DEMAND_NGL = $VISION_ON_DEMAND_NGL
$env:VISION_CPU_THREADS = $VISION_CPU_THREADS
$env:VISION_PORT = $VISION_PORT
$env:LLAMA_SERVER_EXE = $LLAMA_SERVER

# LLM on-demand: parametros do perfil de voz para o Rust gerir swap voz<->texto.
# Reflectem o perfil activo (ex: voice-xtts-cuda-partial -> NGL=28, CTX=8192).
# O Rust usa estas vars em ensure_voice_llm / restore_voice_llm_after_chat.
$env:LLM_VOICE_MODEL_PATH = $LLM_MODEL
$env:LLM_VOICE_PORT       = $LLM_PORT          # 8080
$env:LLM_VOICE_NGL        = $LLM_NGL           # 28 no voice-xtts-cuda-partial
$env:LLM_VOICE_CTX        = $LLM_CONTEXT       # 8192
$env:LLM_VOICE_THREADS    = $LLM_THREADS       # 8
$env:LLM_VOICE_MLOCK      = if ($LLM_USE_MLOCK)   { "1" } else { "0" }
$env:LLM_VOICE_NO_MMAP    = if ($LLM_USE_NO_MMAP) { "1" } else { "0" }
# LLM texto (Qwen 35B) — spawnar sob demanda ao abrir chat
$env:LLM_TEXT_MODEL_PATH     = $LLM_MODEL_TEXT
$env:LLM_TEXT_PORT           = $LLM_PORT_TEXT      # 8084
$env:LLM_TEXT_NGL            = $LLM_TEXT_NGL       # 99 (Llama morto antes; ~5-6 GB VRAM livres)
$env:LLM_TEXT_CTX            = $LLM_TEXT_CTX_SIZE  # 16384
$env:LLM_TEXT_THREADS        = $LLM_TEXT_TCOUNT    # 6
$env:LLM_TEXT_MLOCK          = "1"
$env:LLM_TEXT_NO_MMAP        = "1"
$env:LLM_TEXT_CTX_CHECKPOINTS = "0"               # fix issue #119 (turbo KV + --n-cpu-moe race)
$env:LLM_CPU_MOE             = $LLM_CPU_MOE        # 33

# ── XTTS (gerido pelo Rust durante swap) ──
$env:XTTS_SERVER_PATH      = "C:\llama.cpp\xtts-api-server\main.py"
$env:XTTS_PYTHON_EXE       = "python"
$env:XTTS_PORT             = "8005"
$env:XTTS_STARTUP_TIMEOUT_SECS = "180"   # swap voz→chat→voz: 1ª carga CUDA do XTTS pode levar ~1–2 min
$env:DEXTER_TTS_INFER_DEVICE = if ($script:UseXtts) { $XTTS_DEVICE } else { $CHATTERBOX_DEVICE }

if (-not (Test-Path $VISION_MODEL)) {
    Write-Host "[Vision] Modelo Qwen2.5-VL 3B nao encontrado em: $VISION_MODEL" -ForegroundColor Yellow
    Write-Host "[Vision] Baixe com .\download-vision-model.ps1" -ForegroundColor Gray
    Write-Host "[Vision] Screenshots usarao o LLM principal como fallback." -ForegroundColor Gray
} elseif (-not (Test-Path $VISION_MMPROJ)) {
    Write-Host "[Vision] mmproj nao encontrado em: $VISION_MMPROJ" -ForegroundColor Yellow
    Write-Host "[Vision] O modelo de visao precisa do mmproj para funcionar." -ForegroundColor Gray
} else {
    Write-Host "[Vision] Modo on-demand ativado (porta $VISION_PORT, -ngl $VISION_ON_DEMAND_NGL, -t $VISION_CPU_THREADS)" -ForegroundColor Green
    Write-Host "[Vision] O servidor sera iniciado automaticamente na primeira screenshot e desligado apos 5 min ocioso." -ForegroundColor Gray
    Write-Host "[Vision] CPU-only: zero VRAM consumida pelo servidor de visao. Performance via $VISION_CPU_THREADS threads." -ForegroundColor Gray
}

Invoke-StartupStagger "antes do TTS"

# 4. TTS (Chatterbox ou XTTS v2)
$script:UseXtts = $script:UseXtts -or $false  # ensure variable exists
$TTS_LABEL = if ($script:UseXtts) { "XTTS v2" } else { "Chatterbox" }
$TTS_SERVER_DIR = if ($script:UseXtts) { $XTTS_DIR } else { $CHATTERBOX_DIR }

function Get-TtsProcesses {
    param([string]$DirPattern)
    $dirRegex = [regex]::Escape($DirPattern)
    @(Get-CimInstance Win32_Process -Filter "Name = 'python.exe'" -ErrorAction SilentlyContinue |
        Where-Object {
            $_.CommandLine -and (
                $_.CommandLine -match $dirRegex -or
                ($_.CommandLine -match "main\.py" -and $_.CommandLine -match "tts-api")
            )
        })
}

function Stop-TtsProcesses {
    param([string]$Label, [string]$DirPattern)
    $procs = Get-TtsProcesses -DirPattern $DirPattern
    foreach ($proc in $procs) {
        Write-Host "[TTS] Encerrando $Label antigo/em inicializacao (PID $($proc.ProcessId))..." -ForegroundColor Yellow
        Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
    }
    if ($procs.Count -gt 0) {
        Start-Sleep -Seconds 1
    }
}

if ($NoTts) {
    Write-Host "[TTS] Ignorado por parametro -NoTts" -ForegroundColor Yellow
} elseif ($TTS_MODE -eq "windows") {
    Write-Host "[TTS] Usando Windows TTS nativo (perfil rapido). $TTS_LABEL nao sera iniciado." -ForegroundColor Green
    if ($ForceRestartServices) {
        Stop-PortListeners -Port $CHATTERBOX_PORT -Name "TTS"
        Stop-TtsProcesses -Label $TTS_LABEL -DirPattern $TTS_SERVER_DIR
    }
} elseif (Test-Path $TTS_SERVER_DIR) {
    $ttsEnvPath = Join-Path $TTS_SERVER_DIR ".env"
    $ttsDevice = if ($script:UseXtts) { $XTTS_DEVICE } else { $CHATTERBOX_DEVICE }
    Set-DotEnvValue -Path $ttsEnvPath -Key "DEVICE" -Value $ttsDevice
    $env:DEVICE = $ttsDevice
    Write-Host "[TTS] $TTS_LABEL device configurado: $ttsDevice" -ForegroundColor Gray

    if ($ForceRestartServices) {
        Stop-PortListeners -Port $CHATTERBOX_PORT -Name "TTS"
        Stop-TtsProcesses -Label $TTS_LABEL -DirPattern $TTS_SERVER_DIR

        $cleanupElapsed = 0
        while ($cleanupElapsed -lt 10 -and ((Get-PortListeners -Port $CHATTERBOX_PORT).Count -gt 0 -or (Get-TtsProcesses -DirPattern $TTS_SERVER_DIR).Count -gt 0)) {
            Start-Sleep -Milliseconds 500
            $cleanupElapsed += 0.5
        }
    }

    $ttsReadyUrl = "http://localhost:$CHATTERBOX_PORT/voices"
    $existingTtsProcesses = Get-TtsProcesses -DirPattern $TTS_SERVER_DIR

    if (Test-HttpReady -Url $ttsReadyUrl -TimeoutSec $TTS_HTTP_PROBE_TIMEOUT_SEC) {
        Write-Host "[TTS] $TTS_LABEL ja esta rodando na porta $CHATTERBOX_PORT" -ForegroundColor Green
    } elseif ($existingTtsProcesses.Count -gt 0 -or (Get-PortListeners -Port $CHATTERBOX_PORT).Count -gt 0) {
        $owners = $existingTtsProcesses | ForEach-Object { "PID $($_.ProcessId)" }
        if ($owners.Count -gt 0) {
            Write-Host "[TTS] $TTS_LABEL ja esta iniciando ($($owners -join ', ')); aguardando ficar pronto..." -ForegroundColor Yellow
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
            Write-Host "[TTS] $TTS_LABEL pronto na porta $CHATTERBOX_PORT" -ForegroundColor Green
        } else {
            Write-Host "[TTS] AVISO: processo/porta existente nao respondeu em ${ttsTimeout}s." -ForegroundColor Yellow
            if ($ForceRestartServices) {
                Write-Host "[TTS] Forcando nova tentativa de inicializacao..." -ForegroundColor Yellow
                Stop-PortListeners -Port $CHATTERBOX_PORT -Name "TTS"
                Stop-TtsProcesses -Label $TTS_LABEL -DirPattern $TTS_SERVER_DIR
                $existingTtsProcesses = @()
            } else {
                Write-Host "[TTS] Use -ForceRestartServices para limpar a porta e reiniciar." -ForegroundColor Gray
            }
        }
    }

    if (-not (Test-HttpReady -Url $ttsReadyUrl -TimeoutSec $TTS_HTTP_PROBE_TIMEOUT_SEC) -and $existingTtsProcesses.Count -eq 0) {
        Write-Host "[TTS] Iniciando $TTS_LABEL TTS (multilingual PT-BR)..." -ForegroundColor Cyan
        
        $env:PYTHONIOENCODING = "utf-8"
        $env:COQUI_TOS_AGREED = "1"
        $venvPython = Join-Path $TTS_SERVER_DIR ".venv\Scripts\python.exe"
        if (Test-Path $venvPython) {
            $ttsProc = Start-Process -FilePath $venvPython `
                -ArgumentList "main.py" `
                -WorkingDirectory $TTS_SERVER_DIR `
                -NoNewWindow -PassThru
            try { $ttsProc.PriorityClass = "High" } catch { }
        } else {
            Write-Host "[TTS] venv nao encontrado. Execute .\setup-tts.ps1 primeiro." -ForegroundColor Red
            $ttsProc = $null
        }
        
        if ($ttsProc) {
            $script:processes += $ttsProc
            Write-Host "[TTS] PID $($ttsProc.Id) - carregando modelo (primeira vez demora mais)..." -ForegroundColor Gray

            # Aguardar TTS ficar pronto (modelo pode demorar para baixar/carregar)
            $ttsTimeout = 180
            $ttsElapsed = 0
            while ($ttsElapsed -lt $ttsTimeout) {
                if (Test-HttpReady -Url $ttsReadyUrl -TimeoutSec $TTS_HTTP_PROBE_TIMEOUT_SEC) {
                    Write-Host "[TTS] $TTS_LABEL pronto na porta $CHATTERBOX_PORT" -ForegroundColor Green
                    break
                }
                Start-Sleep -Seconds 2
                $ttsElapsed += 2
                if ($ttsElapsed % 10 -eq 0) {
                    Write-Host "[TTS] Aguardando... ($ttsElapsed/${ttsTimeout}s)" -ForegroundColor Gray
                }
            }

            if ($ttsElapsed -ge $ttsTimeout) {
                Write-Host "[TTS] AVISO: $TTS_LABEL nao respondeu apos ${ttsTimeout}s - pode ainda estar carregando o modelo" -ForegroundColor Yellow
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
    Write-Host "[TTS] $TTS_LABEL nao encontrado em: $TTS_SERVER_DIR" -ForegroundColor Yellow
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

# App Rust: permite 2 sinteses TTS em paralelo quando o servidor usa CPU (overlap com playback).
# Override no processo: $env:DEXTER_TTS_PARALLEL = "1" | "2" | ...
if ($TTS_MODE -eq "windows") {
    $env:DEXTER_TTS_INFER_DEVICE = "windows"
} elseif (-not $NoTts -and (Test-Path $TTS_SERVER_DIR)) {
    $env:DEXTER_TTS_INFER_DEVICE = if ($script:UseXtts) { $XTTS_DEVICE } else { $CHATTERBOX_DEVICE }
} else {
    Remove-Item Env:DEXTER_TTS_INFER_DEVICE -ErrorAction SilentlyContinue
}

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
