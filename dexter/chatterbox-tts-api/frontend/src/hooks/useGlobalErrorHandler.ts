import { useEffect } from 'react';
import { useToast } from '@/components/ui/toast';

/**
 * Global error handler hook that catches unhandled errors and promise rejections
 * Displays them as toast notifications to the user
 */
export function useGlobalErrorHandler() {
  const { addToast } = useToast();

  useEffect(() => {
    // Handler for uncaught JavaScript errors
    const handleError = (event: ErrorEvent) => {
      console.error('Uncaught error:', event.error);

      // Prevent the default browser error handling
      event.preventDefault();

      // Show toast notification
      addToast({
        type: 'error',
        title: 'Unexpected Error',
        description: event.error?.message || 'An unexpected error occurred',
        duration: 7000,
      });

      // Optional: Send to error reporting service
      if (typeof window !== 'undefined' && (window as any).errorReporter) {
        (window as any).errorReporter.captureException(event.error);
      }
    };

    // Handler for unhandled promise rejections
    const handleUnhandledRejection = (event: PromiseRejectionEvent) => {
      console.error('Unhandled promise rejection:', event.reason);

      // Prevent the default browser error handling
      event.preventDefault();

      // Extract error message
      let errorMessage = 'An unexpected error occurred';
      if (event.reason) {
        if (event.reason instanceof Error) {
          errorMessage = event.reason.message;
        } else if (typeof event.reason === 'string') {
          errorMessage = event.reason;
        } else if (event.reason.message) {
          errorMessage = event.reason.message;
        }
      }

      // Show toast notification
      addToast({
        type: 'error',
        title: 'Promise Rejection',
        description: errorMessage,
        duration: 7000,
      });

      // Optional: Send to error reporting service
      if (typeof window !== 'undefined' && (window as any).errorReporter) {
        (window as any).errorReporter.captureException(event.reason);
      }
    };

    // Add event listeners
    window.addEventListener('error', handleError);
    window.addEventListener('unhandledrejection', handleUnhandledRejection);

    // Cleanup
    return () => {
      window.removeEventListener('error', handleError);
      window.removeEventListener('unhandledrejection', handleUnhandledRejection);
    };
  }, [addToast]);
}

/**
 * API error parser that extracts meaningful error messages from API responses
 */
export function parseApiError(error: any): { title: string; description: string } {
  // Handle axios errors
  if (error.response) {
    const status = error.response.status;
    const data = error.response.data;

    let title = `Error ${status}`;
    let description = 'An error occurred while communicating with the server';

    // Extract error message from various response formats
    if (data) {
      if (typeof data === 'string') {
        description = data;
      } else if (data.message) {
        description = data.message;
      } else if (data.error) {
        if (typeof data.error === 'string') {
          description = data.error;
        } else if (data.error.message) {
          description = data.error.message;
        }
      } else if (data.detail) {
        // FastAPI format
        if (typeof data.detail === 'string') {
          description = data.detail;
        } else if (Array.isArray(data.detail)) {
          description = data.detail.map((d: any) => d.msg || d).join(', ');
        }
      }
    }

    // Provide user-friendly messages for common status codes
    switch (status) {
      case 400:
        title = 'Bad Request';
        break;
      case 401:
        title = 'Unauthorized';
        description = 'You are not authorized to perform this action';
        break;
      case 403:
        title = 'Forbidden';
        description = 'You do not have permission to access this resource';
        break;
      case 404:
        title = 'Not Found';
        description = 'The requested resource was not found';
        break;
      case 408:
        title = 'Request Timeout';
        description = 'The request took too long to complete';
        break;
      case 429:
        title = 'Too Many Requests';
        description = 'You have made too many requests. Please try again later';
        break;
      case 500:
        title = 'Server Error';
        description = 'An internal server error occurred';
        break;
      case 502:
        title = 'Bad Gateway';
        description = 'The server received an invalid response';
        break;
      case 503:
        title = 'Service Unavailable';
        description = 'The service is temporarily unavailable';
        break;
      case 504:
        title = 'Gateway Timeout';
        description = 'The server did not respond in time';
        break;
    }

    return { title, description };
  }

  // Handle network errors
  if (error.request) {
    return {
      title: 'Network Error',
      description: 'Could not connect to the server. Please check your internet connection.',
    };
  }

  // Handle other errors
  if (error.message) {
    return {
      title: 'Error',
      description: error.message,
    };
  }

  return {
    title: 'Unknown Error',
    description: 'An unexpected error occurred',
  };
}

/**
 * Hook for handling API errors with toast notifications
 */
export function useApiErrorHandler() {
  const { addToast } = useToast();

  return (error: any) => {
    const { title, description } = parseApiError(error);
    addToast({
      type: 'error',
      title,
      description,
      duration: 7000,
    });
  };
}
