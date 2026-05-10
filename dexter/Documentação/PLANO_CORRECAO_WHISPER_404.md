# Plano de correção — erro Whisper `404` em `/v1/audio/transcriptions`

Documento para **outro agente ou desenvolvedor** executar as tarefas na ordem sugerida.

---

## 1. Contexto do problema

- **Sintoma:** ao falar com o assistente aparece algo como:  
  `Pipeline error: Transcription failed: Whisper API error 404 Not Found: File Not Found (/v1/audio/transcriptions)`.
- **Stack:** app **Dexter** (Tauri + Rust + React). STT via HTTP em `src-tauri/src/voice.rs` → `transcribe_audio`.
- **Requisição atual do cliente:**  
  `POST {whisper_url}/v1/audio/transcriptions`  
  multipart com `file` (WAV), `model`, `language` (ex.: `pt`).

---

## 2. Causa raiz

- O binário **`whisper-server`** do **whisper.cpp** (`examples/server/server.cpp`) expõe a transcrição em:
  - `POST {request_path}{inference_path}` com **padrão** `inference_path = "/inference"`.
- O cliente Dexter chama **`/v1/audio/transcriptions`** (formato OpenAI). Se o servidor é o whisper-server padrão, essa rota **não existe** → **HTTP 404**.
- `Documentação/WHISPER_STT_SETUP.md` cobre bem o erro **501** (URL apontando para LLM). O **404** é o caso **rota incompatível** no servidor Whisper correto.

---

## 3. Objetivos

1. Garantir que o STT responda em **`POST /v1/audio/transcriptions`** (compatível com o cliente), **ou** que o cliente faça fallback para a rota legada.
2. Manter o fluxo **100% PT-BR** onde couber: `language=pt` na transcrição; prompts e UI em português conforme o projeto.

---

## 4. Tarefas (ordem sugerida)

### Tarefa A — Corrigir o launcher (fix rápido, sem Rust)

**Arquivo:** `dexter/start-all.ps1`

**Ação:** na linha que inicia o Whisper (`Start-Server` com `whisper-server.exe`), acrescentar:

- `--request-path "/v1/audio"`
- `--inference-path "/transcriptions"`

Assim a URL efetiva fica **`/v1/audio` + `/transcriptions`** = **`/v1/audio/transcriptions`**, alinhada ao que `voice.rs` monta.

**Exemplo de argumentos** (adaptar ao estilo de aspas já usado no script):

```text
--model "<caminho-do-gguf>" --host 127.0.0.1 --port 8081 --request-path "/v1/audio" --inference-path "/transcriptions"
```

**Opcional (PT-BR no servidor):** rodar `whisper-server.exe --help` na build local; se existir flag global de idioma no startup e for suportada na versão compilada, considerar `--language pt`. Caso contrário, confiar no campo `language` do multipart (já enviado pelo app).

**Critério de aceite:** com o servidor subido pelo script, `POST` para `http://127.0.0.1:8081/v1/audio/transcriptions` com multipart contendo `file` **não** retorna 404 (400 por corpo inválido ainda indica que a rota existe).

---

### Tarefa B — Robustez no cliente (fallback em 404)

**Arquivo:** `dexter/src-tauri/src/voice.rs` — função `transcribe_audio`

**Ação:**

1. Manter o POST principal: `{whisper_url}/v1/audio/transcriptions` (com `trim_end_matches('/')` na base URL).
2. Se a resposta for **404**, repetir o **mesmo** multipart para `{whisper_url}/inference`.
3. Manter o parse JSON com campo **`text`** (resposta compatível com o whisper-server em formato json).
4. Estender mensagens de erro / hints: para **404**, explicar rota incompatível ou servidor sem mapeamento OpenAI; manter o hint existente para **501**.

**Critério de aceite:** servidor só com `/inference` → app ainda transcreve; servidor com Tarefa A → usa `/v1/audio/transcriptions` sem precisar do fallback.

---

### Tarefa C — Documentação

**Arquivos:** `dexter/Documentação/WHISPER_STT_SETUP.md` e, se aplicável, trecho em `dexter/Documentação/MIGRACAO_WINDOWS.md`

**Ação:**

- Incluir na tabela de diagnóstico: **404** = whisper-server sem rota `/v1/audio/transcriptions`; solução = flags `--request-path` / `--inference-path` no launcher **ou** fallback no cliente (Tarefa B).
- Exemplo de comando alinhado ao `start-all.ps1` após a mudança.

**Critério de aceite:** reprodução e correção possíveis só com a documentação, sem reler o C++ do servidor.

---

### Tarefa D — PT-BR “100%” (escopo mínimo)

**Verificar / ajustar:**

- `voice.rs`: campo `language` = `"pt"` no form — manter.
- Onde estiver o **system prompt** padrão (`VoiceConfig` / `lib.rs`): garantir instrução de responder **sempre em português do Brasil** (alterar só default se hoje estiver em inglês; respeitar `config.json` persistido).
- `App.tsx`: strings do fluxo de voz / pipeline / config visíveis ao usuário em PT-BR, se ainda houver inglês.

**Critério de aceite:** usuário com config padrão: transcrição em `pt` + respostas do assistente consistentes em PT-BR.

---

## 5. Verificação end-to-end

1. Subir LLM (porta padrão do projeto, ex. 8080) e Whisper (ex. 8081) com `start-all.ps1` **após** a Tarefa A.
2. No app: **Whisper Server URL** = `http://localhost:8081` (ou equivalente).
3. Gravar fala curta em português → pipeline passa da etapa de transcrição sem o erro citado.
4. (Opcional) Logs do `whisper-server` mostram recebimento/processamento do áudio.

---

## 6. O que não fazer

- Não migrar STT para nuvem (OpenAI, etc.) sem pedido explícito e sem desenho de autenticação.
- Não refatorar o pipeline de áudio/LLM além do necessário para rotas, mensagens e PT-BR.

---

## 7. Referências no repositório

| O quê | Onde |
|--------|------|
| Cliente STT | `dexter/src-tauri/src/voice.rs` (`transcribe_audio`) |
| Launcher | `dexter/start-all.ps1` (bloco Whisper) |
| Rotas do servidor (referência) | `tools/whisper.cpp/examples/server/server.cpp` (`inference_path`, `Post(...)`) |
| Doc STT existente | `dexter/Documentação/WHISPER_STT_SETUP.md` |

---

*Documento gerado para execução por outro agente ou por revisão humana.*
