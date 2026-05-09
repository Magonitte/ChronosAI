import { useState, useEffect, useCallback } from 'react';

const STORAGE_KEY = 'chatterbox-text-input';

export function useTextInput() {
  const [text, setText] = useState(() => {
    if (typeof window === 'undefined') return '';

    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      return stored || '';
    } catch (error) {
      console.error('Error loading text input:', error);
      return '';
    }
  });

  // Save to localStorage whenever text changes
  useEffect(() => {
    try {
      if (text) {
        localStorage.setItem(STORAGE_KEY, text);
      } else {
        localStorage.removeItem(STORAGE_KEY);
      }
    } catch (error) {
      console.error('Error saving text input:', error);
    }
  }, [text]);

  const updateText = useCallback((newText: string) => {
    setText(newText);
  }, []);

  const clearText = useCallback(() => {
    setText('');
  }, []);

  const hasText = text.trim().length > 0;

  return {
    text,
    updateText,
    clearText,
    hasText
  };
} 