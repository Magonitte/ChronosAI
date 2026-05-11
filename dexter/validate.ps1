# validate.ps1 - Testes de validacao do Chronos AI v2

param(
    [int]$LlmPort = 8080,
    [int]$EmbedPort = 8082,
    [int]$WhisperPort = 8081,
    [int]$TtsPort = 8005,
    [string]$ConfigPath = "$env:APPDATA\voice-assistant\config.json",
    [string]$EmbedModelPath = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Embadding\bge-m3-Q4_K_M.gguf"
)

$ErrorActionPreference = "Continue"
$allPassed = $true

function Test-Result {
    param([string]$Name, [bool]$Passed, [string]$Detail = "")
    $status = if ($Passed) { "PASSOU" } else { "FALHOU"; $script:allPassed = $false }
    $color = if ($Passed) { "Green" } else { "Red" }
    Write-Host "  [$status] $Name" -ForegroundColor $color
    if ($Detail) { Write-Host "    $Detail" -ForegroundColor Gray }
}

Write-Host "`n=== Testes de Validacao - Chronos AI v2 ===`n" -ForegroundColor Cyan

# 1. LLM
Write-Host "[1] Servidor LLM (porta $LlmPort)" -ForegroundColor White
try {
    $resp = Invoke-RestMethod -Uri "http://localhost:$LlmPort/v1/models" -Method GET -TimeoutSec 5
    Test-Result "LLM responde a /v1/models" $true "Modelos: $(($resp.data | ForEach-Object { $_.id }) -join ', ')"
} catch {
    Test-Result "LLM responde a /v1/models" $false "Erro: $_"
}

# 2. Embedding
Write-Host "`n[2] Servidor de Embedding (porta $EmbedPort)" -ForegroundColor White
try {
    $resp = Invoke-RestMethod -Uri "http://localhost:$EmbedPort/v1/models" -Method GET -TimeoutSec 5
    Test-Result "Embedding responde a /v1/models" $true
} catch {
    Test-Result "Embedding responde a /v1/models" $false "Erro: $_"
}
try {
    # Mesmo contrato que rag.rs (POST .../embedding com campo "content")
    $body = @{ content = "Teste de embedding" } | ConvertTo-Json
    $embedBase = "http://localhost:$EmbedPort"
    $resp = Invoke-RestMethod -Uri "$embedBase/embedding" -Method POST -Body $body -ContentType "application/json; charset=utf-8" -TimeoutSec 10
    Test-Result "Embedding retorna vetor" ($resp.embedding.Count -gt 0) "Dimensoes: $($resp.embedding.Count)"
} catch {
    Test-Result "Embedding retorna vetor" $false "Erro: $_"
}

# 3. Whisper
Write-Host "`n[3] Servidor Whisper (porta $WhisperPort)" -ForegroundColor White
$whisperOk = $false
# Tentar /health ou / (alguns servidores respondem na raiz)
foreach ($ep in @("/health", "/")) {
    try {
        $resp = Invoke-WebRequest -Uri "http://localhost:$WhisperPort$ep" -Method GET -TimeoutSec 5
        if ($resp.StatusCode -eq 200) { $whisperOk = $true; break }
    } catch {}
}
# Fallback: tentar POST /inference com arquivo de audio minimo
if (-not $whisperOk) {
    try {
        $resp = Invoke-WebRequest -Uri "http://localhost:$WhisperPort/inference" -Method POST -TimeoutSec 3
        $whisperOk = ($resp.StatusCode -ne 404)
    } catch { $whisperOk = $false }
}
Test-Result "Whisper responde" $whisperOk

# 4. TTS
Write-Host "`n[4] Servidor TTS (porta $TtsPort)" -ForegroundColor White
try {
    $resp = Invoke-RestMethod -Uri "http://localhost:$TtsPort/voices" -Method GET -TimeoutSec 5
    Test-Result "TTS responde a /voices" $true "Vozes: $(($resp.voices | ForEach-Object { $_.name }) -join ', ')"
} catch {
    Test-Result "TTS responde a /voices" $false "Erro: $_ (pode estar usando Windows nativo)"
}

# 5. Configuracao
Write-Host "`n[5] Configuracao do App" -ForegroundColor White
if (Test-Path $ConfigPath) {
    try {
        $config = Get-Content $ConfigPath -Raw | ConvertFrom-Json
        Test-Result "config.json existe e e JSON valido" $true
        $requiredFields = @("llm_url", "llm_model", "personality", "system_prompt", "embed_url", "shortcut_voice", "shortcut_chat")
        foreach ($field in $requiredFields) {
            Test-Result "Campo '$field' presente" ($null -ne $config.$field)
        }
        $validPersonalities = @("default", "coder", "creative", "custom")
        Test-Result "personality tem valor valido" ($config.personality -in $validPersonalities) "Valor: $($config.personality)"
    } catch {
        Test-Result "config.json valido" $false "Erro de parse: $_"
    }
} else {
    Test-Result "config.json encontrado" $false "Caminho: $ConfigPath"
}

# 6. Modelo BGE-M3
Write-Host "`n[6] Modelo de Embedding BGE-M3" -ForegroundColor White
if (Test-Path $EmbedModelPath) {
    $size = (Get-Item $EmbedModelPath).Length / 1GB
    Test-Result "Arquivo do modelo encontrado" $true "Tamanho: $([math]::Round($size, 2)) GB"
} else {
    Test-Result "Arquivo do modelo encontrado" $false "Caminho: $EmbedModelPath"
}

# 7. System Prompt
Write-Host "`n[7] Validacao de System Prompt" -ForegroundColor White
$voiceRs = Join-Path $PSScriptRoot "src-tauri\src\voice.rs"
if (Test-Path $voiceRs) {
    $voiceContent = Get-Content $voiceRs -Raw
    $hasVoiceFallback = $voiceContent -match 'voice_system_prompt\s*=\s*if\s+!config\.system_prompt\.trim\(\)\.is_empty\(\)'
    Test-Result "voice.rs: fallback por personalidade no modo voz" $hasVoiceFallback
    $hasTextFallback = $voiceContent -match 'fn chat_streaming_text'
    Test-Result "voice.rs: funcao chat_streaming_text existe" $hasTextFallback
} else {
    Test-Result "voice.rs encontrado" $false "Caminho: $voiceRs"
}
$libRs = Join-Path $PSScriptRoot "src-tauri\src\lib.rs"
if (Test-Path $libRs) {
    $libContent = Get-Content $libRs -Raw
    $configPerMessage = $libContent -match 'state\.config\.lock\(\)\.unwrap\(\)\.clone\(\)'
    Test-Result "lib.rs: config lido a cada mensagem (lock+clone)" $configPerMessage
} else {
    Test-Result "lib.rs encontrado" $false "Caminho: $libRs"
}

# Resumo
Write-Host "`n=== Resumo ===" -ForegroundColor Cyan
if ($allPassed) {
    Write-Host "Todos os testes passaram!" -ForegroundColor Green
    exit 0
} else {
    Write-Host "Alguns testes falharam. Verifique os itens acima." -ForegroundColor Red
    exit 1
}
