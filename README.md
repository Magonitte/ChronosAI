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

**Chronos AI** reúne o código do assistente de voz **Dexter** (aplicativo desktop) e ferramentas auxiliares usadas no desenvolvimento — por exemplo o código-fonte do **whisper.cpp** em `tools/` para compilar o servidor de transcrição localmente.

| Área | Descrição |
|------|-----------|
| [`dexter/`](dexter/) | Aplicativo **Tauri 2** (Rust + React): captura de voz, STT, chat com LLM, TTS, bandeja do sistema e ferramentas integradas |
| `tools/whisper.cpp` | Árvore do **whisper.cpp** (servidor de transcrição, quando você compila localmente) |

Visão técnica do app está em [**`dexter/README.md`**](dexter/README.md).

---

## Principais capacidades

- **Push-to-talk** — segure **Shift+Z** para falar; **Shift+X** oculta o orb.
- **Pipeline de voz** — áudio → **Whisper** (STT) → **LLM** (API compatível com OpenAI, ex.: llama.cpp) → **Chatterbox** (TTS).
- **Streaming** — respostas do modelo em fluxo; frases enviadas ao TTS assim que fecham, para menor latência percebida.
- **Ferramentas (tool calling)** — captura de tela, área de transferência, URLs, horário, processos, comandos em sandbox, busca na web, RAG local, apps do Windows, mídia e volume — ver tabela no README do `dexter`.
- **Base de conhecimento (RAG)** — SQLite + embeddings para consultar documentos ingeridos localmente.

---

## Requisitos (resumo)

- **Windows** é o ambiente principal atualmente suportado para o fluxo completo de voz e ferramentas de desktop.
- Serviços locais típicos (ajuste portas nos **Configurações** do app):
  - LLM: **llama.cpp** `llama-server` (ex.: `http://localhost:8080`)
  - STT: **whisper.cpp** `whisper-server` (ex.: `http://localhost:8081`)
  - TTS: **Chatterbox** (ou modo configurado no `start-all.ps1`) — ver também `dexter/chatterbox-tts-api/`
- **Node.js**, **Rust**, **Visual Studio Build Tools** (Windows) para compilar o Tauri.

O script **`dexter/start-all.ps1`** ajuda a subir LLM + Whisper + TTS + frontend (perfil **padrão: `voice-chatterbox`**; outros: `voice-fast`, `balanced`, `quality`). Caminhos de executáveis e modelos **precisam ser ajustados** no topo do script para a sua máquina.

---

## Performance (pipeline de voz)

Métricas coletadas com RTX 3070 8 GB, Gemma 4 26B, Chatterbox Turbo, Whisper small.

| Métrica | Baseline | Otimizado | Melhoria |
|---|---|---|---|
| TTFA médio (K1) | 19.3s | **13.0s** | **-33%** |
| TTS chunk 0 médio | 10.6s | **8.0s** | **-25%** |
| Token rate LLM (K7) | 6.5 tok/s | **10.1 tok/s** | **+55%** |
| Respostas vazias | 25% (1/4) | **0% (4/4)** | **100%** |

A otimização completa incluiu:
- **`thinking_budget_tokens=256`** — força o Gemma 4 a encerrar o raciocínio e entregar resposta visível
- **`max_tokens=600`** — espaço para reasoning + conteúdo (era 220)
- **`CFM_TIMESTEPS=2`** — Chatterbox Turbo, ~5x mais rápido na conversão speech token → áudio
- **Contexto LLM reduzido** de 16384 para 8192 — token rate 3.2x maior sem perda perceptível de coerência

Perfil recomendado: **`voice-chatterbox`** (Chatterbox GPU + Turbo). Consulte [`dexter/Documentação/`](dexter/Documentação/) para relatórios detalhados.

---

## Início rápido

```powershell
git clone https://github.com/Magonitte/ChronosAI.git
cd ChronosAI\dexter
npm install
# Configure start-all.ps1 (modelos, caminhos). Depois:
.\start-all.ps1
# Em outro terminal, com os serviços no ar:
npm run tauri dev
```

Build de produção:

```powershell
npm run tauri build
```

---

## Estrutura do repositório

```
ChronosAI/
├── README.md                 ← você está aqui
├── dexter/                   ← aplicativo Voice Assistant (Tauri)
│   ├── README.md             ← visão técnica do Dexter
│   ├── src/                  ← React (Vite)
│   ├── src-tauri/            ← Rust (core, voz, ferramentas, RAG)
│   ├── start-all.ps1         ← orquestra servidores locais (Windows)
│   └── chatterbox-tts-api/   ← API TTS (quando usada no projeto)
└── tools/
    └── whisper.cpp/          ← upstream whisper.cpp (referência / build)
```

---

## Licença

O aplicativo em **`dexter/`** está sob **Apache License 2.0** — ver [`dexter/LICENSE`](dexter/LICENSE).  
Componentes de terceiros (`tools/whisper.cpp`, etc.) seguem as licenças nos respectivos diretórios.

---

<div align="center">

**Chronos AI** · Assistente local · Português (Brasil)

</div>
