import axios, { AxiosInstance, AxiosError } from 'axios';
import { extractErrorDetails } from './errorHandling';

/**
 * Create an enhanced axios instance with error handling and interceptors
 */
export function createApiClient(baseURL: string): AxiosInstance {
  const client = axios.create({
    baseURL,
    timeout: 30000, // 30 seconds default timeout
    headers: {
      'Content-Type': 'application/json',
    },
  });

  // Request interceptor for adding common headers or tokens
  client.interceptors.request.use(
    (config) => {
      // Add any auth tokens or common headers here
      // Example: config.headers.Authorization = `Bearer ${getToken()}`;
      return config;
    },
    (error) => {
      console.error('Request interceptor error:', error);
      return Promise.reject(error);
    }
  );

  // Response interceptor for consistent error handling
  client.interceptors.response.use(
    (response) => {
      // Transform response if needed
      return response;
    },
    (error: AxiosError) => {
      // Log the error with details
      const errorDetails = extractErrorDetails(error);
      console.error('API Error:', {
        title: errorDetails.title,
        description: errorDetails.description,
        code: errorDetails.code,
        url: error.config?.url,
        method: error.config?.method,
      });

      // You can add custom error handling logic here
      // For example, redirect to login on 401, show maintenance page on 503, etc.

      return Promise.reject(error);
    }
  );

  return client;
}

/**
 * Create a safe API call wrapper that returns result or error without throwing
 */
export async function safeApiCall<T>(
  apiCall: () => Promise<T>
): Promise<{ data?: T; error?: string }> {
  try {
    const data = await apiCall();
    return { data };
  } catch (error) {
    const errorDetails = extractErrorDetails(error);
    return { error: errorDetails.description };
  }
}

/**
 * Helper to check if an error is a specific HTTP status code
 */
export function isHttpError(error: unknown, status: number): boolean {
  if (axios.isAxiosError(error)) {
    return error.response?.status === status;
  }
  return false;
}

/**
 * Helper to check if error is a network error
 */
export function isNetworkError(error: unknown): boolean {
  if (axios.isAxiosError(error)) {
    return !error.response && Boolean(error.request);
  }
  return false;
}

/**
 * Helper to check if error is a timeout
 */
export function isTimeoutError(error: unknown): boolean {
  if (axios.isAxiosError(error)) {
    return error.code === 'ECONNABORTED' || error.message.includes('timeout');
  }
  return false;
}

/**
 * Extract user-friendly error message from API error
 */
export function getErrorMessage(error: unknown, fallback: string = 'An error occurred'): string {
  const errorDetails = extractErrorDetails(error);
  return errorDetails.description || fallback;
}
