# Chronos AI v2 — Plano de Correção e Embedding

> **Data:** 11/05/2026
> **Objetivo:** Corrigir problemas encontrados, integrar servidor de embedding dedicado (BGE-M3) e implementar melhorias pendentes.
> **Instrução para LLM:** Execute na ordem. Após cada passo Rust, rode `cargo check` no diretório `dexter/src-tauri/`. Após cada passo frontend, rode `npm run build` no diretório `dexter/`.

---

## 1. Análise do Estado Atual

### 1.1 O que foi implementado corretamente (Lotes 1-3)

| Feature | Status |
|---------|--------|
| `VoiceConfig` com 10 novos campos | ✅ |
| `list_models` (GET /v1/models) | ✅ |
| `get_default_config` | ✅ |
| `export_conversation` | ✅ |
| `embed_url` + fallback para `llm_url` no RAG | ✅ |
| Pipeline usa `temperature`, `enable_thinking`, `response_style` | ✅ |
| `ModelSelect` dropdown na UI | ✅ |
| `ConfigTab` com toggles e sliders | ✅ |
| Badge do modelo no Orb | ✅ |
| Atalhos `Shift+C`, `Shift+T`, `Ctrl+T` | ✅ |
| `ChatView` com streaming, markdown, code blocks, export | ✅ |
| Beep do microfone (`audio_feedback`) | ✅ |
| `tts_volume` no Windows SAPI | ✅ |

### 1.2 Problemas encontrados

| # | Problema | Severidade | Causa raiz |
|---|----------|-----------|------------|
| P1 | **Perfil de personalidade não altera o system prompt de voz** | Alta | O select de personalidade no frontend apenas armazena o valor `personality` no config. O backend (`voice.rs` `chat_streaming`) SEMPRE usa `config.system_prompt`, nunca consulta `config.personality` para injetar um preset. O `system_prompt` textarea permanece com o prompt padrão de voz. |
| P2 | **Não há UI para configurar atalhos** | Média | Os atalhos `Shift+Z`, `Shift+X`, `Shift+C`, `Shift+T` são hardcoded no `lib.rs`. O plano original previa campos `shortcut_talk`, `shortcut_hide`, `shortcut_clear`, `shortcut_chat` mas eles nunca foram adicionados ao `VoiceConfig`. |
| P3 | **Servidor de embedding dedicado não configurado** | Alta | `start-all.ps1` não inicia um servidor llama.cpp separado para embeddings. O LLM atual tem `--embedding` (linha 342) o que significa que o mesmo servidor na porta 8080 serve embeddings — isso funciona mas carrega o modelo de 26B para embeddings, o que é desperdício. |
| P4 | **Modelo BGE-M3 não baixado** | Alta | O modelo de embedding recomendado (BGE-M3 GGUF) não está presente no disco. |
| P5 | **`voice.rs`: personalidade só existe no modo texto** | Alta | Os presets `"coder"` e `"creative"` só existem na função `chat_streaming_text` (~linha 1137). A função `chat_streaming` (voz) nunca os consulta. |
| P6 | **`system_prompt_text` padrão é string vazia** | Baixa | Quando vazio, `chat_streaming_text` faz fallback para presets baseados em `personality`, o que funciona. Mas seria melhor ter um default explícito para texto. |

---

## 2. Plano de Correções

### Correção 1 — Personalidade afeta o system prompt de voz

**Problema:** Ao selecionar "Programador" ou "Criativo" no dropdown de perfil, o `system_prompt` (voz) não muda. O usuário espera que o prompt de voz reflita o perfil escolhido.

**Solução:** No frontend, quando o usuário muda o perfil (e NÃO está no modo "custom"), o `system_prompt` textarea deve ser substituído por um preset apropriado para voz. No backend, a função `chat_streaming` (voz) também deve verificar `config.personality` quando `system_prompt` estiver vazio ou quando o perfil não for "custom".

**Arquivo:** `dexter/src/App.tsx` — `ConfigTab`

**Passo 1.1 — Adicionar presets de system prompt de voz**

Adicionar uma constante com os presets logo acima da função `ConfigTab`:

```typescript
const VOICE_PRESETS: Record<string, string> = {
  default: `Você é um assistente de voz rodando no desktop do usuário. A conversa acontece inteiramente por voz — o usuário fala no microfone, a fala é transcrita via Whisper (STT), enviada como mensagem para você, e sua resposta é convertida de volta em fala via Chatterbox Turbo (TTS) e reproduzida nos alto-falantes. Você pode ouvir o usuário e ele pode ouvir você — trate como uma conversa falada natural. Se perguntarem "você me ouve" a resposta é sim.\n\nIMPORTANTE: Responda SEMPRE em português do Brasil, independentemente do idioma da pergunta.\n\nMantenha respostas curtas e conversacionais — 2-3 frases no máximo. Sem markdown, sem blocos de código, sem bullet points, sem listas numeradas, sem formatação especial. Escreva exatamente como falaria em voz alta. Evite dois-pontos nas respostas pois causam pausas estranhas no TTS.\n\nVocê pode expressar emoções naturalmente usando estas tags paralinguísticas no meio da fala — use com moderação e só quando realmente encaixar:\n[laugh] [chuckle] [sigh] [gasp] [cough] [clear throat] [sniff] [groan] [shush]\nExemplo — "Nossa, isso é muito engraçado [laugh] não esperava isso de jeito nenhum."\nNÃO exagere. A maioria das respostas não precisa de nenhuma tag. Use só quando um humano genuinamente faria aquele som.\n\nQuando decidir usar uma ferramenta, SEMPRE diga o que vai fazer antes em uma frase curta e natural antes de chamar a ferramenta. Por exemplo — "Deixa eu olhar sua tela" antes de tirar screenshot, "Vou procurar isso na web" antes de buscar uma página, "Deixa eu ver que horas são" antes de checar o horário, "Um segundo, vou rodar esse comando" antes de executar um comando. Para música: se não houver player ou aba com vídeo aberta, abra com launch_desktop_app media_player ou open_url no YouTube ou Spotify antes de pedir play no control_media_playback. Se o usuário pedir uma música pelo NOME da faixa ou artista, use play_music_query com o título — nunca use open_url para YouTube nesse caso. Essa ferramenta varre primeiro a pasta Música do Windows, pastas equivalentes, as pastas que o usuário configurou nas Configurações em Pastas de música e só depois tenta o YouTube. Se pedirem tocar ou embaralhar TODA a biblioteca de música do PC, tudo de uma vez, ou equivalente, use SEMPRE native_music_library_shuffle_play — abre o Reprodutor Multimédia e usa o fluxo interno «Biblioteca de músicas» e o botão «Ordem aleatoria e reproduzir» (texto visível na UI, sem acento em aleatoria). Nunca varra disco nem gere M3U gigante para isso. play_local_music_playlist só quando quiserem várias faixas locais por um artista ou pasta concreta e aceitarem lista M3U para esse caso. play_full_local_music_library só se pedirem explicitamente exportar ou criar arquivo de lista M3U enorme por varredura — e a ferramenta exige confirmação no parâmetro; caso contrário não chame. Assim o usuário ouve o que está acontecendo em vez de esperar em silêncio.`,

  coder: `Você é um assistente de voz para programação rodando no desktop do usuário. Responda SEMPRE em português do Brasil. Seu foco é ajudar com código, debugging, terminal e ferramentas de desenvolvimento.\n\nMantenha respostas curtas e conversacionais — 2-4 frases no máximo. Sem markdown pesado, mas você PODE mencionar nomes de funções, comandos e arquivos. Evite blocos de código longos na fala.\n\nQuando decidir usar uma ferramenta, SEMPRE diga o que vai fazer antes em uma frase curta. Prefira run_command, launch_desktop_app, take_screenshot e web_fetch para tarefas de desenvolvimento.`,

  creative: `Você é um assistente de voz criativo rodando no desktop do usuário. Responda SEMPRE em português do Brasil. Você pode dar respostas mais elaboradas, fazer analogias, sugerir ideias fora da caixa e explorar múltiplas perspectivas.\n\nRespostas podem ser um pouco mais longas que o normal — 3-5 frases. Use um tom envolvente e inspirador. Evite markdown mas pode usar pausas naturais.\n\nQuando decidir usar uma ferramenta, SEMPRE diga o que vai fazer antes em uma frase curta e natural.`,
};

const TEXT_PRESETS: Record<string, string> = {
  default: `Você é um assistente de IA rodando no desktop do usuário. Responda em português do Brasil. Seja detalhado, use markdown para estruturar a resposta. Use blocos de código com syntax highlighting quando relevante. Você tem acesso a ferramentas para interagir com o sistema do usuário (captura de tela, comandos, busca na web, etc.).`,

  coder: `Você é um assistente de programação. Responda em português do Brasil. Use blocos de código com syntax highlighting quando relevante. Seja detalhado e técnico. Explique o raciocínio por trás das soluções. Use markdown para estruturar a resposta.`,

  creative: `Você é um assistente criativo. Responda em português do Brasil. Pense fora da caixa, ofereça múltiplas perspectivas. Respostas podem ser mais longas e elaboradas. Use markdown para estruturar a resposta.`,
};
```

> **NOTA SOBRE O SYSTEM PROMPT DEFAULT:** O valor acima é o mesmo que já está no `VoiceConfig::default()` no `lib.rs`. Se o texto não couber exatamente, use o valor já existente no código — o importante é a estrutura.

**Passo 1.2 — Modificar o onChange do select de perfil**

**Local:** Linha 386 do `App.tsx`, dentro de `ConfigTab`

**ANTES:**
```tsx
            onChange={(e) => setConfig({ ...config, personality: e.target.value })}
```

**DEPOIS:**
```tsx
            onChange={(e) => {
              const newPersonality = e.target.value;
              const updates: Partial<VoiceConfig> = { personality: newPersonality };
              // Se não for "custom", aplica o preset de voz e texto
              if (newPersonality !== "custom") {
                if (VOICE_PRESETS[newPersonality]) {
                  updates.system_prompt = VOICE_PRESETS[newPersonality];
                }
                if (TEXT_PRESETS[newPersonality]) {
                  updates.system_prompt_text = TEXT_PRESETS[newPersonality];
                }
              }
              setConfig({ ...config, ...updates });
            }}
```

**Passo 1.3 — Botão "Restaurar padrões" também deve restaurar os presets**

**Local:** Botão "Restaurar padrões" (linha 766-772)

O `get_default_config` do backend retorna o `VoiceConfig::default()` que já tem `personality: "default"` e o `system_prompt` padrão. Isso já resolve. Mas verifique se o frontend aplica corretamente — o spread `{ ...defaults, music_library_paths: defaults.music_library_paths ?? "" }` já deve funcionar.

**Passo 1.4 — Backend: `chat_streaming` (voz) também verificar `personality`**

**Arquivo:** `dexter/src-tauri/src/voice.rs` — função `chat_streaming`

**Local:** Onde o system prompt é montado (~linha 702-707)

**ANTES:**
```rust
    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".to_string(),
        content: serde_json::Value::String(config.system_prompt.clone()),
        tool_calls: None,
        tool_call_id: None,
    }];
```

**DEPOIS:**
```rust
    let voice_prompt = if !config.system_prompt.trim().is_empty() {
        config.system_prompt.clone()
    } else if config.personality == "coder" {
        "Você é um assistente de voz para programação. Responda SEMPRE em português do Brasil. Foco em código, debugging, terminal. Respostas curtas — 2-4 frases. Sem markdown pesado. Quando decidir usar uma ferramenta, diga o que vai fazer antes.".to_string()
    } else if config.personality == "creative" {
        "Você é um assistente de voz criativo. Responda SEMPRE em português do Brasil. Respostas mais elaboradas, 3-5 frases, tom envolvente. Quando decidir usar uma ferramenta, diga o que vai fazer antes.".to_string()
    } else {
        config.system_prompt.clone()
    };

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".to_string(),
        content: serde_json::Value::String(voice_prompt),
        tool_calls: None,
        tool_call_id: None,
    }];
```

> **NOTA:** O preset "default" não precisa ser duplicado aqui porque o `config.system_prompt` já contém o prompt padrão completo (do `VoiceConfig::default()` ou do que o usuário salvou). Os fallbacks "coder" e "creative" são curtos e servem apenas quando o `system_prompt` está vazio — o que não deve acontecer na prática porque o frontend agora preenche ao selecionar o perfil.

**Verificação:** `cargo check` + `npm run build`

---

### Correção 2 — UI para configurar atalhos

**Problema:** Não existe opção nas configurações para alterar os atalhos de teclado.

**Solução:** Adicionar campos de atalho ao `VoiceConfig` com defaults, permitir que o usuário os edite na UI, e registrar os atalhos dinamicamente.

> **NOTA DE COMPLEXIDADE:** O Tauri `global_shortcut` registra atalhos no `setup()` que roda uma vez. Trocar atalhos em runtime exigiria:
> 1. Armazenar os handles dos atalhos (`ShortcutId`)
> 2. No `set_config`, unregister os antigos e register os novos
> 3. Isso é complexo e propenso a race conditions
>
> **Abordagem recomendada (mais simples):** Adicionar os campos ao `VoiceConfig` e à UI, mas os atalhos são aplicados apenas no **próximo restart do app** (lidos do `config.json` na inicialização). Isso é suficiente para 95% dos casos de uso e muito mais seguro.

**Passo 2.1 — Adicionar campos ao `VoiceConfig` (backend)**

**Arquivo:** `dexter/src-tauri/src/lib.rs`

Adicionar ao struct `VoiceConfig`:

```rust
    #[serde(default = "default_shortcut_talk")]
    pub shortcut_talk: String,
    #[serde(default = "default_shortcut_hide")]
    pub shortcut_hide: String,
    #[serde(default = "default_shortcut_clear")]
    pub shortcut_clear: String,
    #[serde(default = "default_shortcut_chat")]
    pub shortcut_chat: String,
```

Adicionar funções default (junto com as outras `default_*`):

```rust
fn default_shortcut_talk() -> String { "Shift+Z".to_string() }
fn default_shortcut_hide() -> String { "Shift+X".to_string() }
fn default_shortcut_clear() -> String { "Shift+C".to_string() }
fn default_shortcut_chat() -> String { "Shift+T".to_string() }
```

Adicionar ao `Default` impl:

```rust
            shortcut_talk: "Shift+Z".to_string(),
            shortcut_hide: "Shift+X".to_string(),
            shortcut_clear: "Shift+C".to_string(),
            shortcut_chat: "Shift+T".to_string(),
```

**Passo 2.2 — Usar os campos do config nos atalhos (backend)**

**Arquivo:** `dexter/src-tauri/src/lib.rs` — função `setup`

**Local:** Substituir as strings hardcoded `"Shift+Z"`, `"Shift+X"`, `"Shift+C"`, `"Shift+T"` por leituras do config.

**ANTES (exemplo para Shift+Z, linha 1403):**
```rust
            app.global_shortcut().on_shortcut("Shift+Z", |app, _shortcut, event| {
```

**DEPOIS:**
```rust
            let shortcut_talk = app.state::<AppState>().config.lock().unwrap().shortcut_talk.clone();
            app.global_shortcut().on_shortcut(&shortcut_talk, |app, _shortcut, event| {
```

> **Repita para Shift+X, Shift+C, Shift+T**, usando `shortcut_hide`, `shortcut_clear`, `shortcut_chat` respectivamente.

**Verificação:** `cargo check`

**Passo 2.3 — Adicionar campos na UI (frontend)**

**Arquivo:** `dexter/src/App.tsx`

**Passo 2.3a — Atualizar interface `VoiceConfig`:**

Adicionar após `audio_feedback`:
```typescript
  shortcut_talk: string;
  shortcut_hide: string;
  shortcut_clear: string;
  shortcut_chat: string;
```

**Passo 2.3b — Adicionar seção de atalhos no `ConfigTab`:**

Após a seção "Personalidade", antes do fechamento do `</div>` principal:

```tsx
      <FieldGroup title="Atalhos de teclado">
        <p className="text-[11px] text-white/30 leading-relaxed -mt-1">
          Os atalhos são aplicados ao reiniciar o Chronos.
        </p>
        <Field label="Falar (hold to talk)">
          <Input value={config.shortcut_talk} onChange={(v) => setConfig({ ...config, shortcut_talk: v })} placeholder="Shift+Z" />
        </Field>
        <Field label="Esconder janela">
          <Input value={config.shortcut_hide} onChange={(v) => setConfig({ ...config, shortcut_hide: v })} placeholder="Shift+X" />
        </Field>
        <Field label="Limpar conversa">
          <Input value={config.shortcut_clear} onChange={(v) => setConfig({ ...config, shortcut_clear: v })} placeholder="Shift+C" />
        </Field>
        <Field label="Abrir chat">
          <Input value={config.shortcut_chat} onChange={(v) => setConfig({ ...config, shortcut_chat: v })} placeholder="Shift+T" />
        </Field>
        <p className="text-[10px] text-white/20 leading-relaxed">
          Formato: Modifier+Key. Ex: Shift+Z, Ctrl+T, Alt+X. Reinicie o Chronos após alterar.
        </p>
      </FieldGroup>
```

**Verificação:** `npm run build`

---

## 3. Plano — Servidor de Embedding Dedicado (BGE-M3)

### Passo 3.1 — Baixar o modelo BGE-M3

**Modelo:** `bartowski/bge-m3-GGUF` → arquivo `bge-m3-Q4_K_M.gguf` (~1.2 GB)

**URL de download:**
```
https://huggingface.co/bartowski/bge-m3-GGUF/resolve/main/bge-m3-Q4_K_M.gguf
```

**Pasta destino:**
```
J:\Modelos LLM\manifests\registry.ollama.ai\library\Embadding\
```

**Comando PowerShell para baixar:**
```powershell
$dest = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Embadding"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$url = "https://huggingface.co/bartowski/bge-m3-GGUF/resolve/main/bge-m3-Q4_K_M.gguf"
$out = Join-Path $dest "bge-m3-Q4_K_M.gguf"
Write-Host "Baixando BGE-M3 (~1.2 GB)..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
Write-Host "Download concluido: $out" -ForegroundColor Green
```

### Passo 3.2 — Configurar `start-all.ps1` para iniciar o servidor de embedding

**Arquivo:** `dexter/start-all.ps1`

**Passo 3.2a — Adicionar variáveis de configuração no topo (após linha 37):**

```powershell
# Embedding server (BGE-M3 dedicado)
$EMBED_MODEL   = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Embadding\bge-m3-Q4_K_M.gguf"
$EMBED_PORT    = 8082
$EMBED_THREADS = 4
```

**Passo 3.2b — Adicionar seção de inicialização do embedding (após a seção do Whisper, antes da seção do Chatterbox, ~linha 381):**

```powershell
# 2.5. Embedding Server (BGE-M3)
$embedOk = $true
if (-not (Test-Path $LLAMA_SERVER)) {
    Write-Host "[Embed] llama-server.exe nao encontrado em: $LLAMA_SERVER" -ForegroundColor Red
    $embedOk = $false
}
if (-not (Test-Path $EMBED_MODEL)) {
    Write-Host "[Embed] Modelo BGE-M3 nao encontrado em: $EMBED_MODEL" -ForegroundColor Yellow
    Write-Host "[Embed] Baixe de: https://huggingface.co/bartowski/bge-m3-GGUF/resolve/main/bge-m3-Q4_K_M.gguf" -ForegroundColor Gray
    $embedOk = $false
}

if ($embedOk) {
    Write-Host "[Embed] Iniciando servidor de embedding BGE-M3 na porta $EMBED_PORT..." -ForegroundColor Cyan
    $embedArgs = @(
        "-m `"$EMBED_MODEL`"",
        "--embeddings",
        "--port $EMBED_PORT",
        "--host 0.0.0.0",
        "-t $EMBED_THREADS",
        "-c 512",
        "-ngl 0"
    ) -join " "

    Start-Server -Name "Embed" -Exe $LLAMA_SERVER -ServerArgs $embedArgs -Port $EMBED_PORT -Priority "BelowNormal"
} else {
    Write-Host "[Embed] Servidor de embedding nao iniciado. O RAG usara o LLM principal como fallback." -ForegroundColor Gray
}
```

> **NOTA:** `-ngl 0` significa que o BGE-M3 roda inteiramente em CPU. Ele tem apenas 567M parâmetros, então é muito leve — ~1.2 GB de RAM, sem impacto na VRAM. Threads=4 é suficiente. Contexto 512 é mais que suficiente para embeddings.

### Passo 3.3 — Atualizar `VoiceConfig` default para apontar para o servidor de embedding

**Arquivo:** `dexter/src-tauri/src/lib.rs` — `impl Default for VoiceConfig`

**ANTES:**
```rust
            embed_url: String::new(),
            embed_model: "gemma-4-26B-A4B".to_string(),
```

**DEPOIS:**
```rust
            embed_url: "http://localhost:8082".to_string(),
            embed_model: "bge-m3".to_string(),
```

> **ATENÇÃO:** Só faça essa alteração se o servidor de embedding estiver configurado e o modelo baixado. Se não, mantenha `embed_url: String::new()` (fallback para LLM). A melhor abordagem é manter o default como vazio e deixar o usuário configurar via UI — o campo `embed_url` e `embed_model` já existem na UI desde o Lote 1.

**Recomendação:** NÃO alterar os defaults. Deixe `embed_url: String::new()` e `embed_model: "gemma-4-26B-A4B"`. O usuário configura via UI conforme o `start-all.ps1` inicia ou não o servidor de embedding.

### Passo 3.4 — Script standalone para baixar o modelo

Criar arquivo `dexter/download-embed-model.ps1`:

```powershell
# Chronos — Download do modelo de embedding BGE-M3 (PT-BR)
# Uso: .\download-embed-model.ps1

$ErrorActionPreference = "Stop"

$dest = "J:\Modelos LLM\manifests\registry.ollama.ai\library\Embadding"
$url = "https://huggingface.co/bartowski/bge-m3-GGUF/resolve/main/bge-m3-Q4_K_M.gguf"
$out = Join-Path $dest "bge-m3-Q4_K_M.gguf"

Write-Host "╔════════════════════════════════════════════════╗" -ForegroundColor Magenta
Write-Host "║   Download BGE-M3 Embedding Model (PT-BR)     ║" -ForegroundColor Magenta
Write-Host "╚════════════════════════════════════════════════╝" -ForegroundColor Magenta
Write-Host ""

if (Test-Path $out) {
    $size = (Get-Item $out).Length / 1GB
    Write-Host "[OK] Modelo ja existe: $out ({0:N2} GB)" -f $size -ForegroundColor Green
    Write-Host "Para baixar novamente, delete o arquivo e rode o script." -ForegroundColor Gray
    exit 0
}

Write-Host "Destino: $dest" -ForegroundColor Gray
Write-Host "Tamanho: ~1.2 GB" -ForegroundColor Gray
Write-Host ""

New-Item -ItemType Directory -Force -Path $dest | Out-Null

Write-Host "Baixando BGE-M3 Q4_K_M..." -ForegroundColor Cyan
try {
    Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
    $size = (Get-Item $out).Length / 1GB
    Write-Host "[OK] Download concluido: $out ({0:N2} GB)" -f $size -ForegroundColor Green
} catch {
    Write-Host "[ERRO] Falha no download: $_" -ForegroundColor Red
    Write-Host "URL alternativa: https://huggingface.co/bartowski/bge-m3-GGUF" -ForegroundColor Gray
    exit 1
}

Write-Host ""
Write-Host "Proximo passo: Inicie o Chronos com start-all.ps1." -ForegroundColor Cyan
Write-Host "O servidor de embedding iniciara automaticamente na porta 8082." -ForegroundColor Gray
Write-Host "Depois, nas Configuracoes do Chronos, defina:" -ForegroundColor Gray
Write-Host "  URL do embedding: http://localhost:8082" -ForegroundColor White
Write-Host "  Modelo de embedding: bge-m3" -ForegroundColor White
```

---

## 4. Testes de Validação

### 4.1 — Validação do modelo de embedding

```powershell
# Testar se o servidor de embedding está rodando
Invoke-RestMethod -Uri "http://localhost:8082/v1/models" -Method GET

# Deve retornar algo como:
# {"object":"list","data":[{"id":"bge-m3","object":"model",...}]}

# Testar embedding
$body = @{
    input = "Olá, como você está?"
    model = "bge-m3"
} | ConvertTo-Json

$resp = Invoke-RestMethod -Uri "http://localhost:8082/embedding" -Method POST -Body $body -ContentType "application/json"
$resp.data[0].embedding.Count  # Deve retornar 1024 (dimensão do BGE-M3)
```

### 4.2 — Validação da troca de perfil

1. Abrir Configurações do Chronos
2. Selecionar perfil "Programador"
3. Verificar que o textarea "System prompt (voz)" foi atualizado para o preset de programador
4. Salvar
5. Fechar e reabrir Configurações
6. Verificar que o system prompt continua o preset de programador
7. Selecionar "Personalizado"
8. Verificar que o textarea "System prompt (texto)" apareceu
9. Digitar um prompt customizado e salvar
10. Verificar que ao selecionar "Padrão" novamente, o system prompt volta ao default

### 4.3 — Validação dos atalhos

1. Alterar "Falar" para `Ctrl+Shift+Z` nas Configurações
2. Salvar
3. Reiniciar o Chronos
4. Verificar que `Ctrl+Shift+Z` agora abre o microfone
5. Verificar que o `Shift+Z` original NÃO funciona mais

### 4.4 — Validação do `start-all.ps1`

```powershell
# Iniciar tudo
.\start-all.ps1 -Profile voice-chatterbox-cpu

# Verificar portas
netstat -ano | Select-String "8080.*LISTENING"  # LLM
netstat -ano | Select-String "8081.*LISTENING"  # Whisper
netstat -ano | Select-String "8082.*LISTENING"  # Embedding
netstat -ano | Select-String "8005.*LISTENING"  # Chatterbox TTS
```

---

## 5. Checklist Final

### Backend (Rust)
- [ ] `VoiceConfig` tem campos `shortcut_talk`, `shortcut_hide`, `shortcut_clear`, `shortcut_chat`
- [ ] `Default` implementado para os novos campos de atalho
- [ ] Atalhos no `setup()` usam valores do config em vez de strings hardcoded
- [ ] `chat_streaming` (voz) verifica `config.personality` para fallback de prompt
- [ ] `cargo check` passa sem erros

### Frontend (React)
- [ ] Interface `VoiceConfig` inclui campos de atalho
- [ ] `ConfigTab` tem seção "Atalhos de teclado" com inputs
- [ ] `VOICE_PRESETS` e `TEXT_PRESETS` definidos com prompts para cada perfil
- [ ] Select de perfil aplica presets ao `system_prompt` e `system_prompt_text`
- [ ] Botão "Restaurar padrões" restaura os presets corretamente
- [ ] `npm run build` passa sem erros

### Infraestrutura
- [ ] `download-embed-model.ps1` criado e funcional
- [ ] Modelo BGE-M3 baixado em `J:\Modelos LLM\manifests\registry.ollama.ai\library\Embadding\`
- [ ] `start-all.ps1` inicia servidor de embedding na porta 8082
- [ ] Servidor de embedding responde a `/v1/models` e `/embedding`
- [ ] Embedding retorna vetores de 1024 dimensões

### Testes manuais
- [ ] Perfil "Programador" altera system prompt de voz
- [ ] Perfil "Criativo" altera system prompt de voz
- [ ] Perfil "Personalizado" mostra campo de system prompt de texto
- [ ] Atalhos customizados funcionam após restart
- [ ] RAG funciona com o servidor de embedding dedicado
- [ ] RAG faz fallback para LLM quando embedding está offline
