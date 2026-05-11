# ═══════════════════════════════════════════════════════════════
# Chatterbox TTS API — Setup automatico com voz clonada PT-BR
# ═══════════════════════════════════════════════════════════════
#
# Este script:
#   1. Clona o repositorio chatterbox-tts-api
#   2. Instala dependencias (uv ou pip)
#   3. Configura .env com modelo multilingual + porta 8005
#   4. Inicia o servidor
#   5. Registra a voz Clone_voz.mp3 como "dexter-ptbr" (PT-BR)
#
# Uso: .\setup-tts.ps1
# Pre-requisitos: Python 3.10+, GPU NVIDIA (recomendado), Git

param(
    [int]$Port = 8005,
    [string]$VoiceName = "dexter-ptbr",
    [string]$VoiceFile = ".\Clone_voz.mp3",
    [switch]$SkipClone,
    [switch]$SkipInstall
)

$ErrorActionPreference = "Stop"
$TTS_DIR = Join-Path $PSScriptRoot "chatterbox-tts-api"

Write-Host @"

╔════════════════════════════════════════════════════════╗
║   Chatterbox TTS — Setup com Clonagem de Voz PT-BR   ║
╚════════════════════════════════════════════════════════╝

"@ -ForegroundColor Magenta

# ── 1. Verificar pre-requisitos ──

Write-Host "[1/5] Verificando pre-requisitos..." -ForegroundColor Cyan

if (-not (Get-Command "python" -ErrorAction SilentlyContinue)) {
    Write-Host "ERRO: Python nao encontrado. Instale Python 3.10+ e tente novamente." -ForegroundColor Red
    exit 1
}

$pyVersion = python --version 2>&1
Write-Host "  Python: $pyVersion" -ForegroundColor Gray

if (-not (Get-Command "git" -ErrorAction SilentlyContinue)) {
    Write-Host "ERRO: Git nao encontrado." -ForegroundColor Red
    exit 1
}

$hasUv = Get-Command "uv" -ErrorAction SilentlyContinue
if ($hasUv) {
    Write-Host "  uv: encontrado (metodo preferencial)" -ForegroundColor Green
} else {
    Write-Host "  uv: nao encontrado — usando pip" -ForegroundColor Yellow
    Write-Host "  Dica: instale uv para installs 25-40% mais rapidos:" -ForegroundColor Gray
    Write-Host "    powershell -ExecutionPolicy ByPass -c `"irm https://astral.sh/uv/install.ps1 | iex`"" -ForegroundColor Gray
}

$voicePath = Resolve-Path $VoiceFile -ErrorAction SilentlyContinue
if (-not $voicePath) {
    Write-Host "AVISO: Arquivo de voz '$VoiceFile' nao encontrado." -ForegroundColor Yellow
    Write-Host "  Coloque o arquivo Clone_voz.mp3 na pasta dexter/ para clonagem de voz." -ForegroundColor Gray
    $voicePath = $null
}

# ── 2. Clonar repositorio ──

if (-not $SkipClone) {
    Write-Host "`n[2/5] Clonando chatterbox-tts-api..." -ForegroundColor Cyan
    
    if (Test-Path $TTS_DIR) {
        Write-Host "  Diretorio ja existe. Atualizando..." -ForegroundColor Gray
        Push-Location $TTS_DIR
        git pull --ff-only 2>$null
        Pop-Location
    } else {
        git clone https://github.com/travisvn/chatterbox-tts-api $TTS_DIR
    }
} else {
    Write-Host "`n[2/5] Clone pulado (--SkipClone)" -ForegroundColor Gray
}

if (-not (Test-Path $TTS_DIR)) {
    Write-Host "ERRO: Diretorio $TTS_DIR nao encontrado." -ForegroundColor Red
    exit 1
}

# ── 3. Configurar .env ──

Write-Host "`n[3/5] Configurando .env..." -ForegroundColor Cyan

$envContent = @"
# Chatterbox TTS API — Configuracao para Chronos Voice Assistant
# Gerado por setup-tts.ps1

# Servidor
HOST=0.0.0.0
PORT=$Port

# Modelo multilingual (PT-BR + 21 outros idiomas)
USE_MULTILINGUAL_MODEL=true

# Dispositivo: auto detecta GPU CUDA > MPS > CPU
DEVICE=auto

# Voz padrao (sera substituida pela voz clonada apos registro)
DEFAULT_VOICE=$VoiceName

# Diretorios
MODEL_CACHE_DIR=./models
VOICE_LIBRARY_DIR=./voices

# Formato de audio padrao (WAV para menor latencia)
DEFAULT_RESPONSE_FORMAT=wav

# Desabilitar frontend (nao precisamos, o Chronos tem o proprio UI)
ENABLE_FRONTEND=false

# Logs
LOG_LEVEL=info
"@

$envFile = Join-Path $TTS_DIR ".env"
Set-Content -Path $envFile -Value $envContent -Encoding UTF8
Write-Host "  .env salvo em: $envFile" -ForegroundColor Green

# ── 4. Instalar dependencias ──

if (-not $SkipInstall) {
    Write-Host "`n[4/5] Instalando dependencias..." -ForegroundColor Cyan
    Write-Host "  (primeira execucao pode levar varios minutos)" -ForegroundColor Gray
    
    Push-Location $TTS_DIR
    
    if ($hasUv) {
        Write-Host "  [a] uv sync..." -ForegroundColor Gray
        uv sync
        
        Write-Host "  [b] Instalando PyTorch CUDA 12.4 (necessario para carregar modelo)..." -ForegroundColor Gray
        uv pip install torch==2.6.0+cu124 torchaudio==2.6.0+cu124 --index-url https://download.pytorch.org/whl/cu124
        
        Write-Host "  [c] Instalando setuptools (necessario para resemble-perth)..." -ForegroundColor Gray
        uv pip install "setuptools<82"
    } else {
        Write-Host "  Criando venv e instalando com pip..." -ForegroundColor Gray
        python -m venv .venv
        & ".venv\Scripts\pip.exe" install -r requirements.txt
        & ".venv\Scripts\pip.exe" install torch==2.6.0+cu124 torchaudio==2.6.0+cu124 --index-url https://download.pytorch.org/whl/cu124
        & ".venv\Scripts\pip.exe" install "setuptools<82"
    }
    
    Pop-Location
    Write-Host "  Dependencias instaladas." -ForegroundColor Green
} else {
    Write-Host "`n[4/5] Instalacao pulada (--SkipInstall)" -ForegroundColor Gray
}

# ── 5. Copiar voz e exibir instrucoes ──

Write-Host "`n[5/5] Configurando voz clonada..." -ForegroundColor Cyan

$voicesDir = Join-Path $TTS_DIR "voices"
if (-not (Test-Path $voicesDir)) {
    New-Item -ItemType Directory -Path $voicesDir -Force | Out-Null
}

if ($voicePath) {
    $destVoice = Join-Path $voicesDir "Clone_voz.mp3"
    Copy-Item -Path $voicePath -Destination $destVoice -Force
    Write-Host "  Voz copiada para: $destVoice" -ForegroundColor Green
}

Write-Host @"

╔════════════════════════════════════════════════════════╗
║                  Setup Concluido!                     ║
╚════════════════════════════════════════════════════════╝

  Proximo passo — registrar a voz clonada:

  1. Inicie o servidor TTS:
     cd chatterbox-tts-api
     uv run main.py        (ou: python main.py)

  2. Apos o servidor iniciar, registre a voz:
     .\register-voice.ps1

  Ou use o start-all.ps1 que faz tudo automaticamente.

  Porta: $Port
  Voz:   $VoiceName (PT-BR)
  API:   http://localhost:$Port/v1/audio/speech

"@ -ForegroundColor Green
