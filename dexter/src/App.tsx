import { useCallback, useEffect, useLayoutEffect, useState, useRef, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open, save } from "@tauri-apps/plugin-dialog";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import "./App.css";

/** Start next WAV chunk this many seconds before the current clip ends (reduces silent gap vs strict onended). */
const CHUNK_PLAYBACK_PREROLL_SEC = 0.1;

/** Same text as console — also printed by the Rust host so start-all / cargo logs capture it. */
function echoPerfToHost(line: string) {
  void invoke("log_frontend_perf", { line }).catch(() => {});
}

interface ProcessingState {
  stage: string;
  text: string;
}

interface ToolsConfig {
  search_knowledge: boolean;
  screenshot: boolean;
  read_clipboard: boolean;
  open_url: boolean;
  get_current_time: boolean;
  list_apps: boolean;
  run_command: boolean;
  web_fetch: boolean;
  launch_desktop_app: boolean;
  media_controls: boolean;
}

interface SandboxConfig {
  mode: "Guarded" | "Docker";
  timeout_secs: number;
  readable_paths: string[];
  workspace: string;
  docker_image: string;
  allow_network: boolean;
}

interface VoiceConfig {
  whisper_model_path: string;
  whisper_url: string;
  llm_url: string;
  llm_url_voice?: string;
  llm_url_text?: string;
  embed_url: string;
  vision_url: string;
  llm_model: string;
  llm_model_voice?: string;
  llm_model_text?: string;
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
  shortcut_voice: string;
  shortcut_hide: string;
  shortcut_clear: string;
  shortcut_chat: string;
  shortcut_settings: string;
  shortcut_stop: string;
  /** Pastas extra para procurar ficheiros de música (local). */
  music_library_paths: string;
  tools: ToolsConfig;
  sandbox: SandboxConfig;
}

interface AudioChunk {
  index: number;
  audio: string;
}

interface ChatBubble {
  role: "user" | "assistant" | "status" | "tool";
  text: string;
  id: number;
}

interface ChatMessageData {
  role: string;
  content: string;
  created_at_ms?: number;
  elapsed_ms?: number | null;
  tool_calls?: { id: string; type: string; function: { name: string; arguments: string } }[] | null;
  tool_call_id?: string | null;
}

interface ChatDonePayload {
  response: string;
  elapsed_ms: number;
}

type SettingsTab = "config" | "tools" | "knowledge";

type ShortcutFieldKey =
  | "shortcut_voice"
  | "shortcut_hide"
  | "shortcut_clear"
  | "shortcut_chat"
  | "shortcut_settings"
  | "shortcut_stop";

const SHORTCUT_FIELD_KEYS: ShortcutFieldKey[] = [
  "shortcut_voice",
  "shortcut_hide",
  "shortcut_clear",
  "shortcut_chat",
  "shortcut_settings",
  "shortcut_stop",
];

function pickShortcuts(c: VoiceConfig): Record<ShortcutFieldKey, string> {
  return {
    shortcut_voice: c.shortcut_voice ?? "",
    shortcut_hide: c.shortcut_hide ?? "",
    shortcut_clear: c.shortcut_clear ?? "",
    shortcut_chat: c.shortcut_chat ?? "",
    shortcut_settings: c.shortcut_settings ?? "",
    shortcut_stop: c.shortcut_stop ?? "",
  };
}

function shortcutsDifferFromBaseline(c: VoiceConfig, baseline: Record<ShortcutFieldKey, string>): boolean {
  const cur = pickShortcuts(c);
  return SHORTCUT_FIELD_KEYS.some((k) => cur[k].trim() !== baseline[k].trim());
}

/** Tecla principal no formato esperado pelo plugin (enum `Code` do global-hotkey). */
function keyboardPhysicalCodeToToken(code: string): string | null {
  const modCodes = new Set([
    "ControlLeft",
    "ControlRight",
    "ShiftLeft",
    "ShiftRight",
    "AltLeft",
    "AltRight",
    "MetaLeft",
    "MetaRight",
  ]);
  if (modCodes.has(code)) return null;

  if (code.startsWith("Key") && code.length === 4) return code.slice(3);
  if (code.startsWith("Digit") && code.length === 6) return code;
  return code;
}

function formatShortcutFromKeyboardEvent(ev: KeyboardEvent): string | null {
  const token = keyboardPhysicalCodeToToken(ev.code);
  if (!token) return null;

  const parts: string[] = [];
  if (ev.ctrlKey) parts.push("Ctrl");
  if (ev.altKey) parts.push("Alt");
  if (ev.shiftKey) parts.push("Shift");
  if (ev.metaKey) parts.push("Super");

  parts.push(token);
  return parts.join("+");
}

let bubbleId = 0;

const TOOL_LABEL_MAP: Record<string, string> = {
  take_screenshot: "Capturando tela",
  search_knowledge: "Buscando conhecimento",
  read_clipboard: "Lendo área de transferência",
  open_url: "Abrindo URL",
  get_current_time: "Verificando horário",
  list_running_apps: "Listando apps",
  run_command: "Executando comando",
  web_fetch: "Buscando página web",
  fetch_weather: "Consultando clima",
  fetch_fx_quote: "Consultando cotação",
  launch_desktop_app: "Abrindo aplicativo",
  close_desktop_app: "Fechando aplicativo",
  control_media_playback: "Controlando reprodução",
  adjust_system_volume: "Ajustando volume",
  play_music_query: "Abrindo música",
  play_local_music_playlist: "Montando playlist",
  play_full_local_music_library: "Exportar M3U (varredura)",
  native_music_library_shuffle_play: "Biblioteca no reprodutor",
};

/* ─────────────────────────── Settings: Config Tab ─────────────────────────── */

function mergeUniqueMusicPaths(existing: string, additions: string[]): string {
  const normKey = (s: string) =>
    s
      .trim()
      .replace(/[/\\]+$/, "")
      .toLowerCase();
  const keys = new Set<string>();
  const ordered: string[] = [];
  const push = (raw: string) => {
    const display = raw.trim().replace(/[/\\]+$/, "");
    if (!display) return;
    const k = normKey(display);
    if (keys.has(k)) return;
    keys.add(k);
    ordered.push(display);
  };
  for (const part of existing.split(/\r?\n|[|;]/)) {
    push(part);
  }
  for (const a of additions) {
    push(a);
  }
  return ordered.join("\n");
}

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

function ShortcutCaptureField({
  label,
  value,
  onChange,
  fieldKey,
  activeField,
  setActiveField,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  fieldKey: ShortcutFieldKey;
  activeField: ShortcutFieldKey | null;
  setActiveField: (k: ShortcutFieldKey | null) => void;
}) {
  const isActive = activeField === fieldKey;

  const cancelCapture = async () => {
    setActiveField(null);
    try {
      await invoke("resume_global_shortcuts");
    } catch {
      /* ignore */
    }
  };

  const beginCapture = async () => {
    try {
      if (activeField === null) {
        await invoke("pause_global_shortcuts");
      }
      setActiveField(fieldKey);
    } catch (e) {
      console.error("pause_global_shortcuts:", e);
    }
  };

  useEffect(() => {
    if (!isActive) return;

    const onKeyDown = async (ev: KeyboardEvent) => {
      ev.preventDefault();
      ev.stopImmediatePropagation();

      if (ev.key === "Escape") {
        await cancelCapture();
        return;
      }

      const combo = formatShortcutFromKeyboardEvent(ev);
      if (!combo) return;

      onChange(combo);
      setActiveField(null);
      try {
        await invoke("resume_global_shortcuts");
      } catch {
        /* ignore */
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- cancelCapture fecha sobre setActiveField estável
  }, [isActive, onChange, setActiveField]);

  return (
    <div className="flex flex-col gap-1.5" aria-label={`Atalho: ${label}`}>
      <div className="flex flex-wrap items-center gap-2">
        <code className="flex-1 min-w-[120px] text-[12px] text-white/70 bg-black/30 border border-white/10 rounded-md px-2.5 py-2 font-mono truncate">
          {value.trim() || "—"}
        </code>
        <button
          type="button"
          onClick={() => {
            if (isActive) void cancelCapture();
            else void beginCapture();
          }}
          className={`shrink-0 px-3 py-2 rounded-lg text-[12px] font-medium border transition-colors duration-150 ${
            isActive
              ? "border-amber-500/50 bg-amber-500/15 text-amber-200/90 hover:bg-amber-500/25"
              : "border-blue-500/40 bg-blue-500/15 text-blue-200/90 hover:bg-blue-500/25"
          }`}
        >
          {isActive ? "Cancelar" : "Definir atalho"}
        </button>
      </div>
      {isActive && (
        <p className="text-[11px] text-amber-200/70 leading-snug">
          Pressione a combinação desejada. <span className="text-white/40">Esc</span> cancela.
        </p>
      )}
    </div>
  );
}

function ConfigTab({
  config,
  setConfig,
}: {
  config: VoiceConfig;
  setConfig: (c: VoiceConfig) => void;
}) {
  const [activeShortcutField, setActiveShortcutField] = useState<ShortcutFieldKey | null>(null);

  useEffect(() => {
    return () => {
      void invoke("resume_global_shortcuts").catch(() => {});
    };
  }, []);

  const pickMusicFolders = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: true,
        title: "Pastas de música",
      });
      if (selected == null) return;
      const picked = Array.isArray(selected) ? selected : [selected];
      setConfig({
        ...config,
        music_library_paths: mergeUniqueMusicPaths(config.music_library_paths ?? "", picked),
      });
    } catch {
      /* diálogo indisponível fora do Tauri */
    }
  };

  return (
    <div className="flex flex-col gap-5 p-5 px-6">
      <FieldGroup title="Reconhecimento de fala">
        <Field label="Caminho do modelo Whisper">
          <Input value={config.whisper_model_path} onChange={(v) => setConfig({ ...config, whisper_model_path: v })} />
        </Field>
        <Field label="URL do servidor Whisper">
          <Input value={config.whisper_url} onChange={(v) => setConfig({ ...config, whisper_url: v })} placeholder="http://localhost:8081" />
        </Field>
      </FieldGroup>

      <FieldGroup title="Modelo de linguagem (llama.cpp)">
        <Field label="URL do servidor LLM">
          <Input value={config.llm_url} onChange={(v) => setConfig({ ...config, llm_url: v })} />
        </Field>
        <Field label="Modelo de chat (fallback)">
          <ModelSelect value={config.llm_model} onChange={(v) => setConfig({ ...config, llm_model: v })} llmUrl={config.llm_url} placeholder="Selecione o modelo de chat..." />
        </Field>
        <Field label="URL LLM voz (opcional, ex. http://127.0.0.1:8080)">
          <Input value={config.llm_url_voice ?? ""} onChange={(v) => setConfig({ ...config, llm_url_voice: v })} placeholder="Vazio = usa URL acima" />
        </Field>
        <Field label="Modelo de voz (opcional)">
          <ModelSelect value={config.llm_model_voice ?? ""} onChange={(v) => setConfig({ ...config, llm_model_voice: v })} llmUrl={config.llm_url_voice || config.llm_url} placeholder="Llama 8B — vazio = modelo acima" />
        </Field>
        <Field label="URL LLM chat texto (opcional, ex. http://127.0.0.1:8084)">
          <Input value={config.llm_url_text ?? ""} onChange={(v) => setConfig({ ...config, llm_url_text: v })} placeholder="Qwen 35B com -WithTextLlm" />
        </Field>
        <Field label="Modelo de chat texto (opcional)">
          <ModelSelect value={config.llm_model_text ?? ""} onChange={(v) => setConfig({ ...config, llm_model_text: v })} llmUrl={config.llm_url_text || config.llm_url} placeholder="Vazio = modelo de chat acima" />
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

        <Field label="Estilo de resposta (chat texto)">
          <select
            value={config.response_style}
            onChange={(e) => setConfig({ ...config, response_style: e.target.value })}
            className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50"
          >
            <option value="concise">Conciso — respostas curtas e diretas</option>
            <option value="normal">Normal — equilibrado</option>
            <option value="detailed">Detalhado — explicações completas</option>
          </select>
          <p className="text-[11px] text-white/30 leading-relaxed mt-1.5">
            O modo voz (Shift+Z) usa sempre respostas curtas (~2–3 frases), independentemente desta opção.
          </p>
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

      <FieldGroup title="Visão (screenshots e webcam)">
        <Field label="URL do servidor de visão">
          <Input value={config.vision_url} onChange={(v) => setConfig({ ...config, vision_url: v })} placeholder="http://localhost:8083 (usa LLM se vazio)" />
        </Field>
        <Field label="Modelo de visão">
          <ModelSelect value={config.vision_model} onChange={(v) => setConfig({ ...config, vision_model: v })} llmUrl={config.vision_url || config.llm_url} placeholder="qwen2.5-vl-3b-instruct (recomendado)" />
        </Field>
        <p className="text-[11px] text-white/30 leading-relaxed -mt-1">
          Recomendado: Qwen2.5-VL 3B (~2 GB em Q4_K_M). Servidor dedicado na porta 8083. Suporta screenshots e webcam.
        </p>
      </FieldGroup>

      <FieldGroup title="Síntese de voz">
        <Field label="URL do Chatterbox">
          <Input value={config.chatterbox_url} onChange={(v) => setConfig({ ...config, chatterbox_url: v })} />
        </Field>
        <Field label="Voz">
          <Input value={config.chatterbox_voice} onChange={(v) => setConfig({ ...config, chatterbox_voice: v })} />
        </Field>
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
      </FieldGroup>

      <FieldGroup title="Música local">
        <Field label="Pastas de música">
          <div className="flex flex-col gap-2">
            <button
              type="button"
              onClick={() => void pickMusicFolders()}
              className="self-start px-3 py-2 rounded-lg text-[13px] font-medium border border-white/15 bg-white/[0.06] text-white/85 hover:bg-white/[0.1] hover:border-white/25 cursor-pointer transition-all duration-200"
              title="Escolher uma ou mais pastas no explorador"
            >
              Escolher pasta…
            </button>
            <textarea
              id="dexter-music-library-paths"
              value={config.music_library_paths ?? ""}
              onChange={(e) => setConfig({ ...config, music_library_paths: e.target.value })}
              rows={4}
              spellCheck={false}
              placeholder={"D:\\Música\nE:\\Media\\MP3"}
              aria-label="Lista de pastas de música (uma por linha)"
              className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-mono outline-none resize-y min-h-[88px] transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] placeholder:text-white/25"
            />
          </div>
        </Field>
        <p className="text-[12px] text-white/45 leading-relaxed -mt-2">
          Basta indicar a <span className="text-white/70">pasta raiz</span> (ex.: Música) — o Chronos procura <span className="text-white/70">em todas as subpastas</span> (artistas, álbuns, etc.); você não precisa
          adicionar cada pasta de artista. Você pode usar <span className="text-white/70">Escolher pasta</span> ou editar a lista manualmente (uma por linha, ou <span className="text-white/55">;</span> /{" "}
          <span className="text-white/55">|</span>). Junta-se à pasta Música do sistema.{" "}
          <span className="text-white/55 font-mono text-[11px]">DEXTER_MUSIC_PATHS</span> continua válida em paralelo.
        </p>
      </FieldGroup>

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
              placeholder={`Prompt usado no modo chat de texto (${config.shortcut_chat || "Shift+T"})`}
              className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-inherit outline-none resize-y min-h-[80px] transition-all duration-200 focus:border-blue-500/50 placeholder:text-white/20"
            />
          </Field>
        )}
      </FieldGroup>

      <FieldGroup title="Atalhos de teclado">
        <p className="text-[11px] text-white/30 leading-relaxed -mt-1">
          Use <span className="text-white/45">Definir atalho</span> e pressione a combinação desejada (os atalhos globais pausam durante a captura). Depois de salvar alterações nos atalhos, reinicie o app para aplicar por completo.
        </p>
        <Field label="Gravar voz">
          <ShortcutCaptureField
            label="Gravar voz"
            value={config.shortcut_voice}
            onChange={(v) => setConfig({ ...config, shortcut_voice: v })}
            fieldKey="shortcut_voice"
            activeField={activeShortcutField}
            setActiveField={setActiveShortcutField}
          />
        </Field>
        <Field label="Esconder janela">
          <ShortcutCaptureField
            label="Esconder janela"
            value={config.shortcut_hide}
            onChange={(v) => setConfig({ ...config, shortcut_hide: v })}
            fieldKey="shortcut_hide"
            activeField={activeShortcutField}
            setActiveField={setActiveShortcutField}
          />
        </Field>
        <Field label="Limpar conversa">
          <ShortcutCaptureField
            label="Limpar conversa"
            value={config.shortcut_clear}
            onChange={(v) => setConfig({ ...config, shortcut_clear: v })}
            fieldKey="shortcut_clear"
            activeField={activeShortcutField}
            setActiveField={setActiveShortcutField}
          />
        </Field>
        <Field label="Abrir chat">
          <ShortcutCaptureField
            label="Abrir chat"
            value={config.shortcut_chat}
            onChange={(v) => setConfig({ ...config, shortcut_chat: v })}
            fieldKey="shortcut_chat"
            activeField={activeShortcutField}
            setActiveField={setActiveShortcutField}
          />
        </Field>
        <Field label="Configurações">
          <ShortcutCaptureField
            label="Configurações"
            value={config.shortcut_settings}
            onChange={(v) => setConfig({ ...config, shortcut_settings: v })}
            fieldKey="shortcut_settings"
            activeField={activeShortcutField}
            setActiveField={setActiveShortcutField}
          />
        </Field>
        <Field label="Parar voz / leitura">
          <ShortcutCaptureField
            label="Parar voz / leitura"
            value={config.shortcut_stop ?? "Ctrl+5"}
            onChange={(v) => setConfig({ ...config, shortcut_stop: v })}
            fieldKey="shortcut_stop"
            activeField={activeShortcutField}
            setActiveField={setActiveShortcutField}
          />
        </Field>
      </FieldGroup>
    </div>
  );
}

/* ─────────────────────────── Settings: Tools Tab ─────────────────────────── */

const TOOL_DEFINITIONS: { key: keyof ToolsConfig; name: string; desc: string; icon: string }[] = [
  { key: "screenshot", name: "Captura de tela", desc: "Captura e descreve o que aparece na sua tela", icon: "📸" },
  { key: "read_clipboard", name: "Ler área de transferência", desc: "Lê o texto atual da área de transferência", icon: "📋" },
  { key: "search_knowledge", name: "Busca na base de conhecimento", desc: "Pesquisa na sua base local para dar contexto", icon: "🔍" },
  { key: "open_url", name: "Abrir URL", desc: "Abre sites no navegador padrão", icon: "🌐" },
  { key: "get_current_time", name: "Data e hora", desc: "Obtém data, hora e dia da semana atuais", icon: "🕐" },
  { key: "list_apps", name: "Apps em execução", desc: "Lista aplicativos em execução no momento", icon: "🖥" },
  { key: "web_fetch", name: "Buscar na web", desc: "Baixa e lê páginas para obter informação", icon: "🕸" },
  { key: "run_command", name: "Comando no shell", desc: "Executa comandos de terminal no seu PC", icon: "⚡" },
  { key: "launch_desktop_app", name: "Abrir / fechar apps", desc: "Abre ou fecha Cursor, VS Code, Terminal, navegadores, Office etc.", icon: "🚀" },
  { key: "media_controls", name: "Mídia e volume", desc: "Play/pause/pular música ou vídeo (sessão do sistema) e volume principal", icon: "🎵" },
];

function ToolsTab({ config, setConfig }: { config: VoiceConfig; setConfig: (c: VoiceConfig) => void }) {
  const toggleTool = (key: keyof ToolsConfig) => {
    setConfig({ ...config, tools: { ...config.tools, [key]: !config.tools[key] } });
  };

  const setSandbox = (patch: Partial<SandboxConfig>) => {
    setConfig({ ...config, sandbox: { ...config.sandbox, ...patch } });
  };

  const enabledCount = TOOL_DEFINITIONS.filter((t) => config.tools[t.key]).length;

  return (
    <div className="flex flex-col gap-5 p-5 px-6">
      <p className="text-[13px] text-white/40 leading-relaxed flex items-center gap-2">
        Ative ou desative as ferramentas que o assistente pode usar.
        <span className="text-[11px] text-cyan-400/60 bg-cyan-400/[0.08] px-2 py-0.5 rounded">
          {enabledCount}/{TOOL_DEFINITIONS.length} ativas
        </span>
      </p>

      <div className="flex flex-col gap-1">
        {TOOL_DEFINITIONS.map((tool) => {
          const enabled = config.tools[tool.key];
          return (
            <div
              key={tool.key}
              className={`flex items-center gap-3 px-4 py-3.5 rounded-xl border transition-all duration-200 ${
                enabled
                  ? "bg-white/[0.03] border-white/[0.06]"
                  : "bg-white/[0.01] border-white/[0.03] opacity-50"
              }`}
            >
              <span className="text-xl w-8 text-center shrink-0">{tool.icon}</span>
              <div className="flex-1 min-w-0">
                <div className="text-[13px] font-medium text-white/85">{tool.name}</div>
                <div className="text-[11px] text-white/30 leading-relaxed mt-0.5">{tool.desc}</div>
              </div>
              <Toggle on={enabled} onToggle={() => toggleTool(tool.key)} />
            </div>
          );
        })}
      </div>

      {config.tools.run_command && (
        <FieldGroup title="Sandbox do shell">
          <p className="text-[12px] text-white/30 leading-relaxed -mt-1">
            Os comandos são validados, o ambiente é sanitizado e todas as execuções ficam registradas.
          </p>

          <div className="flex gap-1 bg-white/[0.04] rounded-lg p-0.5">
            {(["Guarded", "Docker"] as const).map((mode) => (
              <button
                key={mode}
                onClick={() => setSandbox({ mode })}
                className={`flex-1 py-2 px-3 rounded-md text-[13px] font-medium border-none cursor-pointer transition-all duration-200 ${
                  config.sandbox.mode === mode
                    ? "bg-blue-500/30 text-white/90"
                    : "bg-transparent text-white/40 hover:text-white/60"
                }`}
              >
                {mode === "Guarded" ? "Protegido" : "Docker"}
              </button>
            ))}
          </div>

          <p className="text-[11px] text-white/25 leading-relaxed">
            {config.sandbox.mode === "Guarded"
              ? "Workspace isolado, ambiente sanitizado e comandos perigosos bloqueados."
              : "Contêiner Docker com limites de memória/CPU e raiz somente leitura. Exige Docker Desktop."}
          </p>

          <Field label="Pasta de trabalho">
            <Input value={config.sandbox.workspace} onChange={(v) => setSandbox({ workspace: v })} />
          </Field>
          <Field label="Tempo limite (segundos)">
            <input
              type="number"
              value={config.sandbox.timeout_secs}
              onChange={(e) => setSandbox({ timeout_secs: parseInt(e.target.value) || 30 })}
              className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07]"
            />
          </Field>

          {config.sandbox.mode === "Docker" && (
            <>
              <Field label="Imagem Docker">
                <Input value={config.sandbox.docker_image} onChange={(v) => setSandbox({ docker_image: v })} />
              </Field>
              <Field label="Pastas legíveis (montadas como somente leitura)">
                <textarea
                  value={config.sandbox.readable_paths.join("\n")}
                  onChange={(e) => setSandbox({ readable_paths: e.target.value.split("\n").filter(Boolean) })}
                  rows={3}
                  placeholder={"~/Documents\n~/Desktop\n~/Downloads"}
                  className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-inherit outline-none resize-y transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] placeholder:text-white/20"
                />
              </Field>
              <div className="flex items-center gap-3 px-4 py-3 rounded-xl bg-white/[0.02] border border-white/[0.04]">
                <div className="flex-1">
                  <div className="text-[13px] font-medium text-white/80">Permitir rede</div>
                  <div className="text-[11px] text-white/30 mt-0.5">Permite que os comandos acessem a internet</div>
                </div>
                <Toggle on={config.sandbox.allow_network} onToggle={() => setSandbox({ allow_network: !config.sandbox.allow_network })} />
              </div>
            </>
          )}
        </FieldGroup>
      )}
    </div>
  );
}

/* ─────────────────────────── Settings: Knowledge Tab ─────────────────────────── */

function KnowledgeTab() {
  const [sources, setSources] = useState<[string, number][]>([]);
  const [ingesting, setIngesting] = useState(false);
  const [textSource, setTextSource] = useState("");
  const [textContent, setTextContent] = useState("");
  const [status, setStatus] = useState("");

  const loadSources = async () => {
    try {
      const result = await invoke<[string, number][]>("list_knowledge_sources");
      setSources(result);
    } catch (e) {
      console.error(e);
    }
  };

  useEffect(() => { loadSources(); }, []);

  const ingestText = async () => {
    if (!textSource.trim() || !textContent.trim()) return;
    setIngesting(true);
    setStatus("");
    try {
      const chunks = await invoke<number>("ingest_text", { source: textSource, text: textContent });
      setStatus(`${chunks} trechos indexados de "${textSource}"`);
      setTextSource("");
      setTextContent("");
      loadSources();
    } catch (e) {
      setStatus(`Erro: ${e}`);
    }
    setIngesting(false);
  };

  const ingestFile = async () => {
    setIngesting(true);
    setStatus("");
    try {
      const path = prompt("Digite o caminho do arquivo:");
      if (!path) { setIngesting(false); return; }
      const chunks = await invoke<number>("ingest_file", { path });
      setStatus(`${chunks} trechos indexados`);
      loadSources();
    } catch (e) {
      setStatus(`Erro: ${e}`);
    }
    setIngesting(false);
  };

  const deleteSource = async (source: string) => {
    try {
      await invoke("delete_knowledge_source", { source });
      loadSources();
    } catch (e) {
      setStatus(`Erro: ${e}`);
    }
  };

  return (
    <div className="flex flex-col gap-5 p-5 px-6">
      <p className="text-[13px] text-white/40 leading-relaxed">
        Adicione documentos para o assistente consultar durante a conversa.
      </p>

      <FieldGroup title="Adicionar texto">
        <Field label="Nome da fonte">
          <Input value={textSource} onChange={setTextSource} placeholder="ex.: notas-do-projeto" />
        </Field>
        <Field label="Conteúdo">
          <textarea
            value={textContent}
            onChange={(e) => setTextContent(e.target.value)}
            rows={5}
            placeholder="Cole o texto aqui..."
            className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] font-inherit outline-none resize-y min-h-[80px] transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] placeholder:text-white/20"
          />
        </Field>
        <div className="flex gap-2">
          <button
            onClick={ingestText}
            disabled={ingesting || !textSource.trim() || !textContent.trim()}
            className="px-4 py-2 rounded-lg text-[13px] font-medium border-none cursor-pointer bg-blue-500 text-white transition-all duration-150 hover:bg-blue-600 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {ingesting ? "Indexando..." : "Adicionar texto"}
          </button>
          <button
            onClick={ingestFile}
            disabled={ingesting}
            className="px-4 py-2 rounded-lg text-[13px] font-medium border-none cursor-pointer bg-white/10 text-white/80 transition-all duration-150 hover:bg-white/[0.15] disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Adicionar arquivo
          </button>
        </div>
      </FieldGroup>

      {status && (
        <div className="text-[12px] text-cyan-400/80 px-3 py-2 bg-cyan-400/[0.08] rounded-lg">
          {status}
        </div>
      )}

      <FieldGroup title={`Fontes (${sources.length})`}>
        {sources.length === 0 ? (
          <div className="text-[13px] text-white/25 text-center py-5">
            Ainda não há documentos na base de conhecimento.
          </div>
        ) : (
          <div className="flex flex-col gap-1">
            {sources.map(([name, chunks]) => (
              <div key={name} className="flex items-center justify-between px-3 py-2.5 rounded-lg bg-white/[0.03] hover:bg-white/[0.06] transition-colors duration-150">
                <div className="flex items-center gap-2.5">
                  <span className="text-[13px] text-white/80 font-medium">{name}</span>
                  <span className="text-[10px] text-white/25 bg-white/[0.05] px-2 py-0.5 rounded">{chunks} trechos</span>
                </div>
                <button
                  onClick={() => deleteSource(name)}
                  className="w-7 h-7 rounded-md border-none bg-red-500/10 text-red-400/60 text-base cursor-pointer flex items-center justify-center transition-all duration-150 hover:bg-red-500/20 hover:text-red-400/90"
                  title="Remover fonte"
                >
                  x
                </button>
              </div>
            ))}
          </div>
        )}
      </FieldGroup>
    </div>
  );
}

/* ─────────────────────────── Shared Components ─────────────────────────── */

function FieldGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-3 bg-white/[0.025] border border-white/[0.06] rounded-xl p-4">
      <div className="text-[11px] font-semibold text-white/40 uppercase tracking-wider">{title}</div>
      {children}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-[12px] font-medium text-white/40">{label}</label>
      {children}
    </div>
  );
}

function Input({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder?: string }) {
  return (
    <input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      className="w-full bg-white/[0.05] border border-white/10 text-white/90 px-3 py-2.5 rounded-lg text-[13px] outline-none transition-all duration-200 focus:border-blue-500/50 focus:bg-white/[0.07] placeholder:text-white/20"
    />
  );
}

function Toggle({ on, onToggle }: { on: boolean; onToggle: () => void }) {
  return (
    <button
      onClick={onToggle}
      className={`relative w-11 h-6 rounded-full border-none cursor-pointer shrink-0 transition-colors duration-200 ${
        on ? "bg-blue-500" : "bg-white/10"
      }`}
    >
      <div
        className={`absolute top-[3px] left-[3px] w-[18px] h-[18px] rounded-full bg-white transition-transform duration-200 ${
          on ? "translate-x-5" : ""
        }`}
      />
    </button>
  );
}

/* ─────────────────────────── Settings View ─────────────────────────── */

function Settings() {
  const [config, setConfig] = useState<VoiceConfig | null>(null);
  const [saved, setSaved] = useState(false);
  const [tab, setTab] = useState<SettingsTab>("config");
  const [showRestartAfterShortcuts, setShowRestartAfterShortcuts] = useState(false);
  const shortcutsBaselineRef = useRef<Record<ShortcutFieldKey, string> | null>(null);

  useEffect(() => {
    invoke<VoiceConfig>("get_config").then((c) => {
      const merged = {
        ...c,
        music_library_paths: c.music_library_paths ?? "",
      };
      shortcutsBaselineRef.current = pickShortcuts(merged);
      setConfig(merged);
    });
  }, []);

  const save = async () => {
    if (!config) return;
    try {
      await invoke("set_config", { config });
      const baseline = shortcutsBaselineRef.current;
      if (baseline && shortcutsDifferFromBaseline(config, baseline)) {
        setShowRestartAfterShortcuts(true);
      } else {
        setShowRestartAfterShortcuts(false);
      }
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch (e) {
      console.error("set_config:", e);
    }
  };

  const restartApp = () => {
    invoke("restart_app").catch((e) => console.error(e));
  };

  if (!config) return null;

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: "config", label: "Configuração" },
    { id: "tools", label: "Ferramentas" },
    { id: "knowledge", label: "Conhecimento" },
  ];

  return (
    <div className="h-screen flex flex-col settings-bg backdrop-blur-xl overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-6 pt-5 pb-3.5" style={{ WebkitAppRegion: "drag" } as React.CSSProperties}>
        <h2 className="text-base font-semibold text-white/85 tracking-tight">Configurações</h2>
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
      </div>

      {showRestartAfterShortcuts && (
        <div
          className="flex flex-wrap items-center justify-between gap-3 px-6 py-3 border-b border-amber-500/25 bg-amber-500/[0.08]"
          style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
        >
          <p className="text-[12px] text-amber-100/90 leading-snug max-w-[min(100%,42rem)]">
            Atalhos alterados e guardados. Reinicie o Chronos para aplicar tudo de forma fiável.
          </p>
          <button
            type="button"
            onClick={restartApp}
            className="shrink-0 px-4 py-2 rounded-lg text-[12px] font-semibold border border-amber-400/55 bg-amber-500/30 text-amber-50 hover:bg-amber-500/45 transition-colors cursor-pointer"
          >
            Reiniciar
          </button>
        </div>
      )}

      {/* Tab bar */}
      <div className="flex gap-0.5 px-6 border-b border-white/[0.08]">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`px-4 py-2.5 text-[13px] font-medium border-none bg-transparent cursor-pointer -mb-px transition-colors duration-150 border-b-2 ${
              tab === t.id
                ? "text-white/90 border-b-blue-500"
                : "text-white/35 border-b-transparent hover:text-white/55"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Body */}
      <div className="flex-1 overflow-y-auto custom-scrollbar">
        {tab === "config" && <ConfigTab config={config} setConfig={setConfig} />}
        {tab === "tools" && <ToolsTab config={config} setConfig={setConfig} />}
        {tab === "knowledge" && <KnowledgeTab />}
      </div>
    </div>
  );
}

async function resolveTextModelLabel(config?: VoiceConfig): Promise<string> {
  const c = config ?? (await invoke<VoiceConfig>("get_config"));
  const fromConfig = c.llm_model_text?.trim();
  if (fromConfig) return fromConfig;
  const textUrl = c.llm_url_text?.trim() || "http://127.0.0.1:8084";
  try {
    const models = await invoke<string[]>("list_models", { llmUrl: textUrl });
    if (models.length > 0) return models[0];
  } catch {
    /* servidor ainda a subir */
  }
  return "Modelo de texto (Qwen)";
}

function resolveVoiceModelLabel(config: VoiceConfig): string {
  const voice = config.llm_model_voice?.trim();
  return voice || config.llm_model;
}

/* ─────────────────────────── Orb View ─────────────────────────── */

function Orb() {
  const [stage, setStage] = useState("idle");
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const bubblesEndRef = useRef<HTMLDivElement>(null);

  const audioQueueRef = useRef<{ index: number; url: string }[]>([]);
  const isPlayingRef = useRef(false);
  const totalChunksRef = useRef<number | null>(null);
  const playedCountRef = useRef(0);
  const currentAudioRef = useRef<HTMLAudioElement | null>(null);
  const lastChunkEndRef = useRef<number>(0); // perf: track inter-sentence gap
  /** Next chunk index required for strict playback order (parallel TTS may finish out of order). */
  const nextRequiredIndexRef = useRef(0);
  /** Gaps (ms) between consecutive chunk playbacks for one assistant utterance — used for A/B logs. */
  const gapSessionMsRef = useRef<number[]>([]);
  /** Index currently playing (-1 = none). Used to prefetch N+1. */
  const currentlyPlayingIndexRef = useRef(-1);
  /** Pre-created Audio for the next index (same blob URL as queue); reduces startup gap. */
  const preloadPackRef = useRef<{ index: number; url: string; el: HTMLAudioElement } | null>(null);
  const playNextRef = useRef<() => void>(() => {});

  const [currentModel, setCurrentModel] = useState("");
  const [chatPending, setChatPending] = useState(false);
  const [voiceSwapLoading, setVoiceSwapLoading] = useState(false);
  const [voiceStackReady, setVoiceStackReady] = useState(true);
  const [llmModeLabel, setLlmModeLabel] = useState("");

  const refreshVoiceModel = useCallback(() => {
    invoke<VoiceConfig>("get_config")
      .then((c) => setCurrentModel(resolveVoiceModelLabel(c)))
      .catch(() => {});
  }, []);

  useEffect(() => {
    refreshVoiceModel();
  }, [refreshVoiceModel]);

  useEffect(() => {
    const unlisteners: Array<() => void | Promise<void>> = [];

    listen("llm_swap_started", () => {
      setVoiceSwapLoading(true);
      setVoiceStackReady(false);
    }).then((fn) => unlisteners.push(fn));

    listen("llm_swap_done", () => {
      setVoiceSwapLoading(false);
      setVoiceStackReady(true);
      refreshVoiceModel();
    }).then((fn) => unlisteners.push(fn));

    listen("llm_swap_failed", () => {
      setVoiceSwapLoading(false);
      setVoiceStackReady(false);
    }).then((fn) => unlisteners.push(fn));

    listen<string>("llm_mode_changed", (e) => {
      setLlmModeLabel(e.payload);
      if (e.payload.includes("Modo voz")) {
        setVoiceStackReady(true);
        setVoiceSwapLoading(false);
      }
    }).then((fn) => unlisteners.push(fn));

    return () => {
      for (const u of unlisteners) void u();
    };
  }, [refreshVoiceModel]);

  useEffect(() => {
    const start = listen("chat_processing_started", () => setChatPending(true));
    const end = listen("chat_processing_ended", () => setChatPending(false));
    const done = listen<ChatDonePayload>("chat_done", () => setChatPending(false));
    return () => {
      start.then((fn) => fn());
      end.then((fn) => fn());
      done.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    bubblesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [bubbles]);

  const addBubble = (role: ChatBubble["role"], text: string) => {
    setBubbles((prev) => [...prev, { role, text, id: bubbleId++ }]);
  };

  const stopAllAudio = () => {
    if (preloadPackRef.current) {
      try {
        preloadPackRef.current.el.pause();
        preloadPackRef.current.el.removeAttribute("src");
        preloadPackRef.current.el.load();
      } catch {
        /* ignore */
      }
      preloadPackRef.current = null;
    }
    if (currentAudioRef.current) {
      currentAudioRef.current.pause();
      currentAudioRef.current.onended = null;
      currentAudioRef.current.onerror = null;
      currentAudioRef.current = null;
    }
    for (const item of audioQueueRef.current) {
      URL.revokeObjectURL(item.url);
    }
    audioQueueRef.current = [];
    isPlayingRef.current = false;
    totalChunksRef.current = null;
    playedCountRef.current = 0;
    lastChunkEndRef.current = 0;
    nextRequiredIndexRef.current = 0;
    gapSessionMsRef.current = [];
    currentlyPlayingIndexRef.current = -1;
  };

  const logGapSessionSummary = () => {
    const gaps = gapSessionMsRef.current;
    if (gaps.length === 0) return;
    const sum = gaps.reduce((a, b) => a + b, 0);
    const avg = sum / gaps.length;
    const max = Math.max(...gaps);
    const line = `[perf] frontend_gap_session | n=${gaps.length} | avg_ms=${avg.toFixed(1)} | max_ms=${max.toFixed(1)}`;
    console.log(line);
    echoPerfToHost(line);
    gapSessionMsRef.current = [];
  };

  useLayoutEffect(() => {
    playNextRef.current = () => {
      if (isPlayingRef.current) return;
      audioQueueRef.current.sort((a, b) => a.index - b.index);
      if (audioQueueRef.current.length === 0) {
        if (totalChunksRef.current !== null && playedCountRef.current >= totalChunksRef.current) {
          logGapSessionSummary();
          setStage("idle");
          totalChunksRef.current = null;
          playedCountRef.current = 0;
          lastChunkEndRef.current = 0;
          nextRequiredIndexRef.current = 0;
          currentlyPlayingIndexRef.current = -1;
        }
        return;
      }
      const need = nextRequiredIndexRef.current;
      const pos = audioQueueRef.current.findIndex((x) => x.index === need);
      if (pos === -1) {
        return;
      }

      let next: { index: number; url: string };
      let audio: HTMLAudioElement;

      if (preloadPackRef.current?.index === need) {
        const qpos = audioQueueRef.current.findIndex((x) => x.index === need);
        if (qpos !== -1) {
          audioQueueRef.current.splice(qpos, 1);
        }
        const pack = preloadPackRef.current;
        preloadPackRef.current = null;
        next = { index: pack.index, url: pack.url };
        audio = pack.el;
      } else {
        next = audioQueueRef.current.splice(pos, 1)[0];
        audio = new Audio(next.url);
      }

      if (lastChunkEndRef.current > 0) {
        const gapMs = performance.now() - lastChunkEndRef.current;
        const gapLine = `[perf] frontend_gap_${next.index} | gap_ms=${gapMs.toFixed(1)}`;
        console.log(gapLine);
        echoPerfToHost(gapLine);
        gapSessionMsRef.current.push(gapMs);
      }

      isPlayingRef.current = true;
      currentlyPlayingIndexRef.current = next.index;
      currentAudioRef.current = audio;

      const onTimeUpdate = () => {
        const el = currentAudioRef.current;
        if (!el || el !== audio) return;
        const d = el.duration;
        if (!Number.isFinite(d) || d <= 0) return;
        if (d - el.currentTime > CHUNK_PLAYBACK_PREROLL_SEC) return;
        const nx = next.index + 1;
        const hasNext =
          audioQueueRef.current.some((x) => x.index === nx) || preloadPackRef.current?.index === nx;
        if (!hasNext) return;
        el.removeEventListener("timeupdate", onTimeUpdate);
        el.pause();
        el.onended = null;
        el.onerror = null;
        URL.revokeObjectURL(next.url);
        currentAudioRef.current = null;
        isPlayingRef.current = false;
        playedCountRef.current++;
        nextRequiredIndexRef.current = nx;
        currentlyPlayingIndexRef.current = -1;
        lastChunkEndRef.current = performance.now();
        playNextRef.current();
      };

      let timeUpdateAttached = false;
      const ensureTimeUpdate = () => {
        if (timeUpdateAttached) return;
        timeUpdateAttached = true;
        audio.addEventListener("timeupdate", onTimeUpdate);
      };

      audio.addEventListener("loadedmetadata", ensureTimeUpdate, { once: true });
      audio.addEventListener("playing", ensureTimeUpdate, { once: true });

      const playStart = performance.now();
      audio
        .play()
        .then(() => {
          const playDelay = performance.now() - playStart;
          const playLine = `[perf] frontend_playback_started_${next.index} | play_delay_ms=${playDelay.toFixed(1)} | duration_s=${audio.duration?.toFixed(2) ?? "N/A"}`;
          console.log(playLine);
          echoPerfToHost(playLine);
          if (next.index === 0) {
            const ttfsLine = `[perf] TTFS | chunk_0_playback_started | timestamp_ms=${performance.now().toFixed(0)}`;
            console.log(ttfsLine);
            echoPerfToHost(ttfsLine);
          }
          const want = next.index + 1;
          if (preloadPackRef.current && preloadPackRef.current.index !== want) {
            try {
              preloadPackRef.current.el.pause();
              preloadPackRef.current.el.removeAttribute("src");
              preloadPackRef.current.el.load();
            } catch {
              /* ignore */
            }
            preloadPackRef.current = null;
          }
          if (!preloadPackRef.current) {
            const item = audioQueueRef.current.find((x) => x.index === want);
            if (item) {
              const el = new Audio(item.url);
              el.preload = "auto";
              void el.load();
              preloadPackRef.current = { index: want, url: item.url, el };
            }
          }
        })
        .catch(() => {});

      audio.onended = () => {
        audio.removeEventListener("timeupdate", onTimeUpdate);
        lastChunkEndRef.current = performance.now();
        URL.revokeObjectURL(next.url);
        currentAudioRef.current = null;
        isPlayingRef.current = false;
        playedCountRef.current++;
        nextRequiredIndexRef.current = next.index + 1;
        currentlyPlayingIndexRef.current = -1;
        playNextRef.current();
      };
      audio.onerror = () => {
        audio.removeEventListener("timeupdate", onTimeUpdate);
        URL.revokeObjectURL(next.url);
        currentAudioRef.current = null;
        isPlayingRef.current = false;
        playedCountRef.current++;
        nextRequiredIndexRef.current = next.index + 1;
        currentlyPlayingIndexRef.current = -1;
        playNextRef.current();
      };
    };
  });

  const playNext = useCallback(() => {
    playNextRef.current();
  }, []);

  useEffect(() => {
    const unInterrupted = listen("pipeline_interrupted", () => {
      stopAllAudio();
    });
    const unPressed = listen("hotkey_pressed", () => {
      stopAllAudio();
      setStage("listening");
    });
    const unReleased = listen("hotkey_released", () => { setStage("transcribing"); });
    return () => {
      unInterrupted.then((fn) => fn());
      unPressed.then((fn) => fn());
      unReleased.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<ProcessingState>("processing", (event) => {
      const { stage: newStage, text } = event.payload;
      setStage(newStage);
      if (newStage === "transcribed") {
        addBubble("user", text);
      } else if (newStage === "tool_call") {
        // Replace any existing tool/status chip with the new one (only keep one)
        setBubbles((prev) => {
          const filtered = prev.filter((b) => b.role !== "tool" && b.role !== "status");
          return [...filtered, { role: "tool", text, id: bubbleId++ }];
        });
      } else if (newStage === "speaking") {
        // Remove ephemeral tool/status chips when assistant starts speaking
        setBubbles((prev) => {
          const filtered = prev.filter((b) => b.role !== "tool" && b.role !== "status");
          const last = filtered[filtered.length - 1];
          if (last && last.role === "assistant") {
            const updated = [...filtered];
            updated[updated.length - 1] = { ...last, text };
            return updated;
          }
          return [...filtered, { role: "assistant", text, id: bubbleId++ }];
        });
      } else if (newStage === "error") {
        addBubble("status", text);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<AudioChunk>("play_audio_chunk", (event) => {
      const { index, audio } = event.payload;
      if (index === 0) {
        nextRequiredIndexRef.current = 0;
        gapSessionMsRef.current = [];
      }
      const decodeLabel = `[perf] frontend_decode_${index}`;
      console.time(decodeLabel);
      const audioBytes = Uint8Array.from(atob(audio), (c) => c.charCodeAt(0));
      const audioBlob = new Blob([audioBytes], { type: "audio/wav" });
      const url = URL.createObjectURL(audioBlob);
      console.timeEnd(decodeLabel);
      audioQueueRef.current.push({ index, url });
      const playingIdx = currentlyPlayingIndexRef.current;
      if (isPlayingRef.current && playingIdx >= 0 && index === playingIdx + 1) {
        if (!preloadPackRef.current || preloadPackRef.current.index !== index) {
          if (preloadPackRef.current) {
            try {
              preloadPackRef.current.el.pause();
              preloadPackRef.current.el.removeAttribute("src");
              preloadPackRef.current.el.load();
            } catch {
              /* ignore */
            }
            preloadPackRef.current = null;
          }
          const el = new Audio(url);
          el.preload = "auto";
          void el.load();
          preloadPackRef.current = { index, url, el };
        }
      }
      playNext();
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [playNext]);

  useEffect(() => {
    const unlisten = listen<number>("play_audio_done", (event) => {
      totalChunksRef.current = event.payload;
      if (playedCountRef.current >= event.payload && !isPlayingRef.current) {
        logGapSessionSummary();
        setStage("idle");
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen("messages_cleared", () => { setBubbles([]); setStage("idle"); });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  const orbClass = [
    "orb-container",
    stage === "listening" && "orb-listening",
    stage === "transcribing" && "orb-processing",
    stage === "transcribed" && "orb-processing",
    stage === "thinking" && "orb-thinking",
    stage === "tool_call" && "orb-toolcall",
    stage === "speaking" && "orb-speaking",
    stage === "error" && "orb-error",
  ].filter(Boolean).join(" ");

  // Glow animation class based on state
  const glowAnim =
    stage === "listening" ? "animate-pulse-slow" :
    stage === "speaking" ? "animate-speak-pulse" :
    stage === "tool_call" ? "animate-breathe-fast" :
    (stage === "processing" || stage === "transcribing" || stage === "transcribed") ? "animate-breathe-fast" :
    stage === "thinking" ? "animate-breathe" :
    stage === "error" ? "" :
    "animate-breathe";

  // Ring animation based on state
  const ringAnim =
    stage === "listening" ? "animate-ring-pulse" :
    (stage === "transcribing" || stage === "transcribed" || stage === "processing") ? "animate-spin-medium" :
    stage === "thinking" ? "animate-spin-slow" :
    stage === "tool_call" ? "animate-spin-fast" :
    "";

  return (
    <div className="flex flex-col h-screen orb-bg px-5 py-4">
      {voiceSwapLoading && (
        <div className="llm-swap-banner shrink-0 rounded-lg mb-2 mx-0" role="status" aria-live="polite">
          <span className="llm-swap-spinner" aria-hidden="true" />
          {llmModeLabel || "Restaurando modo voz (Llama + XTTS)..."}
        </div>
      )}
      {!voiceSwapLoading && voiceStackReady && !llmModeLabel.includes("chat") && (
        <div
          className="shrink-0 mb-2 px-3 py-1.5 rounded-lg text-center text-[11px] font-medium bg-emerald-500/10 text-emerald-300/90 border border-emerald-500/20"
          role="status"
        >
          Modo voz pronto — segure Shift+Z para falar
        </div>
      )}
      {/* Conversation bubbles */}
      <div className="flex-1 overflow-y-auto flex flex-col justify-end px-3.5 pt-4 pb-2.5 gap-2 no-scrollbar bubble-mask">
        {bubbles.map((b) => (
          <BubbleComponent key={b.id} bubble={b} />
        ))}
        {(stage === "listening" || stage === "transcribing" || stage === "thinking") && (
          <div className="self-center animate-fade-in px-3 py-1 text-white/25 text-[11px] font-medium">
            {stage === "listening" ? "Ouvindo..." : stage === "transcribing" ? "Transcrevendo..." : "Pensando..."}
          </div>
        )}
        <div ref={bubblesEndRef} />
      </div>

      {/* Orb */}
      <div className="flex justify-center pb-5 pt-2 shrink-0">
        <div className={`${orbClass} relative w-20 h-20`}>
          <div className={`orb-glow absolute -inset-[5%] rounded-full blur-[14px] z-[1] ${glowAnim}`} />
          <div className="orb-core absolute inset-[18%] rounded-full z-[2]" />
          <div className={`orb-ring absolute inset-[8%] rounded-full border-[1.5px] z-[3] ${ringAnim}`} />
        </div>
      </div>

      {/* Model badge + pending indicator */}
      <div className="flex justify-center items-center gap-2 pb-3">
        {chatPending && (
          <span
            className="w-2 h-2 rounded-full bg-amber-400/80 animate-pulse"
            title="Resposta pendente no chat"
          />
        )}
        {currentModel && (
          <span className="px-2.5 py-0.5 rounded-full bg-white/[0.05] text-[10px] text-white/30 font-medium border border-white/[0.04]">
            {currentModel}
          </span>
        )}
      </div>
    </div>
  );
}

/* ─────────────────────────── Chat View ─────────────────────────── */

function ChatView() {
  const [messages, setMessages] = useState<ChatMessageData[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [modelName, setModelName] = useState("");
  const [statusConfig, setStatusConfig] = useState<VoiceConfig | null>(null);
  const [showAutocomplete, setShowAutocomplete] = useState(false);
  const [autocompleteIndex, setAutocompleteIndex] = useState(0);
  const [llmSwapLoading, setLlmSwapLoading] = useState(false);
  const [llmModeLabel, setLlmModeLabel] = useState<string>("");
  const chatEndRef = useRef<HTMLDivElement>(null);

  const SLASH_COMMANDS_LIST = [
    { cmd: "/config", desc: "Ver ou alterar configurações" },
    { cmd: "/config personality", desc: "Alterar perfil (default, coder, creative, custom)" },
    { cmd: "/config temperature", desc: "Ajustar temperatura (0-2)" },
    { cmd: "/config thinking", desc: "Ligar/desligar thinking (on|off)" },
    { cmd: "/config style", desc: "Estilo de resposta (concise, normal, detailed)" },
    { cmd: "/config model", desc: "Trocar modelo de chat" },
    { cmd: "/clear", desc: "Limpar conversa" },
    { cmd: "/export", desc: "Exportar conversa" },
    { cmd: "/model", desc: "Mostrar modelo ativo" },
    { cmd: "/help", desc: "Listar todos os comandos" },
  ];

  const filteredCommands = input.startsWith("/")
    ? SLASH_COMMANDS_LIST.filter((c) =>
        c.cmd.startsWith(input.split(/\s+/)[0].toLowerCase())
      )
    : [];

  useEffect(() => {
    invoke<ChatMessageData[]>("load_history")
      .then((saved) => {
        if (saved && saved.length > 0) setMessages(saved);
        return invoke<ChatMessageData[]>("get_messages");
      })
      .then((current) => {
        if (current && current.length > 0) setMessages(current);
      })
      .catch(() => {
        invoke<ChatMessageData[]>("get_messages")
          .then((m) => { if (m) setMessages(m); })
          .catch(() => {});
      });

    void resolveTextModelLabel()
      .then(setModelName)
      .catch(() => {});
  }, []);

  useEffect(() => {
    const loadStatus = () => {
      invoke<VoiceConfig>("get_config").then(setStatusConfig).catch(() => {});
    };
    loadStatus();
    const interval = setInterval(loadStatus, 30000); // 30s em vez de 5s — reduz chamadas IPC
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    if (input.startsWith("/") && filteredCommands.length > 0) {
      setShowAutocomplete(true);
      if (autocompleteIndex >= filteredCommands.length) setAutocompleteIndex(0);
    } else {
      setShowAutocomplete(false);
    }
  }, [input, filteredCommands.length]);

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streaming]);

  /** Eventos do chat vêm do Webview "chat" via window.emit — usar listen na webview atual evita falhas com listen global. */
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void | Promise<void>> = [];
    const ww = getCurrentWebviewWindow();

    void ww.listen<{ token: string }>("chat_token", (event) => {
      setStreaming((prev) => prev + event.payload.token);
    }).then((fn) => {
      if (cancelled) void fn();
      else unlisteners.push(fn);
    });

    void ww.listen<ChatDonePayload>("chat_done", (event) => {
      const response = event.payload.response;
      setStreaming("");
      setMessages((prev) => [
        ...prev,
        {
          role: "assistant",
          content: response,
          created_at_ms: Date.now(),
          elapsed_ms: event.payload.elapsed_ms,
        },
      ]);
      setIsLoading(false);
    }).then((fn) => {
      if (cancelled) void fn();
      else unlisteners.push(fn);
    });

    void ww.listen("messages_cleared", () => {
      setMessages([]);
      setStreaming("");
      setIsLoading(false);
    }).then((fn) => {
      if (cancelled) void fn();
      else unlisteners.push(fn);
    });

    void ww.listen("llm_swap_started", () => {
      setLlmSwapLoading(true);
    }).then((fn) => {
      if (cancelled) void fn();
      else unlisteners.push(fn);
    });

    void ww.listen("llm_swap_done", () => {
      setLlmSwapLoading(false);
      void resolveTextModelLabel().then(setModelName).catch(() => {});
    }).then((fn) => {
      if (cancelled) void fn();
      else unlisteners.push(fn);
    });

    void ww.listen("llm_swap_failed", () => {
      setLlmSwapLoading(false);
    }).then((fn) => {
      if (cancelled) void fn();
      else unlisteners.push(fn);
    });

    void ww.listen<string>("llm_mode_changed", (e) => {
      setLlmModeLabel(e.payload);
    }).then((fn) => {
      if (cancelled) void fn();
      else unlisteners.push(fn);
    });

    return () => {
      cancelled = true;
      for (const u of unlisteners) {
        void u();
      }
    };
  }, []);

  /**
   * Processa comandos iniciados com "/".
   * @returns
   *   - `string`: resposta para exibir como bolha do assistente
   *   - `null`: comando processado silenciosamente (limpa input, sem bolhas)
   *   - `undefined`: comando não reconhecido - envia ao LLM como mensagem normal
   */
  async function handleSlashCommand(text: string): Promise<string | null | undefined> {
    const args = text.trim().split(/\s+/);
    const cmd = args[0].toLowerCase();

    try {
      const currentConfig = await invoke<VoiceConfig>("get_config");

      switch (cmd) {
        case "/clear":
          await invoke("clear_messages");
          setMessages([]);
          setStreaming("");
          setIsLoading(false);
          return null;

        case "/export": {
          const path = await save({
            defaultPath: "chronos-conversa.md",
            filters: [{ name: "Markdown", extensions: ["md"] }, { name: "Texto", extensions: ["txt"] }],
          });
          if (path) await invoke("export_conversation", { path });
          return null;
        }

        case "/config": {
          if (args.length < 2) {
            let summary = "**Configurações atuais:**\n";
            summary += `- Perfil: \`${currentConfig.personality}\`\n`;
            summary += `- Modelo: \`${currentConfig.llm_model}\`\n`;
            summary += `- Temperatura: \`${currentConfig.temperature}\`\n`;
            summary += `- Thinking: \`${currentConfig.enable_thinking ? "ligado" : "desligado"}\`\n`;
            summary += `- Estilo: \`${currentConfig.response_style}\`\n`;
            summary += `- TTS Volume: \`${currentConfig.tts_volume}%\`\n`;
            return summary;
          }

          const subCmd = args[1].toLowerCase();
          const value = args.slice(2).join(" ");

          switch (subCmd) {
            case "personality": {
              const valid = ["default", "coder", "creative", "custom"];
              if (!valid.includes(value)) return `Perfil inválido. Use: ${valid.join(", ")}`;
              await invoke("set_config", { config: { ...currentConfig, personality: value } });
              invoke<VoiceConfig>("get_config").then(setStatusConfig).catch(() => {});
              return `Perfil alterado para **${value}**.`;
            }
            case "temperature": {
              const t = parseFloat(value);
              if (isNaN(t) || t < 0 || t > 2) return "Temperatura deve ser um número entre 0 e 2.";
              await invoke("set_config", { config: { ...currentConfig, temperature: t } });
              invoke<VoiceConfig>("get_config").then(setStatusConfig).catch(() => {});
              return `Temperatura alterada para **${t}**.`;
            }
            case "thinking": {
              const on = value === "on" || value === "true" || value === "1";
              const off = value === "off" || value === "false" || value === "0";
              if (!on && !off) return "Use: `/config thinking on` ou `/config thinking off`";
              await invoke("set_config", { config: { ...currentConfig, enable_thinking: on } });
              invoke<VoiceConfig>("get_config").then(setStatusConfig).catch(() => {});
              return `Thinking **${on ? "ligado" : "desligado"}**.`;
            }
            case "style": {
              const valid = ["concise", "normal", "detailed"];
              if (!valid.includes(value)) return `Estilo inválido. Use: ${valid.join(", ")}`;
              await invoke("set_config", { config: { ...currentConfig, response_style: value } });
              invoke<VoiceConfig>("get_config").then(setStatusConfig).catch(() => {});
              return `Estilo alterado para **${value}**.`;
            }
            case "model": {
              if (!value) return "Informe o nome do modelo. Ex: `/config model gemma-4`";
              await invoke("set_config", { config: { ...currentConfig, llm_model: value } });
              setModelName(value);
              invoke<VoiceConfig>("get_config").then(setStatusConfig).catch(() => {});
              return `Modelo alterado para **${value}**.`;
            }
            case "tts_volume": {
              const v = parseInt(value, 10);
              if (isNaN(v) || v < 0 || v > 100) return "Volume deve ser entre 0 e 100.";
              await invoke("set_config", { config: { ...currentConfig, tts_volume: v } });
              invoke<VoiceConfig>("get_config").then(setStatusConfig).catch(() => {});
              return `Volume do TTS alterado para **${v}%**.`;
            }
            default:
              return `Subcomando desconhecido: \`${subCmd}\`. Use \`/help\`.`;
          }
        }

        case "/model":
          return `Modelo ativo: **${currentConfig.llm_model}**`;

        case "/help":
          return SLASH_COMMANDS_LIST.map(c => `- \`${c.cmd}\` — ${c.desc}`).join("\n");

        case "/dictate":
          return "Modo ditado ativo: pressione **Shift+Z** para transcrever fala sem enviar ao LLM.";

        default:
          return undefined;
      }
    } catch (e) {
      return `Erro ao processar comando: ${e}`;
    }
  }

  const sendMessage = async () => {
    if (!input.trim() || isLoading) return;

    const text = input.trim();

    if (text.startsWith("/")) {
      const result = await handleSlashCommand(text);
      if (result === undefined) {
        /* comando não reconhecido: segue para o LLM */
      } else if (result === null) {
        setInput("");
        return;
      } else {
        setMessages((prev) => [
          ...prev,
          { role: "user", content: text, created_at_ms: Date.now() },
          { role: "assistant", content: result, created_at_ms: Date.now() },
        ]);
        setInput("");
        return;
      }
    }

    setInput("");
    setMessages((prev) => [...prev, { role: "user", content: text, created_at_ms: Date.now() }]);
    setStreaming("");
    setIsLoading(true);

    try {
      await invoke("send_chat_message", { text });
      invoke("save_history").catch(() => {}); // fire-and-forget: persiste historico a cada resposta
      const latestMessages = await invoke<ChatMessageData[]>("get_messages");
      setMessages(latestMessages);
      setStreaming("");
      setIsLoading(false);
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `Erro: ${String(e)}` },
      ]);
      setStreaming("");
      setIsLoading(false);
    }
  };

  const clearChat = async () => {
    try {
      await invoke("clear_messages");
      setMessages([]);
      setStreaming("");
      setIsLoading(false);
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className="h-screen flex flex-col chat-window">
      <div className="chat-topbar tauri-drag">
        <div>
          <h2 className="chat-title">Chronos Chat</h2>
          <p className="chat-subtitle">Modo texto com raciocinio automatico para tarefas complexas</p>
        </div>
        <div className="flex items-center gap-2 tauri-no-drag">
          {modelName && (
            <span className="chat-model-pill">
              {modelName}
            </span>
          )}
          <button
            onClick={async () => {
              try {
                const path = await save({
                  defaultPath: "chronos-conversa.md",
                  filters: [
                    { name: "Markdown", extensions: ["md"] },
                    { name: "Texto", extensions: ["txt"] },
                  ],
                });

                if (path) {
                  await invoke("export_conversation", { path });
                }
              } catch (e) {
                console.error("Export error:", e);
              }
            }}
            className="chat-ghost-button"
          >
            Exportar
          </button>
          <button
            onClick={() => void clearChat()}
            className="chat-ghost-button"
          >
            Limpar
          </button>
        </div>
      </div>

      {statusConfig && (
        <div className="flex items-center gap-2 px-4 py-2 border-b border-white/[0.04] bg-white/[0.01] overflow-x-auto" style={{ WebkitAppRegion: "no-drag" } as CSSProperties}>
          <span className="text-[10px] text-white/25 font-medium shrink-0">Status:</span>
          <span className="text-[10px] px-2 py-0.5 rounded-full bg-indigo-500/15 text-indigo-300/90 font-medium whitespace-nowrap">
            {statusConfig.personality === "coder" ? "Programador" :
             statusConfig.personality === "creative" ? "Criativo" :
             statusConfig.personality === "custom" ? "Personalizado" : "Padrão"}
          </span>
          <span className="text-[10px] px-2 py-0.5 rounded-full bg-white/[0.06] text-white/50 font-medium whitespace-nowrap">
            {modelName || statusConfig.llm_model_text?.trim() || "Texto"}
          </span>
          <span className="text-[10px] px-2 py-0.5 rounded-full bg-amber-500/12 text-amber-300/85 font-medium whitespace-nowrap">
            t={statusConfig.temperature.toFixed(1)}
          </span>
          <span className={`text-[10px] px-2 py-0.5 rounded-full font-medium whitespace-nowrap ${statusConfig.enable_thinking ? "bg-green-500/12 text-green-400/85" : "bg-white/[0.04] text-white/30"}`}>
            {statusConfig.enable_thinking ? "thinking" : "rápido"}
          </span>
        </div>
      )}

      {llmSwapLoading && (
        <div className="llm-swap-banner" role="status" aria-live="polite">
          <span className="llm-swap-spinner" aria-hidden="true" />
          {llmModeLabel || "Carregando modelo..."}
        </div>
      )}
      <div className="chat-scroll custom-scrollbar">
        <div className="chat-thread">
        {messages.length === 0 && !streaming && (
          <div className="chat-empty">
            <div className="chat-empty-card">
              <div className="chat-empty-icon">Chronos</div>
              <p className="chat-empty-title">Pergunte com calma. Aqui eu posso pensar mais.</p>
              <p className="chat-empty-copy">
                Use o chat para codigo, analise de tela, planejamento, investigacao e respostas completas.
              </p>
            </div>
          </div>
        )}

        {messages.map((msg, index) => (
          <ChatBubbleView
            key={`${msg.created_at_ms ?? 0}-${index}-${msg.role}`}
            role={msg.role}
            content={msg.content}
            createdAtMs={msg.created_at_ms}
            elapsedMs={msg.elapsed_ms}
          />
        ))}

        {streaming && (
          <ChatBubbleView
            key="streaming"
            role="assistant"
            content={streaming}
            isStreaming
          />
        )}

        {isLoading && !streaming && (
          <div className="chat-thinking">
            <span className="chat-thinking-dot" />
            Pensando...
          </div>
        )}

        <div ref={chatEndRef} />
        </div>
      </div>

      <div className="chat-composer-wrap relative">
        {showAutocomplete && filteredCommands.length > 0 && (
          <div className="absolute bottom-full left-0 right-0 mb-1 bg-[#1a1a1e] border border-white/[0.08] rounded-lg overflow-hidden max-h-60 overflow-y-auto z-20 shadow-[0_-4px_20px_rgba(0,0,0,0.3)]">
            {filteredCommands.map((item, i) => (
              <button
                key={item.cmd}
                type="button"
                className={`flex items-center gap-2.5 w-full px-3.5 py-2 border-none bg-transparent cursor-pointer text-left transition-colors duration-150 ${
                  i === autocompleteIndex ? "bg-white/[0.06]" : "hover:bg-white/[0.03]"
                }`}
                onMouseDown={(e) => {
                  e.preventDefault();
                  setInput(item.cmd + " ");
                  setShowAutocomplete(false);
                }}
                onMouseEnter={() => setAutocompleteIndex(i)}
              >
                <span className="text-[13px] font-mono text-blue-400/90 whitespace-nowrap">{item.cmd}</span>
                <span className="text-[11px] text-white/30">{item.desc}</span>
              </button>
            ))}
          </div>
        )}
        <div className="chat-composer">
          <textarea
            id="chat-message-input"
            name="message"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (showAutocomplete && filteredCommands.length > 0) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setAutocompleteIndex((prev) => (prev + 1) % filteredCommands.length);
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setAutocompleteIndex((prev) => (prev - 1 + filteredCommands.length) % filteredCommands.length);
                  return;
                }
                if (e.key === "Tab") {
                  e.preventDefault();
                  const selected = filteredCommands[autocompleteIndex];
                  if (selected) {
                    setInput(selected.cmd + " ");
                    setShowAutocomplete(false);
                  }
                  return;
                }
                if (e.key === "Escape") {
                  setShowAutocomplete(false);
                  return;
                }
              }
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void sendMessage();
              }
            }}
            placeholder={llmSwapLoading
              ? (llmModeLabel || "Carregando modelo...")
              : "Digite sua mensagem... (Enter envia, Shift+Enter quebra linha)"}
            disabled={isLoading || llmSwapLoading}
            rows={1}
            className="chat-input"
          />
          <button
            onClick={async () => {
              if (!statusConfig) return;
              const updated = { ...statusConfig, enable_thinking: !statusConfig.enable_thinking };
              await invoke("set_config", { config: updated });
              setStatusConfig(updated);
            }}
            className={`chat-thinking-toggle${statusConfig?.enable_thinking ? " chat-thinking-toggle--on" : ""}`}
            title={statusConfig?.enable_thinking
              ? "Thinking sempre ativo — clique para voltar ao modo automático"
              : "Thinking automático — clique para forçar sempre"}
            aria-label="Alternar modo thinking"
            aria-pressed={statusConfig?.enable_thinking ?? false}
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M9.5 2A2.5 2.5 0 0 1 12 4.5v15a2.5 2.5 0 0 1-4.96-.44 2.5 2.5 0 0 1-2.96-3.08 3 3 0 0 1-.34-5.58 2.5 2.5 0 0 1 1.32-4.24 2.5 2.5 0 0 1 1.98-3A2.5 2.5 0 0 1 9.5 2Z"/>
              <path d="M14.5 2A2.5 2.5 0 0 0 12 4.5v15a2.5 2.5 0 0 0 4.96-.44 2.5 2.5 0 0 0 2.96-3.08 3 3 0 0 0 .34-5.58 2.5 2.5 0 0 0-1.32-4.24 2.5 2.5 0 0 0-1.98-3A2.5 2.5 0 0 0 14.5 2Z"/>
            </svg>
            Thinking
          </button>
          <button
            onClick={() => void sendMessage()}
            disabled={isLoading || !input.trim()}
            className="chat-send-button"
            aria-label="Enviar mensagem"
            title="Enviar mensagem"
          >
            <span aria-hidden="true">➤</span>
          </button>
        </div>
        <p className="chat-shortcuts">
          {(statusConfig?.shortcut_settings || "Ctrl+Comma")} configurações · {(statusConfig?.shortcut_chat || "Shift+T")} abre o chat · {(statusConfig?.shortcut_voice || "Shift+Z")} para voz · {(statusConfig?.shortcut_stop || "Ctrl+5")} para parar · {(statusConfig?.shortcut_clear || "Shift+C")} para limpar · {(statusConfig?.shortcut_hide || "Shift+X")} para esconder
        </p>
      </div>
    </div>
  );
}

function ChatBubbleView({
  role,
  content,
  createdAtMs,
  elapsedMs,
  isStreaming = false,
}: {
  role: string;
  content: string;
  createdAtMs?: number;
  elapsedMs?: number | null;
  isStreaming?: boolean;
}) {
  const isUser = role === "user";

  if (role === "tool" || !content.trim()) {
    return null;
  }

  const author = isUser ? "Voce" : "Chronos";
  const timeLabel = !isStreaming ? formatMessageTime(createdAtMs) : "";
  const elapsedLabel = !isUser && !isStreaming && elapsedMs ? `respondido em ${formatDuration(elapsedMs)}` : null;

  return (
    <div className={`chat-message-row ${isUser ? "chat-message-user" : "chat-message-assistant"}`}>
      <div className="chat-message-meta">
        <span>{author}</span>
        {timeLabel && <span>{timeLabel}</span>}
        {elapsedLabel && <span>{elapsedLabel}</span>}
        {isStreaming && <span>respondendo...</span>}
      </div>
      <div className={`chat-bubble ${isUser ? "chat-bubble-user" : "chat-bubble-assistant"}`}>
        {!isUser && !isStreaming && (
          <button
            className="chat-copy-message"
            onClick={() => void copyText(content)}
            title="Copiar resposta"
            aria-label="Copiar resposta"
          >
            Copiar
          </button>
        )}
        {isStreaming && !isUser ? (
          <div className="chat-markdown chat-markdown-streaming">{content}</div>
        ) : (
          <div className="chat-markdown">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || "");
                  const code = String(children).replace(/\n$/, "");
                  if (!match) {
                    return (
                      <code className={className} {...props}>
                        {children}
                      </code>
                    );
                  }
                  return <CodeBoard language={match[1]} code={code} />;
                },
              }}
            >
              {content}
            </ReactMarkdown>
          </div>
        )}
      </div>
    </div>
  );
}

function CodeBoard({ language, code }: { language: string; code: string }) {
  return (
    <div className="code-board">
      <div className="code-board-header">
        <span>{language}</span>
        <button
          onClick={() => void copyText(code)}
          className="code-copy-button"
          title="Copiar codigo"
          aria-label="Copiar codigo"
        >
          Copiar codigo
        </button>
      </div>
      <SyntaxHighlighter
        language={language}
        style={oneDark}
        customStyle={{
          margin: 0,
          background: "transparent",
          padding: "16px",
          fontSize: "13px",
          lineHeight: 1.6,
        }}
      >
        {code}
      </SyntaxHighlighter>
    </div>
  );
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    console.error("Falha ao copiar:", e);
  }
}

function formatMessageTime(timestamp?: number) {
  if (!timestamp) return "";
  return new Intl.DateTimeFormat("pt-BR", {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp));
}

function formatDuration(ms: number) {
  const seconds = ms / 1000;
  return `${seconds.toFixed(seconds < 10 ? 1 : 0).replace(".", ",")}s`;
}

/* ─────────────────────────── Bubble Component ─────────────────────────── */

function BubbleComponent({ bubble }: { bubble: ChatBubble }) {
  if (bubble.role === "user") {
    return (
      <div className="self-end max-w-[85%] animate-bubble-in">
        <div className="px-3.5 py-2.5 rounded-2xl rounded-br-md bg-[#1c2044] text-white/90 text-[13px] leading-relaxed">
          {bubble.text}
        </div>
      </div>
    );
  }

  if (bubble.role === "assistant") {
    return (
      <div className="self-start max-w-[85%] animate-bubble-in">
        <div className="px-3.5 py-2.5 rounded-2xl rounded-bl-md bg-[#252529] text-white/85 text-[13px] leading-relaxed">
          {bubble.text}
        </div>
      </div>
    );
  }

  if (bubble.role === "tool") {
    const label = TOOL_LABEL_MAP[bubble.text] || bubble.text;
    return (
      <div className="self-center animate-fade-in">
        <div className="inline-flex items-center gap-1.5 px-3 py-1 rounded-md bg-[#222228] text-white/40 text-[11px] font-medium">
          <span className="text-[10px] animate-gear-spin">&#9881;</span>
          {label}
        </div>
      </div>
    );
  }

  // status
  return (
    <div className="self-center animate-fade-in">
      <div className="px-3 py-1 text-white/25 text-[11px] font-medium">
        {bubble.text}
      </div>
    </div>
  );
}

/* ─────────────────────────── App ─────────────────────────── */

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

export default App;
