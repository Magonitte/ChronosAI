#!/usr/bin/env python3
"""
Test script for voice upload functionality

This script demonstrates how to use the voice upload feature
with both the main JSON endpoint and the new upload endpoint.
"""

import os
import sys
import requests
from pathlib import Path

# Add the project root to the path so we can import from tests
project_root = Path(__file__).parent.parent
sys.path.append(str(project_root))

BASE_URL = "http://localhost:4123"
OUTPUT_DIR = Path("test_output")


def test_voice_upload():
    """Test the voice upload functionality"""
    print("🎤 Testing Voice Upload Functionality")
    print("=" * 50)
    
    # Create output directory
    OUTPUT_DIR.mkdir(exist_ok=True)
    
    # Test 1: Main JSON endpoint (default voice)
    print("\n1️⃣ Testing main JSON endpoint (default voice)...")
    try:
        response = requests.post(
            f"{BASE_URL}/v1/audio/speech",
            json={
                "input": "Hello! This is using the default configured voice sample.",
                "exaggeration": 0.7
            },
            timeout=60
        )
        response.raise_for_status()
        
        output_file = OUTPUT_DIR / "test_main_json_voice.wav"
        with open(output_file, "wb") as f:
            f.write(response.content)
        
        print(f"✅ Success! Generated {len(response.content):,} bytes")
        print(f"   Saved to: {output_file}")
        
    except Exception as e:
        print(f"❌ Failed: {e}")
    
    # Test 2: Upload endpoint (default voice, no file)
    print("\n2️⃣ Testing upload endpoint (default voice, no file upload)...")
    try:
        response = requests.post(
            f"{BASE_URL}/v1/audio/speech/upload",
            data={
                "input": "Hello! This is using the upload endpoint without a file.",
                "exaggeration": 0.6,
                "temperature": 0.9
            },
            timeout=60
        )
        response.raise_for_status()
        
        output_file = OUTPUT_DIR / "test_upload_default_voice.wav"
        with open(output_file, "wb") as f:
            f.write(response.content)
        
        print(f"✅ Success! Generated {len(response.content):,} bytes")
        print(f"   Saved to: {output_file}")
        
    except Exception as e:
        print(f"❌ Failed: {e}")
    
    # Test 3: Custom voice upload
    print("\n3️⃣ Testing custom voice upload...")
    
    # Look for the default voice sample to use as test upload
    voice_sample_paths = [
        project_root / "voice-sample.mp3",
        project_root / "assets" / "voice-sample.mp3",
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
                    f"{BASE_URL}/v1/audio/speech/upload",
                    data={
                        "input": "Amazing! This is using an uploaded custom voice file.",
                        "exaggeration": 0.8,
                        "cfg_weight": 0.4,
                        "temperature": 1.0
                    },
                    files={
                        "voice_file": ("custom_voice.mp3", voice_file, "audio/mpeg")
                    },
                    timeout=120  # Longer timeout for custom voice processing
                )
                response.raise_for_status()
                
                output_file = OUTPUT_DIR / "test_custom_voice.wav"
                with open(output_file, "wb") as f:
                    f.write(response.content)
                
                print(f"✅ Success! Generated {len(response.content):,} bytes")
                print(f"   Saved to: {output_file}")
                
        except Exception as e:
            print(f"❌ Failed: {e}")
    else:
        print("⚠️ No voice sample file found. Skipping custom voice test.")
        print("   Place a voice sample at ./voice-sample.mp3 to test voice upload.")
    
    # Test 4: Error handling - invalid file format
    print("\n4️⃣ Testing error handling (invalid file format)...")
    try:
        # Create a dummy text file to test invalid format
        dummy_file_content = b"This is not an audio file"
        
        response = requests.post(
            f"{BASE_URL}/v1/audio/speech/upload",
            data={
                "input": "This should fail due to invalid voice file.",
            },
            files={
                "voice_file": ("invalid.txt", dummy_file_content, "text/plain")
            },
            timeout=30
        )
        
        if response.status_code == 400:
            error_data = response.json()
            print(f"✅ Correctly rejected invalid file format")
            print(f"   Error: {error_data.get('error', {}).get('message', 'Unknown error')}")
        else:
            print(f"❌ Expected 400 error, got {response.status_code}")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
    
    # Test 5: Error handling - empty input (JSON endpoint)
    print("\n5️⃣ Testing error handling (empty input - JSON endpoint)...")
    try:
        response = requests.post(
            f"{BASE_URL}/v1/audio/speech",
            json={
                "input": "",  # Empty input
            },
            timeout=30
        )
        
        if response.status_code == 422:  # FastAPI validation error
            print(f"✅ Correctly rejected empty input (JSON)")
        else:
            print(f"❌ Expected 422 error, got {response.status_code}")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
    
    # Test 6: Error handling - empty input (upload endpoint)
    print("\n6️⃣ Testing error handling (empty input - upload endpoint)...")
    try:
        response = requests.post(
            f"{BASE_URL}/v1/audio/speech/upload",
            data={
                "input": "",  # Empty input
            },
            timeout=30
        )
        
        if response.status_code == 422:  # FastAPI validation error
            print(f"✅ Correctly rejected empty input (upload)")
        else:
            print(f"❌ Expected 422 error, got {response.status_code}")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
    
    print("\n🎯 Testing Complete!")
    print(f"Check the '{OUTPUT_DIR}' directory for generated audio files.")


def test_api_docs():
    """Test that API documentation is updated"""
    print("\n📚 Testing API Documentation...")
    
    try:
        # Check OpenAPI schema
        response = requests.get(f"{BASE_URL}/openapi.json")
        response.raise_for_status()
        
        openapi_data = response.json()
        
        # Check main speech endpoint
        speech_endpoint = openapi_data.get("paths", {}).get("/v1/audio/speech", {})
        if speech_endpoint:
            print("✅ Main /v1/audio/speech endpoint documented")
        else:
            print("❌ Main speech endpoint not found in API documentation")
        
        # Check upload endpoint
        upload_endpoint = openapi_data.get("paths", {}).get("/v1/audio/speech/upload", {})
        if upload_endpoint:
            # Check if voice_file parameter is documented
            post_params = upload_endpoint.get("post", {}).get("requestBody", {})
            if "multipart/form-data" in str(post_params):
                print("✅ Upload endpoint includes multipart form data support")
            else:
                print("⚠️ Upload endpoint may not have multipart support documented")
        else:
            print("❌ Upload endpoint not found in API documentation")
            
        # Check if docs are accessible
        docs_response = requests.get(f"{BASE_URL}/docs")
        if docs_response.status_code == 200:
            print("✅ API docs accessible at /docs")
        else:
            print("❌ API docs not accessible")
            
    except Exception as e:
        print(f"❌ Documentation test failed: {e}")


def show_usage_examples():
    """Show usage examples for the voice upload feature"""
    print("\n📖 Usage Examples")
    print("=" * 50)
    
    print("\n🔹 Python example (default voice - JSON endpoint):")
    print("""
import requests

# Main JSON endpoint (recommended for default voice)
response = requests.post(
    "http://localhost:4123/v1/audio/speech",
    json={
        "input": "Hello world!",
        "exaggeration": 0.7
    }
)

with open("output.wav", "wb") as f:
    f.write(response.content)
""")
    
    print("\n🔹 Python example (default voice - upload endpoint):")
    print("""
import requests

# Upload endpoint without file (uses default voice)
response = requests.post(
    "http://localhost:4123/v1/audio/speech/upload",
    data={
        "input": "Hello world!",
        "exaggeration": 0.7
    }
)

with open("output.wav", "wb") as f:
    f.write(response.content)
""")
    
    print("\n🔹 Python example (custom voice upload):")
    print("""
import requests

with open("my_voice.mp3", "rb") as voice_file:
    response = requests.post(
        "http://localhost:4123/v1/audio/speech/upload",
        data={
            "input": "Hello with my custom voice!",
            "exaggeration": 0.8,
            "temperature": 1.0
        },
        files={
            "voice_file": ("my_voice.mp3", voice_file, "audio/mpeg")
        }
    )

with open("output.wav", "wb") as f:
    f.write(response.content)
""")
    
    print("\n🔹 cURL example (JSON - default voice):")
    print("""
curl -X POST http://localhost:4123/v1/audio/speech \\
  -H "Content-Type: application/json" \\
  -d '{"input": "Hello world!", "exaggeration": 0.8}' \\
  --output output.wav
""")
    
    print("\n🔹 cURL example (custom voice upload):")
    print("""
curl -X POST http://localhost:4123/v1/audio/speech/upload \\
  -F "input=Hello with my custom voice!" \\
  -F "exaggeration=0.8" \\
  -F "voice_file=@my_voice.mp3" \\
  --output custom_voice_output.wav
""")
    
    print("\n🔹 Endpoint summary:")
    print("   • /v1/audio/speech - JSON only, uses configured voice sample")
    print("   • /v1/audio/speech/upload - Form data, optional voice file upload")
    
    print("\n🔹 Supported voice file formats:")
    print("   • MP3 (.mp3)")
    print("   • WAV (.wav)")
    print("   • FLAC (.flac)")
    print("   • M4A (.m4a)")
    print("   • OGG (.ogg)")
    print("   • Maximum file size: 10MB")


if __name__ == "__main__":
    try:
        # Check if the API is running
        response = requests.get(f"{BASE_URL}/health", timeout=5)
        if response.status_code != 200:
            print(f"❌ API not responding properly. Status: {response.status_code}")
            sys.exit(1)
            
        print("✅ API is running")
        
        # Run tests
        test_voice_upload()
        test_api_docs()
        show_usage_examples()
        
    except requests.exceptions.ConnectionError:
        print(f"❌ Cannot connect to API at {BASE_URL}")
        print("   Make sure the API is running with: python main.py")
        sys.exit(1)
    except Exception as e:
        print(f"❌ Test failed: {e}")
        sys.exit(1) 