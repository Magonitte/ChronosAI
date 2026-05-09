# Voice Assistant

A local, privacy-first desktop voice assistant for macOS. Hold a hotkey, speak, and get a spoken response — all running on your machine.

Built with **Tauri 2** (Rust + React), powered by **Whisper** (STT) + **Ollama** (LLM) + **Chatterbox** (TTS).

## How It Works

```
You speak ──► Whisper transcribes ──► Ollama thinks ──► Chatterbox speaks back
               (local STT)            (local LLM)         (local TTS)
```

1. **Hold Shift+Z** — the orb appears and starts listening
2. **Release** — your speech is transcribed via Whisper (runs natively in Rust, no server needed)
3. **Ollama generates a response** — streamed sentence-by-sentence for low latency
4. **Each sentence is sent to Chatterbox TTS** — audio plays back sequentially as chunks arrive
5. **Press Shift+X** to dismiss the window

The app lives in the system tray — no dock icon, no window on launch. Just a floating orb that appears when you talk.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  macOS System Tray                                      │
│  ┌───────────────────────────────────┐                  │
│  │  Tauri (Rust backend)             │                  │
│  │  ├── whisper-rs (native STT)      │                  │
│  │  ├── cpal (mic recording)         │                  │
│  │  ├── Ollama client (LLM + tools)  │                  │
│  │  ├── Chatterbox client (TTS)      │                  │
│  │  ├── RAG store (SQLite + embeds)  │                  │
│  │  └── Tool executors               │                  │
│  └───────────────┬───────────────────┘                  │
│                  │ events + invoke                       │
│  ┌───────────────▼───────────────────┐                  │
│  │  React frontend                   │                  │
│  │  ├── Orb UI (transparent overlay) │                  │
│  │  ├── Chat bubbles                 │                  │
│  │  └── Settings (tabbed window)     │                  │
│  └───────────────────────────────────┘                  │
└─────────────────────────────────────────────────────────┘
```

### Streaming TTS Pipeline

The response doesn't wait for the full LLM output. Instead:

1. Ollama streams tokens
2. Sentence boundary detection splits the stream (on `.` `!` `?` followed by whitespace)
3. Each complete sentence is immediately sent to Chatterbox for TTS
4. Audio chunks are emitted to the frontend with an index
5. The frontend queues chunks and plays them in order

This means the user hears the first sentence while the LLM is still generating the rest.

## Tool Calling

The LLM has access to tools it can invoke mid-conversation. When you ask something that needs a tool, the flow is:

1. Your message + tool definitions go to Ollama
2. Ollama decides whether to call a tool or respond directly
3. If it calls a tool, the backend executes it and feeds the result back
4. Ollama can chain multiple tools (up to 5 rounds) before responding
5. The final text response is streamed and spoken

### Available Tools

| Tool | What It Does | How It's Used |
|------|-------------|---------------|
| **Screenshot** | Captures the screen via `screencapture`, sends the image to a vision model (llava) which describes what it sees | "What's on my screen?" / "Is there an error?" |
| **Read Clipboard** | Reads clipboard text via `pbpaste` | "What did I just copy?" / "Summarize what's in my clipboard" |
| **Knowledge Search** | Vector similarity search over ingested documents (SQLite + Ollama embeddings) | "What do my notes say about X?" |
| **Open URL** | Opens a URL in the default browser | "Open YouTube" / "Search Google for X" |
| **Current Time** | Returns the current date, time, and day of week | "What time is it?" / "What day is today?" |
| **Running Apps** | Lists all foreground applications via AppleScript | "What apps do I have open?" |

All tools can be individually enabled/disabled from the Settings > Tools tab.

### Screenshot Tool — Two Models Working Together

The screenshot tool uses two models in sequence:

1. **Chat model** (e.g. qwen3) reads your question and calls `take_screenshot(question: "Look for error messages")` — it decides *what* to look for
2. **Vision model** (e.g. llava) receives the actual screenshot image + that question — it *sees* the screen and describes what's there
3. The description goes back to the chat model, which formulates the spoken answer

Configure the vision model in Settings > Config > Vision Model (defaults to `llava`).

## RAG (Retrieval-Augmented Generation)

A local knowledge base backed by SQLite with vector embeddings via Ollama.

### How It Works

1. **Ingest** — text is chunked (512 chars, 64 overlap, sentence-boundary-aware), each chunk is embedded via Ollama's `/api/embed` endpoint, and stored in SQLite
2. **Search** — when the LLM calls `search_knowledge`, the query is embedded and compared against all chunks using cosine similarity
3. **Top results** (above 0.3 threshold) are returned to the LLM as context

### Managing Knowledge

From Settings > Knowledge tab:
- **Add Text** — paste content with a source name
- **Add File** — ingest a text file from disk
- **View Sources** — see all ingested sources with chunk counts
- **Delete** — remove a source and all its chunks

Requires an embedding model pulled in Ollama (e.g. `ollama pull nomic-embed-text`).

## The Orb

The UI is a frameless transparent window with a glowing orb at the bottom-right of the screen. Chat bubbles stack upward like notifications.

### Orb States

| Color | State | Meaning |
|-------|-------|---------|
| Blue (breathing) | Idle | Ready |
| Red (pulsing) | Listening | Recording your voice |
| Amber (spinning) | Processing | Transcribing speech |
| Purple (spinning) | Thinking | Waiting for LLM |
| Cyan (pulsing) | Speaking | Playing TTS audio |
| Dim red | Error | Something went wrong |

## Settings

Accessible from the system tray menu. Three tabs:

### Config
- **Whisper Model Path** — path to a GGML whisper model file (e.g. `ggml-base.en.bin`)
- **Ollama URL** — where Ollama is running (default `http://localhost:11434`)
- **Chat Model** — Ollama model for conversation (e.g. `qwen3:4b`)
- **Embedding Model** — for RAG vector embeddings (e.g. `nomic-embed-text`)
- **Vision Model** — for screenshot description (e.g. `llava`)
- **Chatterbox URL** — TTS server address
- **Voice** — voice file name on the Chatterbox server
- **System Prompt** — personality and behavior instructions

### Tools
Toggle each tool on/off. Changes take effect on the next conversation turn.

### Knowledge
Manage the RAG knowledge base — ingest, view, and delete document sources.

All settings persist to `~/Library/Application Support/voice-assistant/config.json`.

## Prerequisites

- **Ollama** running locally with at least a chat model pulled
  ```bash
  ollama pull qwen3:4b          # chat
  ollama pull nomic-embed-text   # embeddings (for RAG)
  ollama pull llava              # vision (for screenshots)
  ```
- **Chatterbox TTS** server running (OpenAI-compatible `/v1/audio/speech` endpoint)
- **Whisper GGML model** downloaded:
  ```bash
  mkdir -p ~/.cache/whisper
  curl -L -o ~/.cache/whisper/ggml-base.en.bin \
    https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
  ```
- **macOS** (uses `screencapture`, `pbpaste`, `open`, AppleScript)

## Build & Run

```bash
# Install dependencies
npm install

# Development
cargo tauri dev

# Production build
cargo tauri build
```

### Build Notes

- Requires `CMAKE_OSX_DEPLOYMENT_TARGET=11.0` for whisper-rs (handled via `src-tauri/.cargo/config.toml`)
- First build compiles whisper.cpp and SQLite from source — takes a few minutes
- `macOSPrivateApi: true` is required in `tauri.conf.json` for transparent windows

## Tech Stack

| Layer | Technology |
|-------|-----------|
| App framework | Tauri 2 |
| Backend | Rust |
| Frontend | React 19 + TypeScript + Vite |
| STT | whisper-rs (native, no server) |
| LLM | Ollama (local, streaming, tool calling) |
| TTS | Chatterbox (self-hosted, OpenAI-compatible) |
| Audio capture | cpal |
| Vector store | SQLite + Ollama embeddings |
| Global hotkeys | tauri-plugin-global-shortcut |

## File Structure

```
voice-assistant/
├── src/
│   ├── App.tsx          # React app — Orb + Settings (URL-routed)
│   └── App.css          # All styles — orb animations, settings UI
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs       # Tauri setup, tray, hotkeys, pipeline orchestration
│   │   ├── voice.rs     # Whisper STT, Ollama streaming, Chatterbox TTS, tool defs
│   │   ├── tools.rs     # Tool implementations (screenshot, clipboard, etc.)
│   │   └── rag.rs       # RAG store — chunking, embedding, SQLite vector search
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── capabilities/
│       └── default.json # Window permissions (main + settings)
└── package.json
```

## Hotkeys

| Shortcut | Action |
|----------|--------|
| **Shift+Z** (hold) | Push-to-talk — hold to record, release to process |
| **Shift+X** | Dismiss/hide the orb window |
