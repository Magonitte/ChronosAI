#!/usr/bin/env python3
"""
Test script for SSE (Server-Side Events) streaming functionality
"""

import requests
import json
import base64
import time
import io
import wave

def test_sse_streaming():
    """Test SSE streaming endpoint"""
    
    print("🧪 Testing SSE streaming functionality...")
    
    # Test data
    test_text = "Hello world! This is a test of Server-Side Events streaming for text-to-speech conversion. The audio should be streamed in chunks with base64 encoding."
    
    # Test endpoint
    url = "http://localhost:4123/v1/audio/speech"
    
    # Request data for SSE streaming
    request_data = {
        "input": test_text,
        "stream_format": "sse",
        "exaggeration": 0.8,
        "streaming_strategy": "sentence",
        "streaming_chunk_size": 150
    }
    
    print(f"📝 Text to convert: {test_text}")
    print(f"🔧 Request parameters: {json.dumps(request_data, indent=2)}")
    
    try:
        # Make streaming request
        print("\n🚀 Starting SSE streaming request...")
        response = requests.post(
            url,
            json=request_data,
            stream=True,
            headers={'Accept': 'text/event-stream'}
        )
        
        if response.status_code != 200:
            print(f"❌ Request failed with status {response.status_code}")
            print(f"Response: {response.text}")
            return False
        
        print(f"✅ Response status: {response.status_code}")
        print(f"📋 Content-Type: {response.headers.get('content-type')}")
        
        # Process SSE events
        audio_chunks = []
        event_count = 0
        
        print("\n📡 Receiving SSE events...")
        
        for line in response.iter_lines(decode_unicode=True):
            if line.startswith('data: '):
                event_data = line[6:]  # Remove 'data: ' prefix
                
                try:
                    event_json = json.loads(event_data)
                    event_count += 1
                    
                    print(f"📦 Event #{event_count}: type={event_json.get('type', 'unknown')}")
                    
                    if event_json.get('type') == 'speech.audio.delta':
                        # Audio chunk event
                        audio_b64 = event_json.get('audio', '')
                        if audio_b64:
                            audio_data = base64.b64decode(audio_b64)
                            audio_chunks.append(audio_data)
                            print(f"   🎵 Audio chunk: {len(audio_data)} bytes (base64: {len(audio_b64)} chars)")
                    
                    elif event_json.get('type') == 'speech.audio.done':
                        # Completion event
                        usage = event_json.get('usage', {})
                        print(f"   ✅ Completion event:")
                        print(f"      - Input tokens: {usage.get('input_tokens', 0)}")
                        print(f"      - Output tokens: {usage.get('output_tokens', 0)}")
                        print(f"      - Total tokens: {usage.get('total_tokens', 0)}")
                        break
                    
                except json.JSONDecodeError as e:
                    print(f"   ⚠️ Failed to parse event JSON: {e}")
                    print(f"   Raw data: {event_data}")
        
        print(f"\n📊 Summary:")
        print(f"   - Total events received: {event_count}")
        print(f"   - Audio chunks: {len(audio_chunks)}")
        
        if audio_chunks:
            total_audio_size = sum(len(chunk) for chunk in audio_chunks)
            print(f"   - Total audio data: {total_audio_size:,} bytes")
            
            # Save combined audio for verification
            output_file = "test_sse_output.raw"
            with open(output_file, 'wb') as f:
                for chunk in audio_chunks:
                    f.write(chunk)
            print(f"   - Saved raw audio to: {output_file}")
            print(f"   💡 Note: This is raw PCM audio data (without WAV headers)")
            
            return True
        else:
            print("   ❌ No audio chunks received")
            return False
            
    except requests.exceptions.RequestException as e:
        print(f"❌ Request failed: {e}")
        return False
    except Exception as e:
        print(f"❌ Unexpected error: {e}")
        return False


def test_sse_vs_regular():
    """Compare SSE streaming vs regular audio generation"""
    
    print("\n🔄 Comparing SSE vs regular streaming...")
    
    test_text = "This is a comparison test between SSE streaming and regular audio generation."
    
    # Test regular audio generation
    print("\n1️⃣ Testing regular audio generation...")
    start_time = time.time()
    
    regular_response = requests.post(
        "http://localhost:4123/v1/audio/speech",
        json={
            "input": test_text,
            "stream_format": "audio"  # Regular audio
        }
    )
    
    regular_time = time.time() - start_time
    
    if regular_response.status_code == 200:
        regular_size = len(regular_response.content)
        print(f"   ✅ Regular audio: {regular_size:,} bytes in {regular_time:.2f}s")
        
        # Save regular audio
        with open("test_regular_output.wav", "wb") as f:
            f.write(regular_response.content)
        print("   💾 Saved to: test_regular_output.wav")
    else:
        print(f"   ❌ Regular audio failed: {regular_response.status_code}")
    
    # Test SSE streaming
    print("\n2️⃣ Testing SSE streaming...")
    start_time = time.time()
    
    sse_response = requests.post(
        "http://localhost:4123/v1/audio/speech",
        json={
            "input": test_text,
            "stream_format": "sse"
        },
        stream=True
    )
    
    if sse_response.status_code == 200:
        chunks_received = 0
        first_chunk_time = None
        
        for line in sse_response.iter_lines(decode_unicode=True):
            if line.startswith('data: '):
                event_data = line[6:]
                try:
                    event_json = json.loads(event_data)
                    if event_json.get('type') == 'speech.audio.delta':
                        chunks_received += 1
                        if first_chunk_time is None:
                            first_chunk_time = time.time() - start_time
                    elif event_json.get('type') == 'speech.audio.done':
                        break
                except:
                    pass
        
        total_sse_time = time.time() - start_time
        
        print(f"   ✅ SSE streaming: {chunks_received} chunks in {total_sse_time:.2f}s")
        if first_chunk_time:
            print(f"   ⚡ First chunk received in: {first_chunk_time:.2f}s")
    else:
        print(f"   ❌ SSE streaming failed: {sse_response.status_code}")


def main():
    """Main test function"""
    print("🎯 Chatterbox TTS API - SSE Streaming Test")
    print("=" * 50)
    
    # Check if API is running
    try:
        health_response = requests.get("http://localhost:4123/health")
        if health_response.status_code != 200:
            print("❌ API is not responding properly")
            return
        print("✅ API is running")
    except:
        print("❌ Cannot connect to API at http://localhost:4123")
        print("   Make sure the Chatterbox TTS API is running")
        return
    
    # Run tests
    print("\n" + "=" * 50)
    success = test_sse_streaming()
    
    if success:
        print("\n" + "=" * 50)
        test_sse_vs_regular()
    
    print("\n" + "=" * 50)
    if success:
        print("🎉 SSE streaming test completed successfully!")
    else:
        print("❌ SSE streaming test failed")
    
    print("\n💡 Usage examples:")
    print("   curl -X POST http://localhost:4123/v1/audio/speech \\")
    print("     -H 'Content-Type: application/json' \\")
    print("     -d '{\"input\": \"Hello world!\", \"stream_format\": \"sse\"}' \\")
    print("     -H 'Accept: text/event-stream'")


if __name__ == "__main__":
    main() 