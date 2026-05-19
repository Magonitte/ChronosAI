"""
Voice library management for storing and retrieving user-uploaded voices.
Simplified from Chatterbox — no aliases, rename, or complex metadata tracking.
"""

import os
import json
import hashlib
from typing import Dict, List, Optional
from datetime import datetime
from pathlib import Path

from app.config import Config

SUPPORTED_VOICE_FORMATS = {'.mp3', '.wav', '.flac', '.m4a', '.ogg'}


class VoiceLibrary:
    """Manages a library of voice samples for TTS generation."""

    def __init__(self, library_dir: str = None):
        self.library_dir = Path(library_dir or Config.VOICE_LIBRARY_DIR)
        self.metadata_file = self.library_dir / "voices.json"
        self._ensure_library_dir()
        self._metadata = self._load_metadata()

    def _ensure_library_dir(self):
        self.library_dir.mkdir(parents=True, exist_ok=True)

    def _load_metadata(self) -> Dict:
        if self.metadata_file.exists():
            try:
                with open(self.metadata_file, 'r', encoding='utf-8') as f:
                    return json.load(f)
            except (json.JSONDecodeError, FileNotFoundError):
                pass
        return {"voices": {}, "version": "1.0"}

    def _save_metadata(self):
        with open(self.metadata_file, 'w', encoding='utf-8') as f:
            json.dump(self._metadata, f, indent=2, ensure_ascii=False)

    def _get_file_hash(self, file_path: Path) -> str:
        hash_md5 = hashlib.md5()
        with open(file_path, "rb") as f:
            for chunk in iter(lambda: f.read(4096), b""):
                hash_md5.update(chunk)
        return hash_md5.hexdigest()

    def add_voice(self, voice_name: str, file_content: bytes, original_filename: str, language: str = "en") -> Dict:
        if not voice_name or not voice_name.strip():
            raise ValueError("Voice name cannot be empty")

        voice_name = voice_name.strip()

        if not language or not language.strip():
            raise ValueError("Language code cannot be empty")

        language = language.strip().lower()

        invalid_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|']
        if any(char in voice_name for char in invalid_chars):
            raise ValueError(f"Voice name contains invalid characters: {invalid_chars}")

        file_ext = Path(original_filename).suffix.lower()
        if file_ext not in SUPPORTED_VOICE_FORMATS:
            raise ValueError(f"Unsupported file format: {file_ext}. Supported: {', '.join(SUPPORTED_VOICE_FORMATS)}")

        if voice_name in self._metadata["voices"]:
            raise FileExistsError(f"Voice '{voice_name}' already exists")

        voice_filename = f"{voice_name}{file_ext}"
        voice_path = self.library_dir / voice_filename

        with open(voice_path, 'wb') as f:
            f.write(file_content)

        file_hash = self._get_file_hash(voice_path)

        metadata = {
            "name": voice_name,
            "filename": voice_filename,
            "original_filename": original_filename,
            "file_extension": file_ext,
            "file_size": len(file_content),
            "file_hash": file_hash,
            "upload_date": datetime.now().isoformat(),
            "path": str(voice_path),
            "language": language,
        }

        self._metadata["voices"][voice_name] = metadata
        self._save_metadata()

        return metadata

    def get_voice_path(self, voice_name: str) -> Optional[str]:
        if voice_name in self._metadata["voices"]:
            metadata = self._metadata["voices"][voice_name]
            voice_path = Path(metadata["path"])

            if not voice_path.exists():
                del self._metadata["voices"][voice_name]
                self._save_metadata()
                return None

            return str(voice_path)

        return None

    def list_voices(self) -> List[Dict]:
        voices = []
        voices_to_remove = []

        for voice_name, metadata in self._metadata["voices"].items():
            voice_path = Path(metadata["path"])
            if voice_path.exists():
                voice_data = {
                    **metadata,
                    "exists": True,
                    "language": metadata.get("language", "en"),
                }
                voices.append(voice_data)
            else:
                voices_to_remove.append(voice_name)

        for voice_name in voices_to_remove:
            del self._metadata["voices"][voice_name]

        if voices_to_remove:
            self._save_metadata()

        voices.sort(key=lambda x: x["upload_date"], reverse=True)
        return voices

    def delete_voice(self, voice_name: str) -> bool:
        if voice_name not in self._metadata["voices"]:
            return False

        metadata = self._metadata["voices"][voice_name]
        voice_path = Path(metadata["path"])

        if voice_path.exists():
            try:
                voice_path.unlink()
            except OSError:
                pass

        del self._metadata["voices"][voice_name]
        self._save_metadata()

        return True

    def get_voice_language(self, voice_name: str) -> Optional[str]:
        if voice_name not in self._metadata["voices"]:
            return None

        metadata = self._metadata["voices"][voice_name]
        return metadata.get("language", "en")

    def get_default_voice(self) -> Optional[str]:
        if self._metadata["voices"]:
            voices = list(self._metadata["voices"].keys())
            return voices[0]
        return None


_voice_library = None


def get_voice_library() -> VoiceLibrary:
    global _voice_library
    if _voice_library is None:
        _voice_library = VoiceLibrary()
    return _voice_library
