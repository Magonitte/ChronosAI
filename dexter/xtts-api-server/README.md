# XTTS v2 TTS API (Chronos)

Servidor **FastAPI** para síntese de voz com **Coqui XTTS v2**, usado pelo assistente Chronos/Dexter como backend TTS principal (porta **8005**).

Compatível com o endpoint OpenAI-style `POST /v1/audio/speech` consumido por `dexter/src-tauri/src/voice.rs`.

---

## Endpoints principais

| Método | Rota | Descrição |
|--------|------|-----------|
| GET | `/health` | Estado do modelo e dispositivo (`cuda` / `cpu`) |
| POST | `/v1/audio/speech` | Síntese (JSON: `input`, `voice`, `model`) |
| GET/POST | `/voices` | Biblioteca de vozes clonadas |

Documentação interativa: `http://localhost:8005/docs`

---

## Setup

```powershell
cd dexter\xtts-api-server

# Com uv (recomendado — usado por start-all.ps1)
uv sync
uv run main.py

# Ou venv manual
python -m venv .venv
.\.venv\Scripts\activate
pip install -r requirements.txt
python main.py
```

Variáveis úteis (definidas pelo `start-all.ps1` ou manualmente):

| Variável | Descrição |
|----------|-----------|
| `DEXTER_TTS_INFER_DEVICE` | `cuda` ou `cpu` |
| `XTTS_PORT` | Porta HTTP (padrão 8005) |

---

## Registrar voz PT-BR

Com o servidor no ar:

```powershell
cd dexter
.\register-voice.ps1
```

Registra `Clone_voz.mp3` como **`dexter-ptbr`** (idioma `pt`). Ficheiros de referência ficam em `voices/`.

---

## Integração com Chronos

- Launcher: `dexter/start-all.ps1` — perfis `voice-xtts-*` definem `TTS_MODE=xtts` e `DEXTER_TTS_MODE`.
- O Rust pode **subir/parar** este servidor durante o swap LLM on-demand (`llm_ondemand.rs`).
- Caminhos padrão: `XTTS_SERVER_PATH` → `xtts-api-server/main.py`, `XTTS_PYTHON_EXE` → `.venv\Scripts\python.exe`.

Ver [**`../README.md`**](../README.md) para arquitetura completa e perfis de performance.

---

## Dependências

- Python **3.12+**
- `coqui-tts` (fork idiap), `torch`, `fastapi`, `uvicorn`
- GPU NVIDIA opcional (perfil `voice-xtts-cuda-partial`)

Detalhes em `requirements.txt`.
