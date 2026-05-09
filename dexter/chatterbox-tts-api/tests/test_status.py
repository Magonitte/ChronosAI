#!/usr/bin/env python3
"""
Test script for TTS status endpoints

This script tests the new status tracking endpoints to ensure they work correctly.
"""

import requests
import time
import threading
from concurrent.futures import ThreadPoolExecutor

# Configuration
API_BASE_URL = "http://localhost:4123"


def test_status_endpoints():
    """Test all status endpoints"""
    print("🧪 Testing TTS Status Endpoints")
    print("=" * 60)
    
    # Test 1: Basic status endpoint
    print("\n1️⃣ Testing basic status endpoint...")
    try:
        response = requests.get(f"{API_BASE_URL}/status")
        response.raise_for_status()
        
        status_data = response.json()
        print(f"✅ Status endpoint working")
        print(f"   Processing: {status_data.get('is_processing', False)}")
        print(f"   Status: {status_data.get('status', 'unknown')}")
        print(f"   Total requests: {status_data.get('total_requests', 0)}")
        
    except Exception as e:
        print(f"❌ Status endpoint failed: {e}")
    
    # Test 2: Progress endpoint
    print("\n2️⃣ Testing progress endpoint...")
    try:
        response = requests.get(f"{API_BASE_URL}/status/progress")
        response.raise_for_status()
        
        progress_data = response.json()
        print(f"✅ Progress endpoint working")
        print(f"   Processing: {progress_data.get('is_processing', False)}")
        
        if progress_data.get('is_processing'):
            print(f"   Current step: {progress_data.get('current_step', '')}")
            print(f"   Progress: {progress_data.get('progress_percentage', 0):.1f}%")
        
    except Exception as e:
        print(f"❌ Progress endpoint failed: {e}")
    
    # Test 3: Statistics endpoint
    print("\n3️⃣ Testing statistics endpoint...")
    try:
        response = requests.get(f"{API_BASE_URL}/status/statistics")
        response.raise_for_status()
        
        stats_data = response.json()
        print(f"✅ Statistics endpoint working")
        print(f"   Total requests: {stats_data.get('total_requests', 0)}")
        print(f"   Success rate: {stats_data.get('success_rate', 0):.1f}%")
        print(f"   Average duration: {stats_data.get('average_duration_seconds', 0):.2f}s")
        
    except Exception as e:
        print(f"❌ Statistics endpoint failed: {e}")
    
    # Test 4: History endpoint
    print("\n4️⃣ Testing history endpoint...")
    try:
        response = requests.get(f"{API_BASE_URL}/status/history?limit=3")
        response.raise_for_status()
        
        history_data = response.json()
        print(f"✅ History endpoint working")
        print(f"   History records: {history_data.get('total_records', 0)}")
        
        if history_data.get('request_history'):
            for i, record in enumerate(history_data['request_history'][:2]):
                print(f"   Recent #{i+1}: {record.get('status', 'unknown')} - {record.get('text_preview', '')[:30]}...")
        
    except Exception as e:
        print(f"❌ History endpoint failed: {e}")
    
    # Test 5: API info endpoint
    print("\n5️⃣ Testing API info endpoint...")
    try:
        response = requests.get(f"{API_BASE_URL}/info")
        response.raise_for_status()
        
        info_data = response.json()
        print(f"✅ API info endpoint working")
        print(f"   API: {info_data.get('api_name', 'Unknown')} v{info_data.get('version', '0.0.0')}")
        print(f"   Status: {info_data.get('status', 'unknown')}")
        
        if 'memory_info' in info_data:
            mem = info_data['memory_info']
            print(f"   Memory: {mem.get('cpu_memory_mb', 0):.1f}MB CPU")
        
    except Exception as e:
        print(f"❌ API info endpoint failed: {e}")


def test_status_during_generation():
    """Test status tracking during actual TTS generation"""
    print("\n" + "=" * 60)
    print("🎤 Testing Status During TTS Generation")
    print("=" * 60)
    
    def monitor_status():
        """Monitor status in a separate thread"""
        print("\n📊 Status monitoring started...")
        for i in range(15):  # Monitor for 15 seconds
            try:
                response = requests.get(f"{API_BASE_URL}/status/progress", timeout=2)
                if response.status_code == 200:
                    progress = response.json()
                    if progress.get('is_processing'):
                        print(f"   [{i:2d}s] Processing: {progress.get('current_step', '')} "
                              f"({progress.get('current_chunk', 0)}/{progress.get('total_chunks', 0)})")
                    else:
                        print(f"   [{i:2d}s] Idle")
                else:
                    print(f"   [{i:2d}s] Status check failed")
            except Exception as e:
                print(f"   [{i:2d}s] Status error: {e}")
            
            time.sleep(1)
        print("📊 Status monitoring finished")
    
    def generate_speech():
        """Generate speech to test status tracking"""
        try:
            print("\n🎵 Starting TTS generation...")
            response = requests.post(
                f"{API_BASE_URL}/v1/audio/speech",
                json={
                    "input": "This is a test of the status tracking system. We want to see how the progress is reported during text-to-speech generation with multiple chunks of text that should demonstrate the chunking and processing stages.",
                    "exaggeration": 0.7,
                    "temperature": 0.8
                },
                timeout=60
            )
            
            if response.status_code == 200:
                print(f"✅ TTS generation completed ({len(response.content):,} bytes)")
            else:
                print(f"❌ TTS generation failed: {response.status_code}")
                
        except Exception as e:
            print(f"❌ TTS generation error: {e}")
    
    # Start monitoring in background
    monitor_thread = threading.Thread(target=monitor_status, daemon=True)
    monitor_thread.start()
    
    # Generate speech in foreground
    generate_speech()
    
    # Wait for monitoring to finish
    monitor_thread.join(timeout=20)
    
    # Check final status
    print("\n📈 Final status check...")
    try:
        response = requests.get(f"{API_BASE_URL}/status?include_stats=true&include_history=true")
        if response.status_code == 200:
            final_status = response.json()
            print(f"   Processing: {final_status.get('is_processing', False)}")
            
            if 'statistics' in final_status:
                stats = final_status['statistics']
                print(f"   Total requests: {stats.get('total_requests', 0)}")
                print(f"   Completed: {stats.get('completed_requests', 0)}")
                print(f"   Success rate: {stats.get('success_rate', 0):.1f}%")
            
            if 'request_history' in final_status and final_status['request_history']:
                last_request = final_status['request_history'][0]
                print(f"   Last request: {last_request.get('status', 'unknown')} "
                      f"({last_request.get('duration_seconds', 0):.2f}s)")
        
    except Exception as e:
        print(f"❌ Final status check failed: {e}")


def test_concurrent_requests():
    """Test status tracking with concurrent requests"""
    print("\n" + "=" * 60)
    print("🔄 Testing Concurrent Requests")
    print("=" * 60)
    
    def make_request(request_id: int):
        """Make a TTS request"""
        try:
            print(f"🎤 Request {request_id} starting...")
            response = requests.post(
                f"{API_BASE_URL}/v1/audio/speech",
                json={
                    "input": f"This is concurrent request number {request_id} testing the status system.",
                    "exaggeration": 0.5 + (request_id * 0.1)
                },
                timeout=30
            )
            
            if response.status_code == 200:
                print(f"✅ Request {request_id} completed ({len(response.content):,} bytes)")
                return True
            else:
                print(f"❌ Request {request_id} failed: {response.status_code}")
                return False
                
        except Exception as e:
            print(f"❌ Request {request_id} error: {e}")
            return False
    
    # Make 3 concurrent requests
    with ThreadPoolExecutor(max_workers=3) as executor:
        futures = [executor.submit(make_request, i+1) for i in range(3)]
        results = [future.result() for future in futures]
    
    print(f"\n📊 Concurrent test results: {sum(results)}/{len(results)} successful")


def main():
    """Run all status tests"""
    print("🚀 Chatterbox TTS Status Testing Suite")
    print("Make sure the API is running at", API_BASE_URL)
    print()
    
    # Test basic endpoints
    test_status_endpoints()
    
    # Test status during generation
    test_status_during_generation()
    
    # Test concurrent requests
    test_concurrent_requests()
    
    print("\n" + "=" * 60)
    print("🎯 Testing Complete!")
    print("Check the API logs to see detailed status tracking information.")


if __name__ == "__main__":
    main() 