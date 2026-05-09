import { useState, useEffect, useCallback } from 'react';
import type { AudioRecord } from '../types';
import { extractAudioDuration } from '../lib/utils';

const STORAGE_KEY = 'chatterbox-audio-history';
const DB_NAME = 'chatterbox-audio';
const DB_VERSION = 1;
const STORE_NAME = 'audio-files';

// IndexedDB utilities for storing audio files
const openDB = (): Promise<IDBDatabase> => {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onerror = () => {
      console.error('Failed to open IndexedDB:', request.error);
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
        console.error('Failed to create object store:', error);
        reject(error);
      }
    };

    request.onblocked = () => {
      console.warn('IndexedDB connection blocked');
      reject(new Error('Database connection blocked'));
    };
  });
};

const storeAudioFile = async (id: string, blob: Blob): Promise<void> => {
  // Convert blob to array buffer first, outside of the transaction
  const arrayBuffer = await blob.arrayBuffer();

  const db = await openDB();
  const transaction = db.transaction([STORE_NAME], 'readwrite');
  const store = transaction.objectStore(STORE_NAME);

  await new Promise<void>((resolve, reject) => {
    const request = store.put({
      id,
      data: arrayBuffer,
      type: blob.type,
      size: blob.size
    });
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);

    // Also handle transaction errors
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
    console.error('Error retrieving audio file:', error);
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
    console.error('Error deleting audio file:', error);
  }
};

interface StoredAudioMetadata {
  id: string;
  name: string;
  createdAt: string;
  // duration?: number; // Duration in seconds
  duration?: number | null | undefined;
  settings: {
    text: string;
    exaggeration: number;
    cfgWeight: number;
    temperature: number;
    voiceId?: string;
    voiceName?: string;
  };
}

export function useAudioHistory() {
  const [audioHistory, setAudioHistory] = useState<AudioRecord[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  // Load audio history from storage on mount
  useEffect(() => {
    const loadAudioHistory = async () => {
      try {
        const stored = localStorage.getItem(STORAGE_KEY);
        if (stored) {
          const metadata: StoredAudioMetadata[] = JSON.parse(stored);

          const loadedRecords: AudioRecord[] = [];
          for (const meta of metadata) {
            const blob = await getAudioFile(meta.id);
            if (blob) {
              const audioUrl = URL.createObjectURL(blob);

              // Migration: extract duration for records that don't have it
              let duration = meta.duration;
              if (duration === undefined) {
                try {
                  duration = await extractAudioDuration(blob);
                } catch (error) {
                  console.warn('Failed to extract duration for record', meta.id, error);
                  duration = undefined;
                }
              }

              loadedRecords.push({
                id: meta.id,
                name: meta.name,
                audioUrl,
                blob,
                createdAt: new Date(meta.createdAt),
                duration,
                settings: meta.settings
              });
            }
          }

          setAudioHistory(loadedRecords);
        }
      } catch (error) {
        console.error('Error loading audio history:', error);
      } finally {
        setIsLoading(false);
      }
    };

    loadAudioHistory();

    // Cleanup URLs on unmount
    return () => {
      audioHistory.forEach(record => URL.revokeObjectURL(record.audioUrl));
    };
  }, []);

  // Save metadata to localStorage whenever audio history changes
  useEffect(() => {
    if (!isLoading) {
      const metadata: StoredAudioMetadata[] = audioHistory.map(record => ({
        id: record.id,
        name: record.name,
        createdAt: record.createdAt.toISOString(),
        duration: record.duration,
        settings: record.settings
      }));
      localStorage.setItem(STORAGE_KEY, JSON.stringify(metadata));
    }
  }, [audioHistory, isLoading]);

  const addAudioRecord = useCallback(async (
    blob: Blob,
    settings: {
      text: string;
      exaggeration: number;
      cfgWeight: number;
      temperature: number;
      voiceId?: string;
      voiceName?: string;
    },
    customName?: string
  ) => {
    const id = Date.now().toString() + Math.random().toString(36).substr(2, 9);
    const timestamp = new Date();
    const name = customName || `Speech ${timestamp.toLocaleDateString()} ${timestamp.toLocaleTimeString()}`;

    try {
      // Store blob in IndexedDB
      await storeAudioFile(id, blob);

      // Extract duration from the audio blob
      const duration = await extractAudioDuration(blob);

      // Create URL for immediate use
      const audioUrl = URL.createObjectURL(blob);

      const record: AudioRecord = {
        id,
        name,
        audioUrl,
        blob,
        createdAt: timestamp,
        duration: duration || undefined,
        settings
      };

      setAudioHistory(prev => [record, ...prev]); // Add to beginning for newest first
      return record;
    } catch (error) {
      console.error('Error adding audio record:', error);
      throw error;
    }
  }, []);

  const deleteAudioRecord = useCallback(async (recordId: string) => {
    const record = audioHistory.find(r => r.id === recordId);
    if (record) {
      // Revoke URL
      URL.revokeObjectURL(record.audioUrl);

      // Remove from IndexedDB
      await deleteAudioFile(recordId);

      // Remove from state
      setAudioHistory(prev => prev.filter(r => r.id !== recordId));
    }
  }, [audioHistory]);

  const renameAudioRecord = useCallback((recordId: string, newName: string) => {
    setAudioHistory(prev => prev.map(record =>
      record.id === recordId ? { ...record, name: newName } : record
    ));
  }, []);

  const clearHistory = useCallback(async () => {
    // Revoke all URLs
    audioHistory.forEach(record => URL.revokeObjectURL(record.audioUrl));

    // Clear IndexedDB
    try {
      const db = await openDB();
      const transaction = db.transaction([STORE_NAME], 'readwrite');
      const store = transaction.objectStore(STORE_NAME);
      await new Promise<void>((resolve, reject) => {
        const request = store.clear();
        request.onsuccess = () => resolve();
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('Error clearing audio history:', error);
    }

    // Clear state
    setAudioHistory([]);
  }, [audioHistory]);

  return {
    audioHistory,
    addAudioRecord,
    deleteAudioRecord,
    renameAudioRecord,
    clearHistory,
    isLoading
  };
} 