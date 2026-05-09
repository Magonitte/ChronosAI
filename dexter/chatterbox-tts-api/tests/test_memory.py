#!/usr/bin/env python3
"""
Pytest-based memory management tests for Chatterbox TTS API
Tests memory usage patterns and cleanup functionality
"""

import pytest
import time

# Test data
TEST_TEXTS = {
    "short": "Hello, this is a simple test.",
    "medium": "The quick brown fox jumps over the lazy dog. This sentence contains every letter of the alphabet.",
    "long": "In a hole in the ground there lived a hobbit. Not a nasty, dirty, wet hole, filled with the ends of worms and an oozy smell, nor yet a dry, bare, sandy hole with nothing in it to sit down on or to eat: it was a hobbit-hole, and that means comfort.",
}


# Helper functions for tests
def generate_speech_and_validate(api_client, text, output_file=None, params=None, endpoint="/v1/audio/speech"):
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


def run_concurrent_requests(api_client, requests_data, max_workers=3):
    """Run multiple requests concurrently"""
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


class TestMemoryBaseline:
    """Test baseline memory functionality"""
    
    def test_memory_status_endpoint(self, api_client):
        """Test that memory status endpoint is available"""
        response = api_client.get("/memory")
        assert response.status_code == 200
        
        data = response.json()
        assert "memory_info" in data
        
    def test_memory_reset(self, api_client):
        """Test memory tracking reset"""
        response = api_client.post("/memory/reset?confirm=true")
        assert response.status_code == 200
        
        data = response.json()
        assert data.get("status") == "reset"
        
    def test_memory_cleanup(self, api_client):
        """Test manual memory cleanup"""
        response = api_client.get("/memory?cleanup=true&force_cuda_clear=true")
        assert response.status_code == 200
        
        data = response.json()
        assert "cleanup_performed" in data


class TestMemoryTracking:
    """Test memory tracking during operations"""
    
    def test_memory_tracking_during_speech_generation(self, api_client, memory_baseline):
        """Test memory tracking during a single speech generation"""
        # Get initial memory
        initial_response = api_client.get("/memory")
        assert initial_response.status_code == 200
        initial_memory = initial_response.json()
        
        # Generate speech
        text = TEST_TEXTS["medium"]
        result = generate_speech_and_validate(
            api_client, text, params={"exaggeration": 0.6}
        )
        assert result["success"]
        
        # Get memory after generation
        final_response = api_client.get("/memory")
        assert final_response.status_code == 200
        final_memory = final_response.json()
        
        # Verify memory tracking shows the request
        initial_requests = initial_memory.get("request_counter", 0)
        final_requests = final_memory.get("request_counter", 0)
        assert final_requests > initial_requests
        
    @pytest.mark.parametrize("text_key", ["short", "medium", "long"])
    def test_memory_usage_by_text_length(self, api_client, text_key):
        """Test memory usage with different text lengths"""
        # Reset memory tracking
        reset_response = api_client.post("/memory/reset?confirm=true")
        assert reset_response.status_code == 200
        
        # Get baseline
        baseline_response = api_client.get("/memory")
        assert baseline_response.status_code == 200
        baseline = baseline_response.json()
        
        # Generate speech
        text = TEST_TEXTS[text_key]
        result = generate_speech_and_validate(api_client, text)
        assert result["success"]
        
        # Check memory after
        after_response = api_client.get("/memory")
        assert after_response.status_code == 200
        after = after_response.json()
        
        # Memory should have increased
        if "memory_info" in baseline and "memory_info" in after:
            baseline_cpu = baseline["memory_info"].get("cpu_memory_mb", 0)
            after_cpu = after["memory_info"].get("cpu_memory_mb", 0)
            
            # Memory should increase with longer texts
            assert after_cpu >= baseline_cpu


class TestSequentialRequests:
    """Test memory usage with sequential requests"""
    
    @pytest.mark.slow
    def test_sequential_memory_usage(self, api_client, memory_baseline):
        """Test memory usage with multiple sequential requests"""
        memory_snapshots = []
        num_requests = 5
        
        for i in range(num_requests):
            # Get memory before request
            before_response = api_client.get("/memory")
            assert before_response.status_code == 200
            memory_before = before_response.json()
            
            # Generate speech
            text = f"Sequential memory test request {i+1}. {TEST_TEXTS['short']}"
            result = generate_speech_and_validate(
                api_client, text, params={"exaggeration": 0.5 + (i * 0.1)}
            )
            assert result["success"]
            
            # Get memory after request
            after_response = api_client.get("/memory")
            assert after_response.status_code == 200
            memory_after = after_response.json()
            
            memory_snapshots.append({
                "request_id": i+1,
                "memory_before": memory_before.get("memory_info", {}),
                "memory_after": memory_after.get("memory_info", {}),
                "request_counter": memory_after.get("request_counter", 0)
            })
            
            # Brief pause between requests
            time.sleep(0.5)
        
        # Verify memory tracking
        assert len(memory_snapshots) == num_requests
        
        # Check that request counter increased
        first_counter = memory_snapshots[0].get("request_counter", 0)
        last_counter = memory_snapshots[-1].get("request_counter", 0)
        assert last_counter >= first_counter + num_requests - 1
        
    @pytest.mark.slow  
    def test_memory_growth_trend(self, api_client, memory_baseline):
        """Test memory growth trends over multiple requests"""
        num_requests = 10
        cpu_values = []
        
        for i in range(num_requests):
            # Generate speech
            text = f"Memory growth test {i+1}. {TEST_TEXTS['short']}"
            result = generate_speech_and_validate(api_client, text)
            assert result["success"]
            
            # Get memory usage
            response = api_client.get("/memory")
            assert response.status_code == 200
            memory_data = response.json()
            
            if "memory_info" in memory_data:
                cpu_mb = memory_data["memory_info"].get("cpu_memory_mb", 0)
                cpu_values.append(cpu_mb)
            
            time.sleep(0.3)
        
        if cpu_values:
            # Check that memory doesn't grow excessively
            min_memory = min(cpu_values)
            max_memory = max(cpu_values)
            growth_ratio = max_memory / min_memory if min_memory > 0 else 1
            
            # Memory shouldn't grow more than 50% over baseline
            assert growth_ratio < 1.5, f"Memory grew by {((growth_ratio-1)*100):.1f}%"


class TestConcurrentMemoryUsage:
    """Test memory usage with concurrent requests"""
    
    @pytest.mark.slow
    def test_concurrent_memory_impact(self, api_client, memory_baseline):
        """Test memory impact of concurrent requests"""
        # Get memory before concurrent requests
        before_response = api_client.get("/memory")
        assert before_response.status_code == 200
        memory_before = before_response.json()
        
        # Run concurrent requests
        requests_data = [
            {
                "id": i,
                "endpoint": "/v1/audio/speech",
                "payload": {
                    "input": f"Concurrent memory test {i}. {TEST_TEXTS['short']}",
                    "exaggeration": 0.5 + (i * 0.1)
                }
            }
            for i in range(3)
        ]
        
        results = run_concurrent_requests(api_client, requests_data, max_workers=3)
        
        # Verify requests succeeded
        successful_results = [r for r in results if r["success"]]
        assert len(successful_results) >= 2  # Allow for some failures
        
        # Get memory after concurrent requests
        after_response = api_client.get("/memory")
        assert after_response.status_code == 200
        memory_after = after_response.json()
        
        # Verify memory tracking
        if "memory_info" in memory_before and "memory_info" in memory_after:
            before_cpu = memory_before["memory_info"].get("cpu_memory_mb", 0)
            after_cpu = memory_after["memory_info"].get("cpu_memory_mb", 0)
            
            # Memory should not grow excessively from concurrent requests
            if before_cpu > 0:
                growth_ratio = after_cpu / before_cpu
                assert growth_ratio < 2.0, f"Memory doubled during concurrent requests"


class TestMemoryCleanup:
    """Test memory cleanup functionality"""
    
    def test_manual_cleanup_reduces_memory(self, api_client):
        """Test that manual cleanup can reduce memory usage"""
        # Generate some speech to use memory
        for i in range(3):
            text = f"Cleanup test {i}. {TEST_TEXTS['medium']}"
            result = generate_speech_and_validate(api_client, text)
            assert result["success"]
        
        # Get memory before cleanup
        before_response = api_client.get("/memory")
        assert before_response.status_code == 200
        memory_before = before_response.json()
        
        # Perform cleanup
        cleanup_response = api_client.get("/memory?cleanup=true&force_cuda_clear=true")
        assert cleanup_response.status_code == 200
        cleanup_data = cleanup_response.json()
        
        # Check cleanup was performed
        assert cleanup_data.get("cleanup_performed") is True
        
        # Memory after cleanup should be reported
        if "memory_info_after_cleanup" in cleanup_data:
            after_memory = cleanup_data["memory_info_after_cleanup"]
            before_cpu = memory_before.get("memory_info", {}).get("cpu_memory_mb", 0)
            after_cpu = after_memory.get("cpu_memory_mb", 0)
            
            # Cleanup should not increase memory
            assert after_cpu <= before_cpu * 1.1  # Allow 10% variance
            
    def test_cleanup_with_collected_objects(self, api_client):
        """Test cleanup reports collected objects"""
        # Generate speech to create objects for cleanup
        text = TEST_TEXTS["medium"]
        result = generate_speech_and_validate(api_client, text)
        assert result["success"]
        
        # Perform cleanup
        response = api_client.get("/memory?cleanup=true")
        assert response.status_code == 200
        
        data = response.json()
        assert "cleanup_performed" in data
        
        # Should report collected objects (may be 0, which is fine)
        if "collected_objects" in data:
            assert isinstance(data["collected_objects"], int)
            assert data["collected_objects"] >= 0


class TestMemoryLimitsAndErrors:
    """Test memory-related limits and error conditions"""
    
    def test_memory_status_format(self, api_client):
        """Test memory status response format"""
        response = api_client.get("/memory")
        assert response.status_code == 200
        
        data = response.json()
        required_fields = ["memory_info", "request_counter"]
        
        for field in required_fields:
            assert field in data
            
        # Check memory_info structure
        memory_info = data["memory_info"]
        assert "cpu_memory_mb" in memory_info
        assert isinstance(memory_info["cpu_memory_mb"], (int, float))
        assert memory_info["cpu_memory_mb"] >= 0
        
    def test_invalid_cleanup_parameters(self, api_client):
        """Test cleanup with invalid parameters"""
        # Test with invalid parameter
        response = api_client.get("/memory?cleanup=invalid")
        # Should still work, parameter should be ignored or handled gracefully
        assert response.status_code in [200, 400]
        
    def test_reset_without_confirmation(self, api_client):
        """Test reset endpoint requires confirmation"""
        response = api_client.post("/memory/reset")
        # Should require confirmation
        assert response.status_code in [400, 422]


if __name__ == "__main__":
    # For backward compatibility, allow running as script
    pytest.main([__file__, "-v", "--runslow"]) 