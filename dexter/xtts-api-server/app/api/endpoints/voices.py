"""
Voice management endpoints for XTTS v2.
"""

import os
from fastapi import APIRouter, HTTPException, status, Form, File, UploadFile
from app.core.voice_library import get_voice_library, SUPPORTED_VOICE_FORMATS

router = APIRouter()


@router.get(
    "/voices",
    summary="List registered voices",
    description="List all registered voice samples with their metadata.",
)
async def list_voices():
    voice_lib = get_voice_library()
    voices = voice_lib.list_voices()

    return {
        "voices": [
            {
                "name": v["name"],
                "language": v.get("language", "en"),
                "upload_date": v["upload_date"],
                "file_size": v["file_size"],
                "file_extension": v["file_extension"],
            }
            for v in voices
        ]
    }


@router.post(
    "/voices",
    summary="Register a new voice",
    description="Upload a voice sample for cloning. The audio is saved and can be referenced by name in speech requests.",
)
async def register_voice(
    voice_name: str = Form(..., description="Name to register the voice as"),
    language: str = Form("pt", description="Language code for this voice"),
    voice_file: UploadFile = File(..., description="Audio file for voice cloning"),
):
    if not voice_file.filename:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail={"error": {"message": "No filename provided", "type": "invalid_request_error"}},
        )

    file_ext = os.path.splitext(voice_file.filename.lower())[1]
    if file_ext not in SUPPORTED_VOICE_FORMATS:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail={
                "error": {
                    "message": f"Unsupported audio format: {file_ext}. Supported: {', '.join(SUPPORTED_VOICE_FORMATS)}",
                    "type": "invalid_request_error",
                }
            },
        )

    file_content = await voice_file.read()

    max_size = 10 * 1024 * 1024
    if len(file_content) > max_size:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail={
                "error": {
                    "message": f"File too large. Maximum size: 10MB",
                    "type": "invalid_request_error",
                }
            },
        )

    if len(file_content) < 10 * 1024:
        print(f"[XTTS] Warning: Voice file is small ({len(file_content)} bytes). XTTS cloning works best with 6+ second clips.")

    voice_lib = get_voice_library()

    try:
        metadata = voice_lib.add_voice(
            voice_name=voice_name.strip(),
            file_content=file_content,
            original_filename=voice_file.filename,
            language=language.strip(),
        )
    except FileExistsError as e:
        # If voice already exists, delete and re-add (update)
        voice_lib.delete_voice(voice_name.strip())
        metadata = voice_lib.add_voice(
            voice_name=voice_name.strip(),
            file_content=file_content,
            original_filename=voice_file.filename,
            language=language.strip(),
        )
    except ValueError as e:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail={"error": {"message": str(e), "type": "invalid_request_error"}},
        )

    return {
        "status": "ok",
        "voice": {
            "name": metadata["name"],
            "language": metadata["language"],
            "file_extension": metadata["file_extension"],
            "file_size": metadata["file_size"],
            "upload_date": metadata["upload_date"],
        },
    }


@router.delete(
    "/voices/{voice_name}",
    summary="Delete a registered voice",
    description="Remove a voice sample from the library.",
)
async def delete_voice(voice_name: str):
    voice_lib = get_voice_library()
    deleted = voice_lib.delete_voice(voice_name.strip())

    if not deleted:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail={"error": {"message": f"Voice '{voice_name}' not found", "type": "not_found_error"}},
        )

    return {"status": "ok", "message": f"Voice '{voice_name}' deleted"}
