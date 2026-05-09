import { useState, useCallback, useRef } from 'react';
import type { TTSRequest, StreamingProgress, SSEEvent, AudioInfo } from '../types';
import { createTTSService } from '../services/tts';

interface UseStreamingTTSProps {
  apiBaseUrl: string;
  sessionId?: string;
}

interface StreamingState {
  isStreaming: boolean;
  progress: StreamingProgress | null;
  audioUrl: string | null;
  error: string | null;
  currentAudio: HTMLAudioElement | null;
  audioInfo: AudioInfo | null;
}

export function useStreamingTTS({ apiBaseUrl, sessionId }: UseStreamingTTSProps) {
  const [state, setState] = useState<StreamingState>({
    isStreaming: false,
    progress: null,
    audioUrl: null,
    error: null,
    currentAudio: null,
    audioInfo: null,
  });

  const [isStreamingEnabled, setIsStreamingEnabled] = useState(() => {
    if (typeof window === 'undefined') return false;
    try {
      return localStorage.getItem('chatterbox-streaming-enabled') === 'true';
    } catch {
      return false;
    }
  });

  const [streamingFormat, setStreamingFormat] = useState<'sse' | 'audio'>('sse');

  const abortControllerRef = useRef<AbortController | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const scheduledTimeRef = useRef<number>(0);
  const ttsService = createTTSService(apiBaseUrl, sessionId);

  // Save streaming preference
  const toggleStreaming = useCallback(() => {
    const newValue = !isStreamingEnabled;
    setIsStreamingEnabled(newValue);
    try {
      localStorage.setItem('chatterbox-streaming-enabled', newValue.toString());
    } catch (error) {
      console.error('Error saving streaming preference:', error);
    }
  }, [isStreamingEnabled]);

  // Properly concatenate raw PCM data into a WAV file
  const createFinalAudio = useCallback(async (chunks: ArrayBuffer[], audioInfo: AudioInfo): Promise<Blob> => {
    if (chunks.length === 0) return new Blob([], { type: 'audio/wav' });

    const totalLength = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
    const totalSamples = totalLength / (audioInfo.bits_per_sample / 8);
    const buffer = new ArrayBuffer(44 + totalLength);
    const view = new DataView(buffer);

    // WAV header
    const writeString = (offset: number, string: string) => {
      for (let i = 0; i < string.length; i++) {
        view.setUint8(offset + i, string.charCodeAt(i));
      }
    };

    const { sample_rate: sampleRate, channels, bits_per_sample: bitsPerSample } = audioInfo;

    writeString(0, 'RIFF');
    view.setUint32(4, 36 + totalLength, true);
    writeString(8, 'WAVE');
    writeString(12, 'fmt ');
    view.setUint32(16, 16, true); // PCM
    view.setUint16(20, 1, true); // PCM
    view.setUint16(22, channels, true);
    view.setUint32(24, sampleRate, true);
    view.setUint32(28, sampleRate * channels * (bitsPerSample / 8), true); // ByteRate
    view.setUint16(32, channels * (bitsPerSample / 8), true); // BlockAlign
    view.setUint16(34, bitsPerSample, true);
    writeString(36, 'data');
    view.setUint32(40, totalLength, true);

    // Write PCM data
    let offset = 44;
    for (const chunk of chunks) {
      new Uint8Array(buffer).set(new Uint8Array(chunk), offset);
      offset += chunk.byteLength;
    }

    return new Blob([buffer], { type: 'audio/wav' });
  }, []);

  // Play raw PCM audio chunk with proper scheduling
  const playAudioChunk = useCallback(async (pcmData: ArrayBuffer, audioInfo: AudioInfo) => {
    try {
      // Initialize audio context if needed
      if (!audioContextRef.current) {
        audioContextRef.current = new (window.AudioContext || (window as any).webkitAudioContext)();
        scheduledTimeRef.current = audioContextRef.current.currentTime;
      }

      const audioContext = audioContextRef.current;
      const { sample_rate: sampleRate, channels, bits_per_sample } = audioInfo;

      if (bits_per_sample !== 16) {
        throw new Error('Only 16-bit audio is supported for PCM playback');
      }

      const frameCount = pcmData.byteLength / (channels * (bits_per_sample / 8));
      const audioBuffer = audioContext.createBuffer(channels, frameCount, sampleRate);

      const pcmInt16 = new Int16Array(pcmData);
      const float32Data = new Float32Array(pcmInt16.length);
      for (let i = 0; i < pcmInt16.length; i++) {
        float32Data[i] = pcmInt16[i] / 32768;
      }

      audioBuffer.copyToChannel(float32Data, 0);

      // Create source and schedule playback
      const source = audioContext.createBufferSource();
      source.buffer = audioBuffer;
      source.connect(audioContext.destination);

      // Schedule this chunk to play after the previous one
      const startTime = Math.max(audioContext.currentTime, scheduledTimeRef.current);
      source.start(startTime);

      // Update scheduled time for next chunk
      scheduledTimeRef.current = startTime + audioBuffer.duration;
    } catch (error) {
      console.warn('Failed to play audio chunk:', error);
    }
  }, []);

  // Start SSE streaming
  const startSSEStreaming = useCallback(async (request: TTSRequest) => {
    try {
      setState(prev => ({
        ...prev,
        isStreaming: true,
        progress: null,
        error: null,
        audioUrl: null,
        audioInfo: null,
      }));

      abortControllerRef.current = new AbortController();

      // Reset audio scheduling
      if (audioContextRef.current) {
        scheduledTimeRef.current = audioContextRef.current.currentTime;
      }

      let localAudioInfo: AudioInfo | null = null;
      const pcmChunks: ArrayBuffer[] = [];

      for await (const { event, progress } of ttsService.generateSpeechSSE(request)) {
        if (abortControllerRef.current?.signal.aborted) break;

        setState(prev => ({ ...prev, progress }));

        if (event.type === 'speech.audio.info') {
          localAudioInfo = event;
          setState(prev => ({ ...prev, audioInfo: event }));
        }

        if (event.type === 'speech.audio.delta' && localAudioInfo) {
          // Decode base64 raw PCM data
          const audioData = atob(event.audio);
          const bytes = new Uint8Array(audioData.length);
          for (let i = 0; i < audioData.length; i++) {
            bytes[i] = audioData.charCodeAt(i);
          }
          const pcmData = bytes.buffer;
          pcmChunks.push(pcmData);

          // Play each chunk as it arrives for real-time experience
          await playAudioChunk(pcmData, localAudioInfo);
        }

        if (event.type === 'speech.audio.done') {
          // Create final downloadable audio
          if (localAudioInfo) {
            const finalBlob = await createFinalAudio(pcmChunks, localAudioInfo);
            const audioUrl = URL.createObjectURL(finalBlob);

            setState(prev => ({
              ...prev,
              isStreaming: false,
              audioUrl,
              progress: { ...progress, isComplete: true }
            }));
          } else {
            setState(prev => ({ ...prev, isStreaming: false, error: "Audio info not received" }));
          }
          break;
        }
      }
    } catch (error) {
      console.error('SSE streaming error:', error);
      setState(prev => ({
        ...prev,
        isStreaming: false,
        error: error instanceof Error ? error.message : 'Streaming failed'
      }));
    }
  }, [ttsService, createFinalAudio, playAudioChunk]);

  // Start raw audio streaming
  const startAudioStreaming = useCallback(async (request: TTSRequest) => {
    try {
      setState(prev => ({
        ...prev,
        isStreaming: true,
        progress: null,
        error: null,
        audioUrl: null
      }));

      abortControllerRef.current = new AbortController();
      const audioChunks: Blob[] = [];

      const mediaSource = new MediaSource();
      const audio = new Audio();
      audio.src = URL.createObjectURL(mediaSource);
      audio.play();

      mediaSource.addEventListener('sourceopen', async () => {
        const sourceBuffer = mediaSource.addSourceBuffer('audio/wav; codecs="1"');

        try {
          for await (const { chunk, progress } of ttsService.generateSpeechStream(request)) {
            if (abortControllerRef.current?.signal.aborted) break;

            const arrayBuffer = await chunk.arrayBuffer();
            sourceBuffer.appendBuffer(arrayBuffer);
            audioChunks.push(chunk);

            setState(prev => ({ ...prev, progress }));

            if (progress.isComplete) {
              // Let the player finish, then create final downloadable blob
              setTimeout(() => {
                if (mediaSource.readyState === "open") {
                  mediaSource.endOfStream();
                }
              }, 100);

              const finalBlob = new Blob(audioChunks, { type: 'audio/wav' });
              const audioUrl = URL.createObjectURL(finalBlob);

              setState(prev => ({
                ...prev,
                isStreaming: false,
                audioUrl,
                progress: { ...progress, isComplete: true }
              }));
              break;
            }
          }
        } catch (error) {
          console.error('Audio streaming error during stream consumption:', error);
          if (mediaSource.readyState === "open") {
            mediaSource.endOfStream();
          }
          setState(prev => ({
            ...prev,
            isStreaming: false,
            error: error instanceof Error ? error.message : 'Streaming failed'
          }));
        }
      });

    } catch (error) {
      console.error('Audio streaming error:', error);
      setState(prev => ({
        ...prev,
        isStreaming: false,
        error: error instanceof Error ? error.message : 'Streaming failed'
      }));
    }
  }, [ttsService]);

  // Main streaming function
  const startStreaming = useCallback(async (request: TTSRequest) => {
    if (streamingFormat === 'sse') {
      await startSSEStreaming(request);
    } else {
      await startAudioStreaming(request);
    }
  }, [streamingFormat, startSSEStreaming, startAudioStreaming]);

  // Stop streaming
  const stopStreaming = useCallback(() => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }

    // Stop any scheduled audio playback
    if (audioContextRef.current) {
      try {
        audioContextRef.current.close();
      } catch (error) {
        console.warn('Error closing audio context:', error);
      }
      audioContextRef.current = null;
      scheduledTimeRef.current = 0;
    }

    setState(prev => ({
      ...prev,
      isStreaming: false
    }));
  }, []);

  // Clear current audio
  const clearAudio = useCallback(() => {
    if (state.audioUrl) {
      URL.revokeObjectURL(state.audioUrl);
    }
    setState(prev => ({
      ...prev,
      audioUrl: null,
      progress: null,
      error: null
    }));
  }, [state.audioUrl]);

  return {
    // Streaming state
    isStreaming: state.isStreaming,
    progress: state.progress,
    audioUrl: state.audioUrl,
    error: state.error,
    audioInfo: state.audioInfo,

    // Streaming controls
    isStreamingEnabled,
    toggleStreaming,
    streamingFormat,
    setStreamingFormat,

    // Actions
    startStreaming,
    stopStreaming,
    clearAudio
  };
} 