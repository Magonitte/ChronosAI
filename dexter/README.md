# Chronos — Assistente de voz (desktop)

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

No **Windows**, a transcrição usa um servidor **whisper.cpp** (HTTP), compatível com rotas estilo OpenAI — não há mais STT embutido via `whisper-rs` neste alvo.

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

Políticas de apps e mídia estão implementadas em **`src-tauri/src/tools.rs`** e **`media_controls.rs`**.

### Screenshot + visão

Fluxo típico: o modelo decide chamar `take_screenshot` com uma pergunta (“o que há na tela?”); o backend captura a imagem e envia ao **modelo de visão** configurado (por exemplo um modelo multimodal no mesmo servidor ou endpoint dedicado). Ajuste o modelo de visão em **Configurações**.

---

## RAG (base de conhecimento)

1. **Ingestão** — texto dividido em chunks; cada chunk recebe embedding via API **`/embedding`** (servidor compatível com llama.cpp).
2. **Busca** — a consulta é embedada e comparada por similaridade de cosseno no SQLite.
3. **UI** — em **Configurações → Conhecimento**: adicionar texto/arquivo, listar fontes, apagar.

Em **Configurações**, a **URL de embeddings** é opcional: se estiver vazia, o RAG usa a mesma base que a **URL do LLM**. Para um modelo dedicado (por exemplo **BGE-M3** num segundo `llama-server`, ex. `http://localhost:8082`, como no `start-all.ps1`), preencha a URL e o nome do modelo de embedding exposto por esse servidor. O script **`download-bge-m3.ps1`** ajuda a obter o GGUF público (ajuste o destino no arquivo).

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
- **URL de embeddings** — opcional; se vazio, o RAG reutiliza a URL do LLM. Use um host dedicado (ex.: `http://localhost:8082`) com modelo só de embedding (BGE-M3, etc.) quando quiser separar carga ou modelos.
- **Modelo de chat** — nome exposto pelo servidor (ex.: nome do GGUF ou alias).
- **Modelo de embedding** — nome no servidor que atende `/embedding` (pode ser BGE-M3 ou o alias do seu `llama-server`).
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
  - **Servidor de embeddings** (mesmo processo ou segunda instância em outra porta) com rota `/embedding`, se usar RAG com modelo dedicado — ver **`start-all.ps1`** (porta 8082 por padrão) e **`download-bge-m3.ps1`**.
  - **Chatterbox** (ou outro servidor OpenAI-compatible `/v1/audio/speech`) — ver **`chatterbox-tts-api/README.md`** e **`start-all.ps1`**.

---

## Build e execução

```powershell
npm install
npm run tauri dev    # desenvolvimento
npm run tauri build  # instalador / artefatos de release
```

### Um comando para subir serviços (Windows)

```powershell
.\start-all.ps1
# Perfis: voice-chatterbox (padrao) | voice-chatterbox-cpu | voice-fast | balanced | quality
# Exemplos: -NoEmbedding  -NoWhisper  -NoTts  -WhisperTiny  -ForceRestartServices
```

Edite variáveis no topo do script (`start-all.ps1`): caminhos para `llama-server`, `whisper-server`, modelo e porta de **embedding** (8082), modelos e pastas do Chatterbox.

**Perfil recomendado:** `voice-chatterbox` (padrão do script) — Chatterbox em CUDA, contexto LLM 8192, `-ngl 28` no LLM. O perfil **`voice-chatterbox-cpu`** move o Chatterbox para CPU e devolve mais camadas GPU ao modelo de chat. Métricas de latência (Gemma 4 + Turbo TTS) estão no [**README na raiz do repositório**](../README.md#performance-pipeline-de-voz). Notas mais longas podem ficar em `Documentação/` à parte — pasta **não versionada** no Git.

---

## Pilha tecnológica

| Camada | Tecnologia |
|--------|------------|
| Shell do app | Tauri 2 |
| Backend | Rust (reqwest, cpal, SQLite, …) |
| Frontend | React 19, TypeScript, Vite, Tailwind |
| STT | Servidor Whisper (HTTP) |
| Embeddings (RAG) | `llama-server` ou compatível com `/embedding` (pode ser instância dedicada) |
| LLM | Servidor compatível OpenAI (llama.cpp, etc.) |
| TTS | Chatterbox ou modo configurado |
| Atalhos globais | plugin global-shortcut |

---

## Estrutura de pastas (Chronos)

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
├── validate.ps1
├── download-bge-m3.ps1
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
