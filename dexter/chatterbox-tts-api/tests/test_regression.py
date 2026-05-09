#!/usr/bin/env python3
"""
Pytest-based regression tests for Chatterbox TTS API
Tests to ensure backward compatibility and prevent regressions
"""

import pytest

# Test data
TEST_TEXTS = {
    "short": "Hello, this is a simple test.",
    "medium": "The quick brown fox jumps over the lazy dog. This sentence contains every letter of the alphabet.",
    "long": "In a hole in the ground there lived a hobbit. Not a nasty, dirty, wet hole, filled with the ends of worms and an oozy smell, nor yet a dry, bare, sandy hole with nothing in it to sit down on or to eat: it was a hobbit-hole, and that means comfort.",
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


class TestBackwardCompatibility:
    """Test backward compatibility with existing API behavior"""
    
    def test_tts_default_voice(self, api_client, test_output_dir):
        """TTS endpoint should work with no voice parameter (default voice)"""
        output_file = test_output_dir / "regression_default_voice.wav"
        
        result = generate_speech_and_validate(
            api_client,
            "Hello world!",
            output_file,
            {"exaggeration": 0.8, "cfg_weight": 0.5, "temperature": 1.0}
        )
        
        assert result["success"], f"Default voice failed: {result.get('error')}"
        assert result["audio_size"] > 0
        assert output_file.exists()
        
    def test_tts_openai_voice_name(self, api_client, test_output_dir):
        """TTS endpoint should work with OpenAI-compatible voice name (should use default sample)"""
        output_file = test_output_dir / "regression_openai_voice.wav"
        
        result = generate_speech_and_validate(
            api_client,
            "Hello world!",
            output_file,
            {
                "voice": "echo",  # OpenAI voice name
                "exaggeration": 0.8,
                "cfg_weight": 0.5,
                "temperature": 1.0
            }
        )
        
        assert result["success"], f"OpenAI voice compatibility failed: {result.get('error')}"
        assert result["audio_size"] > 0
        
    def test_tts_custom_voice_fallback(self, api_client):
        """TTS endpoint should gracefully fallback to default if custom voice does not exist"""
        response = api_client.post(
            "/v1/audio/speech",
            json={
                "input": "Hello world!",
                "voice": "nonexistent_voice_12345",
                "exaggeration": 0.8,
                "cfg_weight": 0.5,
                "temperature": 1.0
            }
        )
        
        # Should either succeed with fallback or return 404
        assert response.status_code in [200, 404]
        
        if response.status_code == 404:
            error_data = response.json()
            assert "voice" in str(error_data).lower()


class TestEndpointAliases:
    """Test that old endpoint aliases still work"""
    
    def test_health_endpoint_alias(self, api_client):
        """Old health endpoint should still be available"""
        response = api_client.get("/health")
        assert response.status_code == 200
        
        data = response.json()
        assert "status" in data or "message" in data
        
    def test_status_endpoint_alias(self, api_client):
        """Status endpoint should be available"""
        response = api_client.get("/status")
        assert response.status_code == 200
        
        data = response.json()
        # Should have status information
        assert isinstance(data, dict)


class TestParameterHandling:
    """Test parameter handling regressions"""
    
    @pytest.mark.parametrize("param_set", list(TEST_PARAMETERS.keys()))
    def test_parameter_ranges_regression(self, api_client, param_set):
        """Test that parameter ranges haven't changed unexpectedly"""
        params = TEST_PARAMETERS[param_set]
        
        result = generate_speech_and_validate(
            api_client,
            TEST_TEXTS["short"],
            params=params
        )
        
        assert result["success"], f"Parameter set {param_set} failed: {result.get('error')}"
        
    def test_ignored_openai_parameters_regression(self, api_client):
        """Test that OpenAI parameters are still gracefully ignored"""
        openai_params = {
            "voice": "alloy",
            "response_format": "wav", 
            "speed": 1.5,
            "model": "tts-1",
            # Mix with valid params
            "exaggeration": 0.7,
            "temperature": 0.8
        }
        
        result = generate_speech_and_validate(
            api_client,
            "Testing OpenAI parameter compatibility",
            params=openai_params
        )
        
        assert result["success"], "OpenAI parameter compatibility broken"
        
    def test_parameter_validation_regression(self, api_client):
        """Test that parameter validation hasn't become more restrictive"""
        # These should all fail with validation errors, not server errors
        invalid_cases = [
            {"exaggeration": -0.1},  # Below range
            {"exaggeration": 1.1},   # Above range
            {"cfg_weight": -0.1},    # Below range
            {"cfg_weight": 1.1},     # Above range
            {"temperature": -0.1},   # Below range
            {"temperature": 2.1},    # Above range
        ]
        
        for invalid_params in invalid_cases:
            invalid_params["input"] = "test"
            response = api_client.post("/v1/audio/speech", json=invalid_params)
            assert response.status_code == 422, f"Validation failed for {invalid_params}"


class TestResponseFormat:
    """Test response format regressions"""
    
    def test_audio_response_format(self, api_client):
        """Test that audio response format hasn't changed"""
        result = generate_speech_and_validate(
            api_client,
            TEST_TEXTS["short"]
        )
        
        assert result["success"]
        assert result["audio_size"] > 0
        
        # Audio should be reasonable size (not empty, not huge)
        assert 1000 < result["audio_size"] < 10_000_000
        
    def test_error_response_format(self, api_client):
        """Test that error response format is consistent"""
        # Empty input should return validation error
        response = api_client.post("/v1/audio/speech", json={"input": ""})
        assert response.status_code == 422
        
        error_data = response.json()
        assert isinstance(error_data, dict)
        # Should have error details
        assert "detail" in error_data or "error" in error_data


class TestPerformanceRegression:
    """Test for performance regressions"""
    
    def test_response_time_regression(self, api_client):
        """Test that response times haven't significantly increased"""
        text = TEST_TEXTS["medium"]
        
        result = generate_speech_and_validate(
            api_client, 
            text,
            params=TEST_PARAMETERS["default"]
        )
        
        assert result["success"]
        
        # Should complete within reasonable time (adjust based on your hardware)
        assert result["duration"] < 60, f"Response too slow: {result['duration']:.2f}s"
        
        # Should generate reasonable amount of audio per second
        if result["duration"] > 0:
            audio_rate = result["audio_size"] / result["duration"]
            assert audio_rate > 500, f"Audio generation too slow: {audio_rate:.1f} bytes/sec"
            
    def test_memory_usage_regression(self, api_client):
        """Test that memory usage patterns haven't regressed"""
        # Get initial memory
        initial_response = api_client.get("/memory")
        if initial_response.status_code != 200:
            pytest.skip("Memory endpoint not available")
            
        initial_memory = initial_response.json()
        initial_cpu = initial_memory.get("memory_info", {}).get("cpu_memory_mb", 0)
        
        # Generate several requests
        for i in range(3):
            result = generate_speech_and_validate(
                api_client,
                f"Memory regression test {i}. {TEST_TEXTS['short']}"
            )
            assert result["success"]
            
        # Check final memory
        final_response = api_client.get("/memory")
        assert final_response.status_code == 200
        final_memory = final_response.json()
        final_cpu = final_memory.get("memory_info", {}).get("cpu_memory_mb", 0)
        
        # Memory shouldn't have grown excessively
        if initial_cpu > 0:
            growth_ratio = final_cpu / initial_cpu
            assert growth_ratio < 2.0, f"Memory usage doubled: {growth_ratio:.2f}x"


class TestAPIStructureRegression:
    """Test that API structure hasn't changed unexpectedly"""
    
    def test_openapi_schema_structure(self, api_client):
        """Test that OpenAPI schema hasn't lost important endpoints"""
        response = api_client.get("/openapi.json")
        assert response.status_code == 200
        
        schema = response.json()
        paths = schema.get("paths", {})
        
        # Critical endpoints should still exist
        critical_endpoints = [
            "/v1/audio/speech",
            "/v1/audio/speech/upload", 
            "/health",
            "/v1/models"
        ]
        
        for endpoint in critical_endpoints:
            assert endpoint in paths, f"Critical endpoint {endpoint} missing from API"
            
    def test_cors_headers_regression(self, api_client):
        """Test that CORS headers are still present if they were before"""
        response = api_client.get("/health")
        assert response.status_code == 200
        
        # Check if CORS headers are present (this might vary by deployment)
        headers = response.headers
        # Don't enforce CORS headers as they may not always be needed
        # but if they exist, they should be reasonable
        if "Access-Control-Allow-Origin" in headers:
            assert headers["Access-Control-Allow-Origin"] in ["*", "http://localhost:3000"]


class TestFeatureRegression:
    """Test that features haven't been accidentally removed"""
    
    def test_voice_library_absence_fallback(self, api_client, test_output_dir):
        """If no custom voices are present, system should still work with default voice"""
        output_file = test_output_dir / "regression_no_voices.wav"
        
        result = generate_speech_and_validate(
            api_client,
            "Testing default voice when no custom voices available",
            output_file,
            {"exaggeration": 0.8, "cfg_weight": 0.5, "temperature": 1.0}
        )
        
        assert result["success"], "Default voice fallback broken"
        assert result["audio_size"] > 0
        
    def test_upload_endpoint_without_file(self, api_client, test_output_dir):
        """Upload endpoint should work without actual file upload (using default voice)"""
        data = {
            "input": "Testing upload endpoint without file",
            "exaggeration": 0.7,
            "temperature": 0.9
        }
        
        response = api_client.post("/v1/audio/speech/upload", data=data)
        assert response.status_code == 200
        
        output_file = test_output_dir / "regression_upload_no_file.wav"
        with open(output_file, "wb") as f:
            f.write(response.content)
            
        assert len(response.content) > 0
        assert output_file.exists()
        
    def test_concurrent_request_handling(self, api_client):
        """Test that concurrent requests still work"""
        
        requests_data = [
            {
                "id": i,
                "endpoint": "/v1/audio/speech",
                "payload": {
                    "input": f"Regression concurrent test {i}",
                    "exaggeration": 0.5
                }
            }
            for i in range(2)
        ]
        
        results = run_concurrent_requests(api_client, requests_data, max_workers=2)
        
        assert len(results) == 2
        successful_results = [r for r in results if r["success"]]
        assert len(successful_results) >= 1, "Concurrent request handling broken"


if __name__ == "__main__":
    # For backward compatibility, allow running as script
    pytest.main([__file__, "-v"])
