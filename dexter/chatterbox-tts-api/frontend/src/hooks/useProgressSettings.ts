import { useState, useEffect, useRef } from 'react';

interface ProgressSettings {
  onlyShowMyRequests: boolean;
  isDismissed: boolean;
}

const DEFAULT_SETTINGS: ProgressSettings = {
  onlyShowMyRequests: false,
  isDismissed: false,
};

// Generate a unique session ID for this frontend instance
const generateSessionId = () => {
  return `frontend-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
};

export function useProgressSettings() {
  const [settings, setSettings] = useState<ProgressSettings>(DEFAULT_SETTINGS);
  const sessionId = useRef<string>(generateSessionId());
  const [currentRequestId, setCurrentRequestId] = useState<string | null>(null);

  // Load settings from localStorage
  useEffect(() => {
    try {
      const savedSettings = localStorage.getItem('tts-progress-settings');
      if (savedSettings) {
        const parsed = JSON.parse(savedSettings);
        setSettings(prev => ({ ...prev, ...parsed }));
      }
    } catch (error) {
      console.warn('Failed to load progress settings:', error);
    }
  }, []);

  // Save settings to localStorage
  const updateSettings = (updates: Partial<ProgressSettings>) => {
    const newSettings = { ...settings, ...updates };
    setSettings(newSettings);

    try {
      localStorage.setItem('tts-progress-settings', JSON.stringify(newSettings));
    } catch (error) {
      console.warn('Failed to save progress settings:', error);
    }
  };

  // Track when a request is made from this frontend
  const trackRequest = (requestId?: string, requestType: 'regular' | 'long-text' = 'regular') => {
    if (requestId) {
      setCurrentRequestId(requestId);
      // Store the request ID with our session ID and type for tracking
      try {
        const myRequests = JSON.parse(localStorage.getItem('tts-my-requests') || '{}');
        myRequests[requestId] = {
          sessionId: sessionId.current,
          timestamp: Date.now(),
          requestType,
        };
        localStorage.setItem('tts-my-requests', JSON.stringify(myRequests));
      } catch (error) {
        console.warn('Failed to track request:', error);
      }
    }
  };

  // Check if a request was made from this frontend
  const isMyRequest = (requestId?: string): boolean => {
    if (!requestId || !settings.onlyShowMyRequests) {
      return true; // Show all requests if setting is disabled
    }

    try {
      const myRequests = JSON.parse(localStorage.getItem('tts-my-requests') || '{}');
      const requestInfo = myRequests[requestId];
      return requestInfo && requestInfo.sessionId === sessionId.current;
    } catch (error) {
      console.warn('Failed to check request ownership:', error);
      return false;
    }
  };

  // Check if a request is for long text processing
  const isLongTextRequest = (requestId?: string): boolean => {
    if (!requestId) {
      return false;
    }

    try {
      const myRequests = JSON.parse(localStorage.getItem('tts-my-requests') || '{}');
      const requestInfo = myRequests[requestId];
      return requestInfo && requestInfo.requestType === 'long-text';
    } catch (error) {
      console.warn('Failed to check request type:', error);
      return false;
    }
  };

  // Clean up old request tracking data (older than 24 hours)
  const cleanupOldRequests = () => {
    try {
      const myRequests = JSON.parse(localStorage.getItem('tts-my-requests') || '{}');
      const cutoffTime = Date.now() - (24 * 60 * 60 * 1000); // 24 hours ago

      const cleanedRequests: any = {};
      Object.entries(myRequests).forEach(([requestId, info]: [string, any]) => {
        if (info.timestamp > cutoffTime) {
          cleanedRequests[requestId] = info;
        }
      });

      localStorage.setItem('tts-my-requests', JSON.stringify(cleanedRequests));
    } catch (error) {
      console.warn('Failed to cleanup old requests:', error);
    }
  };

  // Run cleanup on mount and periodically
  useEffect(() => {
    cleanupOldRequests();
    const cleanup = setInterval(cleanupOldRequests, 60 * 60 * 1000); // Every hour
    return () => clearInterval(cleanup);
  }, []);

  // Dismiss progress overlay temporarily
  const dismissProgress = () => {
    updateSettings({ isDismissed: true });

    // Auto-reset dismissed state after a reasonable time
    setTimeout(() => {
      updateSettings({ isDismissed: false });
    }, 5 * 60 * 1000); // 5 minutes
  };

  // Determine if progress should be shown
  const shouldShowProgress = (requestId?: string): boolean => {
    if (settings.isDismissed) {
      return false;
    }

    return isMyRequest(requestId);
  };

  return {
    settings,
    updateSettings,
    trackRequest,
    isMyRequest,
    isLongTextRequest,
    shouldShowProgress,
    dismissProgress,
    sessionId: sessionId.current,
    currentRequestId,
  };
} 