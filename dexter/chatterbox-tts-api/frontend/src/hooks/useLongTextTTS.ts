import { useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { createLongTextTTSService } from '../services/longTextTTS';
import type {
  LongTextRequest,
  LongTextJobResponse,
  LongTextJobListItem,
  LongTextProgress,
  LongTextSSEEvent,
  LongTextJobStatus
} from '../types';

interface UseLongTextTTSProps {
  apiBaseUrl: string;
  sessionId?: string;
}

interface LongTextTTSState {
  currentJob: LongTextJobResponse | null;
  progress: LongTextProgress | null;
  isJobActive: boolean;
  error: string | null;
  audioUrl: string | null;
}

export function useLongTextTTS({ apiBaseUrl, sessionId }: UseLongTextTTSProps) {
  const [state, setState] = useState<LongTextTTSState>({
    currentJob: null,
    progress: null,
    isJobActive: false,
    error: null,
    audioUrl: null
  });

  // Track job IDs in localStorage for persistence across sessions
  const [trackedJobIds, setTrackedJobIds] = useState<string[]>(() => {
    if (typeof window === 'undefined') return [];
    try {
      const stored = localStorage.getItem('chatterbox-long-text-jobs');
      return stored ? JSON.parse(stored) : [];
    } catch {
      return [];
    }
  });

  const sseCleanupRef = useRef<(() => void) | null>(null);

  // Memoize service creation to prevent recreating on every render
  const service = useMemo(
    () => createLongTextTTSService(apiBaseUrl, sessionId),
    [apiBaseUrl, sessionId]
  );

  // Save tracked job IDs to localStorage
  const updateTrackedJobIds = useCallback((jobIds: string[]) => {
    setTrackedJobIds(jobIds);
    try {
      localStorage.setItem('chatterbox-long-text-jobs', JSON.stringify(jobIds));
    } catch (error) {
      console.error('Failed to save job IDs to localStorage:', error);
    }
  }, []);

  // Add a job ID to tracking
  const addJobId = useCallback((jobId: string) => {
    setTrackedJobIds(current => {
      if (current.includes(jobId)) return current;
      const updated = [...current, jobId];
      try {
        localStorage.setItem('chatterbox-long-text-jobs', JSON.stringify(updated));
      } catch (error) {
        console.error('Failed to save job IDs to localStorage:', error);
      }
      return updated;
    });
  }, []);

  // Remove a job ID from tracking
  const removeJobId = useCallback((jobId: string) => {
    setTrackedJobIds(current => {
      const updated = current.filter(id => id !== jobId);
      try {
        localStorage.setItem('chatterbox-long-text-jobs', JSON.stringify(updated));
      } catch (error) {
        console.error('Failed to save job IDs to localStorage:', error);
      }
      return updated;
    });
  }, []);

  // Submit new job mutation
  const submitJobMutation = useMutation({
    mutationFn: service.submitJob,
    onSuccess: (response) => {
      addJobId(response.job_id);
      setState(prev => ({
        ...prev,
        error: null,
        isJobActive: true
      }));
      // Start monitoring this job
      startJobMonitoring(response.job_id);
    },
    onError: (error: Error) => {
      setState(prev => ({
        ...prev,
        error: error.message,
        isJobActive: false
      }));
    }
  });

  // Job control mutations
  const pauseJobMutation = useMutation({
    mutationFn: service.pauseJob,
    onError: (error: Error) => {
      setState(prev => ({ ...prev, error: error.message }));
    }
  });

  const resumeJobMutation = useMutation({
    mutationFn: service.resumeJob,
    onError: (error: Error) => {
      setState(prev => ({ ...prev, error: error.message }));
    }
  });

  const cancelJobMutation = useMutation({
    mutationFn: service.cancelJob,
    onSuccess: (_, jobId) => {
      removeJobId(jobId);
      setState(prev => ({
        ...prev,
        isJobActive: false,
        currentJob: null,
        progress: null,
        audioUrl: null
      }));
      cleanupSSE();
    },
    onError: (error: Error) => {
      setState(prev => ({ ...prev, error: error.message }));
    }
  });

  // Query for listing all jobs
  const {
    data: jobList,
    isLoading: isLoadingJobs,
    refetch: refetchJobs
  } = useQuery({
    queryKey: ['longTextJobs', apiBaseUrl, sessionId],
    queryFn: () => service.listJobs({ limit: 10 }),
    refetchInterval: 5000, // Refresh every 5 seconds for active jobs
    retry: true
  });

  // Clean up SSE connection
  const cleanupSSE = useCallback(() => {
    if (sseCleanupRef.current) {
      sseCleanupRef.current();
      sseCleanupRef.current = null;
    }
  }, []);

  // Download completed audio (defined before startJobMonitoring to avoid forward reference)
  const downloadCompletedAudio = useCallback(async (jobId: string) => {
    try {
      const audioBlob = await service.downloadJobAudio(jobId);
      const audioUrl = URL.createObjectURL(audioBlob);
      setState(prev => ({ ...prev, audioUrl }));
    } catch (error) {
      console.error('Failed to download completed audio:', error);
      setState(prev => ({
        ...prev,
        error: `Failed to download audio: ${error instanceof Error ? error.message : 'Unknown error'}`
      }));
    }
  }, [service]);

  // Start monitoring a job with SSE
  const startJobMonitoring = useCallback((jobId: string) => {
    cleanupSSE();

    const cleanup = service.subscribeToSSE(
      jobId,
      (event: LongTextSSEEvent) => {
        console.log(`[Long Text TTS] Received SSE event for job ${jobId}:`, event.event_type, event.data);
        setState(prev => {
          let newState = { ...prev };

          // Update progress if provided (check that data exists first)
          if (event.data && event.data.progress !== undefined) {
            newState.progress = event.data.progress;
          }

          // Handle different event types
          switch (event.event_type) {
            case 'completed':
            case 'job_completed':
              console.log(`Long text job ${jobId} completed, setting isJobActive to false`);
              newState.isJobActive = false;
              // Remove job from tracking since it's completed
              removeJobId(jobId);
              // Trigger download of completed audio
              downloadCompletedAudio(jobId);
              break;

            case 'error':
            case 'job_failed':
              console.log(`Long text job ${jobId} failed, setting isJobActive to false`);
              newState.isJobActive = false;
              newState.error = event.data?.error || event.data?.message || 'Job failed';
              // Remove job from tracking since it failed
              removeJobId(jobId);
              break;

            case 'job_paused':
              // Job is paused but still active (can be resumed)
              break;

            case 'job_resumed':
              newState.error = null; // Clear any previous errors
              break;

            case 'progress':
            case 'chunk_completed':
              // Progress updates are handled above
              break;
          }

          return newState;
        });
      },
      (error: Error) => {
        setState(prev => ({
          ...prev,
          error: `Connection error: ${error.message}`
        }));
      }
    );

    sseCleanupRef.current = cleanup;
  }, [service, cleanupSSE, removeJobId, downloadCompletedAudio]);

  // Get job status manually (fallback if SSE fails)
  const getJobStatus = useCallback(async (jobId: string) => {
    try {
      const jobResponse = await service.getJobStatus(jobId);
      setState(prev => ({
        ...prev,
        currentJob: jobResponse,
        progress: jobResponse.progress || null,
        isJobActive: ['pending', 'chunking', 'processing', 'paused'].includes(jobResponse.job.status),
        error: jobResponse.job.error || null
      }));

      // If job is completed and we don't have audio yet, download it
      if (jobResponse.job.status === 'completed' && !state.audioUrl) {
        downloadCompletedAudio(jobId);
      }

      return jobResponse;
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Unknown error';
      setState(prev => ({ ...prev, error: errorMessage }));
      throw error;
    }
  }, [service, state.audioUrl, downloadCompletedAudio]);

  // Submit a new long text TTS job
  const submitJob = useCallback((request: LongTextRequest) => {
    // Clear previous state
    setState(prev => ({
      ...prev,
      currentJob: null,
      progress: null,
      error: null,
      audioUrl: null
    }));

    return submitJobMutation.mutate(request);
  }, [submitJobMutation]);

  // Control job functions
  const pauseJob = useCallback((jobId: string) => {
    return pauseJobMutation.mutate(jobId);
  }, [pauseJobMutation]);

  const resumeJob = useCallback((jobId: string) => {
    return resumeJobMutation.mutate(jobId);
  }, [resumeJobMutation]);

  const cancelJob = useCallback((jobId: string) => {
    return cancelJobMutation.mutate(jobId);
  }, [cancelJobMutation]);

  // Utility functions
  const estimateProcessingTime = useCallback((textLength: number) => {
    return service.estimateProcessingTime(textLength);
  }, [service]);

  const shouldUseLongText = useCallback((text: string) => {
    return service.shouldUseLongText(text);
  }, [service]);

  const getStatusMessage = useCallback((status: LongTextJobStatus) => {
    return service.getStatusMessage(status);
  }, [service]);

  // Clean up on unmount
  useEffect(() => {
    return () => {
      cleanupSSE();
      // Clean up audio URL if it exists
      if (state.audioUrl) {
        URL.revokeObjectURL(state.audioUrl);
      }
    };
  }, [cleanupSSE, state.audioUrl]);

  // Resume monitoring active jobs on mount
  useEffect(() => {
    if (trackedJobIds.length > 0 && jobList?.jobs) {
      const activeJob = jobList.jobs.find(job =>
        trackedJobIds.includes(job.job_id) &&
        ['pending', 'chunking', 'processing', 'paused'].includes(job.status)
      );

      if (activeJob && !state.isJobActive) {
        console.log(`Resuming monitoring for active job ${activeJob.job_id}`);
        setState(prev => ({ ...prev, isJobActive: true }));
        startJobMonitoring(activeJob.job_id);
      } else if (!activeJob && state.isJobActive) {
        // No active jobs found but state thinks there's an active job - clear it
        console.log('No active jobs found, clearing isJobActive state');
        setState(prev => ({ ...prev, isJobActive: false }));
      }
    }
  }, [trackedJobIds, jobList, state.isJobActive, startJobMonitoring]);

  return {
    // State
    currentJob: state.currentJob,
    progress: state.progress,
    isJobActive: state.isJobActive,
    error: state.error,
    audioUrl: state.audioUrl,

    // Job list - filter to show only active jobs
    jobList: jobList?.jobs?.filter(job =>
      ['pending', 'chunking', 'processing', 'paused'].includes(job.status)
    ) || [],
    totalJobCount: jobList?.jobs?.filter(job =>
      ['pending', 'chunking', 'processing', 'paused'].includes(job.status)
    ).length || 0,
    isLoadingJobs,

    // Actions
    submitJob,
    pauseJob,
    resumeJob,
    cancelJob,
    getJobStatus,
    refetchJobs,

    // Utilities
    estimateProcessingTime,
    shouldUseLongText,
    getStatusMessage,

    // Loading states
    isSubmitting: submitJobMutation.isPending,
    isPausing: pauseJobMutation.isPending,
    isResuming: resumeJobMutation.isPending,
    isCancelling: cancelJobMutation.isPending,

    // Job ID management
    trackedJobIds,
    addJobId,
    removeJobId
  };
}