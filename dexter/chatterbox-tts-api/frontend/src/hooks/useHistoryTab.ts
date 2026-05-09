import { useState, useEffect, useCallback } from 'react';

const STORAGE_KEY = 'chatterbox-history-tab';

export type HistoryTab = 'regular' | 'longtext';

export function useHistoryTab() {
  const [historyTab, setHistoryTab] = useState<HistoryTab>(() => {
    if (typeof window === 'undefined') return 'regular';

    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      return (stored === 'longtext' ? 'longtext' : 'regular') as HistoryTab;
    } catch (error) {
      console.error('Error loading history tab preference:', error);
      return 'regular';
    }
  });

  // Save to localStorage whenever tab selection changes
  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, historyTab);
    } catch (error) {
      console.error('Error saving history tab preference:', error);
    }
  }, [historyTab]);

  const updateHistoryTab = useCallback((newTab: HistoryTab) => {
    setHistoryTab(newTab);
  }, []);

  return {
    historyTab,
    updateHistoryTab
  };
}