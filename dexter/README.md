# Chronos — Assistente de voz (desktop)

Assistente de voz **local** e focado em privacidade: segure uma tecla de atalho, fale e ouve a resposta sintetizada — com LLM, STT e TTS na sua máquina.

Implementação em **Tauri 2** (Rust no backend, **React 19** na interface).

---

## Fluxo resumido

```
Microfone ──► Whisper (STT :8081) ──► LLM voz (Llama 8B :8080) ──► XTTS / Chatterbox (:8005) ──► alto-falantes
                         │                      │
                         │              fast-path (comandos curtos, sem LLM)
                         │
              Chat texto (Shift+T) ──► Qwen 35B (:8084) — swap on-demand
```

1. **Segure Shift+Z** — o orb aparece e a gravação começa.
2. **Solte** — áudio → **Whisper**; o texto vira mensagem do usuário.
3. **Fast-path** (opcional) — se a frase casar com um comando conhecido, a ferramenta roda direto (hora, calculadora, clipboard, apps, mídia, arquivos, tradução, etc.).
4. Caso contrário, o **LLM de voz** gera a resposta; pode chamar **ferramentas** várias vezes.
5. O texto é segmentado em frases e cada trecho vai ao **TTS** assim que possível (*streaming* de áudio).
6. **Shift+X** oculta a janela do orb.

O app fica na **bandeja do sistema**. A janela transparente com o orb não precisa ficar sempre visível.

---

## Arquitetura

```
┌─────────────────────────────────────────────────────────────┐
│  Windows — bandeja do sistema                               │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  Tauri (Rust)                                           ││
│  │  • Captura de áudio (cpal)                              ││
│  │  • Cliente HTTP Whisper (STT)                           ││
│  │  • Cliente LLM streaming + tool calling                   ││
│  │  • LLM on-demand (swap Llama ↔ Qwen, XTTS gerido)      ││
│  │  • Fast-path (comandos sem LLM)                         ││
│  │  • Cliente XTTS / Chatterbox / Windows TTS              ││
│  │  • 53 ferramentas (tools) + fast-path ampliado          ││
│  │  • RAG (SQLite + embeddings)                            ││
│  └───────────────────────┬─────────────────────────────────┘│
│                          │ eventos + invoke                  │
│  ┌───────────────────────▼─────────────────────────────────┐│
│  │  React — orb, chat de texto, bolhas, configurações      ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### STT no Windows

Transcrição via servidor **whisper.cpp** (HTTP), rotas estilo OpenAI — não há STT embutido via `whisper-rs` neste alvo.

### TTS

| Modo | Perfil `start-all` | Backend |
|------|-------------------|---------|
| **XTTS v2** (recomendado) | `voice-xtts-cuda-partial`, `voice-xtts-cuda`, `voice-xtts-cpu`, … | [`xtts-api-server/`](xtts-api-server/) — clonagem PT-BR |
| **Chatterbox** | `voice-chatterbox`, `voice-chatterbox-cpu`, … | [`chatterbox-tts-api/`](chatterbox-tts-api/) |
| **Windows SAPI** | `voice-fast`, `balanced` | Sem servidor Python |

A URL do TTS no app continua em **Configurações → chatterbox_url** (`http://localhost:8005`); o Rust envia `model: "xtts"` ou usa Chatterbox conforme `DEXTER_TTS_MODE` definido pelo launcher.

Fallback: se o servidor TTS falhar, o app usa **voz nativa do Windows**.

### LLM on-demand (voz vs chat)

| Modo | Modelo | Porta | Quando |
|------|--------|-------|--------|
| **Voz** | Llama 3.1 8B (perfis XTTS) ou GGUF do perfil | 8080 | Push-to-talk, orb |
| **Texto** | Qwen 3.6 35B | 8084 | Janela de chat (**Shift+T**) |

Ao abrir o chat, o Rust **mata Llama + XTTS**, sobe **Qwen** e libera VRAM. Ao fechar, **repõe XTTS + Llama** (a primeira carga do XTTS pode levar 1–2 min). A UI mostra eventos `llm_swap_started` / `llm_swap_done`.

Implementação: `src-tauri/src/llm_ondemand.rs`. Variáveis repassadas por `start-all.ps1` (`LLM_VOICE_*`, `LLM_TEXT_*`, `XTTS_*`).

### Visão on-demand

**Qwen2.5-VL 3B** em `:8083` — o app sobe o `llama-server` na primeira screenshot e desliga após ~5 min ocioso (CPU-only por padrão, zero VRAM fixa). Download: `download-vision-model.ps1`.

### TTS em fluxo

1. O LLM envia tokens em fluxo.
2. Detecção de **fim de frase** (`.`, `!`, `?` + espaço) corta o texto.
3. Cada sentença vai ao TTS (chunks até `DEXTER_TTS_MAX_CHUNK_CHARS`, padrão 140; perfis XTTS usam 260).
4. O frontend enfileira e reproduz os pedaços em ordem.

Variáveis: `DEXTER_TTS_MAX_CHUNK_CHARS`, `DEXTER_TTS_SPLIT_COMMA` (0 = só fim de frase).

---

## Serviços e portas

| Serviço | Porta | Script / notas |
|---------|-------|----------------|
| LLM voz | 8080 | `llama-server` — Llama 8B nos perfis `voice-xtts*` |
| Whisper | 8081 | `whisper-server` |
| Embeddings | 8082 | BGE-M3 — `-NoEmbedding` para desligar |
| Visão | 8083 | Qwen2.5-VL — on-demand |
| LLM texto | 8084 | Qwen — on-demand ao abrir chat |
| TTS | 8005 | XTTS ou Chatterbox |
| Vite (dev) | 1420 | `npm run dev` |

---

## Ferramentas disponíveis

**53 ferramentas** expostas ao LLM (tool calling), mais **fast-path** para comandos frequentes sem round-trip ao modelo. Ligue ou desligue cada uma em **Configurações → Ferramentas**.

Implementação: `src-tauri/src/tools.rs`, `file_tools.rs`, `system_tools.rs`, `notification_tools.rs`, `media_controls.rs` e registro em `voice.rs`. Roteamento por categoria: `context_modifier.rs`.

### Base (17)

| Ferramenta | Função |
|------------|--------|
| `get_current_time` | Data e hora atuais |
| `launch_desktop_app` / `close_desktop_app` | Abrir/fechar apps no Windows |
| `list_running_apps` | Apps em execução |
| `run_command` | PowerShell no sandbox |
| `read_clipboard` | Ler área de transferência |
| `adjust_system_volume` | Volume do sistema (multimídia simulada) |
| `control_media_playback` | Play/pause/próximo/anterior (SMTC) |
| `play_music_query` | Música por nome (biblioteca local → web) |
| `play_local_music_playlist` | Playlist M3U por artista/pasta |
| `native_music_library_shuffle_play` | Biblioteca inteira em aleatório |
| `play_full_local_music_library` | Varredura/M3U da biblioteca completa (pedido explícito) |
| `take_screenshot` | Captura e descrição (visão on-demand) |
| `open_url` | Abrir URL no navegador |
| `fetch_fx_quote` | Cotação de moedas (ex.: dólar, euro) |
| `fetch_weather` | Previsão do tempo |
| `web_fetch` | Texto de página HTTP(S) |
| `search_knowledge` | Busca semântica na base local (RAG) |

### Tier 1 — clipboard, sistema, arquivos, notificações (+9)

| Ferramenta | Função |
|------------|--------|
| `write_clipboard` | Escrever na área de transferência |
| `get_active_window` | Janela em foco |
| `system_info` | RAM, CPU, bateria, disco |
| `schedule_notification` | Lembrete em X segundos ou horário |
| `clipboard_history` | Histórico e restauração do clipboard |
| `search_files` | Buscar arquivos por nome/padrão |
| `get_recent_files` | Arquivos abertos recentemente |
| `read_file` / `write_file` | Ler/criar arquivos (com sandbox de caminho) |

### Tier 2 — janelas, sessão, visão, áudio (+15)

| Ferramenta | Função |
|------------|--------|
| `manage_processes` | Listar/encerrar processos |
| `lock_screen` | Bloquear a sessão |
| `open_folder` | Abrir pasta no Explorer |
| `set_wallpaper` | Alterar papel de parede |
| `get_open_windows` | Listar janelas abertas |
| `toggle_do_not_disturb` | Modo foco / não perturbe |
| `read_selected_text` | Texto selecionado (acessibilidade) |
| `translate_selection` | Traduzir seleção ou clipboard |
| `paste_to_active_window` | Colar texto na janela ativa |
| `session_notes` | Anotações locais da sessão |
| `diff_clipboard` | Diferença entre clipboard atual e anterior |
| `ocr_image` | OCR na tela ou imagem |
| `transcribe_audio_file` | Transcrever áudio local (Whisper) |
| `audio_device_switch` | Listar/trocar dispositivo de áudio |
| `run_powershell_script` | Executar script `.ps1` (sandbox) |

### Tier 3 — rede, produtividade, captura (+8)

| Ferramenta | Função |
|------------|--------|
| `get_network_info` | IP, gateway, interfaces |
| `send_keys` | Teclas e atalhos (Enter, Ctrl+S, digitar texto) |
| `set_audio_volume_app` | Volume por aplicativo |
| `calendar_events` | Eventos do calendário (Outlook/agenda) |
| `send_email` | Rascunho de e-mail |
| `snippet_library` | Snippets salvos e colagem |
| `take_screenshot_region` | Captura de região da tela |
| `watch_file` | Avisar quando um arquivo mudar |

### Tier 4 — automação e manutenção (+3)

| Ferramenta | Função |
|------------|--------|
| `ui_automation` | Clique, scroll, duplo clique por coordenadas |
| `disk_cleanup` | Espaço em disco, temp, lixeira |
| `image_generation` | Gerar imagem por prompt (serviço local configurável) |

### Fast-path (sem LLM)

Em **`fast_path.rs`**, frases em português que casam com padrões executam a ação direto — menor latência que tool calling. Cobre, entre outros: hora/data, **calculadora** (`calculator`, exclusivo do fast-path), clipboard, apps, mídia/volume, cotação, clima, busca web rápida, arquivos comuns e tradução. Se não houver match, o fluxo segue para o LLM com as 53 ferramentas acima.

---

## RAG (base de conhecimento)

1. **Ingestão** — chunks + embedding via **`/embedding`**.
2. **Busca** — similaridade de cosseno no SQLite.
3. **UI** — **Configurações → Conhecimento**.

URL de embeddings opcional: se vazia, usa a URL do LLM. Servidor dedicado (BGE-M3 em `:8082`): `download-bge-m3.ps1` ou `download-bge-m3-hf.ps1`.

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

Durante swap de modelo (chat ↔ voz), a UI pode indicar carregamento do stack de voz.

---

## Configurações

Menu da **bandeja do sistema** → **Configurações**.

### URLs e modelos

| Campo | Exemplo | Uso |
|-------|---------|-----|
| Whisper URL | `http://localhost:8081` | STT |
| URL do LLM | `http://localhost:8080` | Voz (e texto se Qwen não estiver ativo) |
| URL de embeddings | `http://localhost:8082` | RAG (opcional) |
| URL de visão | `http://localhost:8083` | Screenshots |
| chatterbox_url | `http://localhost:8005` | TTS (XTTS ou Chatterbox) |
| chatterbox_voice | `dexter-ptbr` | Voz clonada |
| Modelo de chat / embedding / visão | Nomes no `llama-server` | Conforme seus GGUF |

### Atalhos (padrões — personalizáveis)

| Atalho | Ação |
|--------|------|
| **Shift+Z** (segurar) | Push-to-talk |
| **Shift+X** | Ocultar orb |
| **Shift+C** | Limpar conversa |
| **Shift+T** | Abrir/fechar chat de texto (dispara LLM on-demand) |
| **Ctrl+Comma** | Configurações |

No `config.json` de exemplo na raiz do repositório também há atalhos numéricos (`Ctrl+Numpad1` chat, etc.) — o app usa os valores salvos em `%APPDATA%\voice-assistant\config.json`.

### Persistência

`%APPDATA%\voice-assistant\config.json`

---

## Pré-requisitos de build

- **Rust** (edition 2021), **Node.js**, **MSVC** / Visual Studio Build Tools.
- **Python 3.12+** e **uv** para XTTS/Chatterbox.
- Serviços locais para voz de ponta a ponta:
  - **llama-server** (Llama 8B voz + opcional Qwen texto, BGE-M3, Qwen-VL).
  - **whisper-server**.
  - **xtts-api-server** ou **chatterbox-tts-api** na porta 8005.

---

## Build e execução

```powershell
npm install
npm run tauri dev    # desenvolvimento
npm run tauri build  # instalador / release
```

### Launcher oficial

```powershell
.\start-all.ps1
# Perfil padrão: voice-xtts-cuda-partial
# Menu interativo se omitir -Profile

.\start-all.ps1 -Profile voice-chatterbox -ForceRestartServices
.\start-all.ps1 -Profile voice-xtts-safe -EasyOnRam
.\start-all.ps1 -NoEmbedding -NoWhisper -WhisperTiny -StartupStaggerSec 5
```

Edite o bloco **CONFIGURACAO** no topo de `start-all.ps1`: caminhos de `llama-server`, `whisper-server`, GGUF, pastas XTTS/Chatterbox.

### Registro de voz (XTTS)

```powershell
# Com XTTS rodando em :8005
.\register-voice.ps1
# Usa Clone_voz.mp3 → voz "dexter-ptbr" (pt)
```

### Sandbox / testes

```powershell
.\teste.ps1 -Profile voice-xtts-cuda-partial   # mesmo catálogo de perfis; não altera start-all.ps1
.\bench-fast-path.ps1
.\bench-voice-xtts-cpu-measure.ps1
```

Scripts de benchmark e métricas adicionais em `Documentação/scripts/` (pasta local).

---

## Perfis `start-all.ps1`

| Perfil | TTS | LLM GPU | Notas |
|--------|-----|---------|-------|
| **voice-xtts-cuda-partial** | XTTS CUDA | `-ngl 28` | **Padrão** — RTX 3070 8 GB |
| voice-xtts-cuda | XTTS CUDA | `-ngl 20` | Mais VRAM para TTS |
| voice-xtts / voice-xtts-cpu | CUDA / CPU | 28/99 | Alias ou baseline CPU |
| voice-xtts-safe | XTTS CPU | `-ngl 99`, ctx 4096 | PC trava ao subir CUDA |
| voice-chatterbox | Chatterbox GPU | `-ngl 28` | Clone Chatterbox |
| voice-chatterbox-cpu | Chatterbox CPU | `-ngl 99` | Menos disputa VRAM |
| voice-fast / balanced | Windows SAPI | `-ngl 99` | Sem Python TTS |
| quality | Chatterbox GPU | ctx 16k | Mais exigente |

Perfis `voice-xtts*` trocam automaticamente para **`LLM_MODEL_VOICE`** (Llama 8B) quando o ficheiro existe.

---

## Pilha tecnológica

| Camada | Tecnologia |
|--------|------------|
| Shell | Tauri 2 |
| Backend | Rust (reqwest, cpal, SQLite, …) |
| Frontend | React 19, TypeScript, Vite, Tailwind |
| STT | whisper.cpp `whisper-server` |
| LLM voz | llama.cpp — Llama 3.1 8B |
| LLM texto | llama.cpp — Qwen 35B (on-demand) |
| TTS | XTTS v2 (Coqui) ou Chatterbox |
| Embeddings | BGE-M3 / `/embedding` |
| Visão | Qwen2.5-VL 3B (on-demand) |
| Atalhos | plugin global-shortcut |

---

## Estrutura de pastas

```
dexter/
├── src/                      # React — orb, chat, configurações
├── src-tauri/src/
│   ├── lib.rs                # Tray, atalhos, orquestração
│   ├── voice.rs              # STT / LLM / TTS streaming + registro de tools
│   ├── llm_ondemand.rs       # Swap Llama ↔ Qwen, gestão XTTS
│   ├── fast_path.rs          # Comandos sem LLM (incl. calculadora)
│   ├── tools.rs              # Tools base, mídia, web, visão
│   ├── file_tools.rs         # Arquivos (busca, leitura, escrita)
│   ├── system_tools.rs       # Sistema, janelas, rede, UI, disco
│   ├── notification_tools.rs # Notificações e clipboard history
│   ├── context_modifier.rs   # Roteamento por categoria de tool
│   ├── media_controls.rs     # SMTC, volume, biblioteca musical
│   ├── rag.rs                # Base de conhecimento
│   └── sandbox.rs            # Comandos e caminhos restritos
├── xtts-api-server/          # API XTTS v2 (produção)
├── chatterbox-tts-api/       # API Chatterbox (alternativa)
├── Documentação/             # INDEX.md, planos, métricas (local, .gitignore)
├── start-all.ps1             # Launcher oficial
├── teste.ps1                 # Launcher sandbox
├── register-voice.ps1
├── download-bge-m3.ps1
├── download-vision-model.ps1
└── package.json
```

Documentação interna: **`Documentação/INDEX.md`** (convenção `PLANO-*`, `RELATORIO-*`, `METRICA-*`, etc.).

---

## Documentação relacionada

| Tópico | Ficheiro (em `Documentação/`) |
|--------|-------------------------------|
| Plano voz rápida | `planos/PLANO-VOZ-assistente-rapido-suave.md` |
| LLM on-demand | `planos/PLANO-LLM-on-demand-voz-llama-chat-qwen.md` |
| Métricas Fase 0 | `metricas/METRICA-VOZ-performance-fase0-2026-05-18.md` |
| Chatterbox PT-BR | `guias/GUIA-TTS-chatterbox-clonagem-ptbr.md` |
| Whisper 501 | `guias/GUIA-WHISPER-stt-configuracao-erro-501.md` |

---

## Licença

Ver [**LICENSE**](LICENSE) (Apache 2.0).
