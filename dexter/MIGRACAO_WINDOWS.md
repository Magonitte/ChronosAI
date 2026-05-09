# Migração macOS → Windows — Voice Assistant

> **Status atual (09/05/2026):** Projeto compila e executa no Windows. STT migrado de embedded (`whisper-rs`) para HTTP. Chat por texto funcional; áudio pendente de teste fim-a-fim.

---

## Objetivo

Migrar completamente o Voice Assistant de macOS para Windows, com duas fases:

| Fase | Descrição | Status |
|------|-----------|--------|
| **Fase 1** | Chat por **texto** funcionando (LLM + tools + settings) | ✅ Concluído |
| **Fase 2** | Chat por **voz** completo (STT + LLM + TTS fim-a-fim) | 🔄 Em andamento |

---

## 1. Ambiente de Desenvolvimento (Windows)

### Ferramentas instaladas

| Ferramenta | Versão | Caminho |
|-----------|--------|---------|
| Rust / Cargo | 1.95.0 | `cargo` e `rustc` no PATH |
| Node.js | (sistema) | `node` e `npx` no PATH |
| Visual Studio 2022 BuildTools | 17.14.31 | `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools` |
| Windows SDK | 10.0.26100.0 | `C:\Program Files (x86)\Windows Kits\10` |
| MSVC | 14.44.35207 | dentro do BuildTools |
| LLVM / Clang | 19.1.7 | `C:\Program Files\LLVM\bin` |

### Servidores C++ externos (já rodando)

| Servidor | Porta | Função |
|----------|-------|--------|
| llama.cpp (LLM) | `8080` | Chat + Embeddings |
| whisper.cpp (STT) | `8081` | Speech-to-Text (via `/v1/audio/transcriptions`) |
| Chatterbox (TTS) | `8005` | Text-to-Speech |

> **Nota:** O whisper-server deve ser iniciado com `--request-path "/v1/audio" --inference-path "/transcriptions"` para expor a rota compatível com o formato OpenAI. O `start-all.ps1` já faz isso automaticamente. Caso a rota não esteja disponível, o cliente faz fallback para `/inference`.

---

## 2. Alterações Realizadas

### 2.1. `src-tauri/Cargo.toml` — Dependências

```diff
- whisper-rs = "0.16"
+ # Removido — incompatível com Windows (bindgen/libclang)
```

**Motivo:** O crate `whisper-rs` (v0.16) usa `bindgen` para gerar bindings FFI do `whisper.cpp`. No Windows:
- O `bindgen` requer `libclang.dll` (biblioteca compartilhada do LLVM)
- As bindings empacotadas (`bundled bindings.rs`) foram geradas para macOS/Linux (glibc), com tamanhos de struct incompatíveis com MSVC (`_IO_FILE`: 216 bytes no Linux vs tamanho diferente no Windows)
- Mesmo com `LIBCLANG_PATH` configurado, o LLVM instalado via `winget` **não inclui** `libclang.dll`

### 2.2. `src-tauri/src/voice.rs` — Transcrição

**Antes:** Embedded `whisper-rs` (carregava modelo GGML do disco, inferência in-process)

```rust
// Antigo (removido)
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub fn transcribe_audio(
    model_path: &str,
    samples: &[f32],
    source_sample_rate: u32,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let ctx = WhisperContext::new_with_params(model_path, ...)?;
    // ... inferência local ...
}
```

**Depois:** HTTP para servidor whisper externo (OpenAI-compatible)

```rust
// Novo
pub async fn transcribe_audio(
    whisper_url: &str,
    samples: &[f32],
    source_sample_rate: u32,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let wav_bytes = samples_to_wav(&audio_16k, 16000);
    // POST multipart para {whisper_url}/v1/audio/transcriptions
    let result: TranscriptionResponse = resp.json().await?;
    Ok(result.text.trim().to_string())
}
```

**Mudanças:**
- Função agora é `async` (antes era síncrona com `spawn_blocking`)
- Assinatura mudou: `model_path: &str` → `whisper_url: &str`
- Adicionada função auxiliar `samples_to_wav()` para converter `Vec<f32>` → WAV bytes
- Usa `hound` (já era dependência) para encoding WAV
- Endpoint: `POST {whisper_url}/v1/audio/transcriptions` com multipart form

### 2.3. `src-tauri/src/lib.rs` — Config e Pipeline

#### VoiceConfig — novo campo

```diff
  pub struct VoiceConfig {
      pub whisper_model_path: String,  // mantido para backward-compat
+     #[serde(default = "default_whisper_url")]
+     pub whisper_url: String,         // NOVO: URL do servidor whisper HTTP
      pub llm_url: String,
      // ...
  }

+ fn default_whisper_url() -> String {
+     "http://localhost:8080".to_string()
+ }
```

#### process_pipeline — chamada assíncrona

```diff
- let model_path = config.whisper_model_path.clone();
- let transcript = tokio::task::spawn_blocking(move || {
-     voice::transcribe_audio(&model_path, &samples, sample_rate)
- }).await...;

+ let whisper_url = config.whisper_url.clone();
+ let transcript = voice::transcribe_audio(&whisper_url, &samples, sample_rate).await...;
```

### 2.4. `src-tauri/src/rag.rs` — Correção de bug pré-existente

```diff
- .send()
- .await?;
+ .send()
+ .await
+ .map_err(|e| format!("Embedding request failed: {}", e))?;
```

**Motivo:** Dentro de um `async move` que retorna `Result<_, String>`, o operador `?` tentava converter `reqwest::Error` → `String`, mas `reqwest::Error` não implementa `Into<String>`. Este bug estava mascarado porque builds anteriores falhavam no `whisper-rs` antes de chegar nesta etapa de compilação.

### 2.5. `src/App.tsx` — Frontend

#### Interface VoiceConfig

```diff
  interface VoiceConfig {
    whisper_model_path: string;
+   whisper_url: string;
    // ...
  }
```

#### UI de Configurações (Speech Recognition)

```diff
  <FieldGroup title="Speech Recognition">
    <Field label="Whisper Model Path">
      <Input ... />
    </Field>
+   <Field label="Whisper Server URL">
+     <Input value={config.whisper_url} ... placeholder="http://localhost:8080" />
+   </Field>
  </FieldGroup>
```

---

## 3. Erros Encontrados e Soluções

### 3.1. Porta 1420 já em uso

**Sintoma:** `Error: Port 1420 is already in use`

**Causa:** Instâncias anteriores do Vite dev server não foram encerradas ao matar o processo Tauri.

**Solução:**
```powershell
$proc = (Get-NetTCPConnection -LocalPort 1420).OwningProcess
foreach ($p in $proc) { Stop-Process -Id $p -Force }
```

### 3.2. `cargo tauri` não encontrado

**Sintoma:** `error: no such command: 'tauri'`

**Causa:** O CLI do Tauri não estava instalado como subcomando do Cargo.

**Solução:** Usar `npx tauri dev` (o pacote `@tauri-apps/cli` está em `devDependencies`).

### 3.3. Compilação do `whisper-rs-sys` — bindings incorretos (Windows)

**Sintoma:**
```
error[E0080]: attempt to compute `208_usize - 216_usize`, which would overflow
["Size of _IO_FILE"][::std::mem::size_of::<_IO_FILE>() - 216usize];
```

**Causa:** As bindings FFI empacotadas foram geradas para glibc (Linux/macOS), onde `_IO_FILE` tem 216 bytes. No Windows/MSVC, o struct equivalente tem tamanho diferente, causando overflow na assertion de tamanho.

**Tentativas de solução:**
1. ❌ Instalar LLVM via `winget` — instalação sem `libclang.dll`
2. ❌ Baixar LLVM 19.1.7 do GitHub — instalador `.exe` requer elevação de admin; `.zip` deu 404
3. ❌ Configurar `LIBCLANG_PATH` e `BINDGEN_EXTRA_CLANG_ARGS` — não resolveu porque `libclang.dll` não existia

**Solução final:** Remover `whisper-rs` do Cargo.toml e migrar STT para HTTP (ver seção 2.2).

### 3.4. `libclang.dll` não encontrada

**Sintoma:**
```
thread 'main' panicked at bindgen/lib.rs:616:27:
Unable to find libclang: "couldn't find any valid shared libraries matching:
['clang.dll', 'libclang.dll']"
```

**Causa:** A instalação do LLVM via `winget` (`LLVM.LLVM`) é uma instalação mínima, sem as bibliotecas compartilhadas necessárias para o `bindgen`.

**Verificação:**
```powershell
Get-ChildItem "C:\Program Files\LLVM\bin" -Filter "*.dll"
# → vazio (sem DLLs)
```

### 3.5. Erro no `rag.rs` — `?` operator com `reqwest::Error` → `String`

**Sintoma:**
```
error[E0277]: `?` couldn't convert the error to `std::string::String`
  --> src\rag.rs:223:23
```

**Causa:** Bug pré-existente no código. O closure `async move` retorna `Result<Vec<f64>, String>`, mas o `?` na linha 223 tentava converter `reqwest::Error` diretamente.

**Solução:** Adicionar `.map_err(|e| format!("...", e))?` (ver seção 2.4).

### 3.6. Cabeçalhos C não encontrados pelo Clang

**Sintoma:**
```
warning: whisper-rs-sys: Unable to generate bindings: clang diagnosed error:
./whisper.cpp/ggml/include\ggml.h:214:10: fatal error: 'stdio.h' file not found
```

**Causa:** O Clang não encontrava os headers do Windows SDK e MSVC.

**Solução (não necessária após remover whisper-rs):** Usar o Visual Studio Developer Command Prompt para configurar `INCLUDE`, `LIB` e `PATH` corretamente:
```batch
"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=amd64
```

---

## 4. Comando de Build

```powershell
# No diretório do projeto (dexter/)
npx tauri dev
```

**Pré-requisitos para build limpo:**
- Node.js com `node_modules` instalados (`npm install`)
- Rust + MSVC toolchain
- Visual Studio BuildTools (para linker MSVC)
- Nenhum processo na porta 1420

**Build atual:** ~495 crates compilados (vs 522 antes, pois whisper-rs-sys e suas dependências foram removidos).

---

## 5. O Que Já Funciona

| Funcionalidade | Status | Observação |
|---------------|--------|------------|
| Interface Orb (UI) | ✅ | Janela transparente, animações, chat bubbles |
| Settings (3 abas) | ✅ | Config, Tools, Knowledge |
| System tray | ✅ | Ícone na bandeja do Windows |
| Hotkeys (Shift+Z, Shift+X) | ✅ | `tauri-plugin-global-shortcut` |
| LLM Chat (texto) | ✅ | Streaming via `llama.cpp` na porta 8080 |
| Tool calling | ✅ | Screenshot, clipboard, web fetch, etc. |
| RAG / Knowledge base | ✅ | SQLite + embeddings |
| Gravação de áudio (cpal) | ✅ | Microfone captura samples f32 |
| STT (HTTP whisper) | ✅ | Rota `/v1/audio/transcriptions` + fallback `/inference` |
| TTS (Chatterbox) | 🔄 | Codificado, não testado fim-a-fim |
| Sandbox (shell) | 🔄 | Compila, não testado |

---

## 6. Próximos Passos (Fase 2 — Voz)

1. **~~Testar STT HTTP~~** ✅ Servidor whisper na porta 8081 com `--request-path "/v1/audio" --inference-path "/transcriptions"` + fallback automático para `/inference`
2. **Testar TTS:** Verificar se o Chatterbox na porta 8005 está respondendo a `POST /v1/audio/speech`
3. **Testar pipeline completo:** Shift+Z → gravar → transcrever → LLM → TTS → tocar áudio
4. **Ajustar tools para Windows:**
   - Screenshot: PowerShell (já adaptado)
   - Clipboard: `Get-Clipboard` (PowerShell, já adaptado)
   - Open URL: `start` (CMD, já adaptado)
   - Running apps: `Get-Process` (PowerShell, já adaptado)
   - AppleScript: removido/substituído por PowerShell
5. **Ajustar paths:** Substituir paths macOS (`~/Library/Application Support/`) por Windows (`%APPDATA%`)
6. **Testar em outra máquina Windows limpa** (sem ferramentas de dev)

---

## 7. Notas Técnicas

### Por que `whisper-rs` falha no Windows?

O ecossistema `whisper-rs` + `bindgen` + `libclang` é frágil no Windows porque:

1. **bindgen** requer `libclang.dll` (biblioteca compartilhada), não apenas `clang.exe`
2. O LLVM para Windows distribuído via `winget` é uma build mínima (sem shared libs)
3. As bindings FFI empacotadas assumem structs glibc (Linux/macOS), não MSVC
4. A geração de bindings requer headers C padrão (`stdio.h`), que no Windows vêm do Windows SDK (não do sistema)

A abordagem HTTP é mais portável e alinha o STT com o padrão já usado pelo LLM e TTS (ambos HTTP).

### Alternativas futuras para STT local

Caso queira voltar ao STT embedded no Windows:
- Usar `whisper.cpp` como subprocesso (chamada CLI) em vez de linkar via FFI
- Usar `sonos-rs` ou outra crate com melhor suporte Windows
- Usar WebAssembly (whisper.wasm) no frontend
