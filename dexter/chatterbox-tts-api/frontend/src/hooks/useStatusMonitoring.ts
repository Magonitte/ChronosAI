import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { createTTSService } from '../services/tts';
import type { TTSProgress, TTSStatistics, APIInfo } from '../types';

export function useStatusMonitoring(apiBaseUrl: string) {
  const ttsService = useMemo(() => createTTSService(apiBaseUrl), [apiBaseUrl]);

  // Real-time progress monitoring (polls every second when processing)
  const progressQuery = useQuery<TTSProgress>({
    queryKey: ['tts-progress', apiBaseUrl],
    queryFn: ttsService.getProgress,
    refetchInterval: (query) => {
      // Poll every second if processing, every 5 seconds if idle
      return query.state.data?.is_processing ? 1000 : 5000;
    },
    refetchIntervalInBackground: false,
    retry: 2,
    retryDelay: 1000,
  });

  // Statistics (updates less frequently)
  const statisticsQuery = useQuery<TTSStatistics>({
    queryKey: ['tts-statistics', apiBaseUrl],
    queryFn: () => ttsService.getStatistics(true), // Include memory
    refetchInterval: 10000, // Every 10 seconds
    retry: 1,
    retryDelay: 2000,
  });

  // API info (updates infrequently)
  const apiInfoQuery = useQuery<APIInfo>({
    queryKey: ['api-info', apiBaseUrl],
    queryFn: ttsService.getApiInfo,
    refetchInterval: 30000, // Every 30 seconds
    retry: 1,
    staleTime: 15000,
  });

  const progress = progressQuery.data;
  const statistics = statisticsQuery.data;
  const apiInfo = apiInfoQuery.data;

  const isProcessing = progress?.is_processing ?? false;
  const hasError = progressQuery.isError || statisticsQuery.isError || apiInfoQuery.isError;

  return {
    // Data
    progress,
    statistics,
    apiInfo,

    // Status
    isProcessing,
    hasError,

    // Loading states
    isLoadingProgress: progressQuery.isLoading,
    isLoadingStats: statisticsQuery.isLoading,
    isLoadingApiInfo: apiInfoQuery.isLoading,

    // Error states
    progressError: progressQuery.error,
    statsError: statisticsQuery.error,
    apiInfoError: apiInfoQuery.error,

    // Refetch functions
    refetchProgress: progressQuery.refetch,
    refetchStats: statisticsQuery.refetch,
    refetchApiInfo: apiInfoQuery.refetch,
  };
} 