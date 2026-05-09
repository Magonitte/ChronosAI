import { useState, useEffect, useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { createTTSService } from '../services/tts';
import { useApiEndpoint } from './useApiEndpoint';
import type { DefaultVoiceResponse, HealthResponse } from '../types';

export function useDefaultVoice() {
  const [defaultVoice, setDefaultVoice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { apiBaseUrl } = useApiEndpoint();

  // Memoize ttsService to prevent recreation on every render
  const ttsService = useMemo(() => createTTSService(apiBaseUrl), [apiBaseUrl]);

  // Monitor backend health to know when it's ready
  const healthQuery = useQuery<HealthResponse>({
    queryKey: ['health', apiBaseUrl],
    queryFn: ttsService.getHealth,
    refetchInterval: 3000, // Check every 3 seconds
    retry: true,
    retryDelay: 1000,
    staleTime: 1000,
  });

  // Load default voice from backend with proper dependency on health
  const defaultVoiceQuery = useQuery<DefaultVoiceResponse>({
    queryKey: ['default-voice', apiBaseUrl],
    queryFn: ttsService.getDefaultVoice,
    enabled: healthQuery.data?.status === 'healthy' || healthQuery.data?.status === 'initializing',
    retry: (failureCount, error: any) => {
      // Retry if the backend is still starting up or if it's a network error
      if (failureCount < 5) {
        const isNetworkError = error?.message?.includes('fetch') || error?.message?.includes('Failed to');
        const isServerError = error?.message?.includes('500') || error?.message?.includes('503');
        return isNetworkError || isServerError;
      }
      return false;
    },
    retryDelay: (attemptIndex) => Math.min(1000 * 2 ** attemptIndex, 5000),
    refetchOnWindowFocus: true,
    staleTime: 5000,
  });

  // Update local state when query data changes
  useEffect(() => {
    if (defaultVoiceQuery.data) {
      setDefaultVoice(defaultVoiceQuery.data.default_voice);
      setError(null);
    } else if (defaultVoiceQuery.error) {
      console.error('Failed to load default voice:', defaultVoiceQuery.error);
      setError('Failed to load default voice');
    }
  }, [defaultVoiceQuery.data, defaultVoiceQuery.error]);

  // Update default voice
  const updateDefaultVoice = useCallback(async (voiceName: string) => {
    try {
      setError(null);
      await ttsService.setDefaultVoice(voiceName);
      setDefaultVoice(voiceName);
      // Refetch to sync with backend
      defaultVoiceQuery.refetch();
      return true;
    } catch (error) {
      console.error('Failed to set default voice:', error);
      setError('Failed to set default voice');
      return false;
    }
  }, [ttsService, defaultVoiceQuery]);

  // Reset to no default (use file system default)
  const clearDefaultVoice = useCallback(async () => {
    try {
      setError(null);
      await ttsService.clearDefaultVoice();
      setDefaultVoice(null);
      // Refetch to sync with backend
      defaultVoiceQuery.refetch();
      return true;
    } catch (error) {
      console.error('Failed to reset default voice:', error);
      setError('Failed to reset default voice');
      return false;
    }
  }, [ttsService, defaultVoiceQuery]);

  return {
    defaultVoice,
    isLoading: defaultVoiceQuery.isLoading || healthQuery.isLoading,
    error,
    updateDefaultVoice,
    clearDefaultVoice,
    refetch: defaultVoiceQuery.refetch,
    // Expose backend ready state
    isBackendReady: healthQuery.data?.status === 'healthy' || healthQuery.data?.status === 'initializing',
    healthStatus: healthQuery.data?.status
  };
} 