# Whisper / STT — configuração e erro 501 (Windows)

Este documento descreve como o Voice Assistant envia áudio para transcrição, o erro **501 Not Implemented**, e como corrigir com um servidor Whisper dedicado.

---

## Como o app transcreve

O backend Rust faz:

```http
POST {whisper_url}/v1/audio/transcriptions
```

- Corpo: multipart com arquivo WAV (`file`), `model`, `language` (ex.: `pt`).
- Implementação: `src-tauri/src/voice.rs` → função `transcribe_audio`.
- A URL padrão de **Whisper Server** é `http://localhost:8081` (porta separada do LLM).
- A URL do **LLM** é `http://localhost:8080` (porta padrão do llama-server).

---

## Erro: `501 Not Implemented` + *does not support audio*

Exemplo de mensagem no terminal:

```text
Transcription failed: Whisper API error 501 Not Implemented:
{"error":{"code":501,"message":"The current model does not support audio input.","type":"not_supported_error"}}
```

### Significado

A URL configurada em **Whisper Server URL** aponta para um processo que **não** está servindo transcrição com o modelo carregado — em geral é o **mesmo** servidor do LLM (`llama-server` com um GGUF **só de texto**). Esse servidor responde ao chat, mas **não** implementa entrada de áudio.

Isso **não** é falha do frontend Tauri em si; é incompatibilidade entre **URL usada para STT** e **o que realmente roda nessa porta**.

---

## Solução recomendada: dois serviços em portas separadas

| Serviço | Porta padrão | Função |
|---------|-------------|--------|
| LLM (chat) | `8080` | Conversa / tools / streaming |
| STT (Whisper) | `8081` | `POST /v1/audio/transcriptions` |

### Passo 1 — Instalar e compilar o whisper.cpp

```powershell
# Clonar o repositório
git clone https://github.com/ggerganov/whisper.cpp.git
cd whisper.cpp

# Compilar com CMake
cmake -B build
cmake --build build --config Release
```

O executável estará em `build\bin\Release\whisper-server.exe`.

### Passo 2 — Baixar o modelo Whisper (se ainda não tiver)

```powershell
# Modelo small (bom equilíbrio entre velocidade e precisão):
# Baixe de https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
# Ou use o script do whisper.cpp:
powershell -Command "Invoke-WebRequest -Uri 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin' -OutFile 'ggml-small.bin'"
```

### Passo 3 — Subir o servidor Whisper

```powershell
.\build\bin\Release\whisper-server.exe --model "ggml-small.bin" --host 127.0.0.1 --port 8081
```

Deixe esse terminal rodando. O servidor expõe `POST /v1/audio/transcriptions` na porta 8081.

### Passo 4 — Verificar no app

1. Abra as **Settings** do Voice Assistant (bandeja → botão direito → Settings).
2. Na aba **Config** → seção **Speech Recognition**, confira:
   - **Whisper Server URL**: `http://localhost:8081`
3. Clique em **Save**.

Agora o app deve transcrever corretamente.

---

## Alternativa: usar OpenAI / outro provedor

Se preferir não rodar whisper.cpp localmente, configure **Whisper Server URL** para um endpoint compatível com OpenAI, ex.:

- OpenAI: `https://api.openai.com` (requer API key como header `Authorization: Bearer sk-...` — necessária modificação no código para enviar o header)
- Groq: `https://api.groq.com/openai`
- Outro servidor compatível com `/v1/audio/transcriptions`

---

## Resumo de diagnóstico

| Sintoma | Causa provável | Solução |
|---------|---------------|---------|
| HTTP **404** + *File Not Found (/v1/audio/transcriptions)* | O `whisper-server` está rodando mas **sem** as flags de rota OpenAI-compatible. A rota padrão é `/inference`, não `/v1/audio/transcriptions`. | Inicie o whisper-server com `--request-path "/v1/audio" --inference-path "/transcriptions"` (já configurado no `start-all.ps1`). O cliente também faz fallback automático para `/inference` caso receba 404. |
| HTTP **501** + *The current model does not support audio input* | O host em `whisper_url` não é um backend STT válido (ex.: é o llama-server de texto). | Configure **Whisper Server URL** para apontar para o whisper-server dedicado (ex.: `http://localhost:8081`), não para o LLM. |
| Connection refused | Nenhum servidor rodando na porta configurada. | Verifique se o whisper-server está rodando e na porta correta. |
| Timeout | Servidor não está respondendo (modelo muito grande? porta errada?). | Verifique o modelo carregado e a porta configurada. |

### Comando atualizado do whisper-server (alinhado ao `start-all.ps1`)

```powershell
.\build\bin\Release\whisper-server.exe --model "ggml-small.bin" --host 127.0.0.1 --port 8081 --request-path "/v1/audio" --inference-path "/transcriptions"
```

Com isso a URL efetiva fica `/v1/audio` + `/transcriptions` = `/v1/audio/transcriptions`, compatível com o formato OpenAI que o cliente Dexter espera.

Caso o servidor não suporte essas flags (versão antiga do whisper.cpp), o cliente faz **fallback automático** para `POST /inference`.

---

*Última atualização do documento: 2026-05-09.*
