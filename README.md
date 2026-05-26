<div align="center">

# Chronos AI

**Assistente de voz local para desktop — privacidade primeiro, processamento na sua máquina.**

[![Licença](https://img.shields.io/badge/licença-Apache%202.0-blue.svg)](dexter/LICENSE)
[![Tauri](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=black)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)](https://react.dev/)

*Interface em português do Brasil · Modelos e serviços configuráveis · Ferramentas (tools) para desktop e automação*

</div>

---

## O que é este repositório

**Chronos AI** reúne o aplicativo desktop **Dexter** (Tauri 2 + React) e dependências locais para STT, LLM, TTS e visão. O fluxo completo de voz roda no **Windows**, com serviços HTTP locais orquestrados por scripts PowerShell.

| Área | Descrição |
|------|-----------|
| [`dexter/`](dexter/) | App **Tauri 2** (Rust + React): push-to-talk, STT, LLM, TTS, chat de texto, bandeja do sistema, RAG e ferramentas de desktop |
| [`dexter/xtts-api-server/`](dexter/xtts-api-server/) | API **XTTS v2** (Coqui) — TTS com clonagem PT-BR; perfil de produção recomendado |
| [`dexter/chatterbox-tts-api/`](dexter/chatterbox-tts-api/) | API **Chatterbox** (alternativa de TTS; perfis `voice-chatterbox*`) |
| [`tools/whisper.cpp/`](tools/whisper.cpp/) | **whisper.cpp** (upstream) para compilar `whisper-server` localmente |

Documentação técnica detalhada: [**`dexter/README.md`**](dexter/README.md).

---

## Principais capacidades

- **Push-to-talk** — segure **Shift+Z** para falar; **Shift+X** oculta o orb.
- **Pipeline de voz** — microfone → **Whisper** (STT) → **LLM** (Llama 8B em `:8080`) → **XTTS** ou **Chatterbox** (TTS em `:8005`) → alto-falantes.
- **Fast-path** — dezenas de comandos em português (hora, calculadora, clipboard, apps, mídia, arquivos, tradução, etc.) executados **sem LLM** quando o texto casa com padrões em `fast_path.rs`.
- **LLM on-demand** — voz usa **Llama 8B**; ao abrir o **chat de texto** (**Shift+T**), o app troca para **Qwen** em `:8084` e libera VRAM; ao fechar, repõe Llama + XTTS automaticamente.
- **Streaming** — tokens do LLM em fluxo; frases completas vão ao TTS assim que fecham (menor latência percebida).
- **Visão on-demand** — **Qwen2.5-VL** em `:8083` sobe na primeira screenshot e desliga após inatividade.
- **53 ferramentas (tools)** — sistema, arquivos, clipboard, notificações, janelas, rede, calendário, e-mail, OCR, transcrição, automação de UI, geração de imagem, mídia, web, RAG e mais (lista completa em [`dexter/README.md`](dexter/README.md#ferramentas-disponíveis)).
- **RAG** — SQLite + embeddings (BGE-M3 ou servidor compatível com `/embedding`).

---

## Serviços locais (portas padrão)

| Serviço | Porta | Função |
|---------|-------|--------|
| LLM voz | **8080** | Llama 3.1 8B (perfis `voice-xtts*`) ou modelo maior nos perfis Chatterbox |
| Whisper STT | **8081** | Transcrição (`whisper-server`) |
| Embeddings | **8082** | BGE-M3 para RAG (`llama-server`, opcional com `-NoEmbedding`) |
| Visão | **8083** | Qwen2.5-VL 3B (on-demand pelo app) |
| LLM texto | **8084** | Qwen 35B — sobe ao abrir chat (**Shift+T**) |
| TTS | **8005** | XTTS v2 ou Chatterbox (conforme perfil) |
| Vite (dev) | **1420** | Frontend React em desenvolvimento |

Ajuste caminhos de executáveis e modelos no topo de **`dexter/start-all.ps1`**.

---

## Requisitos (resumo)

- **Windows 11** (ambiente principal do fluxo completo).
- **Node.js**, **Rust**, **Visual Studio Build Tools** (MSVC) para compilar o Tauri.
- **Python 3.12+** com `uv` (recomendado) para XTTS/Chatterbox.
- **llama.cpp** — `llama-server` com modelos GGUF (Llama 8B voz, Qwen texto, BGE-M3, Qwen-VL).
- **whisper.cpp** — `whisper-server` (pode usar a árvore em `tools/whisper.cpp`).
- **GPU NVIDIA** recomendada para perfis `voice-xtts-cuda-partial` (RTX 8 GB validado).

---

## Início rápido

```powershell
git clone https://github.com/Magonitte/ChronosAI.git
cd ChronosAI\dexter
npm install

# 1) Edite caminhos em start-all.ps1 (llama-server, modelos, whisper, etc.)
# 2) Registre a voz clonada (com XTTS no ar):
.\register-voice.ps1

# 3) Suba todos os serviços + app (perfil padrão: voice-xtts-cuda-partial)
.\start-all.ps1

# Ou só o frontend Tauri, se os serviços já estiverem rodando:
npm run tauri dev
```

Build de produção:

```powershell
npm run tauri build
```

**Perfil padrão:** `voice-xtts-cuda-partial` — XTTS em CUDA + Llama `-ngl 28` (~28/33 camadas na GPU). Se o PC travar ao subir: `.\start-all.ps1 -Profile voice-xtts-safe -EasyOnRam`.

**Sandbox / testes:** `.\teste.ps1` — mesmos perfis que `start-all.ps1`, para validar mudanças antes de promover.

Parâmetros úteis do launcher: `-NoWhisper`, `-NoTts`, `-NoEmbedding`, `-WhisperTiny`, `-ForceRestartServices`, `-StartupStaggerSec`, `-EasyOnRam`.

---

## Performance (pipeline de voz — Fase 0, maio/2026)

Medições com **RTX 3070 8 GB**, **Llama 3.1 8B**, **XTTS v2 CUDA**, Whisper small. Detalhes em `dexter/Documentação/metricas/METRICA-VOZ-performance-fase0-2026-05-18.md` (pasta local, não versionada no Git).

| Perfil | XTTS | Llama GPU | Gap entre frases (UI) | TTS ~100 chars | LLM tok/s |
|--------|------|-----------|------------------------|----------------|-----------|
| `voice-xtts-cpu` | CPU | 33/33 | ~10 s | ~15–35 s | ~55–64 |
| `voice-xtts-cuda` | CUDA | 20/33 | ~0 ms | ~3,3–5,8 s | ~8–9 |
| **`voice-xtts-cuda-partial`** | **CUDA** | **28/33** | **~0 ms** | **~3,0–6,6 s** | **~14–16** |

O perfil **`voice-xtts-cuda-partial`** equilibra VRAM entre LLM e TTS: fluência entre frases sem os gaps de ~10 s do baseline CPU, com geração do LLM ~1,7× mais rápida que `-ngl 20`.

Benchmarks históricos com **Gemma 4 + Chatterbox Turbo** permanecem nos relatórios em `dexter/Documentação/`; a stack de produção atual prioriza **Llama 8B + XTTS**.

---

## Estrutura do repositório

```
Chronos_AI_v2/
├── README.md                      ← você está aqui
├── config.json                    ← exemplo de config (URLs, tools, atalhos)
├── dexter/                        ← aplicativo Chronos / Dexter
│   ├── README.md                  ← visão técnica completa
│   ├── src/                       ← React (Vite)
│   ├── src-tauri/                 ← Rust (voz, 53 tools, RAG, LLM on-demand, fast-path)
│   ├── xtts-api-server/           ← API XTTS v2 (TTS padrão)
│   ├── chatterbox-tts-api/        ← API Chatterbox (alternativa)
│   ├── start-all.ps1              ← launcher oficial (Windows)
│   ├── teste.ps1                  ← launcher sandbox / regressão
│   ├── register-voice.ps1         ← registra voz dexter-ptbr no XTTS
│   ├── download-bge-m3.ps1        ← download BGE-M3 (embeddings)
│   ├── download-vision-model.ps1  ← download Qwen2.5-VL
│   └── Documentação/              ← planos, métricas, guias (local, .gitignore)
└── tools/
    └── whisper.cpp/               ← upstream whisper.cpp
```

---

## Licença

O aplicativo em **`dexter/`** está sob **Apache License 2.0** — ver [`dexter/LICENSE`](dexter/LICENSE).

Componentes de terceiros (`tools/whisper.cpp`, `chatterbox-tts-api`, etc.) seguem as licenças nos respectivos diretórios.

---

<div align="center">

**Chronos AI** · Assistente local · Português (Brasil)

</div>
