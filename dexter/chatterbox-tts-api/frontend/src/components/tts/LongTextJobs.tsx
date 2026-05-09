import React, { useState } from 'react';
import { Clock, Download, Trash2, Play, Pause, X, FileText, Volume2, AlertCircle, CheckCircle, RefreshCw } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '../ui/card';
import { Button } from '../ui/button';
import type { LongTextJobListItem, LongTextJobStatus } from '../../types';
import { removeDecimal } from '../../lib/utils';

interface LongTextJobsProps {
  jobs: LongTextJobListItem[];
  totalCount: number;
  isLoading: boolean;
  onRefresh: () => void;
  onDownload?: (jobId: string) => void;
  onResume?: (jobId: string) => void;
  onPause?: (jobId: string) => void;
  onCancel?: (jobId: string) => void;
  onViewJob?: (jobId: string) => void;
}

interface JobActionState {
  jobId: string | null;
  action: 'resuming' | 'pausing' | 'cancelling' | 'downloading' | null;
}

export default function LongTextJobs({
  jobs,
  totalCount,
  isLoading,
  onRefresh,
  onDownload,
  onResume,
  onPause,
  onCancel,
  onViewJob
}: LongTextJobsProps) {
  const [actionState, setActionState] = useState<JobActionState>({ jobId: null, action: null });

  const getStatusColor = (status: LongTextJobStatus): string => {
    switch (status) {
      case 'pending':
        return 'text-yellow-600';
      case 'chunking':
      case 'processing':
        return 'text-blue-600';
      case 'paused':
        return 'text-orange-600';
      case 'completed':
        return 'text-green-600';
      case 'failed':
        return 'text-red-600';
      case 'cancelled':
        return 'text-gray-600';
      default:
        return 'text-gray-600';
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
        return <CheckCircle className="w-4 h-4" />;
      case 'failed':
        return <AlertCircle className="w-4 h-4" />;
      case 'cancelled':
        return <X className="w-4 h-4" />;
      default:
        return <Clock className="w-4 h-4" />;
    }
  };

  const formatTime = (seconds: number): string => {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;

    if (hours > 0) {
      return `${hours}h ${minutes}m`;
    } else if (minutes > 0) {
      return `${minutes}m ${secs}s`;
    } else {
      return `${secs}s`;
    }
  };

  const formatDate = (dateString: string): string => {
    return new Date(dateString).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit'
    });
  };

  const handleAction = async (jobId: string, action: 'resume' | 'pause' | 'cancel' | 'download') => {
    setActionState({ jobId, action: `${action}ing` as any });

    try {
      switch (action) {
        case 'resume':
          await onResume?.(jobId);
          break;
        case 'pause':
          await onPause?.(jobId);
          break;
        case 'cancel':
          await onCancel?.(jobId);
          break;
        case 'download':
          await onDownload?.(jobId);
          break;
      }
    } catch (error) {
      console.error(`Failed to ${action} job:`, error);
    } finally {
      setActionState({ jobId: null, action: null });
    }
  };

  const isActionInProgress = (jobId: string, action: string): boolean => {
    return actionState.jobId === jobId && actionState.action === `${action}ing`;
  };


  return (
    <Card className="w-full">
      <CardHeader>
        <div className="flex items-center justify-between">
          <div>
            <CardTitle className="text-lg font-semibold">Active Processing</CardTitle>
            <p className="text-sm text-muted-foreground mt-1">
              Currently processing jobs ({totalCount} active)
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={onRefresh}
            disabled={isLoading}
            className="h-8 px-3"
          >
            <RefreshCw className={`w-3 h-3 mr-1 ${isLoading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>

      </CardHeader>

      <CardContent>
        {isLoading && jobs.length === 0 ? (
          <div className="text-center py-8 text-muted-foreground">
            <RefreshCw className="w-8 h-8 animate-spin mx-auto mb-2" />
            <p>Loading jobs...</p>
          </div>
        ) : jobs.length === 0 ? (
          <div className="text-center py-8 text-muted-foreground">
            <CheckCircle className="w-8 h-8 mx-auto mb-2" />
            <p>No jobs currently processing</p>
            <p className="text-sm mt-1">Completed jobs will appear in the history below</p>
          </div>
        ) : (
          <div className="space-y-3">
            {jobs.map((job) => (
              <div
                key={job.job_id}
                className="border rounded-lg p-3 hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors"
              >
                <div className="flex items-start justify-between mb-2">
                  <div className="flex-1 min-w-0">
                    {/* Job Status and Info */}
                    <div className="flex items-center gap-2 mb-1">
                      <span className={`flex items-center gap-1 text-sm font-medium ${getStatusColor(job.status)}`}>
                        {getStatusIcon(job.status)}
                        {job.status.charAt(0).toUpperCase() + job.status.slice(1)}
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {formatDate(job.created_at)}
                      </span>
                    </div>

                    {/* Text Preview */}
                    <p className="text-sm text-gray-700 dark:text-gray-300 truncate mb-1">
                      {job.text_preview}
                    </p>

                    {/* Job Details */}
                    <div className="flex items-center gap-4 text-xs text-muted-foreground">
                      <span>{job.text_length.toLocaleString()} chars</span>
                      <span>Voice: {job.voice}</span>
                      {job.progress_percentage > 0 && (
                        <span>{removeDecimal(job.progress_percentage)}% complete</span>
                      )}
                      {job.estimated_remaining_seconds && (
                        <span>~{formatTime(job.estimated_remaining_seconds)} left</span>
                      )}
                    </div>
                  </div>

                  {/* Action Buttons */}
                  <div className="flex items-center gap-1 ml-3">
                    {/* Resume button for paused jobs */}
                    {job.status === 'paused' && onResume && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleAction(job.job_id, 'resume')}
                        disabled={isActionInProgress(job.job_id, 'resum')}
                        className="h-7 px-2"
                      >
                        <Play className="w-3 h-3" />
                      </Button>
                    )}

                    {/* Pause button for active jobs */}
                    {job.status === 'processing' && onPause && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleAction(job.job_id, 'pause')}
                        disabled={isActionInProgress(job.job_id, 'paus')}
                        className="h-7 px-2"
                      >
                        <Pause className="w-3 h-3" />
                      </Button>
                    )}

                    {/* Download button for completed jobs */}
                    {job.status === 'completed' && onDownload && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleAction(job.job_id, 'download')}
                        disabled={isActionInProgress(job.job_id, 'download')}
                        className="h-7 px-2 text-green-600 border-green-200 hover:bg-green-50"
                      >
                        <Download className="w-3 h-3" />
                      </Button>
                    )}

                    {/* View/Monitor button */}
                    {onViewJob && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => onViewJob(job.job_id)}
                        className="h-7 px-2"
                        title="View job details"
                      >
                        <FileText className="w-3 h-3" />
                      </Button>
                    )}

                    {/* Cancel/Delete button */}
                    {(['pending', 'processing', 'paused'].includes(job.status) || job.status === 'completed') && onCancel && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleAction(job.job_id, 'cancel')}
                        disabled={isActionInProgress(job.job_id, 'cancel')}
                        className="h-7 px-2 text-red-600 border-red-200 hover:bg-red-50"
                        title={job.status === 'completed' ? 'Delete job' : 'Cancel job'}
                      >
                        {job.status === 'completed' ? <Trash2 className="w-3 h-3" /> : <X className="w-3 h-3" />}
                      </Button>
                    )}
                  </div>
                </div>

                {/* Progress Bar for Active Jobs - Enhanced */}
                {['processing', 'paused', 'chunking'].includes(job.status) && (
                  <div className="mt-3">
                    <div className="flex justify-between items-center mb-1">
                      <span className="text-xs text-muted-foreground">
                        Progress: {removeDecimal(job.progress_percentage)}%
                      </span>
                      {job.estimated_remaining_seconds && (
                        <span className="text-xs text-muted-foreground">
                          {formatTime(job.estimated_remaining_seconds)} remaining
                        </span>
                      )}
                    </div>
                    <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2">
                      <div
                        className={`h-2 rounded-full transition-all duration-500 ${
                          job.status === 'processing' ? 'bg-blue-500' :
                          job.status === 'paused' ? 'bg-orange-500' :
                          'bg-yellow-500'
                        }`}
                        style={{ width: `${Math.max(2, removeDecimal(job.progress_percentage))}%` }}
                      />
                    </div>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        {/* Total Count */}
        {totalCount > jobs.length && (
          <div className="mt-3 pt-3 border-t text-center text-xs text-muted-foreground">
            Showing {jobs.length} of {totalCount} jobs
          </div>
        )}
      </CardContent>
    </Card>
  );
}