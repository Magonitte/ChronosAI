"""
Main FastAPI application for XTTS v2 TTS API.
"""

from contextlib import asynccontextmanager
from fastapi import FastAPI, HTTPException, status
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse

from app.core.tts_model import initialize_model
from app.core.voice_library import get_voice_library
from app.config import Config
from app.api.endpoints import speech, voices, health


@asynccontextmanager
async def lifespan(app: FastAPI):
    print("[XTTS] Initializing voice library...")
    voice_lib = get_voice_library()
    default_voice = voice_lib.get_default_voice()
    if default_voice:
        print(f"[XTTS] Found default voice: {default_voice}")
    else:
        print("[XTTS] No voices registered yet. Use POST /voices to register one.")

    print("[XTTS] Loading XTTS v2 model...")
    await initialize_model()
    print("[XTTS] Model ready.")

    yield

    print("[XTTS] Shutting down...")


app = FastAPI(
    title="XTTS v2 TTS API",
    description="REST API for XTTS v2 (Coqui TTS) with OpenAI-compatible endpoints",
    version="1.0.0",
    docs_url="/docs",
    redoc_url="/redoc",
    lifespan=lifespan,
)

cors_origins = Config.CORS_ORIGINS
if cors_origins == "*":
    allowed_origins = ["*"]
else:
    allowed_origins = [origin.strip() for origin in cors_origins.split(",")]

app.add_middleware(
    CORSMiddleware,
    allow_origins=allowed_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Include routers with /v1 prefix for OpenAI compatibility
app.include_router(speech.router, prefix="/v1")
app.include_router(voices.router)
app.include_router(health.router)


@app.exception_handler(HTTPException)
async def http_exception_handler(request, exc):
    return JSONResponse(
        status_code=exc.status_code,
        content=exc.detail,
    )


@app.exception_handler(Exception)
async def general_exception_handler(request, exc):
    return JSONResponse(
        status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
        content={
            "error": {
                "message": f"Internal server error: {str(exc)}",
                "type": "internal_error",
            }
        },
    )
