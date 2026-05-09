#!/usr/bin/env python3
"""
Pytest-based API tests for the Chatterbox TTS API
"""

import pytest
from pathlib import Path

# Import test utilities (these are defined in conftest.py and available as fixtures)
# Import TEST_TEXTS and TEST_PARAMETERS directly here
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


# Helper functions for tests
def generate_speech_and_validate(api_client, text, output_file=None, params=None, endpoint="/v1/audio/speech"):
    """Generate speech and validate response"""
    import time
    
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


def run_concurrent_requests(api_client, requests_data, max_workers=3):
    """Run multiple requests concurrently"""
    import time
    from concurrent.futures import ThreadPoolExecutor, as_completed
    
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


class TestHealthEndpoints:
    """Test health and basic endpoint functionality"""
    
    def test_health_check(self, api_client):
        """Test the health check endpoint"""
        response = api_client.get("/health")
        assert response.status_code == 200
        
        data = response.json()
        assert "status" in data
        
    def test_models_endpoint(self, api_client):
        """Test the models listing endpoint"""
        response = api_client.get("/v1/models")
        assert response.status_code == 200
        
        data = response.json()
        assert "data" in data or "models" in data
        
    def test_api_documentation_endpoints(self, api_client):
        """Test that FastAPI documentation is available"""
        # Test OpenAPI schema
        response = api_client.get("/openapi.json")
        assert response.status_code == 200
        
        # Test Swagger UI
        response = api_client.get("/docs")
        assert response.status_code == 200
        
        # Test ReDoc
        response = api_client.get("/redoc")
        assert response.status_code == 200


class TestTextToSpeechJSON:
    """Test the main JSON text-to-speech endpoint"""
    
    @pytest.mark.parametrize("text_key", ["short", "medium", "long"])
    def test_tts_json_with_various_texts(self, api_client, test_output_dir, text_key):
        """Test TTS JSON endpoint with various text lengths"""
        text = TEST_TEXTS[text_key]
        output_file = test_output_dir / f"test_json_{text_key}.wav"
        
        result = generate_speech_and_validate(
            api_client, text, output_file, TEST_PARAMETERS["default"]
        )
        
        assert result["success"], f"TTS failed: {result.get('error', 'Unknown error')}"
        assert result["audio_size"] > 0
        assert result["duration"] > 0
        assert output_file.exists()
        
    @pytest.mark.parametrize("param_key", list(TEST_PARAMETERS.keys()))
    def test_tts_json_with_custom_parameters(self, api_client, test_output_dir, param_key):
        """Test TTS JSON endpoint with various parameter combinations"""
        text = TEST_TEXTS["medium"]
        params = TEST_PARAMETERS[param_key]
        output_file = test_output_dir / f"test_json_params_{param_key}.wav"
        
        result = generate_speech_and_validate(
            api_client, text, output_file, params
        )
        
        assert result["success"], f"TTS failed with {param_key}: {result.get('error', 'Unknown error')}"
        assert result["audio_size"] > 0
        
    def test_tts_json_with_ignored_openai_params(self, api_client, test_output_dir):
        """Test that OpenAI parameters are gracefully ignored"""
        text = TEST_TEXTS["short"]
        params = {
            "voice": "alloy",  # Should be ignored
            "response_format": "wav",  # Should be ignored
            "speed": 1.0,  # Should be ignored
            "exaggeration": 0.7
        }
        output_file = test_output_dir / "test_json_openai_params.wav"
        
        result = generate_speech_and_validate(
            api_client, text, output_file, params
        )
        
        assert result["success"], f"TTS failed: {result.get('error', 'Unknown error')}"
        assert result["audio_size"] > 0


class TestTextToSpeechUpload:
    """Test the upload endpoint (form data, no file)"""
    
    @pytest.mark.parametrize("text_key", ["short", "medium"])
    def test_tts_upload_without_file(self, api_client, test_output_dir, text_key):
        """Test upload endpoint without file upload"""
        text = TEST_TEXTS[text_key]
        params = TEST_PARAMETERS["default"]
        
        data = {"input": text, **params}
        
        response = api_client.post("/v1/audio/speech/upload", data=data)
        assert response.status_code == 200
        
        output_file = test_output_dir / f"test_upload_{text_key}.wav"
        with open(output_file, "wb") as f:
            f.write(response.content)
        
        assert len(response.content) > 0
        assert output_file.exists()
        
    def test_tts_upload_with_custom_params(self, api_client, test_output_dir):
        """Test upload endpoint with custom parameters"""
        text = TEST_TEXTS["short"]
        params = TEST_PARAMETERS["high_exaggeration"]
        
        data = {"input": text, **params}
        
        response = api_client.post("/v1/audio/speech/upload", data=data)
        assert response.status_code == 200
        
        output_file = test_output_dir / "test_upload_custom.wav"
        with open(output_file, "wb") as f:
            f.write(response.content)
        
        assert len(response.content) > 0


class TestErrorHandling:
    """Test error handling with invalid requests"""
    
    def test_missing_input_json(self, api_client):
        """Test missing input field (JSON endpoint)"""
        response = api_client.post(
            "/v1/audio/speech",
            json={"voice": "alloy"}
        )
        assert response.status_code == 422  # FastAPI validation error
        
    def test_missing_input_upload(self, api_client):
        """Test missing input field (upload endpoint)"""
        response = api_client.post(
            "/v1/audio/speech/upload",
            data={"voice": "alloy"}
        )
        assert response.status_code == 422  # FastAPI validation error
        
    def test_empty_input_json(self, api_client):
        """Test empty input (JSON)"""
        response = api_client.post(
            "/v1/audio/speech",
            json={"input": ""}
        )
        assert response.status_code == 422  # FastAPI validation error
        
    def test_empty_input_upload(self, api_client):
        """Test empty input (upload)"""
        response = api_client.post(
            "/v1/audio/speech/upload",
            data={"input": ""}
        )
        assert response.status_code == 422  # FastAPI validation error
        
    def test_invalid_parameter_range_json(self, api_client):
        """Test invalid parameter ranges (JSON)"""
        response = api_client.post(
            "/v1/audio/speech",
            json={"input": "test", "exaggeration": 5.0}  # Out of range
        )
        assert response.status_code == 422
        
    def test_text_too_long_json(self, api_client):
        """Test very long text (JSON)"""
        long_text = TEST_TEXTS["very_long"]
        response = api_client.post(
            "/v1/audio/speech",
            json={"input": long_text}
        )
        assert response.status_code == 400  # Text too long


class TestConcurrentRequests:
    """Test concurrent request handling"""
    
    @pytest.mark.slow
    def test_concurrent_json_requests(self, api_client):
        """Test concurrent JSON requests"""
        requests_data = [
            {
                "id": i,
                "endpoint": "/v1/audio/speech",
                "payload": {
                    "input": f"Concurrent test request {i}",
                    "exaggeration": 0.5 + (i * 0.1)
                }
            }
            for i in range(3)
        ]
        
        results = run_concurrent_requests(api_client, requests_data, max_workers=3)
        
        assert len(results) == 3
        successful_results = [r for r in results if r["success"]]
        assert len(successful_results) >= 2  # Allow for some failures in concurrent scenarios
        
        for result in successful_results:
            assert result["audio_size"] > 0
            assert result["duration"] > 0
            
    @pytest.mark.slow
    def test_concurrent_upload_requests(self, api_client):
        """Test concurrent upload requests"""
        requests_data = [
            {
                "id": i,
                "endpoint": "/v1/audio/speech/upload", 
                "payload": {
                    "input": f"Concurrent upload test {i}",
                    "temperature": 0.8 + (i * 0.1)
                }
            }
            for i in range(2)
        ]
        
        results = run_concurrent_requests(api_client, requests_data, max_workers=2)
        
        assert len(results) == 2
        successful_results = [r for r in results if r["success"]]
        assert len(successful_results) >= 1  # Allow for some failures
        
        for result in successful_results:
            assert result["audio_size"] > 0


class TestPerformanceMetrics:
    """Test performance and timing"""
    
    def test_tts_performance_baseline(self, api_client):
        """Test basic performance metrics"""
        text = TEST_TEXTS["medium"]
        
        result = generate_speech_and_validate(
            api_client, text, params=TEST_PARAMETERS["default"]
        )
        
        assert result["success"]
        assert result["duration"] < 30  # Should complete within 30 seconds
        assert result["audio_size"] > 1000  # Should generate reasonable amount of audio
        
        # Check audio size to duration ratio (rough quality check)
        audio_per_second = result["audio_size"] / result["duration"]
        assert audio_per_second > 1000  # Basic sanity check


if __name__ == "__main__":
    # For backward compatibility, allow running as script
    pytest.main([__file__, "-v"]) 