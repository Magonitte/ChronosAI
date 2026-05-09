# ═══════════════════════════════════════════════════════════════
# Registrar voz clonada no Chatterbox TTS API
# ═══════════════════════════════════════════════════════════════
#
# Envia o arquivo Clone_voz.mp3 para o servidor Chatterbox e
# registra como voz "dexter-ptbr" com idioma Portugues (pt).
#
# Pre-requisito: servidor Chatterbox rodando na porta configurada.
# Uso: .\register-voice.ps1

param(
    [int]$Port = 8005,
    [string]$VoiceName = "dexter-ptbr",
    [string]$VoiceFile = ".\Clone_voz.mp3",
    [string]$Language = "pt",
    [int]$MaxRetries = 30,
    [int]$RetryIntervalSec = 2
)

$ErrorActionPreference = "Stop"
$BaseUrl = "http://localhost:$Port"

Write-Host @"

╔════════════════════════════════════════════════╗
║   Registrando Voz Clonada — PT-BR            ║
╚════════════════════════════════════════════════╝

"@ -ForegroundColor Magenta

# ── Verificar arquivo de voz ──

$voicePath = Resolve-Path $VoiceFile -ErrorAction SilentlyContinue
if (-not $voicePath) {
    Write-Host "ERRO: Arquivo de voz '$VoiceFile' nao encontrado." -ForegroundColor Red
    Write-Host "  Coloque Clone_voz.mp3 na pasta dexter/" -ForegroundColor Gray
    exit 1
}
Write-Host "  Arquivo: $voicePath" -ForegroundColor Gray

# ── Aguardar servidor ficar pronto ──

Write-Host "  Aguardando servidor TTS em $BaseUrl..." -ForegroundColor Cyan

$ready = $false
for ($i = 0; $i -lt $MaxRetries; $i++) {
    try {
        $resp = Invoke-WebRequest -Uri "$BaseUrl/voices" -Method GET -TimeoutSec 3 -ErrorAction SilentlyContinue
        if ($resp.StatusCode -eq 200) {
            $ready = $true
            break
        }
    } catch {
        # servidor ainda nao esta pronto
    }
    Write-Host "    Tentativa $($i+1)/$MaxRetries — aguardando ${RetryIntervalSec}s..." -ForegroundColor Gray
    Start-Sleep -Seconds $RetryIntervalSec
}

if (-not $ready) {
    Write-Host "ERRO: Servidor TTS nao respondeu apos $($MaxRetries * $RetryIntervalSec)s." -ForegroundColor Red
    Write-Host "  Verifique se o Chatterbox esta rodando na porta $Port" -ForegroundColor Gray
    exit 1
}

Write-Host "  Servidor TTS pronto." -ForegroundColor Green

# ── Verificar se a voz ja existe ──

try {
    $voicesList = Invoke-RestMethod -Uri "$BaseUrl/voices" -Method GET
    $existing = $voicesList | Where-Object { $_.name -eq $VoiceName -or $_.voice_name -eq $VoiceName }
    if ($existing) {
        Write-Host "  Voz '$VoiceName' ja registrada. Atualizando..." -ForegroundColor Yellow
    }
} catch {
    Write-Host "  Aviso: nao foi possivel listar vozes existentes." -ForegroundColor Gray
}

# ── Upload da voz ──

Write-Host "  Enviando '$VoiceName' (idioma: $Language)..." -ForegroundColor Cyan

$boundary = [System.Guid]::NewGuid().ToString()
$LF = "`r`n"

$voiceBytes = [System.IO.File]::ReadAllBytes($voicePath.Path)
$voiceBase64 = $null

$bodyLines = @(
    "--$boundary",
    "Content-Disposition: form-data; name=`"voice_name`"$LF",
    $VoiceName,
    "--$boundary",
    "Content-Disposition: form-data; name=`"language`"$LF",
    $Language,
    "--$boundary",
    "Content-Disposition: form-data; name=`"voice_file`"; filename=`"Clone_voz.mp3`"",
    "Content-Type: audio/mpeg$LF"
)

$bodyStart = ($bodyLines -join $LF) + $LF
$bodyEnd = "$LF--$boundary--$LF"

$startBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyStart)
$endBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyEnd)

$bodyStream = New-Object System.IO.MemoryStream
$bodyStream.Write($startBytes, 0, $startBytes.Length)
$bodyStream.Write($voiceBytes, 0, $voiceBytes.Length)
$bodyStream.Write($endBytes, 0, $endBytes.Length)

$bodyArray = $bodyStream.ToArray()
$bodyStream.Dispose()

try {
    $response = Invoke-RestMethod `
        -Uri "$BaseUrl/voices" `
        -Method POST `
        -ContentType "multipart/form-data; boundary=$boundary" `
        -Body $bodyArray `
        -TimeoutSec 30

    Write-Host @"

  Voz registrada com sucesso!

  Nome:   $VoiceName
  Idioma: Portugues ($Language)
  Arquivo: Clone_voz.mp3

"@ -ForegroundColor Green
} catch {
    $err = $_.Exception.Message
    Write-Host "ERRO ao registrar voz: $err" -ForegroundColor Red
    
    Write-Host "`n  Tentando via curl como fallback..." -ForegroundColor Yellow
    
    $curlAvailable = Get-Command "curl.exe" -ErrorAction SilentlyContinue
    if ($curlAvailable) {
        $curlResult = & curl.exe -s -w "%{http_code}" -X POST "$BaseUrl/voices" `
            -F "voice_name=$VoiceName" `
            -F "language=$Language" `
            -F "voice_file=@$($voicePath.Path)" `
            2>&1
        
        if ($LASTEXITCODE -eq 0) {
            Write-Host "  Voz registrada via curl!" -ForegroundColor Green
        } else {
            Write-Host "  Falha via curl tambem: $curlResult" -ForegroundColor Red
            exit 1
        }
    } else {
        Write-Host "  curl nao disponivel. Registre manualmente:" -ForegroundColor Gray
        Write-Host "  curl -X POST http://localhost:$Port/voices -F `"voice_name=$VoiceName`" -F `"language=$Language`" -F `"voice_file=@Clone_voz.mp3`"" -ForegroundColor White
        exit 1
    }
}

# ── Testar a voz ──

Write-Host "  Testando sintese com a voz clonada..." -ForegroundColor Cyan

try {
    $testBody = @{
        input = "Ola! Eu sou o Dexter, seu assistente de voz."
        voice = $VoiceName
    } | ConvertTo-Json

    $audioResponse = Invoke-WebRequest `
        -Uri "$BaseUrl/v1/audio/speech" `
        -Method POST `
        -ContentType "application/json" `
        -Body $testBody `
        -TimeoutSec 60

    $testFile = Join-Path $PSScriptRoot "test-voice-output.wav"
    [System.IO.File]::WriteAllBytes($testFile, $audioResponse.Content)

    Write-Host @"

  Teste concluido! Audio salvo em: $testFile
  Reproduza para verificar a qualidade da voz clonada.

"@ -ForegroundColor Green
} catch {
    Write-Host "  Aviso: teste de sintese falhou ($($_.Exception.Message))" -ForegroundColor Yellow
    Write-Host "  A voz foi registrada, mas o primeiro uso pode demorar" -ForegroundColor Gray
    Write-Host "  (o modelo precisa ser baixado na primeira execucao)." -ForegroundColor Gray
}

Write-Host @"
╔════════════════════════════════════════════════╗
║             Pronto para usar!                 ║
╚════════════════════════════════════════════════╝

  A voz '$VoiceName' esta configurada.
  O assistente vai usar essa voz automaticamente.

  Para testar manualmente:
    curl -X POST http://localhost:$Port/v1/audio/speech `
      -H "Content-Type: application/json" `
      -d '{\"input\": \"Ola, tudo bem?\", \"voice\": \"$VoiceName\"}' `
      --output teste.wav

"@ -ForegroundColor Magenta
