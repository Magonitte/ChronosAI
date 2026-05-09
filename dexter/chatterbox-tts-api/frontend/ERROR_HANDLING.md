# Error Handling Guide

This guide explains the error handling system implemented in the frontend application.

## Overview

The application includes a comprehensive error handling system that:
- Catches and displays React component errors with Error Boundaries
- Shows user-friendly error notifications via Toast system
- Handles global JavaScript errors and unhandled promise rejections
- Provides utilities for consistent API error handling

## Components

### 1. ErrorBoundary

A React Error Boundary component that catches errors in the component tree and displays a fallback UI.

**Location**: `src/components/ErrorBoundary.tsx`

**Features**:
- Catches React rendering errors
- Displays user-friendly error UI
- Shows detailed error information in development
- Provides "Try Again" and "Go Home" buttons
- Optional custom fallback UI

**Usage**:
```tsx
// Already integrated at app level in main.tsx
// For component-specific error boundaries:
import { ErrorBoundary } from '@/components/ErrorBoundary';

<ErrorBoundary>
  <YourComponent />
</ErrorBoundary>

// With custom fallback:
<ErrorBoundary fallback={<CustomErrorUI />}>
  <YourComponent />
</ErrorBoundary>
```

### 2. Toast Notification System

A toast notification system for displaying temporary messages to users.

**Location**: `src/components/ui/toast.tsx`

**Usage**:
```tsx
import { useToast, useToastHelpers } from '@/components/ui/toast';

function MyComponent() {
  const toast = useToastHelpers();

  const handleSuccess = () => {
    toast.success('Operation completed!', 'Your changes have been saved.');
  };

  const handleError = () => {
    toast.error('Operation failed', 'Please try again later.');
  };

  const handleWarning = () => {
    toast.warning('Warning', 'This action cannot be undone.');
  };

  const handleInfo = () => {
    toast.info('Info', 'New features are available.');
  };

  return <div>...</div>;
}
```

### 3. Global Error Handler

Catches uncaught JavaScript errors and unhandled promise rejections.

**Location**: `src/hooks/useGlobalErrorHandler.ts`

**Features**:
- Catches window.onerror events
- Catches unhandled promise rejections
- Displays errors as toast notifications
- Logs errors to console
- Optional integration with error reporting services

**Usage**:
Already integrated in `App.tsx` - no additional setup needed.

### 4. API Error Handling

Utilities for handling API errors consistently.

**Location**: `src/hooks/useGlobalErrorHandler.ts` and `src/lib/apiClient.ts`

**parseApiError**: Extracts meaningful error messages from API responses

```tsx
import { parseApiError } from '@/hooks/useGlobalErrorHandler';

try {
  await apiCall();
} catch (error) {
  const { title, description } = parseApiError(error);
  console.error(title, description);
}
```

**useApiErrorHandler**: Hook for displaying API errors as toasts

```tsx
import { useApiErrorHandler } from '@/hooks/useGlobalErrorHandler';

function MyComponent() {
  const handleApiError = useApiErrorHandler();

  const fetchData = async () => {
    try {
      await api.getData();
    } catch (error) {
      handleApiError(error); // Automatically shows toast
    }
  };

  return <div>...</div>;
}
```

**createApiClient**: Enhanced axios instance with interceptors

```tsx
import { createApiClient } from '@/lib/apiClient';

const apiClient = createApiClient('http://localhost:4123/v1');

// Use it like regular axios
const response = await apiClient.get('/endpoint');
```

### 5. Error Handling Utilities

General-purpose error handling utilities.

**Location**: `src/lib/errorHandling.ts`

**extractErrorDetails**: Extract structured error information

```tsx
import { extractErrorDetails } from '@/lib/errorHandling';

try {
  // some operation
} catch (error) {
  const { title, description, code, stack } = extractErrorDetails(error);
}
```

**withErrorHandling**: Wrapper for async functions

```tsx
import { withErrorHandling } from '@/lib/errorHandling';

const result = await withErrorHandling(
  async () => {
    return await someAsyncOperation();
  },
  (error) => {
    console.error('Custom error handler:', error);
  }
);
```

**safeExecute**: Safe execution without throwing

```tsx
import { safeExecute } from '@/lib/errorHandling';

const { data, error } = await safeExecute(async () => {
  return await riskyOperation();
});

if (error) {
  console.error('Operation failed:', error);
} else {
  console.log('Success:', data);
}
```

**withRetry**: Retry failed operations

```tsx
import { withRetry } from '@/lib/errorHandling';

const result = await withRetry(
  async () => await unstableApiCall(),
  {
    maxRetries: 3,
    delayMs: 1000,
    exponentialBackoff: true,
    onRetry: (attempt, error) => {
      console.log(`Retry attempt ${attempt}:`, error);
    }
  }
);
```

## Best Practices

### 1. Always Handle API Errors

```tsx
// ❌ Bad - Errors are not handled
const fetchData = async () => {
  const response = await api.getData();
  setData(response);
};

// ✅ Good - Errors are caught and displayed
const fetchData = async () => {
  try {
    const response = await api.getData();
    setData(response);
  } catch (error) {
    handleApiError(error);
  }
};
```

### 2. Use React Query Error Handling

```tsx
import { useQuery } from '@tanstack/react-query';
import { useApiErrorHandler } from '@/hooks/useGlobalErrorHandler';

function MyComponent() {
  const handleApiError = useApiErrorHandler();

  const { data, error, isError } = useQuery({
    queryKey: ['data'],
    queryFn: fetchData,
    onError: handleApiError, // Automatically show toast on error
  });

  if (isError) {
    return <div>Error loading data</div>;
  }

  return <div>{/* render data */}</div>;
}
```

### 3. Provide User Context

```tsx
// ❌ Generic error message
toast.error('Error', 'An error occurred');

// ✅ Specific, actionable error message
toast.error(
  'Failed to save changes',
  'Your internet connection may be unstable. Please check and try again.'
);
```

### 4. Log Errors for Debugging

```tsx
import { logError } from '@/lib/errorHandling';

try {
  await complexOperation();
} catch (error) {
  logError(error, 'ComplexOperation');
  handleApiError(error);
}
```

### 5. Handle Different Error Types

```tsx
import { isNetworkError, isTimeoutError } from '@/lib/errorHandling';

try {
  await apiCall();
} catch (error) {
  if (isNetworkError(error)) {
    toast.error('Network Error', 'Please check your internet connection');
  } else if (isTimeoutError(error)) {
    toast.error('Request Timeout', 'The server took too long to respond');
  } else {
    handleApiError(error);
  }
}
```

## Error Reporting Integration

To integrate with error reporting services (like Sentry, LogRocket, etc.):

```tsx
// In your error handling setup
window.errorReporter = {
  captureException: (error, context) => {
    // Your error reporting service
    // Sentry.captureException(error, context);
  }
};
```

The ErrorBoundary and global error handler will automatically use this if available.

## Testing Error Handling

### Test Error Boundary

```tsx
// Trigger a component error
function BuggyComponent() {
  throw new Error('Test error');
  return <div>This won't render</div>;
}

// Wrap in ErrorBoundary to see the error UI
<ErrorBoundary>
  <BuggyComponent />
</ErrorBoundary>
```

### Test Toast Notifications

```tsx
import { useToastHelpers } from '@/components/ui/toast';

function TestComponent() {
  const toast = useToastHelpers();

  return (
    <button onClick={() => toast.error('Test Error', 'This is a test')}>
      Test Error Toast
    </button>
  );
}
```

### Test Global Error Handler

```tsx
// Test uncaught error
setTimeout(() => {
  throw new Error('Uncaught error test');
}, 1000);

// Test unhandled promise rejection
Promise.reject('Unhandled promise rejection test');
```

## Troubleshooting

### Errors not being caught

1. Make sure ErrorBoundary is properly wrapped around components
2. Check that ToastProvider is in the component tree
3. Verify useGlobalErrorHandler is called in App component

### Toast notifications not appearing

1. Ensure ToastProvider is in the component tree
2. Check z-index conflicts with other elements
3. Verify toast duration settings

### Console logs but no user notification

1. Make sure you're calling `handleApiError(error)` or `toast.error()`
2. Check that the error is being caught in a try-catch block
3. Verify React Query's `onError` callback is set up

## File Structure

```
frontend/src/
├── components/
│   ├── ErrorBoundary.tsx          # React error boundary
│   └── ui/
│       └── toast.tsx              # Toast notification system
├── hooks/
│   └── useGlobalErrorHandler.ts   # Global error handling hooks
├── lib/
│   ├── apiClient.ts               # Enhanced axios client
│   └── errorHandling.ts           # Error handling utilities
└── ERROR_HANDLING.md              # This file
```
