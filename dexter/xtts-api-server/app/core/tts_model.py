"""
XTTS v2 model management for TTS generation.
"""

import sys
from app.config import Config, detect_device

_model = None
_device = None


async def initialize_model():
    """Load XTTS v2 model once at startup."""
    global _model, _device

    _device = detect_device()
    print(f"[XTTS] Device detected: {_device}")

    print("[XTTS] Loading XTTS v2 model (first run will download ~1.87GB)...")
    try:
        from TTS.api import TTS
        _model = TTS(
            model_name="tts_models/multilingual/multi-dataset/xtts_v2",
            progress_bar=True
        )
        _model.to(_device)
        print(f"[XTTS] Model loaded successfully on {_device}")
    except Exception as e:
        print(f"[XTTS] Failed to load model: {e}", file=sys.stderr)
        raise


def get_model():
    """Get the loaded TTS model instance."""
    return _model


def get_device():
    """Get the device the model is running on."""
    return _device


def is_ready():
    """Check if the model is loaded and ready."""
    return _model is not None


def get_sample_rate():
    """XTTS v2 sample rate is 24000 Hz."""
    return 24000
