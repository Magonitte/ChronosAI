"""
Pytest configuration file with shared fixtures and utilities for Chatterbox TTS API tests
"""

import os
import sys
import pytest
import requests
import time
from pathlib import Path
from typing import Dict, Any, Optional
from concurrent.futures import ThreadPoolExecutor, as_completed

# Add project root to path
PROJECT_ROOT = Path(__file__).parent.parent
sys.path.insert(0, str(PROJECT_ROOT))

# Test configuration
BASE_URL = os.getenv("CHATTERBOX_TEST_URL", "http://localhost:4123")
TEST_TIMEOUT = int(os.getenv("TEST_TIMEOUT", "600"))
API_HEALTH_TIMEOUT = int(os.getenv("API_HEALTH_TIMEOUT", "5"))

# Test data
TEST_TEXTS = {
    "short": "Hello, this is a simple test.",
    "medium": "The quick brown fox jumps over the lazy dog. This sentence contains every letter of the alphabet.",
    "long": "In a hole in the ground there lived a hobbit. Not a nasty, dirty, wet hole, filled with the ends of worms and an oozy smell, nor yet a dry, bare, sandy hole with nothing in it to sit down on or to eat: it was a hobbit-hole, and that means comfort.",
    "very_long": "This is a test. " * 200  # Should exceed max length for testing
}

TEST_PARAMETERS = {
    "default": {"exaggeration": 0.5, "cfg_weight": 0.5, "temperature": 0.8},
    "low_exaggeration": {"exaggeration": 0.3, "cfg_weight": 0.5, "temperature": 0.8},
    "high_exaggeration": {"exaggeration": 0.8, "cfg_weight": 0.5, "temperature": 0.8},
    "fast_speech": {"exaggeration": 0.5, "cfg_weight": 0.2, "temperature": 0.8},
    "slow_speech": {"exaggeration": 0.5, "cfg_weight": 0.8, "temperature": 0.8},
    "low_randomness": {"exaggeration": 0.5, "cfg_weight": 0.5, "temperature": 0.4},
    "high_randomness": {"exaggeration": 0.5, "cfg_weight": 0.5, "temperature": 1.2},
}


class APIClient:
    """Utility class for making API requests with consistent error handling"""
    
    def __init__(self, base_url: str = BASE_URL, timeout: int = TEST_TIMEOUT):
        self.base_url = base_url
        self.timeout = timeout
        
    def get(self, endpoint: str, **kwargs) -> requests.Response:
        """Make GET request"""
        url = f"{self.base_url.rstrip('/')}/{endpoint.lstrip('/')}"
        # Use timeout from kwargs if provided, otherwise use default
        timeout = kwargs.pop('timeout', self.timeout)
        return requests.get(url, timeout=timeout, **kwargs)
    
    def post(self, endpoint: str, **kwargs) -> requests.Response:
        """Make POST request"""
        url = f"{self.base_url.rstrip('/')}/{endpoint.lstrip('/')}"
        timeout = kwargs.pop('timeout', self.timeout)
        return requests.post(url, timeout=timeout, **kwargs)
    
    def put(self, endpoint: str, **kwargs) -> requests.Response:
        """Make PUT request"""
        url = f"{self.base_url.rstrip('/')}/{endpoint.lstrip('/')}"
        timeout = kwargs.pop('timeout', self.timeout)
        return requests.put(url, timeout=timeout, **kwargs)
    
    def delete(self, endpoint: str, **kwargs) -> requests.Response:
        """Make DELETE request"""
        url = f"{self.base_url.rstrip('/')}/{endpoint.lstrip('/')}"
        timeout = kwargs.pop('timeout', self.timeout)
        return requests.delete(url, timeout=timeout, **kwargs)
    
    def is_healthy(self) -> bool:
        """Check if API is healthy and responding"""
        try:
            response = self.get("/health", timeout=API_HEALTH_TIMEOUT)
            return response.status_code == 200
        except Exception:
            return False
    
    def wait_for_health(self, max_attempts: int = 10, delay: float = 1.0) -> bool:
        """Wait for API to become healthy"""
        for attempt in range(max_attempts):
            if self.is_healthy():
                return True
            if attempt < max_attempts - 1:
                time.sleep(delay)
        return False


@pytest.fixture(scope="session")
def api_client():
    """Provide an API client instance for tests"""
    return APIClient()


@pytest.fixture(scope="session", autouse=True)
def check_api_health(api_client):
    """Ensure API is running before running tests"""
    if not api_client.wait_for_health():
        pytest.skip(f"API not available at {BASE_URL}. Please start the server first.")


@pytest.fixture
def test_output_dir():
    """Create and provide test output directory"""
    output_dir = Path("test_outputs")
    output_dir.mkdir(exist_ok=True)
    return output_dir


@pytest.fixture
def voice_sample_file():
    """Find and provide a voice sample file for testing"""
    potential_paths = [
        PROJECT_ROOT / "voice-sample.mp3",
        PROJECT_ROOT / "assets" / "female_american.flac",
        PROJECT_ROOT / "assets" / "voice-sample.mp3",
    ]
    
    for path in potential_paths:
        if path.exists():
            return path
    
    pytest.skip("No voice sample file found for voice upload tests")


@pytest.fixture
def test_voice_name():
    """Provide a unique test voice name"""
    import uuid
    return f"test_voice_{uuid.uuid4().hex[:8]}"


@pytest.fixture
def cleanup_test_voices(api_client):
    """Clean up test voices after tests"""
    created_voices = []
    
    def add_voice(voice_name: str):
        created_voices.append(voice_name)
    
    yield add_voice
    
    # Cleanup
    for voice_name in created_voices:
        try:
            api_client.delete(f"/v1/voices/{voice_name}")
        except Exception:
            pass  # Voice might not exist or already deleted


@pytest.fixture
def memory_baseline(api_client):
    """Get baseline memory status before test"""
    try:
        response = api_client.post("/memory/reset?confirm=true")
        if response.status_code != 200:
            pytest.skip("Could not reset memory tracking")
        
        response = api_client.get("/memory")
        if response.status_code == 200:
            return response.json()
        else:
            pytest.skip("Could not get memory baseline")
    except Exception:
        pytest.skip("Memory endpoint not available")


def pytest_configure(config):
    """Configure pytest"""
    # Add custom markers
    config.addinivalue_line("markers", "api: API integration tests")
    config.addinivalue_line("markers", "slow: slow running tests")
    config.addinivalue_line("markers", "memory: memory management tests")
    config.addinivalue_line("markers", "voice: voice-related tests")
    config.addinivalue_line("markers", "regression: regression tests")


def pytest_collection_modifyitems(config, items):
    """Modify test collection"""
    # Add markers based on test file names
    for item in items:
        if "test_api" in item.nodeid:
            item.add_marker(pytest.mark.api)
        if "test_memory" in item.nodeid:
            item.add_marker(pytest.mark.memory)
            item.add_marker(pytest.mark.slow)
        if "test_voice" in item.nodeid:
            item.add_marker(pytest.mark.voice)
        if "test_regression" in item.nodeid:
            item.add_marker(pytest.mark.regression)
        if "test_status" in item.nodeid:
            item.add_marker(pytest.mark.api)


def pytest_runtest_setup(item):
    """Setup for each test"""
    # Skip slow tests if not explicitly requested
    if "slow" in [mark.name for mark in item.iter_markers()]:
        if not item.config.getoption("--runslow", default=False):
            pytest.skip("need --runslow option to run")


def pytest_addoption(parser):
    """Add custom command line options"""
    parser.addoption(
        "--runslow", action="store_true", default=False,
        help="run slow tests"
    )
    parser.addoption(
        "--api-url", action="store", default=BASE_URL,
        help="API URL for testing (default: http://localhost:4123)"
    )


# Utility functions for tests
def generate_speech_and_validate(
    api_client: APIClient,
    text: str,
    output_file: Optional[Path] = None,
    params: Optional[Dict[str, Any]] = None,
    endpoint: str = "/v1/audio/speech"
) -> Dict[str, Any]:
    """Generate speech and validate response"""
    payload = {"input": text}
    if params:
        payload.update(params)
    
    start_time = time.time()
    response = api_client.post(endpoint, json=payload)
    duration = time.time() - start_time
    
    result = {
        "success": response.status_code == 200,
        "status_code": response.status_code,
        "duration": duration,
        "payload": payload
    }
    
    if response.status_code == 200:
        result["audio_size"] = len(response.content)
        result["content"] = response.content
        
        if output_file:
            output_file.parent.mkdir(parents=True, exist_ok=True)
            with open(output_file, "wb") as f:
                f.write(response.content)
            result["output_file"] = output_file
    else:
        try:
            result["error"] = response.json()
        except:
            result["error"] = response.text
    
    return result


def run_concurrent_requests(
    api_client: APIClient,
    requests_data: list,
    max_workers: int = 3
) -> list:
    """Run multiple requests concurrently"""
    results = []
    
    def make_request(request_data):
        endpoint = request_data.get("endpoint", "/v1/audio/speech")
        payload = request_data.get("payload", {})
        request_id = request_data.get("id", 0)
        
        try:
            start_time = time.time()
            response = api_client.post(endpoint, json=payload)
            duration = time.time() - start_time
            
            return {
                "id": request_id,
                "success": response.status_code == 200,
                "status_code": response.status_code,
                "duration": duration,
                "audio_size": len(response.content) if response.status_code == 200 else 0,
                "payload": payload
            }
        except Exception as e:
            return {
                "id": request_id,
                "success": False,
                "error": str(e),
                "duration": time.time() - start_time,
                "payload": payload
            }
    
    with ThreadPoolExecutor(max_workers=max_workers) as executor:
        futures = [executor.submit(make_request, data) for data in requests_data]
        results = [future.result() for future in as_completed(futures)]
    
    return sorted(results, key=lambda x: x.get("id", 0)) 