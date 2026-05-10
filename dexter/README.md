# Dexter — Assistente de voz (desktop)

Assistente de voz **local** e focado em privacidade: você segura uma tecla de atalho, fala e ouve a resposta sintetizada — com LLM, STT e TTS rodando na sua máquina (ou em serviços locais que você configurar).

Implementação em **Tauri 2** (Rust no backend, **React 19** na interface).

---

## Fluxo resumido

```
Microfone ──► Whisper (STT) ──► LLM (chat + ferramentas) ──► Chatterbox / TTS ──► alto-falantes
               HTTP/API        API compatível OpenAI          HTTP
```

1. **Segure Shift+Z** — o orb aparece e a gravação começa.
2. **Solte** — o áudio é enviado ao servidor **Whisper**; o texto vira mensagem do usuário.
3. O **LLM** (ex.: **llama.cpp** em modo servidor OpenAI) gera a resposta; pode chamar **ferramentas** várias vezes.
4. O texto é segmentado em frases e cada trecho é sintetizado pelo **TTS** assim que possível (*streaming* de áudio).
5. **Shift+X** oculta a janela do orb.

O app fica na **bandeja do sistema**; a janela transparente com o orb não precisa ficar sempre visível.

---

## Arquitetura

```
┌─────────────────────────────────────────────────────────────┐
│  Windows — bandeja do sistema                               │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  Tauri (Rust)                                           ││
│  │  • Captura de áudio (cpal)                              ││
│  │  • Cliente HTTP Whisper (STT)                           ││
│  │  • Cliente LLM streaming + tool calling                 ││
│  │  • Cliente Chatterbox / TTS                           ││
│  │  • Execução de ferramentas (screenshot, clipboard, …)   ││
│  │  • RAG (SQLite + embeddings)                          ││
│  └───────────────────────┬─────────────────────────────────┘│
│                          │ eventos + invoke                  │
│  ┌───────────────────────▼─────────────────────────────────┐│
│  │  React — orb, bolhas de chat, configurações             ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### STT no Windows

No **Windows**, a transcrição usa um servidor **whisper.cpp** (HTTP), compatível com rotas estilo OpenAI — não há mais STT embutido via `whisper-rs` neste alvo (ver [**MIGRACAO_WINDOWS.md**](MIGRACAO_WINDOWS.md)).

### TTS em fluxo

O modelo não precisa terminar a resposta inteira antes de falar:

1. O LLM envia tokens em fluxo.
2. A detecção de **fim de frase** (`.`, `!`, `?` + espaço) corta o texto em sentenças.
3. Cada sentença completa é enviada ao TTS.
4. O frontend enfileira e reproduz os pedaços em ordem.

---

## Ferramentas disponíveis

As ferramentas são expostas ao modelo em JSON (funções). Você liga ou desliga cada uma em **Configurações → Ferramentas**.

| Ferramenta | Função |
|------------|--------|
| **search_knowledge** | Busca semântica na base local (RAG / SQLite + embeddings). |
| **take_screenshot** | Captura a tela e descreve (modelo de visão configurável). |
| **read_clipboard** | Lê o texto da área de transferência. |
| **open_url** | Abre URL no navegador padrão. |
| **get_current_time** | Data e hora atuais. |
| **list_running_apps** | Lista aplicativos em execução (equivalente ao pedido “o que está aberto”). |
| **web_fetch** | Obtém texto de uma página HTTP(S). |
| **run_command** | Executa comando em **PowerShell** dentro do sandbox (uso controlado). |
| **launch_desktop_app** / **close_desktop_app** | Abre ou fecha apps pré-definidos no Windows (Cursor, VS Code, Terminal, navegadores, Office, etc.). |
| **control_media_playback** | Play/pause/próximo volume do sistema de sessão multimídia (SMTC). |
| **adjust_system_volume** | Volume do sistema por “teclas” multimídia simuladas. |
| **play_music_query** | Toca faixa pelo nome (varre biblioteca local; pode recorrer à web se não achar). |
| **play_local_music_playlist** | Playlist M3U local por artista / escopo (casos específicos). |
| **native_music_library_shuffle_play** | Biblioteca inteira em modo aleatório via Reprodutor do Windows (rápido, sem varrer disco). |
| **play_full_local_music_library** | Varredura pesada / export M3U grande — só com pedido explícito do usuário. |

Detalhes de apps, mídia e whitelist: [**DESKTOP_APP_TOOLS.md**](DESKTOP_APP_TOOLS.md).

### Screenshot + visão

Fluxo típico: o modelo decide chamar `take_screenshot` com uma pergunta (“o que há na tela?”); o backend captura a imagem e envia ao **modelo de visão** configurado (por exemplo um modelo multimodal no mesmo servidor ou endpoint dedicado). Ajuste o modelo de visão em **Configurações**.

---

## RAG (base de conhecimento)

1. **Ingestão** — texto dividido em chunks; cada chunk recebe embedding via API do servidor de embeddings (mesmo host que o LLM, conforme config).
2. **Busca** — a consulta é embedada e comparada por similaridade de cosseno no SQLite.
3. **UI** — em **Configurações → Conhecimento**: adicionar texto/arquivo, listar fontes, apagar.

É necessário ter um modelo de embedding disponível no servidor que você usa (equivalente ao que antes era `ollama pull nomic-embed-text` em setups Ollama).

---

## Estados do orb

| Cor / animação | Estado |
|----------------|--------|
| Azul (respiração) | Ocioso / pronto |
| Vermelho (pulsa) | Ouvindo microfone |
| Âmbar | Processando (STT) |
| Roxo | Aguardando LLM |
| Ciano | Reproduzindo TTS |
| Vermelho fraco | Erro |

---

## Configurações

A bandeja do sistema abre o menu; daí você abre **Configurações**.

### Config (URLs e modelos)

- **Whisper URL** — base HTTP do servidor STT (ex.: `http://localhost:8081`).
- **Caminho do modelo Whisper** — usado onde o cliente ainda referencia modelo em disco, conforme versão.
- **URL do LLM** — servidor compatível OpenAI (ex.: llama.cpp em `http://localhost:8080`).
- **Modelo de chat** — nome exposto pelo servidor (ex.: nome do GGUF ou alias).
- **Modelo de embedding** — para RAG.
- **Modelo de visão** — screenshot; se vazio, pode reutilizar o modelo principal conforme implementação.
- **URL do Chatterbox** — TTS (ex.: `http://localhost:8005`).
- **Voz** — identificador da voz no servidor TTS.
- **Prompt do sistema** — personalidade e regras (o padrão incentiva **português do Brasil** e respostas curtas para voz).

### Ferramentas

Alterne cada ferramenta; a mudança vale para o próximo turno da conversa.

### Conhecimento

Gestão da base RAG.

### Persistência

O ficheiro de configuração no Windows fica em:

`%APPDATA%\voice-assistant\config.json`

---

## Pré-requisitos de build

- **Rust** (edition 2021), **Node.js**, ferramentas de compilação **MSVC** / Visual Studio Build Tools no Windows.
- Servidores em execução **antes** de usar voz de ponta a ponta:
  - **llama.cpp** `llama-server` com modelo GGUF.
  - **whisper.cpp** `whisper-server` com modelo compatível.
  - **Chatterbox** (ou outro servidor OpenAI-compatible `/v1/audio/speech`) conforme [**TTS_SETUP.md**](TTS_SETUP.md).

Guias adicionais:

- [**WHISPER_STT_SETUP.md**](WHISPER_STT_SETUP.md)
- [**PLANO_CORRECAO_WHISPER_404.md**](PLANO_CORRECAO_WHISPER_404.md) — rotas e erros 404 no Whisper server.

---

## Build e execução

```powershell
npm install
npm run tauri dev    # desenvolvimento
npm run tauri build  # instalador / artefatos de release
```

### Um comando para subir serviços (Windows)

```powershell
.\start-all.ps1 -Profile voice-fast
# Perfis: voice-fast | balanced | quality | voice-chatterbox
```

Edite variáveis no topo do script (`start-all.ps1`): caminhos para `llama-server`, `whisper-server`, modelos e pastas do Chatterbox.

---

## Pilha tecnológica

| Camada | Tecnologia |
|--------|------------|
| Shell do app | Tauri 2 |
| Backend | Rust (reqwest, cpal, SQLite, …) |
| Frontend | React 19, TypeScript, Vite, Tailwind |
| STT | Servidor Whisper (HTTP) |
| LLM | Servidor compatível OpenAI (llama.cpp, etc.) |
| TTS | Chatterbox ou modo configurado |
| Atalhos globais | plugin global-shortcut |

---

## Estrutura de pastas (Dexter)

```
dexter/
├── src/
│   ├── App.tsx           # Orb, rotas, configurações
│   └── …
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs        # Tray, atalhos, orquestração
│   │   ├── voice.rs      # STT/LLM/TTS streaming, definições de tools
│   │   ├── tools.rs      # Implementação das ferramentas
│   │   ├── rag.rs        # Base de conhecimento
│   │   ├── sandbox.rs    # Comandos restritos
│   │   └── media_controls.rs  # Windows SMTC / volume
│   ├── capabilities/
│   └── tauri.conf.json
├── start-all.ps1
└── package.json
```

---

## Atalhos

| Atalho | Ação |
|--------|------|
| **Shift+Z** (segurar) | Push-to-talk — gravar enquanto segura |
| **Shift+X** | Ocultar / dispensar a janela do orb |

---

## Licença

Ver [**LICENSE**](LICENSE) nesta pasta (Apache 2.0).
