# Ferramentas de abrir e fechar aplicativos no Dexter

Este documento resume o que foi implementado e ajustado para o assistente de voz **abrir** e **fechar** aplicativos no Windows, integrado ao fluxo de tool calling do LLM (llama.cpp / API compatível com OpenAI).

## Visão geral

Foram adicionadas duas ferramentas no backend Rust (`src-tauri`), controladas pela mesma opção em **Settings → Tools** (“Launch / Close Apps”):

| Ferramenta | Função |
|------------|--------|
| `launch_desktop_app` | Abre um aplicativo pré-definido (whitelist). |
| `close_desktop_app` | Encerra processos associados aos mesmos IDs (whitelist). |

Ambas **não** passam pelo sandbox do `run_command`: a lógica é fixa em código, reduzindo risco de comandos arbitrários.

### IDs suportados (`app`)

Os mesmos para abrir e fechar:

`cursor`, `vscode`, `terminal`, `chrome`, `edge`, `discord`, `obs`, `snipping_tool`, `media_player`, `excel`, `word`, `powerpoint`, `outlook`

Aliases aceitos no código (normalização): por exemplo `vs_code`, `ppt`, `groove`, `capture`, etc.

---

## Arquivos alterados

| Arquivo | Alterações |
|---------|------------|
| `src-tauri/src/media_controls.rs` | SMTC (`control_media_playback`) e `SendInput` para volume (`adjust_system_volume`). |
| `src-tauri/src/tools.rs` | `launch_desktop_app`, caminhos absolutos e `cmd /c start`, Office/Groove/Chrome/Edge, etc.; `close_desktop_app`, `taskkill`, fallback PowerShell, mídia por caminho em `WindowsApps`. |
| `src-tauri/src/voice.rs` | Definições JSON das funções em `build_tools` (incl. mídia); `max_tokens` dinâmico (ferramentas / histórico com `tool`); texto do `run_command` orientando uso destas ferramentas. |
| `src-tauri/src/lib.rs` | `ToolsConfig.launch_desktop_app`, `ToolsConfig.media_controls`; `execute_tool` para apps e mídia; **persistência do transcript de ferramentas** em `AppState.messages` (assistant com `tool_calls` + mensagens `tool`). |
| `src/App.tsx` | Toggle e labels (“Launch / Close Apps”, “Media & volume”, “Abrindo aplicativo”, “Fechando aplicativo”, “Controlando reprodução”, “Ajustando volume”, export M3U por varredura). |

Para **biblioteca local / reprodutor / playlists / UI Automation**, ver a secção **«Biblioteca local, playlists e Reprodutor Multimédia (Windows)»** mais abaixo neste documento.

---

## Abrir aplicativos (`launch_desktop_app`)

### Problema resolvido (PATH)

O processo do app Tauri herda um **PATH mais enxuto** que o PowerShell do usuário. Comandos como `cursor` ou `code` podem falhar se não estiverem no PATH do processo; `wt.exe` costumava funcionar porque está em `%LOCALAPPDATA%\Microsoft\WindowsApps`.

**Solução:** usar **caminhos absolutos** derivados de variáveis de ambiente (`ProgramFiles`, `ProgramFiles(x86)`, `LOCALAPPDATA`) e, quando faz sentido, **`cmd /c start "" "<path>"`** para `.exe` e `.cmd`.

### Mapeamentos principais

- **Cursor:** `Cursor.exe`, `cursor.cmd` sob Program Files.
- **VS Code:** `%LOCALAPPDATA%\Programs\Microsoft VS Code\Code.exe` e `code.cmd`.
- **Terminal:** `%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe`.
- **Chrome / Edge:** pastas padrão em Program Files + fallback por nome.
- **Discord:** `%LOCALAPPDATA%\Discord\Update.exe --processStart Discord.exe`.
- **OBS:** `obs-studio\bin\64bit\obs64.exe`.
- **Snipping Tool:** `SnippingTool.exe` em WindowsApps.
- **Reprodutor / Groove (pacote Zune):**  
  `explorer.exe` / PowerShell com  
  `shell:AppsFolder\Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic`
- **Office:** pastas `...\Microsoft Office\root\Office16`, `Office16` direto, `Office15`, em PF e PF (x86).

---

## Fechar aplicativos (`close_desktop_app`)

### Estratégia em três fases

1. **`taskkill /IM <exe> /T /F`** para uma lista de imagens por ID (várias variantes por app).
2. Se nada for encerrado: **PowerShell** `Get-Process -Name <base>` → `Stop-Process -Force` (nome **sem** `.exe`).
3. Só para **`media_player`:** script que encerra processos cujo **Path** está em `\WindowsApps\` e contém trechos como `zunemusic`, `groove`, `media.player`, `music.ui`.

### Exemplos de processos por ID

- **vscode:** `Code.exe`, `Code - Insiders.exe`
- **terminal:** `WindowsTerminal.exe`, `WindowsTerminalPreview.exe`, `wt.exe`
- **discord:** inclui PTB, Canary, Development
- **office:** `EXCEL.EXE` e `excel.exe` (e análogos)
- **mídia:** `GrooveMusic.exe`, `Microsoft.Media.Player.exe`, `Music.UI.exe`

**Removido:** `WinStore.App.exe` (genérico demais — poderia afetar a Microsoft Store).

### Limitações

Apps **UWP** hospedados só em `ApplicationFrameHost.exe` podem **não** ter processo dedicado; fechar só esse app sem fechar outros hospedados nem sempre é possível só com `taskkill`.

---

## Histórico de chat e ferramentas (multi-turn)

### Problema

Só gravava no estado final o **texto falado** do assistente. Faltava a sequência **assistant (tool_calls) → tool → assistant (texto)**, exigida por muitos modelos para **nova** chamada de ferramentas na mesma conversa.

### Solução

- Ao executar ferramentas **nativas** (não XML), as mensagens equivalentes são gravadas em **`AppState.messages`** em paralelo ao vetor `all_msgs` usado na pipeline.
- Texto de follow-up no fluxo **XML** deixou de dizer “never call tools again”; mensagens espelhadas no estado quando aplicável.
- **`max_tokens` em `chat_streaming`:**
  - Com `tools` na requisição: **1024** (evita truncar JSON de `tool_calls`).
  - Sem `tools`, mas histórico já contém role `tool`: **512** (resposta final após ferramentas).
  - Conversa só texto: **220** (respostas curtas para voz).

---

## Referência rápida (PowerShell no seu PC)

Comandos úteis que o usuário validou no ambiente:

- `where.exe cursor code wt SnippingTool`
- Reprodutor Groove:  
  `Start-Process "shell:AppsFolder\Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"`

---

## Configuração (Settings)

- **Launch / Close Apps** (`tools.launch_desktop_app` em `config.json`): habilita **abrir e fechar** os apps da whitelist (uma única opção para ambas as ferramentas).
- **Media & volume** (`tools.media_controls`): habilita **controle da sessão de mídia do sistema** e **volume master** (ver secção abaixo).

---

## Música e vídeo do sistema (play/pause, faixas, volume)

Controle da sessão de mídia **ativa no Windows** (Spotify, Edge/Chrome com vídeo, Groove, etc.) via **SMTC** (System Media Transport Controls), mais teclas multimídia para volume master.

| Ferramenta | Função |
|------------|--------|
| `control_media_playback` | `play`, `pause`, `toggle` (play/pause), `next`, `previous`, `stop`, `status` (faixa, artista, estado). |
| `adjust_system_volume` | `up` / `down` (parâmetro opcional `steps`, padrão 3) ou `mute_toggle`. |

**Arquivos:** `src-tauri/src/media_controls.rs`; definições em `voice.rs`; `execute_tool` e `ToolsConfig.media_controls` em `lib.rs`; toggle em `App.tsx`.

**Limitações:** só funciona se o aplicativo expuser controles ao Windows; sem sessão ativa, comandos ou `status` podem falhar. O volume usa teclas multimídia (cada passo ~2% em muitos PCs). Não controla sliders internos de cada app.

---

## Biblioteca local, playlists e Reprodutor Multimédia (Windows)

Além de `launch_desktop_app` com `media_player` / `groove`, o Dexter expõe ferramentas para **música local**, **YouTube** (`play_music_query`) e **shuffle da biblioteca indexada no app**. Esta secção resume o que foi consolidado quanto a **custo (varredura de disco)** e **automação da UI** do Reprodutor Multimédia.

### Política de custo e encaminhamento

| Pedido do utilizador | Ferramenta esperada | Notas |
|----------------------|---------------------|--------|
| Tocar / embaralhar **toda** a biblioteca de música do PC, “shuffle tudo”, equivalentes | `native_music_library_shuffle_play` | **Sem** varredura de disco nem M3U grande; usa o fluxo interno do reprodutor («Biblioteca de músicas» + botão de ordem aleatória). |
| Várias faixas locais por **artista/pasta** (palavras nos caminhos) | `play_local_music_playlist` | Gera um **M3U** com limite de faixas (ex.: 4000) e abre no programa padrão. |
| Frases que significam **biblioteca inteira** passadas por engano em `play_local_music_playlist` | Redirecionamento no Rust | `is_entire_local_library_request` detecta palavras/frases (“tudo”, “biblioteca inteira”, “todas as músicas”, “minha biblioteca”, “ordem aleatoria e reproduzir”, etc.) e chama `native_music_library_shuffle_play` em vez de varrer disco. |
| **Exportar** lista gigante por **varredura** de disco (M3U completo, VLC, etc.) | `play_full_local_music_library` | **Só** se o utilizador pedir explicitamente criar/exportar esse ficheiro; ver gate abaixo. |

### `native_music_library_shuffle_play`

- **Fluxo:** `launch_desktop_app("media_player")` → espera inicial (~**4,5 s**) → até **5** execuções do script PowerShell de UI Automation, com ~**2,8 s** entre tentativas se falhar.
- **Objetivo:** clicar em «**Biblioteca de músicas**» (ou equivalente) e no botão cuja etiqueta visível em PT-BR costuma ser **`Ordem aleatoria e reproduzir`** (muitas builds **sem** acento em “aleatoria”).
- **Implementação Rust:** `tools.rs` (`native_music_library_shuffle_play`, `run_media_player_shuffle_automation`).
- **Script:** `src-tauri/scripts/media-player-library-shuffle.ps1`, embutido com `include_str!` ao compilar.

### Script PowerShell — UI Automation (`media-player-library-shuffle.ps1`)

- **Assemblies:** `UIAutomationClient`, `UIAutomationTypes`.
- **Janela:** localiza o processo do pacote em `\WindowsApps\` (Zune/Groove/Media.Player/Music.UI) ou pelo título; opcionalmente usa **`Windows.UI.Core.CoreWindow`** como raiz de pesquisa em vez da janela externa.
- **Etiquetas:** junta **Name**, **HelpText**, **AutomationId**, **LocalizedControlType** para casar texto que aparece em filhos (ex.: `Text` dentro de `Button`).
- **Ativação:** `LegacyIAccessiblePattern.DoDefaultAction`, `InvokePattern`, `SelectionItemPattern`, `TogglePattern`, `ExpandCollapsePattern`; se falhar no nó, **sobe ancestrais** (`TreeWalker.RawViewWalker`) até ~18 níveis.
- **Shuffle:** regex inclui explicitamente `ordem aleatoria e reproduzir` e variantes acentuadas / inglês.
- **Foreground:** `ShowWindow` + `SetForegroundWindow` no HWND da janela.
- **Exit codes:** `0` sucesso; `10` janela não encontrada; `11` não encontrou os controlos; `2` falha ao carregar assemblies.

### `play_full_local_music_library` — gate obrigatório

- No **`execute_tool`** (`lib.rs`), a ferramenta **só corre** se o JSON incluir **`explicit_m3u_export_request: true`**.
- Se vier `false` ou omitido, devolve mensagem orientando o modelo a usar **`native_music_library_shuffle_play`** para reprodução normal de biblioteca inteira (sem custo de scan).
- Em **`voice.rs`**, o schema da função marca **`explicit_m3u_export_request`** como **`required`**, para o modelo não omitir o campo.

### `play_local_music_playlist`

- Continua a gerar M3U por palavras-chave em pastas/ficheiros; **não** deve ser usada pelo modelo para “toda a biblioteca” — isso vai para `native_music_library_shuffle_play`.
- Redirecionamento automático quando o argumento `artist` coincide com frases de biblioteca completa (`tools.rs`, `is_entire_local_library_request`).

### Prompt do sistema e descrições das tools

- **`lib.rs`** (`VoiceConfig::default().system_prompt`): instruções em PT-BR para biblioteca inteira → sempre reprodutor nativo + botão «Ordem aleatoria e reproduzir»; M3U gigante só com pedido explícito e parâmetro na ferramenta.
- **`voice.rs`** (`build_tools`): textos de `run_command`, `control_media_playback`, `play_local_music_playlist`, `play_full_local_music_library`, `native_music_library_shuffle_play` alinhados à mesma política.
- **Nota:** instalações que já gravaram `config.json` com `system_prompt` personalizado podem não incluir estas regras até o utilizador atualizar o texto ou repor o default da aplicação.

### Interface (labels)

- **`App.tsx`:** label da ferramenta `play_full_local_music_library` alterada para algo como **«Exportar M3U (varredura)»**, para não sugerir uso como “tocar biblioteca” no dia a dia.

### Arquivos envolvidos (resumo desta linha de trabalho)

| Arquivo | Papel |
|---------|--------|
| `src-tauri/src/tools.rs` | `native_music_library_shuffle_play`, `run_media_player_shuffle_automation`, `is_entire_local_library_request`, `play_local_music_playlist`, `play_full_local_music_library`. |
| `src-tauri/src/lib.rs` | Gate `explicit_m3u_export_request`; texto default do `system_prompt` sobre música. |
| `src-tauri/src/voice.rs` | Schema e descrições das ferramentas de música. |
| `src-tauri/scripts/media-player-library-shuffle.ps1` | Automação UIA do Reprodutor Multimédia. |
| `src/App.tsx` | Labels de estado ao chamar tools (incl. export M3U). |

---

## Checklist de implementação (resumo)

- [x] Ferramenta `launch_desktop_app` com whitelist e caminhos Windows.
- [x] Ajuste PATH / `cmd start` / Office / Groove shell URI.
- [x] Persistência assistant + tool no histórico para multi-turn.
- [x] `max_tokens` conforme ferramentas e histórico.
- [x] Ferramenta `close_desktop_app` com `taskkill` + PowerShell + fallback de mídia.
- [x] Listas de processo ampliadas e mensagens de erro mais claras.
- [x] `control_media_playback` e `adjust_system_volume` (Windows), opção `media_controls`.
- [x] `native_music_library_shuffle_play` com retries e script UIA embutido (`media-player-library-shuffle.ps1`).
- [x] Política “biblioteca inteira = reprodutor nativo”; redirecionamento em `play_local_music_playlist` quando o pedido é biblioteca completa.
- [x] `play_full_local_music_library` condicionada a `explicit_m3u_export_request` no executor + campo obrigatório no schema LLM.
- [x] Prompts (`system_prompt`, descrições em `voice.rs`) e label na UI para export M3U por varredura.

---

*Documento gerado para o projeto Chronos AI / Dexter — ferramentas desktop no assistente de voz.*
