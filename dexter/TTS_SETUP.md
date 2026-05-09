# TTS — Chatterbox com Clonagem de Voz PT-BR

## Visao Geral

O Dexter usa **Chatterbox TTS** (via [chatterbox-tts-api](https://github.com/travisvn/chatterbox-tts-api)) como motor de Text-to-Speech. O modelo **multilingual** suporta 22 idiomas incluindo Portugues (PT-BR) e permite **clonagem de voz** a partir de uma amostra de audio.

### Fluxo de audio

```
LLM gera texto ──► Deteccao de frase ──► Chatterbox TTS ──► Audio WAV ──► Frontend reproduz
                   (ponto, !, ?)          (voz clonada)      (base64)      (queue ordenada)
```

Cada frase completa e enviada imediatamente ao TTS, sem esperar o LLM terminar. Isso garante baixa latencia — o usuario comeca a ouvir enquanto o LLM ainda esta gerando.

## Setup Rapido (1 comando)

```powershell
.\setup-tts.ps1
```

Isso vai:
1. Clonar o repositorio `chatterbox-tts-api`
2. Instalar dependencias (uv ou pip)
3. Configurar `.env` com modelo multilingual na porta 8005
4. Copiar `Clone_voz.mp3` para a biblioteca de vozes

## Pos-Setup

Apos o setup, inicie o servidor e registre a voz:

```powershell
# Iniciar o servidor TTS
cd chatterbox-tts-api
uv run main.py

# Em outro terminal, registrar a voz clonada
.\register-voice.ps1
```

Ou simplesmente use o `start-all.ps1` que faz tudo automaticamente.

## Requisitos

| Requisito | Detalhes |
|-----------|----------|
| Python | 3.10+ |
| GPU | NVIDIA com CUDA (recomendado) |
| RAM GPU | 4GB+ VRAM |
| uv | Opcional mas recomendado (`irm https://astral.sh/uv/install.ps1 \| iex`) |

> Sem GPU CUDA, o TTS roda em CPU mas sera significativamente mais lento.

## Arquitetura

```
dexter/
├── Clone_voz.mp3              # Amostra de voz para clonagem
├── setup-tts.ps1              # Script de setup automatico
├── register-voice.ps1         # Registra a voz no servidor
├── start-all.ps1              # Inicia tudo (LLM + Whisper + TTS + App)
└── chatterbox-tts-api/        # Servidor TTS (clonado pelo setup)
    ├── .env                   # Configuracao (porta, modelo, etc)
    ├── voices/                # Biblioteca de vozes (Clone_voz.mp3)
    └── models/                # Cache dos modelos (baixado automaticamente)
```

## Configuracao

### Porta e Servidor

| Variavel | Padrao | Descricao |
|----------|--------|-----------|
| `PORT` | 8005 | Porta HTTP do servidor TTS |
| `HOST` | 0.0.0.0 | Interface de rede |
| `DEVICE` | auto | Dispositivo (auto/cuda/cpu) |

### Voz Clonada

| Variavel | Padrao | Descricao |
|----------|--------|-----------|
| `DEFAULT_VOICE` | dexter-ptbr | Nome da voz registrada |
| `USE_MULTILINGUAL_MODEL` | true | Habilita modelo multilingual |

### No App (Settings > Config)

- **Chatterbox URL**: `http://localhost:8005`
- **Voice**: `dexter-ptbr`

## Clonagem de Voz

A clonagem usa **zero-shot voice cloning** — basta uma amostra de 5-30 segundos de audio para clonar uma voz. O `Clone_voz.mp3` e usado como referencia.

### Registrar nova voz manualmente

```powershell
# Via script
.\register-voice.ps1 -VoiceName "minha-voz" -VoiceFile ".\minha_amostra.mp3"

# Via curl
curl -X POST http://localhost:8005/voices `
  -F "voice_name=minha-voz" `
  -F "language=pt" `
  -F "voice_file=@minha_amostra.mp3"
```

### Dicas para melhor qualidade

- Audio limpo, sem ruido de fundo
- Duracao ideal: 10-30 segundos
- Fala natural e clara
- Formatos aceitos: MP3, WAV, FLAC

## Solucao de Problemas

### TTS demora muito na primeira vez

Normal — o modelo multilingual (~2GB) precisa ser baixado na primeira execucao. Execucoes subsequentes usam o cache em `chatterbox-tts-api/models/`.

### Erro "CUDA out of memory"

O modelo multilingual precisa de ~4GB VRAM. Se sua GPU nao tem memoria suficiente:
- Feche outros programas que usem GPU
- No `.env`, troque `DEVICE=auto` para `DEVICE=cpu` (mais lento)

### Voz nao soa natural

- Tente ajustar `exaggeration` (0.3-0.8) no request
- Use uma amostra de voz mais longa/limpa
- O modelo melhora com amostras de audio de alta qualidade

### Servidor nao inicia

```powershell
# Verificar se a porta esta em uso
netstat -ano | Select-String ":8005"

# Verificar logs
cd chatterbox-tts-api
uv run main.py  # ver output no terminal
```

## API Reference

O servidor e compativel com a API OpenAI TTS:

```
POST /v1/audio/speech
Content-Type: application/json

{
    "input": "Texto para sintetizar",
    "voice": "dexter-ptbr",
    "model": "chatterbox",
    "response_format": "wav"
}
```

Endpoints uteis:
- `GET /voices` — Listar vozes registradas
- `POST /voices` — Registrar nova voz
- `GET /languages` — Idiomas suportados
