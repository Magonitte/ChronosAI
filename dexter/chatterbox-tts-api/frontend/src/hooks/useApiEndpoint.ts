import { useState, useEffect } from 'react';

// Detect if we're running in Docker fullstack mode (behind proxy)
// This is determined by checking if we're on a different port than the API default
const isDockerFullstack = () => {
  if (typeof window === 'undefined') return false;
  const currentPort = window.location.port;
  const currentHost = window.location.hostname;

  // If we're on port 4321 (default FRONTEND_PORT), we're behind the proxy
  // If we're on localhost/127.0.0.1, we're likely in local development
  return currentPort === '4321' ||
    (currentPort !== '4123' && currentPort !== '3000' && currentHost !== 'localhost' && currentHost !== '127.0.0.1');
};

// Default API endpoints based on environment
const getDefaultApiBase = () => {
  if (typeof window === 'undefined') return 'http://localhost:4123/v1';

  if (isDockerFullstack()) {
    // When running behind proxy, API calls go through the same origin
    return `${window.location.protocol}//${window.location.host}/v1`;
  }

  // Local development or direct API access
  return 'http://localhost:4123/v1';
};

const STORAGE_KEY = 'chatterbox-api-endpoint';

export function useApiEndpoint() {
  const [apiBaseUrl, setApiBaseUrl] = useState(() => {
    if (typeof window === 'undefined') return getDefaultApiBase();
    return localStorage.getItem(STORAGE_KEY) || getDefaultApiBase();
  });

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, apiBaseUrl);
  }, [apiBaseUrl]);

  const updateApiBaseUrl = (url: string) => {
    // Ensure URL doesn't end with slash for consistency
    const cleanUrl = url.replace(/\/$/, '');
    setApiBaseUrl(cleanUrl);
  };

  const defaultApiBase = getDefaultApiBase();

  return {
    apiBaseUrl,
    updateApiBaseUrl,
    isDefault: apiBaseUrl === defaultApiBase,
    defaultApiBase // Export for debugging/display
  };
} 