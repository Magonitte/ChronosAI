import { useState, useCallback, useEffect, useRef } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { createLongTextTTSService } from '../services/longTextTTS';
import type {
  LongTextHistoryItem,
  LongTextHistoryFilter,
  LongTextHistorySort,
  LongTextHistoryStats,
  LongTextJobDetails,
  LongTextJobUpdateRequest,
  LongTextJobRetryRequest,
  BulkJobActionRequest,
  BulkJobActionResponse
} from '../types';

const STORAGE_KEY = 'chatterbox-long-text-history-settings';
const DB_NAME = 'chatterbox-long-text-audio';
const DB_VERSION = 1;
const STORE_NAME = 'long-text-audio-files';

// IndexedDB utilities for storing long text audio files
const openDB = (): Promise<IDBDatabase> => {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onerror = () => {
      console.error('Failed to open Long Text IndexedDB:', request.error);
      reject(request.error);
    };

    request.onsuccess = () => resolve(request.result);

    request.onupgradeneeded = () => {
      const db = request.result;
      try {
        if (!db.objectStoreNames.contains(STORE_NAME)) {
          db.createObjectStore(STORE_NAME, { keyPath: 'id' });
        }
      } catch (error) {
        console.error('Failed to create long text object store:', error);
        reject(error);
      }
    };

    request.onblocked = () => {
      console.warn('Long Text IndexedDB connection blocked');
      reject(new Error('Database connection blocked'));
    };
  });
};

const storeAudioFile = async (id: string, blob: Blob): Promise<void> => {
  const arrayBuffer = await blob.arrayBuffer();
  const db = await openDB();
  const transaction = db.transaction([STORE_NAME], 'readwrite');
  const store = transaction.objectStore(STORE_NAME);

  await new Promise<void>((resolve, reject) => {
    const request = store.put({
      id,
      data: arrayBuffer,
      type: blob.type,
      size: blob.size,
      timestamp: Date.now()
    });
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);

    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(new Error('Transaction aborted'));
  });
};

const getAudioFile = async (id: string): Promise<Blob | null> => {
  try {
    const db = await openDB();
    const transaction = db.transaction([STORE_NAME], 'readonly');
    const store = transaction.objectStore(STORE_NAME);

    return new Promise((resolve, reject) => {
      const request = store.get(id);
      request.onsuccess = () => {
        const result = request.result;
        if (result) {
          const blob = new Blob([result.data], { type: result.type });
          resolve(blob);
        } else {
          resolve(null);
        }
      };
      request.onerror = () => reject(request.error);
    });
  } catch (error) {
    console.error('Error retrieving long text audio file:', error);
    return null;
  }
};

const deleteAudioFile = async (id: string): Promise<void> => {
  try {
    const db = await openDB();
    const transaction = db.transaction([STORE_NAME], 'readwrite');
    const store = transaction.objectStore(STORE_NAME);

    await new Promise<void>((resolve, reject) => {
      const request = store.delete(id);
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  } catch (error) {
    console.error('Error deleting long text audio file:', error);
  }
};

interface UseLongTextHistoryProps {
  apiBaseUrl: string;
  sessionId?: string;
}

interface HistorySettings {
  filters: LongTextHistoryFilter;
  sort: LongTextHistorySort;
  pageSize: number;
  showArchived: boolean;
}

const DEFAULT_SETTINGS: HistorySettings = {
  filters: {},
  sort: 'completed_desc',
  pageSize: 20,
  showArchived: false
};

export function useLongTextHistory({ apiBaseUrl, sessionId }: UseLongTextHistoryProps) {
  // Settings state with persistence
  const [settings, setSettings] = useState<HistorySettings>(() => {
    if (typeof window === 'undefined') return DEFAULT_SETTINGS;
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      return stored ? { ...DEFAULT_SETTINGS, ...JSON.parse(stored) } : DEFAULT_SETTINGS;
    } catch {
      return DEFAULT_SETTINGS;
    }
  });

  const [currentPage, setCurrentPage] = useState(0);
  const [selectedJobs, setSelectedJobs] = useState<string[]>([]);
  const [audioUrls, setAudioUrls] = useState<Map<string, string>>(new Map());

  const service = createLongTextTTSService(apiBaseUrl, sessionId);

  // Save settings to localStorage when they change
  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
    } catch (error) {
      console.error('Failed to save history settings:', error);
    }
  }, [settings]);

  // Query for history jobs
  const {
    data: historyData,
    isLoading: isLoadingHistory,
    error: historyError,
    refetch: refetchHistory
  } = useQuery({
    queryKey: ['longTextHistory', apiBaseUrl, sessionId, settings.filters, settings.sort, settings.showArchived, currentPage, settings.pageSize],
    queryFn: () => service.listHistoryJobs({
      filters: { ...settings.filters, is_archived: settings.showArchived ? undefined : false },
      sort: settings.sort,
      limit: settings.pageSize,
      offset: currentPage * settings.pageSize
    }),
    refetchInterval: 30000, // Refresh every 30 seconds
    retry: true
  });

  // Query for history statistics
  const {
    data: stats,
    isLoading: isLoadingStats,
    refetch: refetchStats
  } = useQuery({
    queryKey: ['longTextHistoryStats', apiBaseUrl, sessionId],
    queryFn: () => service.getHistoryStats(),
    refetchInterval: 60000, // Refresh every minute
    retry: true
  });

  // Mutations for job operations
  const updateJobMutation = useMutation({
    mutationFn: ({ jobId, updates }: { jobId: string; updates: LongTextJobUpdateRequest }) =>
      service.updateJob(jobId, updates),
    onSuccess: () => {
      refetchHistory();
      refetchStats();
    }
  });

  const retryJobMutation = useMutation({
    mutationFn: ({ jobId, options }: { jobId: string; options?: LongTextJobRetryRequest }) =>
      service.retryJob(jobId, options),
    onSuccess: () => {
      refetchHistory();
    }
  });

  const bulkActionMutation = useMutation({
    mutationFn: (request: BulkJobActionRequest) => service.bulkAction(request),
    onSuccess: () => {
      setSelectedJobs([]);
      refetchHistory();
      refetchStats();
    }
  });

  const clearHistoryMutation = useMutation({
    mutationFn: () => service.clearHistory(true),
    onSuccess: () => {
      setSelectedJobs([]);
      setCurrentPage(0);
      // Clear all cached audio URLs
      audioUrls.forEach(url => URL.revokeObjectURL(url));
      setAudioUrls(new Map());
      refetchHistory();
      refetchStats();
    }
  });

  // Update settings
  const updateSettings = useCallback((newSettings: Partial<HistorySettings>) => {
    setSettings(prev => ({ ...prev, ...newSettings }));
    setCurrentPage(0); // Reset to first page when settings change
  }, []);

  // Update filters
  const updateFilters = useCallback((newFilters: Partial<LongTextHistoryFilter>) => {
    setSettings(prev => ({ ...prev, filters: { ...prev.filters, ...newFilters } }));
    setCurrentPage(0);
  }, []);

  // Update sort
  const updateSort = useCallback((sort: LongTextHistorySort) => {
    updateSettings({ sort });
  }, [updateSettings]);

  // Job operations
  const updateJob = useCallback((jobId: string, updates: LongTextJobUpdateRequest) => {
    return updateJobMutation.mutateAsync({ jobId, updates });
  }, [updateJobMutation]);

  const retryJob = useCallback((jobId: string, options?: LongTextJobRetryRequest) => {
    return retryJobMutation.mutateAsync({ jobId, options });
  }, [retryJobMutation]);

  const archiveJob = useCallback((jobId: string) => {
    return updateJob(jobId, { is_archived: true });
  }, [updateJob]);

  const unarchiveJob = useCallback((jobId: string) => {
    return updateJob(jobId, { is_archived: false });
  }, [updateJob]);

  const deleteJob = useCallback((jobId: string) => {
    return bulkActionMutation.mutateAsync({
      action: 'delete',
      job_ids: [jobId],
      confirm: true
    });
  }, [bulkActionMutation]);

  // Bulk operations
  const bulkDelete = useCallback((jobIds: string[]) => {
    return bulkActionMutation.mutateAsync({
      action: 'delete',
      job_ids: jobIds,
      confirm: true
    });
  }, [bulkActionMutation]);

  const bulkArchive = useCallback((jobIds: string[]) => {
    return bulkActionMutation.mutateAsync({
      action: 'archive',
      job_ids: jobIds,
      confirm: true
    });
  }, [bulkActionMutation]);

  const bulkUnarchive = useCallback((jobIds: string[]) => {
    return bulkActionMutation.mutateAsync({
      action: 'unarchive',
      job_ids: jobIds,
      confirm: true
    });
  }, [bulkActionMutation]);

  const bulkRetry = useCallback((jobIds: string[], retryOptions?: LongTextJobRetryRequest) => {
    return bulkActionMutation.mutateAsync({
      action: 'retry',
      job_ids: jobIds,
      retry_options: retryOptions,
      confirm: true
    });
  }, [bulkActionMutation]);

  // Audio management
  const getAudioUrl = useCallback(async (jobId: string): Promise<string | null> => {
    // Check if we already have it cached
    const existingUrl = audioUrls.get(jobId);
    if (existingUrl) return existingUrl;

    try {
      // Try to get from IndexedDB first
      let blob = await getAudioFile(jobId);

      if (!blob) {
        // Download from server and cache it
        blob = await service.downloadJobAudio(jobId);
        await storeAudioFile(jobId, blob);
      }

      const url = URL.createObjectURL(blob);
      setAudioUrls(prev => new Map(prev).set(jobId, url));
      return url;
    } catch (error) {
      console.error('Failed to get audio URL:', error);
      return null;
    }
  }, [audioUrls, service]);

  const downloadAudio = useCallback(async (job: LongTextHistoryItem, filename?: string) => {
    try {
      const audioUrl = await getAudioUrl(job.job_id);
      if (!audioUrl) throw new Error('Failed to get audio URL');

      const a = document.createElement('a');
      a.href = audioUrl;
      a.download = filename || `${job.display_name || 'long-text-audio'}-${job.job_id}.${job.voice || 'audio'}`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    } catch (error) {
      console.error('Failed to download audio:', error);
      throw error;
    }
  }, [getAudioUrl]);

  // Selection management
  const toggleJobSelection = useCallback((jobId: string) => {
    setSelectedJobs(prev =>
      prev.includes(jobId)
        ? prev.filter(id => id !== jobId)
        : [...prev, jobId]
    );
  }, []);

  const selectAllJobs = useCallback(() => {
    const allJobIds = historyData?.jobs.map(job => job.job_id) || [];
    setSelectedJobs(allJobIds);
  }, [historyData]);

  const clearSelection = useCallback(() => {
    setSelectedJobs([]);
  }, []);

  // Pagination
  const goToPage = useCallback((page: number) => {
    setCurrentPage(Math.max(0, page));
  }, []);

  const nextPage = useCallback(() => {
    if (historyData && currentPage < Math.ceil(historyData.total_count / settings.pageSize) - 1) {
      setCurrentPage(prev => prev + 1);
    }
  }, [currentPage, historyData, settings.pageSize]);

  const prevPage = useCallback(() => {
    setCurrentPage(prev => Math.max(0, prev - 1));
  }, []);

  // Clear history
  const clearHistory = useCallback(() => {
    return clearHistoryMutation.mutateAsync();
  }, [clearHistoryMutation]);

  // Search
  const searchJobs = useCallback((searchText: string) => {
    updateFilters({ search_text: searchText });
  }, [updateFilters]);

  // Cleanup audio URLs on unmount
  useEffect(() => {
    return () => {
      audioUrls.forEach(url => URL.revokeObjectURL(url));
    };
  }, [audioUrls]);

  return {
    // Data
    jobs: historyData?.jobs || [],
    totalCount: historyData?.total_count || 0,
    stats,

    // Loading states
    isLoadingHistory,
    isLoadingStats,

    // Error states
    historyError,

    // Settings
    settings,
    updateSettings,
    updateFilters,
    updateSort,

    // Pagination
    currentPage,
    totalPages: Math.ceil((historyData?.total_count || 0) / settings.pageSize),
    goToPage,
    nextPage,
    prevPage,

    // Selection
    selectedJobs,
    toggleJobSelection,
    selectAllJobs,
    clearSelection,

    // Job operations
    updateJob,
    retryJob,
    archiveJob,
    unarchiveJob,
    deleteJob,

    // Bulk operations
    bulkDelete,
    bulkArchive,
    bulkUnarchive,
    bulkRetry,

    // Audio
    getAudioUrl,
    downloadAudio,

    // Other operations
    clearHistory,
    searchJobs,
    refetchHistory,
    refetchStats,

    // Loading states for mutations
    isUpdating: updateJobMutation.isPending,
    isRetrying: retryJobMutation.isPending,
    isBulkActing: bulkActionMutation.isPending,
    isClearing: clearHistoryMutation.isPending
  };
}