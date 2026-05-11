# Chronos AI v2 — Plano de Implementação

> **ATENÇÃO LLM:** Este plano está dividido em 3 lotes independentes. Execute **um lote por vez**, na ordem. Ao final de cada lote, verifique com `cargo check` (Rust) e `npm run build` (frontend) se nada quebrou. Os trechos "ANTES" mostram o código atual exato — use `StrReplace` com `old_string` idêntico ao que está no arquivo.

---

## Visão Geral

Transformar o Chronos de assistente de voz para **assistente de IA com dois modos** (Voz + Texto) e controle total via UI.

**Arquivos que serão modificados:**

| Arquivo | Lote 1 | Lote 2 | Lote 3 |
|---------|--------|--------|--------|
| `dexter/src-tauri/src/lib.rs` | ✅ | ✅ | ✅ |
| `dexter/src-tauri/src/voice.rs` | ✅ | ✅ | ✅ |
| `dexter/src-tauri/src/rag.rs` | ✅ | — | — |
| `dexter/src/App.tsx` | ✅ | ✅ | ✅ |
| `dexter/src/App.css` | — | ✅ | ✅ |
| `dexter/package.json` | — | ✅ | — |

---

# LOTE 1 — Fundação: Modelos, Configs e Atalhos

```
Prazo estimado: ~30-45 minutos de edição
Verificação final: cargo check + npm run build devem passar sem erros
```

## O que este lote implementa

- `VoiceConfig` expandido com 10 novos campos
- Comando `list_models` que consulta a API do llama.cpp
- Novos campos usados no pipeline de voz (temperature, thinking, response_style)
- `embed_url` separado para servidor de embedding dedicado
- UI de configurações refeita: dropdowns de modelo, sliders, toggles
- Badge do modelo ativo na janela Orb
- Atalho `Shift+C` para limpar conversa
- Botão "Restaurar padrões" + comando `get_default_config`

## O que NÃO modificar neste lote

- Não criar modo chat de texto (Fase 5 do plano original)
- Não criar arquivo `audio.rs` nem beep de feedback
- Não mexer em `start-all.ps1`
- Não modificar `media_controls.rs`, `sandbox.rs`
- Não instalar novos packages npm (isso é do Lote 2)

---

## Passo 1 — Expandir `VoiceConfig` no backend

**Arquivo:** `dexter/src-tauri/src/lib.rs`

### 1.1 — Adicionar novas funções default (após `fn default_true()`)

**Local:** Após a linha 81 (fim da `fn default_true()`) e antes da linha 83 (`impl Default for ToolsConfig`)

**ANTES:**
```rust
fn default_true() -> bool {
    true
}

impl Default for ToolsConfig {
```

**DEPOIS:**
```rust
fn default_true() -> bool {
    true
}

fn default_tts_volume() -> u8 {
    100
}
fn default_temperature() -> f32 {
    0.7
}
fn default_response_style() -> String {
    "normal".to_string()
}
fn default_personality() -> String {
    "default".to_string()
}

impl Default for ToolsConfig {
```

### 1.2 — Adicionar novos campos ao struct `VoiceConfig`

**Local:** Linhas 100-120, o struct `VoiceConfig`

**ANTES:**
```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    pub whisper_model_path: String,
    #[serde(default = "default_whisper_url")]
    pub whisper_url: String,
    pub llm_url: String,
    pub llm_model: String,
    pub embed_model: String,
    #[serde(default)]
    pub vision_model: String,
    pub chatterbox_url: String,
    pub chatterbox_voice: String,
    pub system_prompt: String,
    /// Pastas extra onde procurar música (uma por linha ou separadas por ; ou |). Junta-se à pasta Música do Windows e a DEXTER_MUSIC_PATHS.
    #[serde(default)]
    pub music_library_paths: String,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub sandbox: sandbox::SandboxConfig,
}
```

**DEPOIS:**
```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    pub whisper_model_path: String,
    #[serde(default = "default_whisper_url")]
    pub whisper_url: String,
    pub llm_url: String,
    #[serde(default)]
    pub embed_url: String,
    pub llm_model: String,
    pub embed_model: String,
    #[serde(default)]
    pub vision_model: String,
    pub chatterbox_url: String,
    pub chatterbox_voice: String,
    #[serde(default = "default_tts_volume")]
    pub tts_volume: u8,
    #[serde(default)]
    pub enable_thinking: bool,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_response_style")]
    pub response_style: String,
    pub system_prompt: String,
    #[serde(default)]
    pub system_prompt_text: String,
    #[serde(default = "default_personality")]
    pub personality: String,
    #[serde(default = "default_true")]
    pub audio_feedback: bool,
    /// Pastas extra onde procurar música (uma por linha ou separadas por ; ou |). Junta-se à pasta Música do Windows e a DEXTER_MUSIC_PATHS.
    #[serde(default)]
    pub music_library_paths: String,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub sandbox: sandbox::SandboxConfig,
}
```

### 1.3 — Atualizar `impl Default for VoiceConfig`

**Local:** Linhas 126-144

**ANTES:**
```rust
    fn default() -> Self {
        let default_whisper = r"J:\Modelos LLM\manifests\registry.ollama.ai\library\whisper\ggml-small.bin".to_string();
        Self {
            whisper_model_path: default_whisper,
            whisper_url: "http://localhost:8081".to_string(),
            llm_url: "http://localhost:8080".to_string(),
            llm_model: "gemma-4-26B-A4B".to_string(),
            embed_model: "gemma-4-26B-A4B".to_string(),
            vision_model: String::new(),
            chatterbox_url: "http://localhost:8005".to_string(),
            chatterbox_voice: "dexter-ptbr".to_string(),
            system_prompt: "...".to_string(),
            music_library_paths: String::new(),
            tools: ToolsConfig::default(),
            sandbox: sandbox::SandboxConfig::default(),
        }
    }
```

**DEPOIS:**
```rust
    fn default() -> Self {
        let default_whisper = r"J:\Modelos LLM\manifests\registry.ollama.ai\library\whisper\ggml-small.bin".to_string();
        Self {
            whisper_model_path: default_whisper,
            whisper_url: "http://localhost:8081".to_string(),
            llm_url: "http://localhost:8080".to_string(),
            embed_url: String::new(),
            llm_model: "gemma-4-26B-A4B".to_string(),
            embed_model: "gemma-4-26B-A4B".to_string(),
            vision_model: String::new(),
            chatterbox_url: "http://localhost:8005".to_string(),
            chatterbox_voice: "dexter-ptbr".to_string(),
            tts_volume: 100,
            enable_thinking: false,
            temperature: 0.7,
            response_style: "normal".to_string(),
            system_prompt: "...".to_string(),
            system_prompt_text: String::new(),
            personality: "default".to_string(),
            audio_feedback: true,
            music_library_paths: String::new(),
            tools: ToolsConfig::default(),
            sandbox: sandbox::SandboxConfig::default(),
        }
    }
```

> **NOTA:** Mantenha o valor completo de `system_prompt` (linha 138) intacto — ele é muito longo. Apenas adicione os novos campos ao redor dele, sem alterar seu conteúdo.

**Verificação:** `cargo check` deve compilar sem erros.

---

## Passo 2 — Adicionar comando `list_models`

**Arquivo:** `dexter/src-tauri/src/lib.rs`

### 2.1 — Adicionar a função `list_models`

**Local:** Antes da função `get_config` (antes da linha 177), ou após o bloco `impl VoiceConfig` (após linha 175)

**INSERIR:**
```rust
#[tauri::command]
async fn list_models(llm_url: String) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Erro ao criar cliente HTTP: {}", e))?;

    let resp = client
        .get(format!("{}/v1/models", llm_url.trim_end_matches('/')))
        .send()
        .await
        .map_err(|e| format!("Falha ao consultar modelos: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Servidor retornou {}", resp.status()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Resposta inválida: {}", e))?;

    let models = json["data"]
        .as_array()
        .ok_or("Formato de resposta inesperado (esperado 'data' array)")?
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .collect();

    Ok(models)
}
```

### 2.2 — Adicionar comando `get_default_config`

**INSERIR (após `list_models`):**
```rust
#[tauri::command]
fn get_default_config() -> VoiceConfig {
    VoiceConfig::default()
}
```

### 2.3 — Adicionar ao `invoke_handler`

**Local:** Linhas 1147-1160, o `invoke_handler`

**ANTES:**
```rust
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            get_messages,
            clear_messages,
            show_window,
            hide_window,
            ingest_text,
            ingest_file,
            list_knowledge_sources,
            delete_knowledge_source,
            start_recording,
            stop_recording_and_process,
        ])
```

**DEPOIS:**
```rust
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            get_default_config,
            list_models,
            get_messages,
            clear_messages,
            show_window,
            hide_window,
            ingest_text,
            ingest_file,
            list_knowledge_sources,
            delete_knowledge_source,
            start_recording,
            stop_recording_and_process,
        ])
```

**Verificação:** `cargo check` deve compilar sem erros.

---

## Passo 3 — Usar `embed_url` no RAG

**Arquivo:** `dexter/src-tauri/src/rag.rs`

### 3.1 — Renomear parâmetro `llm_url` para `embed_url` em `embed_texts`

**Local:** Assinatura da função `embed_texts` e a linha onde monta a URL de embedding

**ANTES (procure pela definição da função `embed_texts`):**
```rust
pub async fn embed_texts(
    llm_url: &str,
    _model: &str,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, String> {
```

**DEPOIS:**
```rust
pub async fn embed_texts(
    embed_url: &str,
    _model: &str,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, String> {
```

**ANTES (linha que constrói a URL, procure por `/embedding`):**
```rust
    let url = format!("{}/embedding", llm_url.trim_end_matches('/'));
```

**DEPOIS:**
```rust
    let url = format!("{}/embedding", embed_url.trim_end_matches('/'));
```

### 3.2 — Ajustar callers em `lib.rs`

**Arquivo:** `dexter/src-tauri/src/lib.rs`

Nas funções `ingest_text`, `ingest_file` e `execute_tool` (tool `search_knowledge`), onde `config.llm_url` é passado para funções de embedding, substituir por:

```rust
let embed_url = if config.embed_url.is_empty() {
    &config.llm_url
} else {
    &config.embed_url
};
```

E usar `embed_url` no lugar de `&config.llm_url` nas chamadas de embedding.

**Locais específicos (buscar por `config.llm_url` + `embed_model` ou `rag_store`):**

- `ingest_text` (~linha 225): `state.rag_store.ingest(&source, &text, &config.llm_url, &config.embed_model)`
  → Deve passar o `embed_url` calculado em vez de `&config.llm_url`

- `ingest_file` (~linha 241): `state.rag_store.ingest(&source, &text, &config.llm_url, &config.embed_model)`
  → Mesma alteração

- `execute_tool` (~linha 682): `rag_store.search(&query, &config.llm_url, &config.embed_model, 5)`
  → Mesma alteração

**Verificação:** `cargo check` deve compilar sem erros.

---

## Passo 4 — Usar novos campos no pipeline de voz

**Arquivo:** `dexter/src-tauri/src/voice.rs`

### 4.1 — Substituir `thinking_budget_tokens` hardcoded

**Local:** Linhas 732-736

**ANTES:**
```rust
    let thinking_budget_tokens = if tools.is_empty() {
        256
    } else {
        256
    };
```

**DEPOIS:**
```rust
    let thinking_budget_tokens = if config.enable_thinking {
        if tools.is_empty() { 512 } else { 1024 }
    } else {
        0
    };
```

### 4.2 — Substituir `max_tokens` hardcoded com `response_style`

**Local:** Linhas 741-747

**ANTES:**
```rust
    let max_tokens = if !tools.is_empty() {
        1024
    } else if messages.iter().any(|m| m.role == "tool") {
        512
    } else {
        600
    };
```

**DEPOIS:**
```rust
    let has_tool_history = messages.iter().any(|m| m.role == "tool");
    let max_tokens = match config.response_style.as_str() {
        "concise" => if !tools.is_empty() { 512 } else if has_tool_history { 256 } else { 200 },
        "detailed" => if !tools.is_empty() { 2048 } else if has_tool_history { 1024 } else { 1024 },
        _ => if !tools.is_empty() { 1024 } else if has_tool_history { 512 } else { 600 },
    };
```

### 4.3 — Substituir valores hardcoded na struct `OpenAIChatRequest`

**Local:** Linhas 749-758

**ANTES:**
```rust
    let request = OpenAIChatRequest {
        model: config.llm_model.clone(),
        messages: openai_messages,
        stream: true,
        max_tokens,
        temperature: 0.7,
        chat_template_kwargs: serde_json::json!({ "enable_thinking": false }),
        thinking_budget_tokens,
        tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
    };
```

**DEPOIS:**
```rust
    let request = OpenAIChatRequest {
        model: config.llm_model.clone(),
        messages: openai_messages,
        stream: true,
        max_tokens,
        temperature: config.temperature,
        chat_template_kwargs: serde_json::json!({
            "enable_thinking": config.enable_thinking
        }),
        thinking_budget_tokens,
        tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
    };
```

### 4.4 — Usar `tts_volume` no Windows SAPI fallback

**Local:** Função `synthesize_windows_sapi` (~linha 1203), script PowerShell onde `$synth.Volume = 100`

**ANTES (procure por `$synth.Volume = 100`):**
```
$synth.Volume = 100
```

**DEPOIS:** A função `synthesize_windows_sapi` precisa receber o volume como parâmetro. Altere a assinatura:

```rust
async fn synthesize_windows_sapi(
    text: &str,
    volume: u8,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
```

E no script PowerShell, substitua:
```
$synth.Volume = {volume}
```
(usando format! com a variável `volume`)

**Ajustar callers:** Em `synthesize()` (~linha 1132 e 1173 e 1181), passe `config.tts_volume` como parâmetro:
- `synthesize_windows_sapi(text).await` → `synthesize_windows_sapi(text, config.tts_volume).await`

**Verificação:** `cargo check` deve compilar sem erros.

---

## Passo 5 — Atalho `Shift+C` (limpar conversa)

**Arquivo:** `dexter/src-tauri/src/lib.rs` — função `setup`

### 5.1 — Adicionar registro do atalho

**Local:** Após o bloco do `Shift+X` (linhas 1130-1137), antes do bloco de `set_background_color`

**INSERIR (após o fechamento `})?;` do Shift+X):**
```rust
            // Register Shift+C to clear conversation
            app.global_shortcut().on_shortcut("Shift+C", |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let state = app.state::<AppState>();
                    state.messages.lock().unwrap().clear();
                    let _ = app.emit("messages_cleared", ());
                }
            })?;
```

**Verificação:** `cargo check` deve compilar sem erros.

---

## Passo 6 — Frontend: Interface TypeScript + UI de Configurações

**Arquivo:** `dexter/src/App.tsx`

### 6.1 — Atualizar interface `VoiceConfig`

**Local:** Linhas 34-48

**ANTES:**
```typescript
interface VoiceConfig {
  whisper_model_path: string;
  whisper_url: string;
  llm_url: string;
  llm_model: string;
  embed_model: string;
  vision_model: string;
  chatterbox_url: string;
  chatterbox_voice: string;
  system_prompt: string;
  /** Pastas extra para procurar ficheiros de música (local). */
  music_library_paths: string;
  tools: ToolsConfig;
  sandbox: SandboxConfig;
}
```

**DEPOIS:**
```typescript
interface VoiceConfig {
  whisper_model_path: string;
  whisper_url: string;
  llm_url: string;
  embed_url: string;
  llm_model: string;
  embed_model: string;
  vision_model: string;
  chatterbox_url: string;
  chatterbox_voice: string;
  tts_volume: number;
  enable_thinking: boolean;
  temperature: number;
  response_style: string;
  system_prompt: string;
  system_prompt_text: string;
  personality: string;
  audio_feedback: boolean;
  /** Pastas extra para procurar ficheiros de música (local). */
  music_library_paths: string;
  tools: ToolsConfig;
  sandbox: SandboxConfig;
}
```

### 6.2 — Adicionar componente `ModelSelect`

**Inserir antes da função `ConfigTab` (antes da linha 111):**

```tsx
function ModelSelect({ value, onChange, llmUrl, placeholder }: {
  value: string;
  onChange: (v: string) => void;
  llmUrl: string;
  placeholder?: string;
}) {
  const [models, setModels] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");

  useEffect(() => {
    if (!llmUrl) return;
    setLoading(true);
    invoke<string[]>("list_models", { llmUrl })
      .then(setModels)
      .catch(() => setModels([]))
      .finally(() => setLoading(false));
  }, [llmUrl]);

  const filtered = models.filter((m) =>
    m.toLowerCase().includes(search.toLowerCase())
  );

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] text-left outline-none transition-all duration-200 focus:border-blue-500/50 flex items-center justify-between"
      >
        <span className={value ? "text-white/90" : "text-white/20"}>
          {value || placeholder || "Selecione um modelo..."}
        </span>
        <span className="text-white/30 text-[10px]">{open ? "▲" : "▼"}</span>
      </button>
      {open && (
        <div className="absolute z-50 mt-1 w-full bg-[#1a1a1e] border border-white/[0.12] rounded-lg shadow-xl max-h-48 overflow-y-auto custom-scrollbar">
          <div className="sticky top-0 p-2 bg-[#1a1a1e] border-b border-white/[0.06]">
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Filtrar modelos..."
              className="w-full bg-white/[0.05] border border-white/10 text-white/80 px-2 py-1.5 rounded text-[12px] outline-none focus:border-blue-500/50"
              onClick={(e) => e.stopPropagation()}
            />
          </div>
          <button
            type="button"
            onClick={() => { onChange(""); setOpen(false); setSearch(""); }}
            className="w-full text-left px-3 py-2 text-[12px] text-white/40 hover:bg-white/[0.06] hover:text-white/60 transition-colors"
          >
            (usar modelo de chat)
          </button>
          <button
            type="button"
            onClick={() => {
              const custom = prompt("Digite o nome do modelo:");
              if (custom) { onChange(custom); setOpen(false); setSearch(""); }
            }}
            className="w-full text-left px-3 py-2 text-[12px] text-white/40 hover:bg-white/[0.06] hover:text-white/60 transition-colors border-t border-white/[0.04]"
          >
            + Outro (digitar nome)...
          </button>
          {loading && (
            <div className="px-3 py-2 text-[12px] text-white/25">Carregando...</div>
          )}
          {!loading && filtered.length === 0 && search && (
            <div className="px-3 py-2 text-[12px] text-white/25">Nenhum modelo encontrado</div>
          )}
          {filtered.map((m) => (
            <button
              key={m}
              type="button"
              onClick={() => { onChange(m); setOpen(false); setSearch(""); }}
              className={`w-full text-left px-3 py-2 text-[13px] transition-colors ${
                m === value
                  ? "bg-blue-500/20 text-white/90"
                  : "text-white/70 hover:bg-white/[0.06] hover:text-white/90"
              }`}
            >
              {m}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
```

### 6.3 — Refatorar `ConfigTab` com os novos controles

Substituir a seção "Modelo de linguagem" atual (linhas 141-153) pela versão expandida. A seção antiga tem os campos `llm_url`, `llm_model`, `embed_model`, `vision_model` como `<Input>`. Substitua-os:

**ANTES (linhas 141-153):**
```tsx
      <FieldGroup title="Modelo de linguagem (llama.cpp)">
        <Field label="URL do servidor LLM">
          <Input value={config.llm_url} onChange={(v) => setConfig({ ...config, llm_url: v })} />
        </Field>
        <Field label="Modelo de chat">
          <Input value={config.llm_model} onChange={(v) => setConfig({ ...config, llm_model: v })} />
        </Field>
        <Field label="Modelo de embedding">
          <Input value={config.embed_model} onChange={(v) => setConfig({ ...config, embed_model: v })} placeholder="Igual ao de chat (usa o endpoint /embedding)" />
        </Field>
        <Field label="Modelo de visão">
          <Input value={config.vision_model} onChange={(v) => setConfig({ ...config, vision_model: v })} placeholder="llava (ferramenta de captura de tela)" />
        </Field>
      </FieldGroup>
```

**DEPOIS:**
```tsx
      <FieldGroup title="Modelo de linguagem (llama.cpp)">
        <Field label="URL do servidor LLM">
          <Input value={config.llm_url} onChange={(v) => setConfig({ ...config, llm_url: v })} />
        </Field>
        <Field label="Modelo de chat">
          <ModelSelect value={config.llm_model} onChange={(v) => setConfig({ ...config, llm_model: v })} llmUrl={config.llm_url} placeholder="Selecione o modelo de chat..." />
        </Field>
      </FieldGroup>

      <FieldGroup title="Comportamento do modelo">
        <div className="flex items-center justify-between px-1">
          <div>
            <div className="text-[13px] font-medium text-white/80">Modo Thinking</div>
            <div className="text-[11px] text-white/30 mt-0.5">Raciocínio interno antes de responder (mais inteligente, mais lento)</div>
          </div>
          <Toggle on={config.enable_thinking} onToggle={() => setConfig({ ...config, enable_thinking: !config.enable_thinking })} />
        </div>

        <Field label="Temperatura">
          <div className="flex items-center gap-3">
            <input
              type="range"
              min="0"
              max="2"
              step="0.1"
              value={config.temperature}
              onChange={(e) => setConfig({ ...config, temperature: parseFloat(e.target.value) })}
              className="flex-1 accent-blue-500"
            />
            <span className="text-[13px] text-white/60 font-mono w-10 text-right">{config.temperature.toFixed(1)}</span>
          </div>
          <div className="flex justify-between text-[10px] text-white/25 px-1 -mt-1">
            <span>Preciso (0)</span>
            <span>Criativo (2)</span>
          </div>
        </Field>

        <Field label="Estilo de resposta">
          <select
            value={config.response_style}
            onChange={(e) => setConfig({ ...config, response_style: e.target.value })}
            className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50"
          >
            <option value="concise">Conciso — respostas curtas e diretas</option>
            <option value="normal">Normal — equilibrado</option>
            <option value="detailed">Detalhado — explicações completas</option>
          </select>
        </Field>
      </FieldGroup>

      <FieldGroup title="Embedding (RAG)">
        <Field label="URL do servidor de embedding">
          <Input value={config.embed_url} onChange={(v) => setConfig({ ...config, embed_url: v })} placeholder="http://localhost:8082 (usa LLM se vazio)" />
        </Field>
        <Field label="Modelo de embedding">
          <ModelSelect value={config.embed_model} onChange={(v) => setConfig({ ...config, embed_model: v })} llmUrl={config.embed_url || config.llm_url} placeholder="Selecione o modelo de embedding..." />
        </Field>
        <p className="text-[11px] text-white/30 leading-relaxed -mt-1">
          Recomendado: BGE-M3 (multilíngue, 567M params, ~1.2 GB). Rode em um servidor llama.cpp separado com <span className="text-white/50 font-mono">--embeddings --port 8082</span>.
        </p>
      </FieldGroup>

      <FieldGroup title="Visão (screenshots)">
        <Field label="Modelo de visão">
          <ModelSelect value={config.vision_model} onChange={(v) => setConfig({ ...config, vision_model: v })} llmUrl={config.llm_url} placeholder="(usar modelo de chat)" />
        </Field>
        <p className="text-[11px] text-white/30 leading-relaxed -mt-1">
          Deixe em branco para usar o mesmo modelo de chat. Só funciona se o modelo tiver suporte multimodal (mmproj).
        </p>
      </FieldGroup>
```

### 6.4 — Adicionar controle de volume do TTS na seção "Síntese de voz"

**Local:** Após o campo `chatterbox_voice` dentro de `<FieldGroup title="Síntese de voz">`

**INSERIR:**
```tsx
        <Field label="Volume do TTS">
          <div className="flex items-center gap-3">
            <input
              type="range"
              min="0"
              max="100"
              step="5"
              value={config.tts_volume}
              onChange={(e) => setConfig({ ...config, tts_volume: parseInt(e.target.value) })}
              className="flex-1 accent-blue-500"
            />
            <span className="text-[13px] text-white/60 font-mono w-10 text-right">{config.tts_volume}%</span>
          </div>
        </Field>
```

### 6.5 — Adicionar seção de personalidade (opcional — substitui o `system_prompt` textarea atual)

**Local:** A seção "Personalidade" atual (linhas 196-205) mostra apenas um textarea para `system_prompt`. Substitua por:

```tsx
      <FieldGroup title="Personalidade">
        <Field label="Perfil">
          <select
            value={config.personality}
            onChange={(e) => setConfig({ ...config, personality: e.target.value })}
            className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50"
          >
            <option value="default">Padrão — assistente amigável e proativo</option>
            <option value="coder">Programador — foco em código e terminal</option>
            <option value="creative">Criativo — respostas mais longas e variadas</option>
            <option value="custom">Personalizado — editar prompts manualmente</option>
          </select>
        </Field>
        <Field label="System prompt (voz)">
          <textarea
            value={config.system_prompt}
            onChange={(e) => setConfig({ ...config, system_prompt: e.target.value })}
            rows={4}
            className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-inherit outline-none resize-y min-h-[80px] transition-all duration-200 focus:border-blue-500/50 placeholder:text-white/20"
          />
        </Field>
        {config.personality === "custom" && (
          <Field label="System prompt (texto)">
            <textarea
              value={config.system_prompt_text}
              onChange={(e) => setConfig({ ...config, system_prompt_text: e.target.value })}
              rows={4}
              placeholder="Prompt usado no modo chat de texto (Shift+T)"
              className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-inherit outline-none resize-y min-h-[80px] transition-all duration-200 focus:border-blue-500/50 placeholder:text-white/20"
            />
          </Field>
        )}
      </FieldGroup>
```

### 6.6 — Botão "Restaurar padrões" no rodapé das configs

**Local:** Na função `Settings`, no header onde está o botão "Salvar" (~linha 551)

**ANTES:**
```tsx
        {tab !== "knowledge" && (
          <button
            onClick={save}
            className="px-4 py-1.5 rounded-md text-[12px] font-medium border-none cursor-pointer bg-blue-500 text-white hover:bg-blue-600 transition-colors duration-150"
            style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
          >
            {saved ? "Salvo!" : "Salvar"}
          </button>
        )}
```

**DEPOIS:**
```tsx
        <div className="flex items-center gap-2" style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}>
          {tab !== "knowledge" && (
            <>
              <button
                onClick={async () => {
                  try {
                    const defaults = await invoke<VoiceConfig>("get_default_config");
                    setConfig({ ...defaults, music_library_paths: defaults.music_library_paths ?? "" });
                  } catch (e) {
                    console.error("Falha ao restaurar padrões:", e);
                  }
                }}
                className="px-3 py-1.5 rounded-md text-[12px] font-medium border border-white/15 bg-white/[0.04] text-white/50 hover:text-white/75 hover:bg-white/[0.08] cursor-pointer transition-all duration-200"
              >
                Restaurar padrões
              </button>
              <button
                onClick={save}
                className="px-4 py-1.5 rounded-md text-[12px] font-medium border-none cursor-pointer bg-blue-500 text-white hover:bg-blue-600 transition-colors duration-150"
              >
                {saved ? "Salvo!" : "Salvar"}
              </button>
            </>
          )}
        </div>
```

> **NOTA:** O `WebkitAppRegion: "no-drag"` precisa ser aplicado no container `div` em vez de cada botão, já que o CSS property é herdado. O `header` original já tem `WebkitAppRegion: "drag"` — os botões precisam de `no-drag`.

### 6.7 — Corrigir carregamento do config com novos campos

**Local:** `useEffect` dentro de `Settings` (~linha 524)

**ANTES:**
```tsx
    invoke<VoiceConfig>("get_config").then((c) =>
      setConfig({
        ...c,
        music_library_paths: c.music_library_paths ?? "",
      })
    );
```

O spread `...c` já cobre os novos campos com os valores do backend. Nenhuma alteração necessária — apenas verifique se `c.music_library_paths ??` ainda é relevante.

---

## Passo 7 — Badge do modelo ativo no Orb

**Arquivo:** `dexter/src/App.tsx` — função `Orb`

### 7.1 — Adicionar estado e efeito para carregar o modelo

**Local:** Dentro de `function Orb()`, após os estados existentes (~linha 595)

**INSERIR:**
```tsx
  const [currentModel, setCurrentModel] = useState("");

  useEffect(() => {
    invoke<VoiceConfig>("get_config").then(c => setCurrentModel(c.llm_model));
  }, []);
```

### 7.2 — Adicionar badge no JSX

**Local:** Após o fechamento da `div` do orb (~linha 806), antes do fechamento do container principal

**ANTES (final da função Orb):**
```tsx
      {/* Orb */}
      <div className="flex justify-center pb-5 pt-2 shrink-0">
        <div className={`${orbClass} relative w-20 h-20`}>
          ...
        </div>
      </div>
    </div>
  );
}
```

**DEPOIS:**
```tsx
      {/* Orb */}
      <div className="flex justify-center pb-5 pt-2 shrink-0">
        <div className={`${orbClass} relative w-20 h-20`}>
          ...
        </div>
      </div>

      {/* Model badge */}
      {currentModel && (
        <div className="flex justify-center pb-3">
          <span className="px-2.5 py-0.5 rounded-full bg-white/[0.05] text-[10px] text-white/30 font-medium border border-white/[0.04]">
            {currentModel}
          </span>
        </div>
      )}
    </div>
  );
}
```

**Verificação:** `npm run build` (ou `npm run dev`) deve compilar sem erros de TypeScript.

---

## Checklist de verificação do Lote 1

```bash
# No diretório dexter/src-tauri/
cargo check

# No diretório dexter/
npm run build   # ou npm run dev para testar
```

- [ ] `cargo check` passa sem erros
- [ ] `npm run build` passa sem erros de TypeScript
- [ ] O app abre e a janela de configurações carrega
- [ ] Dropdowns de modelo puxam a lista da API (se o llama.cpp estiver rodando)
- [ ] Salvar configurações persiste (verificar `%APPDATA%/voice-assistant/config.json`)
- [ ] Badge do modelo aparece no Orb
- [ ] `Shift+C` limpa a conversa
- [ ] O assistente de voz continua funcionando normalmente

---

# LOTE 2 — Modo Chat de Texto

```
Pré-requisito: Lote 1 completo e funcional
Prazo estimado: ~45-60 minutos
Verificação final: cargo check + npm run build devem passar
```

## O que este lote implementa

- Pipeline `chat_streaming_text` no backend (LLM com potência máxima, system prompt de texto)
- Comando `send_chat_message` que transmite tokens para o frontend
- Janela de chat separada no Tauri, aberta com `Shift+T`
- View `ChatView` com streaming de tokens, bolhas de chat, input de texto
- Histórico compartilhado entre voz e chat
- Estilos CSS para a view de chat

## O que NÃO modificar neste lote

- Não alterar o pipeline de voz existente (`chat_streaming`, `process_pipeline`)
- Não alterar configurações ou UI de settings (já feito no Lote 1)
- Não adicionar beep, exportar, ou outros extras (Lote 3)

---

## Passo 1 — Instalar dependências npm

```bash
cd dexter
npm install react-markdown remark-gfm
```

> Se `npm install` falhar, prossiga sem — a view de chat funciona sem markdown, apenas renderizando texto puro.

---

## Passo 2 — Pipeline de chat no backend

**Arquivo:** `dexter/src-tauri/src/voice.rs`

### 2.1 — Adicionar structs para eventos de chat

**Local:** Após a definição de `StreamResult` (~linha 688) e antes da função `chat_streaming`

**INSERIR:**
```rust
/// Token chunk emitted during text-chat streaming.
#[derive(Clone, Serialize)]
pub struct ChatTokenChunk {
    pub token: String,
}

/// Result of a streaming text-chat round.
pub enum ChatStreamResult {
    Content(String),
    ToolCalls(Vec<ToolCall>, String, bool),
}
```

> **NOTA:** `ChatTokenChunk` precisa de `#[derive(Serialize)]` pois é enviado via `app.emit()`. Adicione `use serde::Serialize;` no topo do arquivo se ainda não estiver importado (já está na linha 4).

### 2.2 — Adicionar função `chat_streaming_text`

**Local:** Após a função `chat_streaming` existente (após `parse_xml_tool_calls`, antes de `find_tts_chunk_end`)

Esta função é uma **cópia simplificada** de `chat_streaming` com estas diferenças:

1. System prompt = `config.system_prompt_text` (ou fallback para um prompt de texto padrão)
2. `enable_thinking = true` sempre
3. `max_tokens` alto (4096 padrão, ou baseado em `response_style`)
4. Sem divisão em sentenças para TTS — tokens vão direto via `token_tx`
5. `temperature` do config

**INSERIR:**
```rust
/// Streaming chat for text mode — full power, no TTS sentence splitting.
pub async fn chat_streaming_text(
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    token_tx: &mpsc::Sender<ChatTokenChunk>,
) -> Result<ChatStreamResult, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    // Build system prompt for text mode
    let text_system_prompt = if !config.system_prompt_text.trim().is_empty() {
        config.system_prompt_text.clone()
    } else if config.personality == "coder" {
        "Você é um assistente de programação. Responda em português do Brasil. \
         Use blocos de código com syntax highlighting quando relevante. \
         Seja detalhado e técnico. Explique o raciocínio por trás das soluções. \
         Use markdown para estruturar a resposta.".to_string()
    } else if config.personality == "creative" {
        "Você é um assistente criativo. Responda em português do Brasil. \
         Pense fora da caixa, ofereça múltiplas perspectivas. \
         Respostas podem ser mais longas e elaboradas. \
         Use markdown para estruturar a resposta.".to_string()
    } else {
        "Você é um assistente de IA rodando no desktop do usuário. \
         Responda em português do Brasil. \
         Seja detalhado, use markdown para estruturar a resposta. \
         Use blocos de código com syntax highlighting quando relevante. \
         Você tem acesso a ferramentas para interagir com o sistema do usuário \
         (captura de tela, comandos, busca na web, etc.).".to_string()
    };

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".to_string(),
        content: serde_json::Value::String(text_system_prompt),
        tool_calls: None,
        tool_call_id: None,
    }];

    for msg in messages {
        let tool_calls: Option<Vec<OpenAIToolCall>> = msg.tool_calls.as_ref().map(|tcs| {
            tcs.iter().map(|tc| OpenAIToolCall {
                id: tc.id.clone(),
                call_type: tc.call_type.clone(),
                function: OpenAIToolFunction {
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                },
            }).collect()
        });

        openai_messages.push(OpenAIMessage {
            role: msg.role.clone(),
            content: serde_json::Value::String(msg.content.clone()),
            tool_calls,
            tool_call_id: msg.tool_call_id.clone(),
        });
    }

    let has_tool_history = messages.iter().any(|m| m.role == "tool");
    let max_tokens = match config.response_style.as_str() {
        "concise" => 1024,
        "detailed" => 4096,
        _ => if has_tool_history { 2048 } else { 2048 },
    };

    let request = OpenAIChatRequest {
        model: config.llm_model.clone(),
        messages: openai_messages,
        stream: true,
        max_tokens,
        temperature: config.temperature,
        chat_template_kwargs: serde_json::json!({
            "enable_thinking": true
        }),
        thinking_budget_tokens: 2048,
        tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
    };

    let llm_start = std::time::Instant::now();
    let resp = client
        .post(format!("{}/v1/chat/completions", config.llm_url))
        .json(&request)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Erro na API do LLM {}: {}", status, body).into());
    }

    let mut byte_stream = resp.bytes_stream();
    use tokio_stream::StreamExt;

    let mut full_response = String::new();
    let mut pending_tool_calls: Vec<ToolCallAccumulator> = Vec::new();
    let mut collected_tool_calls: Vec<ToolCall> = Vec::new();
    let _has_tools = !tools.is_empty();

    // XML tool call detection
    let xml_open_re = regex::Regex::new(r"<(?:\w+:)?tool_call>").unwrap();
    let xml_close_re = regex::Regex::new(r"</(?:\w+:)?tool_call>").unwrap();
    let mut xml_collecting = false;
    let mut xml_buffer = String::new();
    let acc_chars_for_xml: usize = 200;
    let mut xml_check_buffer = String::new();

    let mut first_content_token = false;
    let mut line_buffer = Vec::new();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk?;
        line_buffer.extend_from_slice(&chunk);

        while let Some(newline_pos) = line_buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = line_buffer.drain(..=newline_pos).collect();
            let line_str = String::from_utf8_lossy(&line);
            let line_str = line_str.trim();

            if line_str.is_empty() {
                continue;
            }

            if let Some(data) = line_str.strip_prefix("data: ") {
                if data == "[DONE]" {
                    break;
                }

                if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                    for choice in &chunk.choices {
                        if let Some(reason) = &choice.finish_reason {
                            if reason == "tool_calls" || reason == "stop" {
                                for acc in &pending_tool_calls {
                                    if let Ok(args) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&acc.arguments) {
                                        collected_tool_calls.push(ToolCall {
                                            id: acc.id.clone(),
                                            name: acc.name.clone(),
                                            arguments: args,
                                        });
                                    }
                                }
                                pending_tool_calls.clear();
                                continue;
                            }
                        }

                        if let Some(delta) = &choice.delta {
                            // Handle tool calls
                            if let Some(tool_call_deltas) = &delta.tool_calls {
                                for tc_delta in tool_call_deltas {
                                    while pending_tool_calls.len() <= tc_delta.index {
                                        pending_tool_calls.push(ToolCallAccumulator {
                                            id: String::new(),
                                            name: String::new(),
                                            arguments: String::new(),
                                        });
                                    }
                                    let acc = &mut pending_tool_calls[tc_delta.index];
                                    if let Some(ref id) = tc_delta.id {
                                        acc.id = id.clone();
                                    }
                                    if let Some(ref func) = tc_delta.function {
                                        if let Some(ref name) = func.name {
                                            acc.name = name.clone();
                                        }
                                        if let Some(ref args) = func.arguments {
                                            acc.arguments.push_str(args);
                                        }
                                    }
                                }
                            }

                            // Handle content → stream token by token
                            if let Some(content) = &delta.content {
                                if !content.is_empty() {
                                    if !first_content_token {
                                        first_content_token = true;
                                        eprintln!(
                                            "[chat-text] ttft_ms={}",
                                            llm_start.elapsed().as_millis()
                                        );
                                    }
                                    full_response.push_str(content);

                                    if xml_collecting {
                                        xml_buffer.push_str(content);
                                        if xml_close_re.is_match(&xml_buffer) {
                                            let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                            if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                                collected_tool_calls.extend(parsed);
                                            }
                                            xml_buffer.clear();
                                            xml_collecting = false;
                                        }
                                    } else {
                                        xml_check_buffer.push_str(content);
                                        let should_check = xml_check_buffer.len() >= acc_chars_for_xml || !content.contains(|c: char| c.is_alphanumeric());
                                        if xml_open_re.find(&xml_check_buffer).is_some() && should_check {
                                            let m = xml_open_re.find(&xml_check_buffer).unwrap();
                                            let before = xml_check_buffer[..m.start()].to_string();
                                            let after_tag = xml_check_buffer[m.end()..].to_string();
                                            xml_check_buffer.clear();
                                            if !before.is_empty() {
                                                let _ = token_tx.send(ChatTokenChunk { token: before }).await;
                                            }
                                            xml_buffer = after_tag;
                                            xml_collecting = true;
                                            if xml_close_re.is_match(&xml_buffer) {
                                                let full_xml_str = format!("<tool_call>{}</tool_call>", xml_buffer);
                                                if let Some(parsed) = parse_xml_tool_calls(&full_xml_str) {
                                                    collected_tool_calls.extend(parsed);
                                                }
                                                xml_buffer.clear();
                                                xml_collecting = false;
                                            }
                                        } else if !xml_check_buffer.is_empty() {
                                            let _ = token_tx.send(ChatTokenChunk { token: xml_check_buffer.clone() }).await;
                                            xml_check_buffer.clear();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Flush remaining XML check buffer
    if !xml_check_buffer.is_empty() && !xml_collecting {
        let _ = token_tx.send(ChatTokenChunk { token: xml_check_buffer }).await;
    }

    // Flush pending tool calls
    for acc in &pending_tool_calls {
        if !acc.name.is_empty() {
            if let Ok(args) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&acc.arguments) {
                collected_tool_calls.push(ToolCall {
                    id: acc.id.clone(),
                    name: acc.name.clone(),
                    arguments: args,
                });
            }
        }
    }

    if !collected_tool_calls.is_empty() {
        return Ok(ChatStreamResult::ToolCalls(collected_tool_calls, String::new(), false));
    }

    // Last-resort XML fallback
    if !tools.is_empty() && !full_response.is_empty() {
        if let Some(parsed) = parse_xml_tool_calls(&full_response) {
            if !parsed.is_empty() {
                return Ok(ChatStreamResult::ToolCalls(parsed, String::new(), true));
            }
        }
    }

    Ok(ChatStreamResult::Content(full_response.trim().to_string()))
}
```

> **ATENÇÃO:** Esta função depende de `ToolCallAccumulator`, `OpenAIStreamChunk`, `OpenAIStreamDelta`, e outras structs já definidas no mesmo arquivo. Nenhuma nova struct privada é necessária além de `ChatTokenChunk`.

**Verificação:** `cargo check` deve compilar.

---

## Passo 3 — Comando `send_chat_message` em `lib.rs`

**Arquivo:** `dexter/src-tauri/src/lib.rs`

**Local:** Após `stop_recording_and_process` (após linha 329), antes de `process_pipeline`

**INSERIR:**
```rust
#[tauri::command]
async fn send_chat_message(app: tauri::AppHandle, text: String) -> Result<(), String> {
    let config = {
        let state = app.state::<AppState>();
        state.config.lock().unwrap().clone()
    };

    // Add user message
    {
        let state = app.state::<AppState>();
        state.messages.lock().unwrap().push(ChatMessage {
            role: "user".to_string(),
            content: text.clone(),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let all_messages = app.state::<AppState>().messages.lock().unwrap().clone();
    let tools = voice::build_tools(&config.tools);
    let max_tool_rounds = 5;

    let (token_tx, mut token_rx) = tokio::sync::mpsc::channel::<voice::ChatTokenChunk>(64);

    let app_clone = app.clone();
    let config_clone = config.clone();

    let llm_handle = tokio::spawn(async move {
        let mut all_msgs = all_messages;
        for round in 0..max_tool_rounds {
            let result = voice::chat_streaming_text(
                &config_clone,
                &all_msgs,
                &tools,
                &token_tx,
            ).await.map_err(|e| format!("LLM: {}", e))?;

            match result {
                voice::ChatStreamResult::Content(text) => {
                    return Ok::<String, String>(text);
                }
                voice::ChatStreamResult::ToolCalls(tool_calls, preamble, xml_parsed) => {
                    if xml_parsed {
                        if !preamble.is_empty() {
                            let m = ChatMessage {
                                role: "assistant".to_string(),
                                content: preamble.clone(),
                                tool_calls: None,
                                tool_call_id: None,
                            };
                            all_msgs.push(m.clone());
                            app_clone.state::<AppState>().messages.lock().unwrap().push(m);
                        }

                        let mut tool_results = String::new();
                        for tool_call in &tool_calls {
                            let _ = app_clone.emit("processing", ProcessingState {
                                stage: "tool_call".to_string(),
                                text: tool_call.name.clone(),
                            });
                            let result_text = execute_tool(&app_clone, &config_clone, tool_call).await;
                            tool_results.push_str(&format!(
                                "[Resultado da ferramenta {}]: {}\n",
                                tool_call.name, result_text
                            ));
                        }

                        let follow_up = format!(
                            "Resultados das ferramentas:\n\n{}",
                            tool_results.trim()
                        );
                        let um = ChatMessage {
                            role: "user".to_string(),
                            content: follow_up,
                            tool_calls: None,
                            tool_call_id: None,
                        };
                        all_msgs.push(um.clone());
                        app_clone.state::<AppState>().messages.lock().unwrap().push(um);
                    } else {
                        let tool_calls_out: Vec<voice::ToolCallOut> = tool_calls.iter().map(|tc| tc.to_out()).collect();
                        let assistant_msg = ChatMessage {
                            role: "assistant".to_string(),
                            content: preamble.clone(),
                            tool_calls: Some(tool_calls_out.clone()),
                            tool_call_id: None,
                        };
                        all_msgs.push(assistant_msg.clone());
                        app_clone.state::<AppState>().messages.lock().unwrap().push(assistant_msg);

                        for tool_call in &tool_calls {
                            let _ = app_clone.emit("processing", ProcessingState {
                                stage: "tool_call".to_string(),
                                text: tool_call.name.clone(),
                            });
                            let result_text = execute_tool(&app_clone, &config_clone, tool_call).await;
                            let tool_msg = ChatMessage {
                                role: "tool".to_string(),
                                content: result_text,
                                tool_calls: None,
                                tool_call_id: Some(tool_call.id.clone()),
                            };
                            all_msgs.push(tool_msg.clone());
                            app_clone.state::<AppState>().messages.lock().unwrap().push(tool_msg);
                        }
                    }

                    let _ = app_clone.emit("processing", ProcessingState {
                        stage: "thinking".to_string(),
                        text: "Pensando...".to_string(),
                    });
                }
            }
        }
        // Max rounds hit
        let result = voice::chat_streaming_text(&config_clone, &all_msgs, &[], &token_tx)
            .await.map_err(|e| format!("LLM: {}", e))?;
        match result {
            voice::ChatStreamResult::Content(text) => Ok(text),
            _ => Err("Máximo de rodadas de ferramentas atingido".to_string()),
        }
    });

    drop(token_tx);

    // Stream tokens to frontend
    let mut full_text = String::new();
    while let Some(chunk) = token_rx.recv().await {
        full_text.push_str(&chunk.token);
        let _ = app.emit("chat_token", voice::ChatTokenChunk {
            token: chunk.token,
        });
    }

    let response = llm_handle.await.map_err(|e| format!("Task error: {}", e))?.map_err(|e| e)?;

    // Save assistant message
    {
        let state = app.state::<AppState>();
        state.messages.lock().unwrap().push(ChatMessage {
            role: "assistant".to_string(),
            content: response.clone(),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let _ = app.emit("chat_done", response);

    Ok(())
}
```

### 3.1 — Registrar `send_chat_message` no `invoke_handler`

**Local:** O `invoke_handler` que já foi modificado no Lote 1

**ADICIONAR `send_chat_message` à lista:**
```rust
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            get_default_config,
            list_models,
            send_chat_message,
            get_messages,
            // ... resto
        ])
```

### 3.2 — Adicionar serialização para `ChatTokenChunk`

**Arquivo:** `dexter/src-tauri/src/voice.rs`

**Verificação:** `ChatTokenChunk` já foi definido com `#[derive(Clone, Serialize)]`. Confirme que está correto.

**Verificação:** `cargo check` deve compilar.

---

## Passo 4 — Janela de chat no Tauri + atalho `Shift+T`

**Arquivo:** `dexter/src-tauri/src/lib.rs` — função `setup`

### 4.1 — Adicionar atalho `Shift+T`

**Local:** Após o atalho `Shift+C` (adicionado no Lote 1)

**INSERIR:**
```rust
            // Register Shift+T to open chat window
            app.global_shortcut().on_shortcut("Shift+T", |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    if let Some(window) = app.get_webview_window("chat") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    } else {
                        let url = tauri::WebviewUrl::App("index.html?view=chat".into());
                        let _ = WebviewWindowBuilder::new(app, "chat", url)
                            .title("Chronos — Chat")
                            .inner_size(780.0, 680.0)
                            .min_inner_size(480.0, 400.0)
                            .resizable(true)
                            .decorations(true)
                            .build();
                    }
                }
            })?;
```

> **Verificação:** `cargo check` deve compilar.

---

## Passo 5 — View `ChatView` no frontend

**Arquivo:** `dexter/src/App.tsx`

### 5.1 — Adicionar tipo `ChatMessageData` para mensagens do backend

**Local:** Após a interface `ChatBubble` (~linha 60), antes de `bubbleId`

**INSERIR:**
```typescript
interface ChatMessageData {
  role: string;
  content: string;
  tool_calls?: { id: string; type: string; function: { name: string; arguments: string } }[] | null;
  tool_call_id?: string | null;
}
```

### 5.2 — Adicionar função `ChatView`

**Local:** Após a função `Orb` (~linha 809), antes de `BubbleComponent`

**INSERIR a função completa:**

```tsx
function ChatView() {
  const [messages, setMessages] = useState<ChatMessageData[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [modelName, setModelName] = useState("");
  const chatEndRef = useRef<HTMLDivElement>(null);

  // Load existing messages and model name
  useEffect(() => {
    invoke<ChatMessageData[]>("get_messages").then(setMessages).catch(() => {});
    invoke<VoiceConfig>("get_config").then((c) => setModelName(c.llm_model)).catch(() => {});
  }, []);

  // Auto-scroll
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streaming]);

  // Listen for streaming tokens
  useEffect(() => {
    const unlisten = listen<{ token: string }>("chat_token", (event) => {
      setStreaming((prev) => prev + event.payload.token);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Listen for done signal
  useEffect(() => {
    const unlisten = listen<string>("chat_done", (event) => {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: event.payload },
      ]);
      setStreaming("");
      setIsLoading(false);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Listen for cleared messages
  useEffect(() => {
    const unlisten = listen("messages_cleared", () => {
      setMessages([]);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  const sendMessage = async () => {
    if (!input.trim() || isLoading) return;
    const text = input.trim();
    setInput("");
    setMessages((prev) => [...prev, { role: "user", content: text }]);
    setIsLoading(true);
    try {
      await invoke("send_chat_message", { text });
    } catch (e) {
      setMessages((prev) => [...prev, { role: "assistant", content: `Erro: ${e}` }]);
      setIsLoading(false);
    }
  };

  const clearChat = async () => {
    try {
      await invoke("clear_messages");
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className="h-screen flex flex-col" style={{ backgroundColor: "#111113" }}>
      {/* Header */}
      <div
        className="flex items-center justify-between px-4 py-3 border-b border-white/[0.06] shrink-0"
        style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
      >
        <h2 className="text-sm font-semibold text-white/80">Chronos Chat</h2>
        <div className="flex items-center gap-2" style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}>
          {modelName && (
            <span className="text-[10px] text-white/30 bg-white/[0.05] px-2 py-0.5 rounded-full border border-white/[0.04]">
              {modelName}
            </span>
          )}
          <button
            onClick={clearChat}
            className="text-[11px] text-white/30 hover:text-white/60 px-2 py-0.5 rounded transition-colors duration-150 cursor-pointer border-none bg-transparent"
          >
            Limpar
          </button>
        </div>
      </div>

      {/* Messages area */}
      <div className="flex-1 overflow-y-auto px-4 py-3 custom-scrollbar">
        {messages.length === 0 && !streaming && (
          <div className="flex items-center justify-center h-full">
            <div className="text-center text-white/20">
              <div className="text-4xl mb-3">💬</div>
              <p className="text-sm">Histórico compartilhado com o modo voz.</p>
              <p className="text-xs mt-1">Digite uma mensagem para começar.</p>
            </div>
          </div>
        )}
        {messages.map((msg, i) => (
          <ChatBubbleView key={i} role={msg.role} content={msg.content} />
        ))}
        {streaming && (
          <ChatBubbleView role="assistant" content={streaming} />
        )}
        {isLoading && !streaming && (
          <div className="text-white/20 text-xs px-1 py-2 animate-pulse">Pensando...</div>
        )}
        <div ref={chatEndRef} />
      </div>

      {/* Input area */}
      <div className="px-4 py-3 border-t border-white/[0.06] shrink-0">
        <div className="flex gap-2">
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                sendMessage();
              }
            }}
            placeholder="Digite sua mensagem... (Enter envia, Shift+Enter nova linha)"
            disabled={isLoading}
            className="flex-1 bg-white/[0.04] border border-white/[0.08] text-white/90 px-4 py-2.5 rounded-lg text-sm outline-none transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.06] placeholder:text-white/15 disabled:opacity-40"
          />
          <button
            onClick={sendMessage}
            disabled={isLoading || !input.trim()}
            className="px-5 py-2.5 rounded-lg bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 disabled:opacity-30 disabled:cursor-not-allowed transition-all duration-200 cursor-pointer border-none shrink-0"
          >
            Enviar
          </button>
        </div>
        <p className="text-[10px] text-white/15 mt-1.5 text-center">
          Shift+T para abrir · Shift+Z para voz · Shift+C para limpar
        </p>
      </div>
    </div>
  );
}
```

### 5.3 — Adicionar componente `ChatBubbleView`

**Local:** Após `ChatView`, antes de `BubbleComponent`

```tsx
function ChatBubbleView({ role, content }: { role: string; content: string }) {
  const isUser = role === "user";
  const isTool = role === "tool";

  if (isTool) return null; // Hide tool messages in chat view

  return (
    <div className={`mb-4 ${isUser ? "flex justify-end" : "flex justify-start"}`}>
      <div
        className={`max-w-[82%] px-4 py-3 rounded-2xl text-sm leading-relaxed whitespace-pre-wrap break-words ${
          isUser
            ? "bg-blue-600/25 text-white/90 rounded-br-md border border-blue-500/10"
            : "bg-white/[0.04] text-white/85 rounded-bl-md border border-white/[0.05]"
        }`}
      >
        {content}
      </div>
    </div>
  );
}
```

### 5.4 — Integrar no roteamento `App`

**Local:** Função `App` (~linha 858)

**ANTES:**
```tsx
function App() {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");

  if (view === "settings") {
    return <Settings />;
  }
  return <Orb />;
}
```

**DEPOIS:**
```tsx
function App() {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");

  if (view === "settings") {
    return <Settings />;
  }
  if (view === "chat") {
    return <ChatView />;
  }
  return <Orb />;
}
```

---

## Passo 6 — Estilos CSS para o chat

**Arquivo:** `dexter/src/App.css`

**Adicionar ao final do arquivo:**

```css
/* ── Chat View ── */

/* Custom scrollbar for chat messages */
.custom-scrollbar::-webkit-scrollbar {
  width: 4px;
}
.custom-scrollbar::-webkit-scrollbar-track {
  background: transparent;
}
.custom-scrollbar::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.06);
  border-radius: 2px;
}
.custom-scrollbar::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.1);
}

/* Chat bubble animations */
@keyframes fadeInUp {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* Smooth background for chat window */
body {
  background-color: #111113;
}
```

**Verificação:** `npm run build` deve compilar sem erros.

---

## Checklist de verificação do Lote 2

```bash
# No diretório dexter/src-tauri/
cargo check

# No diretório dexter/
npm run build
```

- [ ] `cargo check` passa sem erros
- [ ] `npm run build` passa sem erros
- [ ] `Shift+T` abre a janela de chat
- [ ] Digitar e enviar mensagem mostra resposta em streaming
- [ ] Mensagens do histórico de voz aparecem no chat (compartilhado)
- [ ] `Limpar` no chat limpa o histórico
- [ ] `Shift+C` limpa e reflete no chat também
- [ ] Tool calls funcionam no chat (ex: "que horas são?")
- [ ] O modo voz continua funcionando normalmente após abrir o chat

---

# LOTE 3 — Extras e Acabamento

```
Pré-requisito: Lotes 1 e 2 completos e funcionais
Prazo estimado: ~20-30 minutos
```

## O que este lote implementa

- Som de feedback (beep) ao abrir microfone
- Botão "Exportar conversa"
- Pequenas melhorias visuais

## O que NÃO modificar

- Não alterar lógica dos pipelines de voz ou chat
- Não adicionar novos campos ao `VoiceConfig`

---

## Passo 1 — Som de feedback do microfone

**Arquivo:** `dexter/src-tauri/src/voice.rs`

### 1.1 — Função `play_beep`

**Inserir no topo do arquivo, após os imports:**

```rust
/// Play a short feedback beep via Windows beep (system speaker).
/// Only plays if audio_feedback is enabled in config.
pub fn play_mic_beep(config: &VoiceConfig) {
    if !config.audio_feedback {
        return;
    }
    // Windows only: use system beep (frequency, duration_ms)
    #[cfg(windows)]
    {
        use std::process::Command;
        // PowerShell Beep: [Console]::Beep(frequency, duration_ms)
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "[Console]::Beep(800, 80)",
            ])
            .spawn();
    }
}
```

### 1.2 — Chamar no início da gravação

**Local:** Em `record_audio`, após `stream.play()?;` (~linha 62)

**INSERIR:**
```rust
    // Play mic-open beep if enabled
    {
        let state = app_clone.state::<AppState>();
        let cfg = state.config.lock().unwrap().clone();
        play_mic_beep(&cfg);
    }
```

### 1.3 — Chamar ao soltar o atalho (fim da gravação)

**Local:** Em `lib.rs`, no handler `ShortcutState::Released` do `Shift+Z` (~linha 1075)

**INSERIR após `let _ = app.emit("hotkey_released", ());`:**
```rust
                        // Play mic-close beep if enabled
                        {
                            let state = app.state::<AppState>();
                            let cfg = state.config.lock().unwrap();
                            voice::play_mic_beep(&cfg);
                        }
```

**Verificação:** `cargo check`

---

## Passo 2 — Exportar conversa

**Arquivo:** `dexter/src-tauri/src/lib.rs`

### 2.1 — Adicionar comando

**INSERIR (após `get_default_config` ou `send_chat_message`):**
```rust
#[tauri::command]
fn export_conversation(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let messages = app.state::<AppState>().messages.lock().unwrap().clone();
    let mut content = String::from("# Chronos — Conversa Exportada\n\n");
    for msg in &messages {
        let role_label = match msg.role.as_str() {
            "user" => "👤 Você",
            "assistant" => "🤖 Chronos",
            "tool" => "🔧 Ferramenta",
            _ => &msg.role,
        };
        content.push_str(&format!("## {}\n\n{}\n\n---\n\n", role_label, msg.content));
    }
    std::fs::write(&path, content)
        .map_err(|e| format!("Falha ao salvar: {}", e))?;
    Ok(())
}
```

### 2.2 — Registrar no `invoke_handler`

```rust
export_conversation,
```

### 2.3 — Botão no frontend (ChatView)

**Local:** No header do `ChatView`, ao lado do botão "Limpar"

**INSERIR:**
```tsx
          <button
            onClick={async () => {
              try {
                // Use Tauri dialog to pick save path
                const { save } = await import("@tauri-apps/plugin-dialog");
                const path = await save({
                  defaultPath: "chronos-conversa.md",
                  filters: [{ name: "Markdown", extensions: ["md"] }, { name: "Texto", extensions: ["txt"] }],
                });
                if (path) {
                  await invoke("export_conversation", { path });
                }
              } catch (e) {
                console.error("Export error:", e);
              }
            }}
            className="text-[11px] text-white/30 hover:text-white/60 px-2 py-0.5 rounded transition-colors duration-150 cursor-pointer border-none bg-transparent"
          >
            Exportar
          </button>
```

**Verificação:** `cargo check` + `npm run build`

---

## Checklist de verificação do Lote 3

- [ ] `cargo check` passa
- [ ] `npm run build` passa
- [ ] Beep toca ao pressionar Shift+Z (se `audio_feedback` ligado)
- [ ] Beep toca ao soltar Shift+Z
- [ ] Botão "Exportar" no chat salva arquivo `.md` corretamente
- [ ] Conteúdo exportado contém todas as mensagens com roles

---

# Apêndice

## O que NUNCA modificar

- `dexter/start-all.ps1` — script de infraestrutura, mexa apenas se for adicionar o servidor de embedding
- `dexter/src-tauri/tauri.conf.json` — configuração do Tauri
- `dexter/src-tauri/src/media_controls.rs` — controles de mídia
- `dexter/src-tauri/src/sandbox.rs` — sandbox de comandos
- `dexter/chatterbox-tts-api/` — serviço TTS Python
- `dexter/package.json` (exceto para instalar dependências no Lote 2)

## Compatibilidade com config.json existente

Todos os novos campos usam `#[serde(default)]` ou `#[serde(default = "...")]`. Isso significa que um `config.json` antigo (sem os novos campos) será carregado sem erro — os novos campos receberão seus valores padrão.

## Debug: logs de performance

Os logs `[perf]` e `[chat-text]` já existem no código. Se precisar depurar, execute o app via terminal e observe o stderr.

## Modelo de embedding recomendado

BGE-M3 em GGUF: https://huggingface.co/bartowski/bge-m3-GGUF

Comando para iniciar servidor de embedding dedicado:
```powershell
llama-server.exe -m bge-m3-Q4_K_M.gguf --port 8082 --embeddings -ngl 0 -c 512
```
