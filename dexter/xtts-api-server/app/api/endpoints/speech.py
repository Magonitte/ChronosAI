"""
XTTS v2 speech generation endpoint — OpenAI-compatible.
"""

import asyncio
import io
import sys
import threading
import time
import torch
import numpy as np
from typing import Optional
from fastapi import APIRouter, HTTPException, status
from fastapi.responses import StreamingResponse
from pydantic import BaseModel, Field

from app.config import Config
from app.core.tts_model import get_model, get_device, is_ready, get_sample_rate
from app.core.voice_library import get_voice_library

router = APIRouter()

# Coqui XTTS / shared PyTorch model is not thread-safe; concurrent model.tts() corrupts state (e.g. index errors).
_TTS_MODEL_LOCK = threading.Lock()


class SpeechRequest(BaseModel):
    input: str = Field(..., description="The text to generate audio for")
    voice: Optional[str] = Field(None, description="Voice name from the voice library")
    model: Optional[str] = Field("xtts", description="Model name (ignored, kept for compatibility)")
    response_format: Optional[str] = Field("wav", description="Audio format (only wav supported)")
    temperature: Optional[float] = Field(None, description="Sampling temperature")
    speed: Optional[float] = Field(None, description="Speech speed multiplier")
    # Backward-compatibility fields from Chatterbox — accepted but ignored
    exaggeration: Optional[float] = Field(None, description="[ignored] Emotion intensity")
    cfg_weight: Optional[float] = Field(None, description="[ignored] Pace control")


def resolve_voice_path_and_language(voice_name: Optional[str]) -> tuple:
    voice_lib = get_voice_library()

    if voice_name:
        voice_path = voice_lib.get_voice_path(voice_name)
        voice_language = voice_lib.get_voice_language(voice_name)
        if voice_path is not None:
            return voice_path, voice_language or Config.DEFAULT_LANGUAGE

        print(f"Warning: Voice '{voice_name}' not found in voice library, using default")

    default_voice = voice_lib.get_default_voice()
    if default_voice:
        voice_path = voice_lib.get_voice_path(default_voice)
        voice_language = voice_lib.get_voice_language(default_voice)
        if voice_path is not None:
            return voice_path, voice_language or Config.DEFAULT_LANGUAGE

    raise HTTPException(
        status_code=status.HTTP_400_BAD_REQUEST,
        detail={
            "error": {
                "message": "No voice registered. Use POST /voices to register a voice sample first.",
                "type": "voice_not_found",
            }
        },
    )


def _blocking_synthesize_to_wav_buffer(
    text: str,
    voice_path: str,
    language: str,
    temperature: float,
    speed: float,
) -> io.BytesIO:
    """Blocking synthesis + WAV encode. model.tts is serialized — safe from multiple asyncio.to_thread workers."""
    model = get_model()
    with _TTS_MODEL_LOCK:
        with torch.no_grad():
            audio_numpy = model.tts(
                text=text,
                speaker_wav=voice_path,
                language=language,
                temperature=temperature,
                speed=speed,
            )

    from scipy.io import wavfile

    sample_rate = get_sample_rate()
    audio_numpy = np.clip(audio_numpy, -1.0, 1.0)
    audio_int16 = (audio_numpy * 32767).astype(np.int16)

    wav_buffer = io.BytesIO()
    wavfile.write(wav_buffer, sample_rate, audio_int16)
    wav_buffer.seek(0)
    return wav_buffer


@router.post(
    "/audio/speech",
    response_class=StreamingResponse,
    responses={
        200: {"content": {"audio/wav": {}}},
        400: {"description": "Invalid request"},
        500: {"description": "Server error"},
    },
    summary="Generate speech from text",
    description="Generate speech audio from input text using XTTS v2 with voice cloning.",
)
async def text_to_speech(request: SpeechRequest):
    if not is_ready():
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail={"error": {"message": "Model not loaded yet", "type": "model_error"}},
        )

    if not request.input or not request.input.strip():
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail={"error": {"message": "Input text cannot be empty", "type": "invalid_request_error"}},
        )

    if request.exaggeration is not None:
        print(f"[XTTS] Warning: 'exaggeration' parameter received ({request.exaggeration}) — ignored (XTTS does not support it)")
    if request.cfg_weight is not None:
        print(f"[XTTS] Warning: 'cfg_weight' parameter received ({request.cfg_weight}) — ignored (XTTS does not support it)")

    voice_path, language = resolve_voice_path_and_language(request.voice)

    temperature = request.temperature if request.temperature is not None else Config.TEMPERATURE
    speed = request.speed if request.speed is not None else Config.SPEED

    device = get_device()

    print(f"[XTTS] Generating speech: text_len={len(request.input)}, voice={voice_path}, lang={language}, temp={temperature}, speed={speed}")

    try:
        loop_start = time.perf_counter()
        text_clean = request.input.strip()

        # CUDA: keep on main thread — concurrent GPU forwards are unsafe. CPU: thread offload allows overlap.
        if str(device).lower() == "cpu":
            wav_buffer = await asyncio.to_thread(
                _blocking_synthesize_to_wav_buffer,
                text_clean,
                voice_path,
                language,
                temperature,
                speed,
            )
        else:
            wav_buffer = _blocking_synthesize_to_wav_buffer(
                text_clean,
                voice_path,
                language,
                temperature,
                speed,
            )

        inference_ms = (time.perf_counter() - loop_start) * 1000
        print(
            f"[perf-tts] inference+encode | input_chars={len(text_clean)} | ms={inference_ms:.0f} | device={device}",
            file=sys.stderr,
        )

        wav_bytes = len(wav_buffer.getvalue())
        print(
            f"[perf-tts] wav_bytes={wav_bytes}",
            file=sys.stderr,
        )

        total_ms = (time.perf_counter() - loop_start) * 1000
        print(
            f"[perf-tts] total | ms={total_ms:.0f}",
            file=sys.stderr,
        )

        return StreamingResponse(
            wav_buffer,
            media_type="audio/wav",
            headers={"Content-Disposition": "attachment; filename=speech.wav"},
        )

    except Exception as e:
        print(f"[XTTS] TTS generation failed: {e}", file=sys.stderr)
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail={
                "error": {
                    "message": f"TTS generation failed: {str(e)}",
                    "type": "generation_error",
                }
            },
        )
