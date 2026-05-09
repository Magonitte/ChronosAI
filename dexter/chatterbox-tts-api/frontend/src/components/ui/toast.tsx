import * as React from "react"
import { X, AlertCircle, CheckCircle, Info, AlertTriangle } from "lucide-react"
import { cn } from "@/lib/utils"

export type ToastType = "success" | "error" | "warning" | "info"

export interface Toast {
  id: string
  title?: string
  description?: string
  type?: ToastType
  duration?: number
}

interface ToastContextType {
  toasts: Toast[]
  addToast: (toast: Omit<Toast, "id">) => void
  removeToast: (id: string) => void
  clearToasts: () => void
}

const ToastContext = React.createContext<ToastContextType | undefined>(undefined)

export function useToast() {
  const context = React.useContext(ToastContext)
  if (!context) {
    throw new Error("useToast must be used within a ToastProvider")
  }
  return context
}

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = React.useState<Toast[]>([])

  const addToast = React.useCallback((toast: Omit<Toast, "id">) => {
    const id = Math.random().toString(36).substring(2, 9)
    const newToast: Toast = {
      id,
      duration: 5000,
      type: "info",
      ...toast,
    }

    setToasts((prev) => [...prev, newToast])

    // Auto remove after duration
    if (newToast.duration && newToast.duration > 0) {
      setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id))
      }, newToast.duration)
    }
  }, [])

  const removeToast = React.useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id))
  }, [])

  const clearToasts = React.useCallback(() => {
    setToasts([])
  }, [])

  return (
    <ToastContext.Provider value={{ toasts, addToast, removeToast, clearToasts }}>
      {children}
      <ToastViewport toasts={toasts} onRemove={removeToast} />
    </ToastContext.Provider>
  )
}

function ToastViewport({
  toasts,
  onRemove,
}: {
  toasts: Toast[]
  onRemove: (id: string) => void
}) {
  return (
    <div className="fixed bottom-0 right-0 z-[100] flex max-h-screen w-full flex-col-reverse gap-2 p-4 sm:bottom-0 sm:right-0 sm:top-auto sm:flex-col md:max-w-[420px]">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} onRemove={onRemove} />
      ))}
    </div>
  )
}

function ToastItem({
  toast,
  onRemove,
}: {
  toast: Toast
  onRemove: (id: string) => void
}) {
  const [isExiting, setIsExiting] = React.useState(false)

  const handleRemove = () => {
    setIsExiting(true)
    setTimeout(() => {
      onRemove(toast.id)
    }, 200) // Match animation duration
  }

  const getIcon = () => {
    switch (toast.type) {
      case "success":
        return <CheckCircle className="h-5 w-5 text-green-500" />
      case "error":
        return <AlertCircle className="h-5 w-5 text-red-500" />
      case "warning":
        return <AlertTriangle className="h-5 w-5 text-yellow-500" />
      case "info":
      default:
        return <Info className="h-5 w-5 text-blue-500" />
    }
  }

  const getBackgroundClass = () => {
    switch (toast.type) {
      case "success":
        return "border-green-500/20 bg-green-500/10"
      case "error":
        return "border-red-500/20 bg-red-500/10"
      case "warning":
        return "border-yellow-500/20 bg-yellow-500/10"
      case "info":
      default:
        return "border-blue-500/20 bg-blue-500/10"
    }
  }

  return (
    <div
      className={cn(
        "group pointer-events-auto relative flex w-full items-start gap-3 overflow-hidden rounded-lg border p-4 shadow-lg transition-all",
        "backdrop-blur-sm",
        getBackgroundClass(),
        isExiting
          ? "animate-out fade-out-0 slide-out-to-right-full"
          : "animate-in fade-in-0 slide-in-from-right-full"
      )}
      style={{
        animationDuration: "200ms",
      }}
    >
      <div className="flex-shrink-0">{getIcon()}</div>

      <div className="flex-1 space-y-1">
        {toast.title && (
          <div className="text-sm font-semibold leading-none tracking-tight">
            {toast.title}
          </div>
        )}
        {toast.description && (
          <div className="text-sm opacity-90">{toast.description}</div>
        )}
      </div>

      <button
        onClick={handleRemove}
        className="flex-shrink-0 rounded-md p-1 opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
      >
        <X className="h-4 w-4" />
        <span className="sr-only">Close</span>
      </button>
    </div>
  )
}

// Utility hook for common toast patterns
export function useToastHelpers() {
  const { addToast } = useToast()

  return React.useMemo(
    () => ({
      success: (title: string, description?: string) =>
        addToast({ title, description, type: "success" }),
      error: (title: string, description?: string) =>
        addToast({ title, description, type: "error", duration: 7000 }),
      warning: (title: string, description?: string) =>
        addToast({ title, description, type: "warning", duration: 6000 }),
      info: (title: string, description?: string) =>
        addToast({ title, description, type: "info" }),
    }),
    [addToast]
  )
}
