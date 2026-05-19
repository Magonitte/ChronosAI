"""
Configuration management for XTTS v2 TTS API
"""

import os
import torch
from dotenv import load_dotenv

load_dotenv()

# Coqui XTTS pergunta CPML no stdin na primeira descarga; processos sem TTY falham com EOF.
# Defina COQUI_TOS_AGREED=0 no .env para desativar (ai precisa aceitar manualmente no terminal).
os.environ.setdefault("COQUI_TOS_AGREED", "1")


class Config:
    """Application configuration class"""

    HOST = os.getenv('HOST', '0.0.0.0')
    PORT = int(os.getenv('PORT', 8005))

    DEVICE_OVERRIDE = os.getenv('DEVICE', 'auto')
    MODEL_CACHE_DIR = os.getenv('MODEL_CACHE_DIR', './models')
    VOICE_LIBRARY_DIR = os.getenv('VOICE_LIBRARY_DIR', './voices')

    TEMPERATURE = float(os.getenv('TEMPERATURE', 0.7))
    SPEED = float(os.getenv('SPEED', 1.0))
    DEFAULT_LANGUAGE = os.getenv('DEFAULT_LANGUAGE', 'pt')

    CORS_ORIGINS = os.getenv('CORS_ORIGINS', '*')

    @classmethod
    def validate(cls):
        """Validate configuration values"""
        if not (0.05 <= cls.TEMPERATURE <= 5.0):
            raise ValueError(f"TEMPERATURE must be between 0.05 and 5.0, got {cls.TEMPERATURE}")
        if not (0.1 <= cls.SPEED <= 5.0):
            raise ValueError(f"SPEED must be between 0.1 and 5.0, got {cls.SPEED}")


def detect_device():
    """Detect the best available device"""
    if Config.DEVICE_OVERRIDE.lower() != 'auto':
        return Config.DEVICE_OVERRIDE.lower()

    if torch.cuda.is_available():
        return 'cuda'
    elif hasattr(torch.backends, 'mps') and torch.backends.mps.is_available():
        return 'mps'
    else:
        return 'cpu'
