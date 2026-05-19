"""
Health check endpoint for XTTS v2 API.
"""

from fastapi import APIRouter
from app.core.tts_model import is_ready, get_device

router = APIRouter()


@router.get(
    "/health",
    summary="Health check",
    description="Check if the server and model are ready.",
)
async def health_check():
    return {
        "status": "ok",
        "model_loaded": is_ready(),
        "device": get_device() or "unknown",
    }
