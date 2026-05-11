# Lote 1 — Summary de Implementação

> **Data:** 11/05/2026
> **Verificação:** `cargo check` ✅ | `npm run build` ✅

---

## O que foi implementado

- `VoiceConfig` expandido com 10 novos campos
- Comando `list_models` que consulta a API do llama.cpp
- Comando `get_default_config` para restaurar configurações padrão
- Novos campos usados no pipeline de voz (`temperature`, `thinking`, `response_style`)
- `embed_url` separado para servidor de embedding dedicado
- UI de configurações refeita: dropdowns de modelo, sliders, toggles
- Badge do modelo ativo na janela Orb
- Atalho `Shift+C` para limpar conversa
- Botão "Restaurar padrões" nas configurações

---

## Arquivos modificados

| Arquivo | Alterações |
|---------|-----------|
| `dexter/src-tauri/src/lib.rs` | VoiceConfig expandido, novos comandos, atalho Shift+C, embed_url nos callers RAG |
| `dexter/src-tauri/src/voice.rs` | Pipeline usa config (temperature, thinking, response_style, tts_volume) |
| `dexter/src-tauri/src/rag.rs` | Parâmetro `llm_url` → `embed_url` |
| `dexter/src/App.tsx` | Interface VoiceConfig, ModelSelect, ConfigTab refatorado, badge, botão Restaurar |

---

## Detalhamento

### Passo 1 — VoiceConfig expandido (`lib.rs`)

**Novas funções default** (após `fn default_true()`):

```rust
fn default_tts_volume() -> u8 { 100 }
fn default_temperature() -> f32 { 0.7 }
fn default_response_style() -> String { "normal".to_string() }
fn default_personality() -> String { "default".to_string() }
```

**Novos campos no struct `VoiceConfig`:**

| Campo | Tipo | Default | Descrição |
|-------|------|---------|-----------|
| `embed_url` | `String` | `""` (vazio) | URL do servidor de embedding dedicado |
| `tts_volume` | `u8` | `100` | Volume do TTS (0–100) |
| `enable_thinking` | `bool` | `false` | Liga/desliga raciocínio interno (thinking) |
| `temperature` | `f32` | `0.7` | Temperatura do LLM (0.0–2.0) |
| `response_style` | `String` | `"normal"` | Estilo de resposta: concise / normal / detailed |
| `system_prompt_text` | `String` | `""` | System prompt alternativo para modo chat de texto |
| `personality` | `String` | `"default"` | Perfil de personalidade |
| `audio_feedback` | `bool` | `true` | Se toca beep ao iniciar gravação |

---

### Passo 2 — Novos comandos (`lib.rs`)

**`list_models`** — consulta `GET {llm_url}/v1/models` (formato OpenAI-compatible) e retorna `Vec<String>` com os IDs dos modelos disponíveis. Timeout de 10s.

**`get_default_config`** — retorna `VoiceConfig::default()` (valores padrão).

**Registrados no `invoke_handler`** como `get_default_config` e `list_models`.

---

### Passo 3 — embed_url no RAG (`rag.rs` + `lib.rs`)

**`rag.rs`:**
- Parâmetro `llm_url` renomeado para `embed_url` em:
  - `ingest()`
  - `search()`
  - `embed_texts()`

**`lib.rs` — callers ajustados:**
- `ingest_text`: calcula `embed_url` (usa `config.embed_url` se não vazio, senão `config.llm_url`)
- `ingest_file`: mesmo padrão
- `execute_tool` → `search_knowledge`: mesmo padrão

---

### Passo 4 — Pipeline de voz usa novos campos (`voice.rs`)

**`thinking_budget_tokens`:**
```rust
if config.enable_thinking {
    if tools.is_empty() { 512 } else { 1024 }
} else {
    0
}
```

**`max_tokens` baseado em `response_style`:**
| Style | c/ tools | c/ tool_history | normal |
|-------|----------|----------------|--------|
| concise | 512 | 256 | 200 |
| detailed | 2048 | 1024 | 1024 |
| normal | 1024 | 512 | 600 |

**`OpenAIChatRequest`:**
- `temperature` ← `config.temperature`
- `chat_template_kwargs.enable_thinking` ← `config.enable_thinking`

**`synthesize_windows_sapi`:**
- Assinatura: `(text: &str, volume: u8)`
- Script PowerShell: `$synth.Volume = {volume}` (antes era hardcoded 100)
- Callers em `synthesize()` passam `config.tts_volume`

---

### Passo 5 — Atalho Shift+C (`lib.rs`)

```rust
app.global_shortcut().on_shortcut("Shift+C", |app, _shortcut, event| {
    if event.state == ShortcutState::Pressed {
        let state = app.state::<AppState>();
        state.messages.lock().unwrap().clear();
        let _ = app.emit("messages_cleared", ());
    }
})?;
```

---

### Passo 6 — Frontend de Configurações (`App.tsx`)

**Interface `VoiceConfig`:** Expandida com todos os novos campos.

**Componente `ModelSelect`:**
- Dropdown customizado que consulta `list_models` da API
- Campo de busca/filtro
- Opção "usar modelo de chat" (string vazia)
- Opção "+ Outro" (input manual via `prompt()`)
- Estado de carregamento

**`ConfigTab` refatorado:**
- Seção "Modelo de linguagem": URL do LLM + `ModelSelect` para modelo de chat
- Seção "Comportamento do modelo": toggle Thinking, slider Temperatura (0–2), select Estilo de resposta
- Seção "Embedding (RAG)": URL de embedding + `ModelSelect` para modelo + dica BGE-M3
- Seção "Visão (screenshots)": `ModelSelect` para modelo de visão
- Seção "Síntese de voz": slider de Volume do TTS (0–100%)
- Seção "Personalidade": select de Perfil + textarea system prompt voz + textarea system prompt texto (só se perfil "custom")

**Botão "Restaurar padrões":**
- Chama `get_default_config` e aplica via `setConfig`
- Exibido ao lado do botão "Salvar" no header
- Ambos com `WebkitAppRegion: "no-drag"` no container

---

### Passo 7 — Badge do modelo no Orb (`App.tsx`)

- Estado `currentModel` carregado via `invoke<VoiceConfig>("get_config")`
- Badge renderizado abaixo do orb: `{currentModel}` em estilo pill/texto pequeno

---

## Checklist de verificação

- [x] `cargo check` passa sem erros
- [x] `npm run build` passa sem erros de TypeScript
- [x] UI de configurações carrega com todos os novos campos
- [x] Dropdowns de modelo conectados ao comando `list_models`
- [x] Botão "Restaurar padrões" funcional
- [x] Badge do modelo aparece no Orb
- [x] `Shift+C` limpa a conversa
- [x] `synthesize_windows_sapi` respeita `tts_volume`

## O que NÃO foi modificado

- Modo chat de texto (Fase 2)
- `audio.rs` / beep de feedback (Lote 3)
- `start-all.ps1`
- `media_controls.rs`, `sandbox.rs`
- `package.json` (sem novos packages npm)
