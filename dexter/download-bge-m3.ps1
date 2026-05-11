# download-bge-m3.ps1 - Baixa o modelo BGE-M3 para embedding (GGUF Q4_K_M)
# Repo publico gpustack/bge-m3-GGUF (o antigo bartowski/bge-m3-GGUF retorna 404).
# HF_TOKEN opcional: https://huggingface.co/settings/tokens - util se houver rate limit.
# Execute: .\download-bge-m3.ps1   ou   $env:HF_TOKEN="hf_..."; .\download-bge-m3.ps1

param(
    [string]$DestDir = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Embadding",
    [string]$ModelFile = "bge-m3-Q4_K_M.gguf"
)

$hfRepoId = "gpustack/bge-m3-GGUF"
$modelUrl = "https://huggingface.co/$hfRepoId/resolve/main/bge-m3-Q4_K_M.gguf"
$destFile = Join-Path $DestDir $ModelFile

Write-Host "=== Download do Modelo BGE-M3 (Embedding) ===" -ForegroundColor Cyan
Write-Host ""

# Criar pasta destino se necessario
if (-not (Test-Path $DestDir)) {
    Write-Host "Criando pasta: $DestDir" -ForegroundColor Gray
    New-Item -ItemType Directory -Path $DestDir -Force | Out-Null
}

# Verificar se ja existe
if (Test-Path $destFile) {
    $size = (Get-Item $destFile).Length
    if ($size -gt 100MB) {
        $gb = $size / 1GB
        Write-Host "Modelo ja existe: $([math]::Round($gb, 2)) GB" -ForegroundColor Green
        Write-Host "Para baixar novamente, delete o arquivo: $destFile" -ForegroundColor Gray
        exit 0
    }
    else {
        Write-Host "Arquivo existente muito pequeno ($size bytes) - sera sobrescrito" -ForegroundColor Yellow
        Remove-Item $destFile -Force
    }
}

# Obter token
$hfToken = $env:HF_TOKEN
if (-not $hfToken) {
    $hfToken = $env:HUGGINGFACE_HUB_TOKEN
}

if (-not $hfToken) {
    Write-Host "AVISO: HF_TOKEN nao definido. Tentando download sem autenticacao..." -ForegroundColor Yellow
    Write-Host "Se falhar, obtenha um token gratuito em https://huggingface.co/settings/tokens" -ForegroundColor Yellow
    Write-Host 'e execute: $env:HF_TOKEN="hf_seu_token"; .\download-bge-m3.ps1' -ForegroundColor Gray
    Write-Host ""
}

Write-Host "Baixando BGE-M3 Q4_K_M (~420 MB) de $hfRepoId..." -ForegroundColor Yellow
Write-Host "URL: $modelUrl" -ForegroundColor Gray
Write-Host "Destino: $destFile" -ForegroundColor Gray
Write-Host ""

# --- Metodo 1: Python huggingface_hub (preferido) ---
$pythonOk = Get-Command python -ErrorAction SilentlyContinue
if ($pythonOk) {
    Write-Host "Usando Python huggingface_hub..." -ForegroundColor Gray
    
    # Escrever script Python em arquivo temporario
    $pyFile = Join-Path $env:TEMP "dl_bge_m3.py"
    $pyLines = @(
        'import sys',
        'sys.stdout.reconfigure(encoding="utf-8")',
        'import os'
    )
    if ($hfToken) {
        $pyLines += "os.environ['HF_TOKEN'] = '$hfToken'"
    }
    $pyLines += @(
        'from huggingface_hub import hf_hub_download',
        "path = hf_hub_download(repo_id='$hfRepoId', filename='bge-m3-Q4_K_M.gguf', local_dir=r'$DestDir')",
        "print(f'OK:{path}')"
    )
    $pyLines -join "`n" | Set-Content -Path $pyFile -Encoding UTF8
    
    try {
        $result = python $pyFile 2>&1
        Remove-Item $pyFile -Force -ErrorAction SilentlyContinue
        if ($LASTEXITCODE -eq 0 -and (Test-Path $destFile)) {
            $size = (Get-Item $destFile).Length
            if ($size -gt 100MB) {
                Write-Host "Download concluido via Python!" -ForegroundColor Green
                exit 0
            }
            Write-Host "Python retornou OK mas arquivo ausente ou pequeno ($size bytes)." -ForegroundColor Yellow
        }
        else {
            Write-Host "Python falhou: $result" -ForegroundColor Yellow
        }
    }
    catch {
        Remove-Item $pyFile -Force -ErrorAction SilentlyContinue
        Write-Host "Python falhou: $_" -ForegroundColor Yellow
    }
}

# --- Metodo 2: curl.exe (call operator evita quebra de args com Start-Process) ---
$curlOk = Get-Command curl.exe -ErrorAction SilentlyContinue
if ($curlOk) {
    Write-Host "Usando curl.exe..." -ForegroundColor Gray
    
    $curlArgs = @(
        '-L', '-f', '--connect-timeout', '60',
        '-o', $destFile,
        '-H', 'User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
    )
    if ($hfToken) {
        $curlArgs += @('-H', "Authorization: Bearer $hfToken")
    }
    $curlArgs += $modelUrl
    
    try {
        & curl.exe @curlArgs
        if ($LASTEXITCODE -eq 0 -and (Test-Path $destFile)) {
            $size = (Get-Item $destFile).Length
            if ($size -gt 100MB) {
                Write-Host "Download concluido via curl: $([math]::Round($size/1GB, 2)) GB" -ForegroundColor Green
                exit 0
            }
        }
    }
    catch {
        Write-Host "curl falhou" -ForegroundColor Yellow
    }
}

# --- Metodo 3: Invoke-WebRequest (fallback) ---
Write-Host "Usando Invoke-WebRequest..." -ForegroundColor Gray
try {
    $iwrHeaders = @{
        'User-Agent' = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
    }
    if ($hfToken) {
        $iwrHeaders['Authorization'] = "Bearer $hfToken"
    }
    Invoke-WebRequest -Uri $modelUrl -OutFile $destFile -UseBasicParsing -Headers $iwrHeaders
    $size = (Get-Item $destFile).Length
    if ($size -gt 100MB) {
        Write-Host "Download concluido: $([math]::Round($size/1GB, 2)) GB" -ForegroundColor Green
        exit 0
    }
    Write-Host "Arquivo baixado muito pequeno: $size bytes" -ForegroundColor Red
}
catch {
    Write-Host "ERRO: $_" -ForegroundColor Red
}

# --- Falhou ---
Write-Host ""
Write-Host "=== DOWNLOAD FALHOU ===" -ForegroundColor Red
Write-Host ""
Write-Host "Verifique rede/firewall ou limite de taxa do Hugging Face." -ForegroundColor Yellow
Write-Host ""
Write-Host "Opcoes:" -ForegroundColor White
Write-Host "  1. Token HF (opcional, ajuda em rate limit): https://huggingface.co/settings/tokens" -ForegroundColor Gray
Write-Host '  2. Execute: $env:HF_TOKEN="hf_seu_token"; .\download-bge-m3.ps1' -ForegroundColor Gray
Write-Host "  3. Baixe manualmente (repo atual):" -ForegroundColor Gray
Write-Host "     https://huggingface.co/$hfRepoId/tree/main" -ForegroundColor Gray
Write-Host "     Salve como: $destFile" -ForegroundColor Gray
Write-Host ""
Write-Host "Enquanto isso, o Chronos usara o LLM principal para embedding (fallback)." -ForegroundColor Gray
exit 1
