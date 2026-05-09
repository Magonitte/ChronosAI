# Sessão — Assistente de voz PT-BR e Chatterbox TTS

**Data de referência:** 9 de maio de 2026  

Este documento registra o trabalho realizado na sessão de desenvolvimento do projeto Dexter (voice assistant): integração de **Text-to-Speech** em **português do Brasil**, clonagem de voz a partir de `Clone_voz.mp3`, correção do erro `TTS failed for sentence` e diagnósticos no ambiente Windows.

---

## Objetivo

- Dar **voz** ao assistente (fala audível), com foco em **baixa latência** relativa e fluxo semelhante ao projeto original [thecodacus/dexter](https://github.com/thecodacus/dexter).
- Idioma de saída: **PT-BR**.
- Opcional: **clonagem de voz** usando o arquivo `Clone_voz.mp3` na pasta `dexter/`.

---

## Contexto técnico (antes)

- O app já chamava `POST {chatterbox_url}/v1/audio/speech` (compatível OpenAI).
- O erro observado pelo usuário:  
  `TTS failed for sentence: error sending request for url (http://localhost:8005/v1/audio/speech)`  
  indicava **nenhum servidor HTTP respondendo na porta 8005** — não era falha do Rust em si, e sim **TTS não instalado / não iniciado**.

---

## O que foi implementado

### 1. Servidor TTS: `chatterbox-tts-api`

Foi adotado o projeto **[travisvn/chatterbox-tts-api](https://github.com/travisvn/chatterbox-tts-api)** (FastAPI, endpoint OpenAI-compatible), com modelo **multilingual** (22 idiomas, incluindo `pt`).

- Clone/local do código em: `dexter/chatterbox-tts-api/`
- Configuração via `dexter/chatterbox-tts-api/.env` (porta **8005**, `USE_MULTILINGUAL_MODEL=true`, etc.)
- `Clone_voz.mp3` copiado como amostra padrão para `voice-sample.mp3` onde aplicável

### 2. Scripts PowerShell

| Arquivo | Função |
|---------|--------|
| `setup-tts.ps1` | Clona o repositório (se necessário), `uv sync`, instala **PyTorch CUDA 12.4** e **setuptools&lt;82**, gera `.env` |
| `register-voice.ps1` | Envia `Clone_voz.mp3` para `POST /voices` com `language=pt` e nome `dexter-ptbr` |

### 3. Documentação de uso

| Arquivo | Função |
|---------|--------|
| `TTS_SETUP.md` | Guia de setup, API, troubleshooting e dicas de qualidade da voz |

### 4. App Dexter (Rust)

- **`src-tauri/src/lib.rs`** — padrão `chatterbox_voice`: `"dexter-ptbr"` (antes `"Anirban.wav"`).
- **`src-tauri/src/voice.rs`** — corpo JSON do Chatterbox inclui `response_format: "wav"` para alinhar com o playback no frontend (blob `audio/wav`).

### 5. Launcher `start-all.ps1`

- Variáveis: `$CHATTERBOX_PORT`, `$CHATTERBOX_VOICE`, `$CHATTERBOX_DIR`.
- Se existir `chatterbox-tts-api/.venv`, inicia com **`.venv\Scripts\python.exe main.py`** (evita `uv run` reverter dependências).
- Define **`$env:PYTHONIOENCODING = "utf-8"`** antes do `Start-Process` (console Windows / logs com emoji).
- Aguarda `GET /voices`; verifica resposta JSON com **`$voicesResp.voices`**; registra voz via `register-voice.ps1` se `dexter-ptbr` não existir.

### 6. Patches no fork local `chatterbox-tts-api` (importante no Windows)

Alterações em cópia local do repositório clonado dentro de `dexter/`:

- **`app/core/tts_model.py`**
  - Removido carregamento do modelo via `asyncio.run_in_executor` (causava **travamento** no Windows).
  - Carregamento **síncrono** de `ChatterboxMultilingualTTS.from_pretrained` / `ChatterboxTTS.from_pretrained`.
  - Mensagens de log sem caracteres Unicode problemáticos no `cp1252` (substituídos por `[OK]` / `[FAIL]`).
- **`app/main.py`**
  - No `lifespan`, **aguarda** `await initialize_model()` antes de liberar o app (modelo pronto antes de tráfego real).

---

## Problemas encontrados e como foram resolvidos

| Sintoma | Causa | Correção |
|---------|--------|----------|
| Conexão recusada em `:8005` | Servidor TTS não estava rodando | Instalar/iniciar `chatterbox-tts-api`; usar `start-all.ps1` ou subir manualmente |
| Erro ao desserializar pesos CUDA com PyTorch CPU | `uv sync` instalava **torch CPU-only** | `uv pip install torch==2.6.0+cu124 torchaudio==2.6.0+cu124` (índice PyTorch cu124) |
| `TypeError: 'NoneType' object is not callable` em `PerthImplicitWatermarker` | Import do watermarker falhava: falta **`pkg_resources`** (setuptools) | `uv pip install "setuptools<82"` |
| Servidor respondia mas `/languages` só tinha `en` | Modelo multilingual não terminava de inicializar + APIs antes do modelo pronto | Carregar modelo de forma síncrona + aguardar no lifespan |
| `UnicodeEncodeError` em logs com emoji | Console Windows `cp1252` | `PYTHONIOENCODING=utf-8` ao iniciar Python |
| Upload de voz com `language=pt` rejeitado | API achava modelo “standard” até multilingual ficar pronto | Mesma correção de inicialização completa do modelo |

---

## Performance observada (referência)

Medições aproximadas na máquina usada na sessão (RTX 3070, etc.):

- **GPU (`DEVICE=cuda` no `.env`)**: primeira síntese mais lenta (warm-up); depois da ordem de **~5–7 s** por trecho curto em testes manuais.
- **CPU**: síntese bem mais lenta (ordem de **~15+ s** para frases curtas nos testes).

**VRAM:** o modelo multilingual na GPU pode usar volumes grandes de VRAM; se o **LLM** também usar a mesma GPU, pode haver **OOM** — nesse caso usar `DEVICE=cpu` no `.env` do Chatterbox ou ajustar carga do LLM.

---

## Como usar depois desta sessão

1. **Setup único (se ainda não fez):**  
   `.\setup-tts.ps1` na pasta `dexter/`

2. **Subir tudo:**  
   `.\start-all.ps1`

3. **Manual (só TTS):**  
   ```powershell
   cd chatterbox-tts-api
   $env:PYTHONIOENCODING = "utf-8"
   .\.venv\Scripts\python.exe main.py
   ```

4. **Registrar voz (se necessário):**  
   `.\register-voice.ps1`

5. **No app (Configurações):**  
   - URL: `http://localhost:8005`  
   - Voz: `dexter-ptbr`

---

## Arquivos tocados (checklist)

- `dexter/setup-tts.ps1` — criado/atualizado  
- `dexter/register-voice.ps1` — criado  
- `dexter/TTS_SETUP.md` — criado  
- `dexter/start-all.ps1` — seção Chatterbox + env UTF-8 + venv Python  
- `dexter/src-tauri/src/lib.rs` — default `chatterbox_voice`  
- `dexter/src-tauri/src/voice.rs` — `response_format: wav`  
- `dexter/chatterbox-tts-api/` — clone + `.env` + patches em `app/core/tts_model.py`, `app/main.py`  
- `dexter/Clone_voz.mp3` — usado como amostra / upload para voz `dexter-ptbr`  

---

## Artefatos de teste gerados na sessão

Podem existir na pasta `dexter/` (podem ser apagados se não forem necessários):

- `test-tts-output.wav`, `test-tts-cuda.wav`, `test-tts-cuda2.wav` — validação manual da API  
- `tts-test-body.json` — tentativa de teste via curl (se ainda existir)

---

## Referências

- Dexter original: [github.com/thecodacus/dexter](https://github.com/thecodacus/dexter)  
- API Chatterbox TTS (documentação do projeto): [chatterboxtts.com/docs](https://chatterboxtts.com/docs)  
- Repositório do servidor usado: [github.com/travisvn/chatterbox-tts-api](https://github.com/travisvn/chatterbox-tts-api)

---

*Documento gerado para registrar o trabalho da sessão; complementa `TTS_SETUP.md` com o histórico de problemas e mudanças no código.*
