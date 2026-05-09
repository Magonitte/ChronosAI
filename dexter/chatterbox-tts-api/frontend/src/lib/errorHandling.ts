/**
 * Error handling utilities for consistent error management across the app
 */

export interface ErrorDetails {
  title: string;
  description: string;
  code?: string | number;
  stack?: string;
}

/**
 * Extracts error details from various error types
 */
export function extractErrorDetails(error: unknown): ErrorDetails {
  // Handle Error objects
  if (error instanceof Error) {
    return {
      title: error.name || 'Error',
      description: error.message,
      stack: error.stack,
    };
  }

  // Handle axios/fetch response errors
  if (typeof error === 'object' && error !== null) {
    const err = error as any;

    // Axios error format
    if (err.response) {
      const status = err.response.status;
      const data = err.response.data;

      return {
        title: `HTTP ${status}`,
        description: data?.message || data?.error || data?.detail || 'Request failed',
        code: status,
      };
    }

    // Fetch error or network error
    if (err.message) {
      return {
        title: 'Request Failed',
        description: err.message,
      };
    }
  }

  // Handle string errors
  if (typeof error === 'string') {
    return {
      title: 'Error',
      description: error,
    };
  }

  // Fallback for unknown error types
  return {
    title: 'Unknown Error',
    description: 'An unexpected error occurred',
  };
}

/**
 * Wrapper for async functions that provides consistent error handling
 * @param fn - Async function to execute
 * @param errorHandler - Optional custom error handler
 * @returns Promise with result or throws formatted error
 */
export async function withErrorHandling<T>(
  fn: () => Promise<T>,
  errorHandler?: (error: ErrorDetails) => void
): Promise<T> {
  try {
    return await fn();
  } catch (error) {
    const errorDetails = extractErrorDetails(error);

    if (errorHandler) {
      errorHandler(errorDetails);
    }

    // Re-throw with formatted error
    const formattedError = new Error(errorDetails.description);
    formattedError.name = errorDetails.title;
    throw formattedError;
  }
}

/**
 * Safe execution wrapper that catches errors and returns them instead of throwing
 * Useful for operations where you want to handle success/error without try-catch
 */
export async function safeExecute<T>(
  fn: () => Promise<T>
): Promise<{ data?: T; error?: ErrorDetails }> {
  try {
    const data = await fn();
    return { data };
  } catch (error) {
    return { error: extractErrorDetails(error) };
  }
}

/**
 * Retry wrapper for operations that might fail temporarily
 */
export async function withRetry<T>(
  fn: () => Promise<T>,
  options: {
    maxRetries?: number;
    delayMs?: number;
    exponentialBackoff?: boolean;
    onRetry?: (attempt: number, error: ErrorDetails) => void;
  } = {}
): Promise<T> {
  const {
    maxRetries = 3,
    delayMs = 1000,
    exponentialBackoff = true,
    onRetry,
  } = options;

  let lastError: ErrorDetails | undefined;

  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      return await fn();
    } catch (error) {
      lastError = extractErrorDetails(error);

      // Don't retry on the last attempt
      if (attempt === maxRetries - 1) {
        break;
      }

      // Call retry callback if provided
      if (onRetry) {
        onRetry(attempt + 1, lastError);
      }

      // Calculate delay with optional exponential backoff
      const delay = exponentialBackoff
        ? delayMs * Math.pow(2, attempt)
        : delayMs;

      // Wait before retrying
      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }

  // If we got here, all retries failed
  const error = new Error(lastError?.description || 'Operation failed after retries');
  error.name = lastError?.title || 'Retry Failed';
  throw error;
}

/**
 * Debounced error handler to prevent error spam
 */
export function createDebouncedErrorHandler(
  handler: (error: ErrorDetails) => void,
  delayMs: number = 1000
): (error: unknown) => void {
  let timeoutId: NodeJS.Timeout | null = null;
  let lastError: ErrorDetails | null = null;

  return (error: unknown) => {
    const errorDetails = extractErrorDetails(error);
    lastError = errorDetails;

    if (timeoutId) {
      clearTimeout(timeoutId);
    }

    timeoutId = setTimeout(() => {
      if (lastError) {
        handler(lastError);
        lastError = null;
      }
    }, delayMs);
  };
}

/**
 * Check if an error is a network error
 */
export function isNetworkError(error: unknown): boolean {
  if (error instanceof Error) {
    return (
      error.message.includes('network') ||
      error.message.includes('Network') ||
      error.message.includes('fetch') ||
      error.name === 'NetworkError'
    );
  }

  if (typeof error === 'object' && error !== null) {
    const err = error as any;
    return err.request && !err.response;
  }

  return false;
}

/**
 * Check if an error is a timeout error
 */
export function isTimeoutError(error: unknown): boolean {
  if (error instanceof Error) {
    return (
      error.message.includes('timeout') ||
      error.message.includes('timed out') ||
      error.name === 'TimeoutError'
    );
  }

  if (typeof error === 'object' && error !== null) {
    const err = error as any;
    return err.code === 'ECONNABORTED' || err.response?.status === 408;
  }

  return false;
}

/**
 * Log error to console with consistent formatting
 */
export function logError(error: unknown, context?: string): void {
  const errorDetails = extractErrorDetails(error);
  const prefix = context ? `[${context}]` : '[Error]';

  console.group(`${prefix} ${errorDetails.title}`);
  console.error('Description:', errorDetails.description);
  if (errorDetails.code) {
    console.error('Code:', errorDetails.code);
  }
  if (errorDetails.stack) {
    console.error('Stack:', errorDetails.stack);
  }
  console.groupEnd();
}
