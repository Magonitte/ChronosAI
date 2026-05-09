import React, { useState, useEffect } from 'react';
import { Loader2, Clock, Cpu, Zap, Minimize2, Maximize2, X } from 'lucide-react';
import type { TTSProgress } from '../types';

interface StatusProgressOverlayProps {
  progress: TTSProgress | undefined;
  isVisible: boolean;
  onDismiss?: () => void;
  isLongText?: boolean;
}

export default function StatusProgressOverlay({
  progress,
  isVisible,
  onDismiss,
  isLongText = false
}: StatusProgressOverlayProps) {
  const [isMinimized, setIsMinimized] = useState(false);

  // Load minimized state from localStorage and auto-minimize for long text
  useEffect(() => {
    const savedMinimized = localStorage.getItem('tts-progress-minimized');
    if (savedMinimized === 'true') {
      setIsMinimized(true);
    } else if (isLongText) {
      // Auto-minimize for long text requests by default
      setIsMinimized(true);
    }
  }, [isLongText]);

  // Save minimized state to localStorage
  const handleMinimize = () => {
    setIsMinimized(true);
    localStorage.setItem('tts-progress-minimized', 'true');
  };

  const handleMaximize = () => {
    setIsMinimized(false);
    localStorage.setItem('tts-progress-minimized', 'false');
  };

  const handleDismiss = () => {
    if (onDismiss) {
      onDismiss();
    }
  };

  if (!isVisible || !progress?.is_processing) {
    return null;
  }

  const progressPercentage = progress.progress_percentage ?? 0;
  const currentChunk = progress.current_chunk ?? 0;
  const totalChunks = progress.total_chunks ?? 0;
  const estimatedCompletion = progress.estimated_completion;
  const timeRemaining = estimatedCompletion ? Math.max(0, estimatedCompletion - Date.now() / 1000) : null;

  const formatTimeRemaining = (seconds: number) => {
    if (seconds < 60) {
      return `${Math.ceil(seconds)}s`;
    }
    return `${Math.ceil(seconds / 60)}m ${Math.ceil(seconds % 60)}s`;
  };

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'initializing':
        return 'text-blue-600 dark:text-blue-400';
      case 'processing_text':
        return 'text-yellow-600 dark:text-yellow-400';
      case 'chunking':
        return 'text-orange-600 dark:text-orange-400';
      case 'generating_audio':
        return 'text-green-600 dark:text-green-400';
      case 'concatenating':
        return 'text-purple-600 dark:text-purple-400';
      case 'finalizing':
        return 'text-indigo-600 dark:text-indigo-400';
      default:
        return 'text-primary';
    }
  };

  const getStatusIcon = (status: string) => {
    switch (status) {
      case 'generating_audio':
        return <Zap className="w-5 h-5" />;
      case 'processing_text':
      case 'chunking':
        return <Cpu className="w-5 h-5" />;
      default:
        return <Loader2 className="w-5 h-5 animate-spin" />;
    }
  };

  // Minimized floating indicator
  if (isMinimized) {
    return (
      <div className="fixed bottom-4 right-4 z-50">
        <button
          onClick={handleMaximize}
          className="bg-card border border-border rounded-lg p-3 shadow-2xl hover:shadow-3xl transition-all duration-300 hover:scale-105 group"
        >
          <div className="flex items-center gap-3">
            <div className={getStatusColor(progress.status)}>
              {getStatusIcon(progress.status)}
            </div>
            <div className="text-left">
              <div className="text-sm font-medium text-foreground">
                Generating Speech
              </div>
              <div className="text-xs text-muted-foreground">
                {progressPercentage.toFixed(0)}% • Click to expand
              </div>
            </div>
            <div className="w-12 h-2 bg-muted rounded-full overflow-hidden">
              <div
                className="h-full bg-primary transition-all duration-300"
                style={{ width: `${Math.min(100, Math.max(0, progressPercentage))}%` }}
              />
            </div>
          </div>
        </button>
      </div>
    );
  }

  // Full modal
  return (
    <div
      className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50"
      onClick={handleMinimize}
    >
      <div
        className="bg-card border border-border rounded-lg p-6 max-w-md w-full mx-4 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header with controls */}
        <div className="flex items-center gap-3 mb-4">
          <div className={getStatusColor(progress.status)}>
            {getStatusIcon(progress.status)}
          </div>
          <div className="flex-1">
            <h3 className="font-semibold text-foreground">Generating Speech</h3>
            <p className="text-sm text-muted-foreground capitalize">
              {progress.status.replace('_', ' ')}
            </p>
          </div>
          <div className="flex items-center gap-1">
            <button
              onClick={handleMinimize}
              className="p-1 hover:bg-accent rounded transition-colors duration-200"
              title="Minimize to corner"
            >
              <Minimize2 className="w-4 h-4 text-muted-foreground" />
            </button>
            {onDismiss && (
              <button
                onClick={handleDismiss}
                className="p-1 hover:bg-destructive/10 text-destructive rounded transition-colors duration-200"
                title="Hide until next request"
              >
                <X className="w-4 h-4" />
              </button>
            )}
          </div>
        </div>

        {/* Progress Bar */}
        <div className="mb-4">
          <div className="flex justify-between items-center mb-2">
            <span className="text-sm font-medium text-foreground">
              Progress: {progressPercentage.toFixed(1)}%
            </span>
            {totalChunks > 0 && (
              <span className="text-sm text-muted-foreground">
                {currentChunk}/{totalChunks} chunks
              </span>
            )}
          </div>
          <div className="w-full bg-muted rounded-full h-2">
            <div
              className="bg-primary h-2 rounded-full transition-all duration-300 ease-out"
              style={{ width: `${Math.min(100, Math.max(0, progressPercentage))}%` }}
            />
          </div>
        </div>

        {/* Current Step */}
        {progress.current_step && (
          <div className="mb-4">
            <p className="text-sm text-muted-foreground mb-1">Current Step:</p>
            <p className="text-sm font-medium text-foreground">{progress.current_step}</p>
          </div>
        )}

        {/* Text Preview */}
        {progress.text_preview && (
          <div className="mb-4">
            <p className="text-sm text-muted-foreground mb-1">Processing:</p>
            <p className="text-sm text-foreground bg-muted rounded p-2 line-clamp-2">
              {progress.text_preview}
            </p>
          </div>
        )}

        {/* Time Information */}
        <div className="flex items-center justify-between text-sm text-muted-foreground">
          <div className="flex items-center gap-1">
            <Clock className="w-4 h-4" />
            <span>Duration: {(progress.duration_seconds ?? 0).toFixed(1)}s</span>
          </div>
          {timeRemaining && timeRemaining > 0 && (
            <div className="flex items-center gap-1">
              <span>ETA: {formatTimeRemaining(timeRemaining)}</span>
            </div>
          )}
        </div>
      </div>
    </div>
  );
} 