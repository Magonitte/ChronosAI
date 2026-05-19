# Chatterbox TTS no projeto Chronos

Esta pasta contém o fork/vendor [**chatterbox-tts-api**](README.md) (upstream: [travisvn/chatterbox-tts-api](https://github.com/travisvn/chatterbox-tts-api)).

No **Chronos / Dexter**, o Chatterbox é o backend TTS **alternativo** aos perfis `voice-chatterbox*` do `start-all.ps1`. O perfil de **produção recomendado** usa **XTTS v2** em `../xtts-api-server/`.

## Quando usar

| Perfil | TTS |
|--------|-----|
| `voice-xtts-cuda-partial` (padrão) | XTTS — ver `../xtts-api-server/README.md` |
| `voice-chatterbox`, `voice-chatterbox-cpu`, `quality` | Chatterbox nesta pasta |

## Setup rápido (Windows)

```powershell
cd dexter\chatterbox-tts-api
uv sync
uv run main.py
# Porta 8005 — mesma URL em Configurações do app (chatterbox_url)
```

Guia PT-BR e clonagem: `../Documentação/guias/GUIA-TTS-chatterbox-clonagem-ptbr.md`

Documentação completa do app: [**`../README.md`**](../README.md)
