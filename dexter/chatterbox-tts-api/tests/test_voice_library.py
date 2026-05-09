#!/usr/bin/env python3
"""
Test script for voice library management functionality
"""

import requests
import json
from pathlib import Path

# Configuration
BASE_URL = "http://localhost:4123"
TEST_VOICE_NAME = "test_voice_sample"

def test_voice_library():
    """Test the voice library management endpoints"""
    
    print("🎵 Testing Voice Library Management API")
    print("=" * 50)
    
    # Test 1: List voices (initially empty)
    print("\n1️⃣ Testing voice library listing...")
    try:
        response = requests.get(f"{BASE_URL}/v1/voices")
        response.raise_for_status()
        
        voices_data = response.json()
        print(f"✅ Success! Found {voices_data['count']} voices in library")
        
        if voices_data['count'] > 0:
            print("📋 Current voices:")
            for voice in voices_data['voices']:
                print(f"   - {voice['name']} ({voice['file_size']} bytes)")
        
    except Exception as e:
        print(f"❌ Failed: {e}")
        return False
    
    # Test 2: Upload a voice (using the default voice sample as test)
    print("\n2️⃣ Testing voice upload...")
    
    # Look for the default voice sample to use as test upload
    project_root = Path(__file__).parent.parent
    voice_sample_paths = [
        project_root / "voice-sample.mp3",
        project_root / "assets" / "female_american.flac",
        Path("./voice-sample.mp3")
    ]
    
    voice_sample_file = None
    for path in voice_sample_paths:
        if path.exists():
            voice_sample_file = path
            break
    
    if voice_sample_file:
        try:
            print(f"   Using voice file: {voice_sample_file}")
            
            with open(voice_sample_file, "rb") as voice_file:
                response = requests.post(
                    f"{BASE_URL}/v1/voices",
                    data={"voice_name": TEST_VOICE_NAME},
                    files={"voice_file": (voice_sample_file.name, voice_file, "audio/mpeg")},
                    timeout=30
                )
                
                if response.status_code == 409:
                    print(f"ℹ️ Voice '{TEST_VOICE_NAME}' already exists, deleting first...")
                    
                    # Delete existing voice
                    delete_response = requests.delete(f"{BASE_URL}/v1/voices/{TEST_VOICE_NAME}")
                    delete_response.raise_for_status()
                    print(f"🗑️ Deleted existing voice")
                    
                    # Try upload again
                    with open(voice_sample_file, "rb") as voice_file_retry:
                        response = requests.post(
                            f"{BASE_URL}/v1/voices",
                            data={"voice_name": TEST_VOICE_NAME},
                            files={"voice_file": (voice_sample_file.name, voice_file_retry, "audio/mpeg")},
                            timeout=30
                        )
                
                response.raise_for_status()
                
                upload_result = response.json()
                print(f"✅ Success! Uploaded voice: {upload_result['voice']['name']}")
                print(f"   File size: {upload_result['voice']['file_size']} bytes")
                
        except Exception as e:
            print(f"❌ Failed: {e}")
            return False
    else:
        print("⚠️ No voice sample file found. Skipping upload test.")
        print("   Place a voice sample at ./voice-sample.mp3 to test voice upload.")
        return True
    
    # Test 3: Get voice info
    print("\n3️⃣ Testing voice info retrieval...")
    try:
        response = requests.get(f"{BASE_URL}/v1/voices/{TEST_VOICE_NAME}")
        response.raise_for_status()
        
        voice_info = response.json()['voice']
        print(f"✅ Success! Voice info for '{voice_info['name']}':")
        print(f"   - Filename: {voice_info['filename']}")
        print(f"   - Size: {voice_info['file_size']} bytes")
        print(f"   - Upload date: {voice_info['upload_date']}")
        
    except Exception as e:
        print(f"❌ Failed: {e}")
        return False
    
    # Test 4: Use voice in speech generation
    print("\n4️⃣ Testing speech generation with uploaded voice...")
    try:
        response = requests.post(
            f"{BASE_URL}/v1/audio/speech",
            headers={"Content-Type": "application/json"},
            json={
                "input": f"Hello! This is a test using the uploaded voice '{TEST_VOICE_NAME}'.",
                "voice": TEST_VOICE_NAME,
                "exaggeration": 0.6,
                "temperature": 0.8
            },
            timeout=60
        )
        response.raise_for_status()
        
        output_file = Path(__file__).parent / "outputs" / f"test_voice_library_{TEST_VOICE_NAME}.wav"
        output_file.parent.mkdir(exist_ok=True)
        
        with open(output_file, "wb") as f:
            f.write(response.content)
        
        print(f"✅ Success! Generated {len(response.content):,} bytes using voice library")
        print(f"   Saved to: {output_file}")
        
    except Exception as e:
        print(f"❌ Failed: {e}")
        return False
    
    # Test 5: Test voice renaming
    print("\n5️⃣ Testing voice renaming...")
    try:
        new_name = f"{TEST_VOICE_NAME}_renamed"
        
        response = requests.put(
            f"{BASE_URL}/v1/voices/{TEST_VOICE_NAME}",
            data={"new_name": new_name},
            timeout=30
        )
        response.raise_for_status()
        
        rename_result = response.json()
        print(f"✅ Success! Renamed voice from '{rename_result['old_name']}' to '{rename_result['new_name']}'")
        
        # Update test voice name for cleanup
        TEST_VOICE_NAME = new_name
        
    except Exception as e:
        print(f"❌ Failed: {e}")
        return False
    
    # Test 6: Clean up - delete the test voice
    print("\n6️⃣ Testing voice deletion...")
    try:
        response = requests.delete(f"{BASE_URL}/v1/voices/{TEST_VOICE_NAME}")
        response.raise_for_status()
        
        delete_result = response.json()
        print(f"✅ Success! Deleted voice: {delete_result['voice_name']}")
        
    except Exception as e:
        print(f"❌ Failed: {e}")
        return False
    
    # Test 7: Test error handling - voice not found
    print("\n7️⃣ Testing error handling (voice not found)...")
    try:
        response = requests.post(
            f"{BASE_URL}/v1/audio/speech",
            headers={"Content-Type": "application/json"},
            json={
                "input": "This should fail with voice not found.",
                "voice": "nonexistent_voice_12345"
            },
            timeout=30
        )
        
        if response.status_code == 404:
            error_data = response.json()
            print(f"✅ Success! Correctly handled voice not found error:")
            print(f"   Error: {error_data['error']['message']}")
        else:
            print(f"❌ Expected 404 error, got {response.status_code}")
            return False
        
    except Exception as e:
        print(f"❌ Failed: {e}")
        return False
    
    print("\n🎉 All voice library tests passed!")
    return True


def show_usage_examples():
    """Show usage examples for the voice library"""
    
    print("\n📖 Voice Library Usage Examples")
    print("=" * 40)
    
    print("\n🔹 Upload a voice:")
    print(f'curl -X POST "{BASE_URL}/v1/voices" \\')
    print('  -F "voice_name=my_voice" \\')
    print('  -F "voice_file=@/path/to/voice.mp3"')
    
    print("\n🔹 List all voices:")
    print(f'curl -X GET "{BASE_URL}/v1/voices"')
    
    print("\n🔹 Use voice in speech generation:")
    print(f'curl -X POST "{BASE_URL}/v1/audio/speech" \\')
    print('  -H "Content-Type: application/json" \\')
    print('  -d \'{"input": "Hello world", "voice": "my_voice"}\' \\')
    print('  --output speech.wav')
    
    print("\n🔹 Delete a voice:")
    print(f'curl -X DELETE "{BASE_URL}/v1/voices/my_voice"')
    
    print("\n🔹 Supported voice formats:")
    print("   • MP3 (.mp3)")
    print("   • WAV (.wav)")
    print("   • FLAC (.flac)")
    print("   • M4A (.m4a)")
    print("   • OGG (.ogg)")
    print("   • Maximum file size: 10MB")


if __name__ == "__main__":
    print("🎵 Chatterbox TTS - Voice Library Test Suite")
    print("Testing voice library management functionality...")
    
    try:
        # Test basic health first
        response = requests.get(f"{BASE_URL}/health", timeout=5)
        response.raise_for_status()
        print("✅ API is healthy and reachable")
        
        # Run voice library tests
        success = test_voice_library()
        
        if success:
            show_usage_examples()
        else:
            print("\n❌ Some tests failed. Check the API logs for more details.")
            
    except requests.exceptions.ConnectionError:
        print(f"❌ Cannot connect to API at {BASE_URL}")
        print("   Make sure the Chatterbox TTS API is running on port 4123")
    except Exception as e:
        print(f"❌ Test setup failed: {e}") 