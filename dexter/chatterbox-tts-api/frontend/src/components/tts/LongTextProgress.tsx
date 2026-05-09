import React, { useEffect, useState } from 'react';
import { Clock, Pause, Play, X, Download, FileText, Volume2, AlertCircle } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '../ui/card';
import { Button } from '../ui/button';
import type { LongTextJobResponse, LongTextProgress, LongTextJobStatus } from '../../types';

interface LongTextProgressProps {
  job: LongTextJobResponse | null;
  progress: LongTextProgress | null;
  isJobActive: boolean;
  audioUrl: string | null;
  error: string | null;
  onPause?: (jobId: string) => void;
  onResume?: (jobId: string) => void;
  onCancel?: (jobId: string) => void;
  onDownload?: (audioUrl: string) => void;
  isPausing?: boolean;
  isResuming?: boolean;
  isCancelling?: boolean;
}

export default function LongTextProgress({
  job,
  progress,
  isJobActive,
  audioUrl,
  error,
  onPause,
  onResume,
  onCancel,
  onDownload,
  isPausing = false,
  isResuming = false,
  isCancelling = false
}: LongTextProgressProps) {
  const [elapsedTime, setElapsedTime] = useState(0);

  // Calculate elapsed time if job is active
  useEffect(() => {
    if (!job || !isJobActive) return;

    const startTime = job.job.processing.started_at ? new Date(job.job.processing.started_at).getTime() : Date.now();

    const interval = setInterval(() => {
      const now = Date.now();
      const elapsed = Math.floor((now - startTime) / 1000);
      setElapsedTime(elapsed);
    }, 1000);

    return () => clearInterval(interval);
  }, [job, isJobActive]);

  if (!job && !error) return null;

  const formatTime = (seconds: number): string => {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;

    if (hours > 0) {
      return `${hours}h ${minutes}m ${secs}s`;
    } else if (minutes > 0) {
      return `${minutes}m ${secs}s`;
    } else {
      return `${secs}s`;
    }
  };

  const getStatusColor = (status: LongTextJobStatus): string => {
    switch (status) {
      case 'pending':
        return 'text-yellow-600 bg-yellow-50 border-yellow-200 dark:bg-yellow-950/30 dark:border-yellow-800';
      case 'chunking':
        return 'text-blue-600 bg-blue-50 border-blue-200 dark:bg-blue-950/30 dark:border-blue-800';
      case 'processing':
        return 'text-blue-600 bg-blue-50 border-blue-200 dark:bg-blue-950/30 dark:border-blue-800';
      case 'paused':
        return 'text-orange-600 bg-orange-50 border-orange-200 dark:bg-orange-950/30 dark:border-orange-800';
      case 'completed':
        return 'text-green-600 bg-green-50 border-green-200 dark:bg-green-950/30 dark:border-green-800';
      case 'failed':
        return 'text-red-600 bg-red-50 border-red-200 dark:bg-red-950/30 dark:border-red-800';
      case 'cancelled':
        return 'text-gray-600 bg-gray-50 border-gray-200 dark:bg-gray-950/30 dark:border-gray-800';
      default:
        return 'text-gray-600 bg-gray-50 border-gray-200 dark:bg-gray-950/30 dark:border-gray-800';
    }
  };

  const getStatusIcon = (status: LongTextJobStatus) => {
    switch (status) {
      case 'pending':
        return <Clock className="w-4 h-4" />;
      case 'chunking':
        return <FileText className="w-4 h-4" />;
      case 'processing':
        return <Volume2 className="w-4 h-4" />;
      case 'paused':
        return <Pause className="w-4 h-4" />;
      case 'completed':
        return <Download className="w-4 h-4" />;
      case 'failed':
        return <AlertCircle className="w-4 h-4" />;
      case 'cancelled':
        return <X className="w-4 h-4" />;
      default:
        return <Clock className="w-4 h-4" />;
    }
  };

  const downloadAudio = () => {
    if (!audioUrl || !onDownload) return;
    onDownload(audioUrl);
  };

  const currentProgress = progress?.overall_progress || 0;
  const currentChunk = progress?.current_chunk;
  const chunksCompleted = progress?.chunks_completed?.length || 0;
  const totalChunks = job?.job.chunks.total || 0;
  const estimatedRemaining = progress?.estimated_remaining_seconds;

  return (
    <Card className={`w-full border ${job ? getStatusColor(job.job.status) : 'border-red-200'}`}>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            {job ? getStatusIcon(job.job.status) : <AlertCircle className="w-4 h-4" />}
            Long Text TTS - {job ? job.job.status.charAt(0).toUpperCase() + job.job.status.slice(1) : 'Error'}
          </CardTitle>
          <div className="flex items-center gap-1">
            {/* Control buttons */}
            {job && isJobActive && (
              <>
                {job.job.status === 'processing' && onPause && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => onPause(job.job.job_id)}
                    disabled={isPausing}
                    className="h-7 px-2"
                  >
                    <Pause className="w-3 h-3 mr-1" />
                    {isPausing ? 'Pausing...' : 'Pause'}
                  </Button>
                )}

                {job.job.status === 'paused' && onResume && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => onResume(job.job.job_id)}
                    disabled={isResuming}
                    className="h-7 px-2"
                  >
                    <Play className="w-3 h-3 mr-1" />
                    {isResuming ? 'Resuming...' : 'Resume'}
                  </Button>
                )}
              </>
            )}

            {/* Download button for completed jobs */}
            {job?.job.status === 'completed' && audioUrl && onDownload && (
              <Button
                variant="outline"
                size="sm"
                onClick={downloadAudio}
                className="h-7 px-2 text-green-600 border-green-200 hover:bg-green-50"
              >
                <Download className="w-3 h-3 mr-1" />
                Download
              </Button>
            )}

            {/* Cancel button */}
            {job && isJobActive && onCancel && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => onCancel(job.job.job_id)}
                disabled={isCancelling}
                className="h-7 px-2 text-red-600 border-red-200 hover:bg-red-50"
              >
                <X className="w-3 h-3 mr-1" />
                {isCancelling ? 'Cancelling...' : 'Cancel'}
              </Button>
            )}
          </div>
        </div>
      </CardHeader>

      <CardContent className="pt-0">
        {/* Error display */}
        {error && (
          <div className="mb-3 p-2 bg-red-50 dark:bg-red-950/30 border border-red-200 dark:border-red-800 rounded text-sm text-red-700 dark:text-red-300">
            {error}
          </div>
        )}

        {job && (
          <>
            {/* Progress bar */}
            <div className="mb-3">
              <div className="flex justify-between text-xs text-muted-foreground mb-1">
                <span>Progress: {currentProgress}%</span>
                {totalChunks > 0 && (
                  <span>Chunks: {chunksCompleted}/{totalChunks}</span>
                )}
              </div>
              <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2">
                <div
                  className="bg-blue-500 h-2 rounded-full transition-all duration-300 ease-in-out"
                  style={{ width: `${currentProgress}%` }}
                />
              </div>
            </div>

            {/* Current chunk info */}
            {currentChunk && (
              <div className="mb-3 p-2 bg-gray-50 dark:bg-gray-800 rounded-md">
                <p className="text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Processing chunk {currentChunk.index + 1}:
                </p>
                <p className="text-xs text-gray-600 dark:text-gray-400 truncate">
                  {currentChunk.text_preview}
                </p>
              </div>
            )}

            {/* Time information */}
            <div className="grid grid-cols-2 gap-4 text-xs text-muted-foreground">
              <div>
                <span className="block font-medium">Elapsed Time</span>
                <span>{formatTime(elapsedTime)}</span>
              </div>
              {estimatedRemaining && (
                <div>
                  <span className="block font-medium">Est. Remaining</span>
                  <span>{formatTime(estimatedRemaining)}</span>
                </div>
              )}
            </div>

            {/* Job info */}
            <div className="mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
              <div className="grid grid-cols-2 gap-4 text-xs text-muted-foreground">
                <div>
                  <span className="block font-medium">Text Length</span>
                  <span>{job.job.text_length.toLocaleString()} characters</span>
                </div>
                <div>
                  <span className="block font-medium">Voice</span>
                  <span>{job.job.voice}</span>
                </div>
              </div>

              {/* Completed job info */}
              {job.job.status === 'completed' && job.job.output.duration_seconds && (
                <div className="mt-2 pt-2 border-t border-gray-200 dark:border-gray-700">
                  <div className="grid grid-cols-2 gap-4 text-xs text-muted-foreground">
                    <div>
                      <span className="block font-medium">Audio Duration</span>
                      <span>{formatTime(job.job.output.duration_seconds)}</span>
                    </div>
                    {job.job.output.size_bytes && (
                      <div>
                        <span className="block font-medium">File Size</span>
                        <span>{(job.job.output.size_bytes / 1024 / 1024).toFixed(1)} MB</span>
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}