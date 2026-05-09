import React, { useState, useRef, useEffect } from 'react';
import { useDebounce } from '../../hooks/useDebounce';
import {
  Play, Pause, Download, Trash2, Edit2, Check, X, History, Clock, Settings,
  FileText, Archive, ArchiveRestore, RotateCcw, Search, Filter, ChevronLeft,
  ChevronRight, MoreHorizontal, CheckSquare, Square, BarChart3, AlertCircle,
  CheckCircle, Volume2, Loader2
} from 'lucide-react';
import { Input } from '../ui/input';
import { Button } from '../ui/button';
import { Card, CardAction, CardContent } from '../ui/card';
import { cn, formatDuration, formatFileSize } from '../../lib/utils';
import type { LongTextHistoryItem, LongTextJobStatus, LongTextHistorySort, LongTextJobCreateResponse, BulkJobActionResponse, MessageResponse } from '../../types';

interface LongTextHistoryProps {
  jobs: LongTextHistoryItem[];
  totalCount: number;
  currentPage: number;
  totalPages: number;
  selectedJobs: string[];
  isLoading: boolean;
  isLoadingStats: boolean;
  stats?: {
    total_jobs: number;
    completed_jobs: number;
    failed_jobs: number;
    total_audio_duration_seconds: number;
    total_storage_used_bytes: number;
    success_rate_percentage: number;
  };
  // Job operations
  onUpdateJob: (jobId: string, updates: { display_name?: string; tags?: string[]; is_archived?: boolean }) => Promise<MessageResponse>;
  onRetryJob: (jobId: string) => Promise<LongTextJobCreateResponse | void>;
  onDeleteJob: (jobId: string) => Promise<BulkJobActionResponse>;
  onArchiveJob: (jobId: string) => Promise<MessageResponse>;
  onUnarchiveJob: (jobId: string) => Promise<MessageResponse>;
  onDownloadAudio: (job: LongTextHistoryItem) => Promise<void>;
  onGetAudioUrl: (jobId: string) => Promise<string | null>;
  // Bulk operations
  onBulkDelete: (jobIds: string[]) => Promise<BulkJobActionResponse>;
  onBulkArchive: (jobIds: string[]) => Promise<BulkJobActionResponse>;
  onBulkUnarchive: (jobIds: string[]) => Promise<BulkJobActionResponse>;
  onBulkRetry: (jobIds: string[]) => Promise<BulkJobActionResponse>;
  // Selection
  onToggleJobSelection: (jobId: string) => void;
  onSelectAllJobs: () => void;
  onClearSelection: () => void;
  // Pagination
  onGoToPage: (page: number) => void;
  onNextPage: () => void;
  onPrevPage: () => void;
  // Filtering and sorting
  onSearch: (searchText: string) => void;
  onUpdateSort: (sort: LongTextHistorySort) => void;
  onClearHistory: () => Promise<MessageResponse>;
  // Settings
  showArchived: boolean;
  onToggleArchived: () => void;
  currentSort: LongTextHistorySort;
  // Restore functionality
  onRestoreSettings?: (settings: { exaggeration: number; cfgWeight: number; temperature: number }) => void;
  onRestoreText?: (text: string) => void;
}

export default function LongTextHistory({
  jobs,
  totalCount,
  currentPage,
  totalPages,
  selectedJobs,
  isLoading,
  isLoadingStats,
  stats,
  onUpdateJob,
  onRetryJob,
  onDeleteJob,
  onArchiveJob,
  onUnarchiveJob,
  onDownloadAudio,
  onGetAudioUrl,
  onBulkDelete,
  onBulkArchive,
  onBulkUnarchive,
  onBulkRetry,
  onToggleJobSelection,
  onSelectAllJobs,
  onClearSelection,
  onGoToPage,
  onNextPage,
  onPrevPage,
  onSearch,
  onUpdateSort,
  onClearHistory,
  showArchived,
  onToggleArchived,
  currentSort,
  onRestoreSettings,
  onRestoreText
}: LongTextHistoryProps) {
  const [searchText, setSearchText] = useState('');
  const debouncedSearchText = useDebounce(searchText, 500);
  const [editingJob, setEditingJob] = useState<string | null>(null);
  const [editName, setEditName] = useState('');
  const [playingAudio, setPlayingAudio] = useState<string | null>(null);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [showBulkConfirm, setShowBulkConfirm] = useState<{ action: string; count: number } | null>(null);
  const [showStats, setShowStats] = useState(false);
  const [actionStates, setActionStates] = useState<Map<string, string>>(new Map());

  const currentAudioRef = useRef<HTMLAudioElement | null>(null);

  // Cleanup audio when component unmounts
  useEffect(() => {
    return () => {
      if (currentAudioRef.current) {
        currentAudioRef.current.pause();
        currentAudioRef.current = null;
      }
    };
  }, []);

  // Trigger search when debounced search text changes
  useEffect(() => {
    onSearch(debouncedSearchText);
  }, [debouncedSearchText, onSearch]);

  const getStatusColor = (status: LongTextJobStatus): string => {
    switch (status) {
      case 'pending':
        return 'text-yellow-600 dark:text-yellow-400';
      case 'chunking':
      case 'processing':
        return 'text-blue-600 dark:text-blue-400';
      case 'paused':
        return 'text-orange-600 dark:text-orange-400';
      case 'completed':
        return 'text-green-600 dark:text-green-400';
      case 'failed':
        return 'text-red-600 dark:text-red-400';
      case 'cancelled':
        return 'text-gray-600 dark:text-gray-400';
      default:
        return 'text-gray-600 dark:text-gray-400';
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

  const handlePlayPause = async (job: LongTextHistoryItem) => {
    // If clicking the same audio that's currently playing/paused
    if (playingAudio === job.job_id && currentAudioRef.current) {
      if (currentAudioRef.current.paused) {
        currentAudioRef.current.play().catch(error => {
          console.error('Error resuming audio:', error);
          alert('Failed to resume audio');
        });
      } else {
        currentAudioRef.current.pause();
      }
      return;
    }

    // Stop any currently playing audio
    if (currentAudioRef.current) {
      currentAudioRef.current.pause();
      currentAudioRef.current = null;
    }

    try {
      setActionStates(prev => new Map(prev).set(job.job_id, 'loading-audio'));

      const audioUrl = await onGetAudioUrl(job.job_id);
      if (!audioUrl) {
        throw new Error('Failed to get audio URL');
      }

      const audio = new Audio(audioUrl);
      currentAudioRef.current = audio;

      audio.onended = () => {
        setPlayingAudio(null);
        currentAudioRef.current = null;
      };

      audio.onerror = () => {
        setPlayingAudio(null);
        currentAudioRef.current = null;
        alert('Error playing audio file');
      };

      await audio.play();
      setPlayingAudio(job.job_id);
    } catch (error) {
      console.error('Error playing audio:', error);
      alert('Failed to play audio');
    } finally {
      setActionStates(prev => {
        const newMap = new Map(prev);
        newMap.delete(job.job_id);
        return newMap;
      });
    }
  };

  const handleRename = (jobId: string, currentName: string) => {
    setEditingJob(jobId);
    setEditName(currentName);
  };

  const handleSaveRename = async () => {
    if (editingJob && editName.trim()) {
      try {
        await onUpdateJob(editingJob, { display_name: editName.trim() });
        setEditingJob(null);
        setEditName('');
      } catch (error) {
        alert('Failed to rename job');
      }
    }
  };

  const handleCancelRename = () => {
    setEditingJob(null);
    setEditName('');
  };

  const handleRestoreSettings = (job: LongTextHistoryItem) => {
    if (onRestoreSettings && job.parameters) {
      const { exaggeration, cfg_weight, temperature } = job.parameters;
      if (exaggeration !== undefined && cfg_weight !== undefined && temperature !== undefined) {
        onRestoreSettings({
          exaggeration,
          cfgWeight: cfg_weight,
          temperature
        });
      }
    }
  };

  const handleRestoreText = (job: LongTextHistoryItem) => {
    if (onRestoreText) {
      // For now, use the text preview. In a full implementation,
      // we might want to fetch the full text from the server
      onRestoreText(job.text_preview);
    }
  };

  const handleAction = async (jobId: string, action: string, actionFn: () => Promise<void | MessageResponse | BulkJobActionResponse | LongTextJobCreateResponse>) => {
    try {
      setActionStates(prev => new Map(prev).set(jobId, action));
      await actionFn();
    } catch (error) {
      console.error(`Failed to ${action}:`, error);
      alert(`Failed to ${action}. Please try again.`);
    } finally {
      setActionStates(prev => {
        const newMap = new Map(prev);
        newMap.delete(jobId);
        return newMap;
      });
    }
  };

  const handleBulkAction = async (action: string, actionFn: () => Promise<void | MessageResponse | BulkJobActionResponse>) => {
    try {
      await actionFn();
      setShowBulkConfirm(null);
    } catch (error) {
      console.error(`Failed to ${action}:`, error);
      alert(`Failed to ${action}. Please try again.`);
    }
  };

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    // Search is now handled automatically by the debounced effect
  };

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    setSearchText(value);
    // Search will be triggered automatically after debounce delay
  };

  const formatDate = (dateString: string): string => {
    return new Date(dateString).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit'
    });
  };

  const allSelected = jobs.length > 0 && selectedJobs.length === jobs.length;
  const someSelected = selectedJobs.length > 0 && selectedJobs.length < jobs.length;

  return (
    <>
      <Card>
        <CardContent>
          {/* Header */}
          <div className="flex justify-between items-center mb-4">
            <div className="flex items-center gap-2">
              <History className="w-5 h-5 text-muted-foreground" />
              <h3 className="text-lg font-semibold text-foreground">Long Text History</h3>
              <span className="text-sm text-muted-foreground">
                ({totalCount} jobs)
              </span>
              {isLoadingStats && <Loader2 className="w-4 h-4 animate-spin" />}
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowStats(!showStats)}
                className="h-8 px-3"
              >
                <BarChart3 className="w-3 h-3 mr-1" />
                Stats
              </Button>
              {totalCount > 0 && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowClearConfirm(true)}
                  className="h-8 px-3 text-destructive hover:text-destructive"
                >
                  Clear All
                </Button>
              )}
            </div>
          </div>

          {/* Stats Panel */}
          {showStats && stats && (
            <div className="mb-4 p-4 bg-muted rounded-lg">
              <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                <div>
                  <div className="font-medium">Total Jobs</div>
                  <div className="text-muted-foreground">{stats.total_jobs}</div>
                </div>
                <div>
                  <div className="font-medium">Success Rate</div>
                  <div className="text-green-600">{stats.success_rate_percentage.toFixed(1)}%</div>
                </div>
                <div>
                  <div className="font-medium">Total Duration</div>
                  <div className="text-muted-foreground">{formatDuration(stats.total_audio_duration_seconds)}</div>
                </div>
                <div>
                  <div className="font-medium">Storage Used</div>
                  <div className="text-muted-foreground">{formatFileSize(stats.total_storage_used_bytes)}</div>
                </div>
              </div>
            </div>
          )}

          {/* Search and Filter Bar */}
          <div className="flex flex-col sm:flex-row gap-2 mb-4">
            <form onSubmit={handleSearch} className="flex gap-2 flex-1">
              <div className="relative flex-1">
                <Search className="absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
                <Input
                  type="text"
                  placeholder="Search jobs..."
                  value={searchText}
                  onChange={handleSearchChange}
                  className="pl-9"
                />
              </div>
              <Button type="submit" variant="outline" size="sm">
                Search
              </Button>
            </form>

            <div className="flex gap-2">
              <Button
                variant={showArchived ? "default" : "outline"}
                size="sm"
                onClick={onToggleArchived}
              >
                <Archive className="w-3 h-3 mr-1" />
                {showArchived ? 'Hide Archived' : 'Show Archived'}
              </Button>

              <select
                value={currentSort}
                onChange={(e) => onUpdateSort(e.target.value as LongTextHistorySort)}
                className="px-3 py-1 text-sm border border-border rounded bg-background"
              >
                <option value="completed_desc">Newest First</option>
                <option value="completed_asc">Oldest First</option>
                <option value="name_asc">Name A-Z</option>
                <option value="name_desc">Name Z-A</option>
                <option value="duration_desc">Longest First</option>
                <option value="duration_asc">Shortest First</option>
                <option value="size_desc">Largest First</option>
                <option value="size_asc">Smallest First</option>
              </select>
            </div>
          </div>

          {/* Bulk Actions */}
          {selectedJobs.length > 0 && (
            <div className="flex items-center justify-between p-3 bg-primary/10 rounded-lg mb-4">
              <span className="text-sm font-medium">
                {selectedJobs.length} job{selectedJobs.length > 1 ? 's' : ''} selected
              </span>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowBulkConfirm({ action: 'archive', count: selectedJobs.length })}
                >
                  <Archive className="w-3 h-3 mr-1" />
                  Archive
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowBulkConfirm({ action: 'retry', count: selectedJobs.length })}
                >
                  <RotateCcw className="w-3 h-3 mr-1" />
                  Retry
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowBulkConfirm({ action: 'delete', count: selectedJobs.length })}
                  className="text-destructive hover:text-destructive"
                >
                  <Trash2 className="w-3 h-3 mr-1" />
                  Delete
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={onClearSelection}
                >
                  Clear
                </Button>
              </div>
            </div>
          )}

          {/* Job List */}
          {isLoading ? (
            <div className="text-center py-8 text-muted-foreground">
              <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary mx-auto mb-2"></div>
              <p>Loading history...</p>
            </div>
          ) : jobs.length === 0 ? (
            <div className="text-center py-8 text-muted-foreground">
              <History className="mx-auto w-12 h-12 mb-2 opacity-50" />
              <p>No {showArchived ? 'archived ' : ''}jobs found</p>
              <p className="text-sm">Completed long text jobs will appear here</p>
            </div>
          ) : (
            <div className="space-y-3">
              {/* Select All Checkbox */}
              {jobs.length > 0 && (
                <div className="flex items-center gap-2 p-2 border-b">
                  <button
                    onClick={allSelected ? onClearSelection : onSelectAllJobs}
                    className="flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground"
                  >
                    {allSelected ? (
                      <CheckSquare className="w-4 h-4" />
                    ) : someSelected ? (
                      <div className="w-4 h-4 border border-primary bg-primary/20 rounded flex items-center justify-center">
                        <div className="w-2 h-2 bg-primary rounded"></div>
                      </div>
                    ) : (
                      <Square className="w-4 h-4" />
                    )}
                    Select All
                  </button>
                </div>
              )}

              {jobs.map(job => (
                <div
                  key={job.job_id}
                  className="group p-4 border border-border rounded-lg hover:border-accent-foreground/30 hover:bg-accent/30 duration-300 relative"
                >
                  <div className="flex items-start justify-between">
                    {/* <div className="flex items-start gap-3 flex-1 min-w-0"> */}
                    {/* Selection Checkbox */}
                    <button
                      onClick={() => onToggleJobSelection(job.job_id)}
                      className={cn(
                        'absolute left-1 top-0.5',
                        "mt-1 opacity-0 group-hover:opacity-100 transition-opacity",
                        selectedJobs.includes(job.job_id) && "opacity-100"
                      )}
                    >
                      {selectedJobs.includes(job.job_id) ? (
                        <CheckSquare className="w-4 h-4 text-primary" />
                      ) : (
                        <Square className="w-4 h-4 text-muted-foreground" />
                      )}
                    </button>

                    <div className="flex-1 min-w-0">
                      {/* Job Title and Status */}
                      <div className="flex-1 items-center gap-2 mb-1">
                        {editingJob === job.job_id ? (
                          <div className="flex items-center gap-2">
                            <Input
                              type="text"
                              value={editName}
                              onChange={(e) => setEditName(e.target.value)}
                              className="flex-1 text-sm"
                              onKeyDown={(e) => {
                                if (e.key === 'Enter') handleSaveRename();
                                if (e.key === 'Escape') handleCancelRename();
                              }}
                              autoFocus
                            />
                            <button
                              onClick={handleSaveRename}
                              className="p-1 hover:bg-green-100 dark:hover:bg-green-900/30 text-green-600 dark:text-green-400 rounded duration-300"
                            >
                              <Check className="w-4 h-4" />
                            </button>
                            <button
                              onClick={handleCancelRename}
                              className="p-1 hover:bg-destructive/10 text-destructive rounded duration-300"
                            >
                              <X className="w-4 h-4" />
                            </button>
                          </div>
                        ) : (
                          <>
                            <h4 className="text-lg font-medium text-foreground truncate">
                              {job.display_name || `Job ${job.job_id.slice(-8)}`}
                            </h4>
                          </>
                        )}
                      </div>

                    </div>
                    {/* </div> */}

                    {/* Action Buttons */}
                    <div className="flex items-center gap-1 ml-4">
                      {/* Hover-only buttons for restoring data */}
                      {/* {onRestoreText && (
                        <button
                          onClick={() => handleRestoreText(job)}
                          className="p-2 hover:bg-primary/10 text-primary rounded duration-300 opacity-0 group-hover:opacity-100"
                          title="Restore text to input"
                        >
                          <FileText className="w-4 h-4" />
                        </button>
                      )} */}
                      {onRestoreSettings && job.parameters && (
                        job.parameters.exaggeration !== undefined ||
                        job.parameters.cfg_weight !== undefined ||
                        job.parameters.temperature !== undefined
                      ) && (
                          <button
                            onClick={() => handleRestoreSettings(job)}
                            className="p-2 hover:bg-green-100 dark:hover:bg-green-900/30 text-green-600 dark:text-green-400 rounded duration-300 opacity-0 group-hover:opacity-100"
                            title="Restore voice settings"
                          >
                            <Settings className="w-4 h-4" />
                          </button>
                        )}

                      {/* Regular always-visible buttons */}
                      {/* Play/Pause Button */}
                      {job.status === 'completed' && (
                        <button
                          onClick={() => handlePlayPause(job)}
                          disabled={actionStates.get(job.job_id) === 'loading-audio'}
                          className="p-2 hover:bg-accent rounded duration-300"
                        >
                          {actionStates.get(job.job_id) === 'loading-audio' ? (
                            <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
                          ) : playingAudio === job.job_id && currentAudioRef.current && !currentAudioRef.current.paused ? (
                            <Pause className="w-4 h-4 text-muted-foreground" />
                          ) : (
                            <Play className="w-4 h-4 text-muted-foreground" />
                          )}
                        </button>
                      )}

                      {/* Download Button */}
                      {job.status === 'completed' && (
                        <button
                          onClick={() => handleAction(job.job_id, 'downloading', () => onDownloadAudio(job))}
                          disabled={actionStates.has(job.job_id)}
                          className="p-2 hover:bg-accent rounded duration-300"
                        >
                          {actionStates.get(job.job_id) === 'downloading' ? (
                            <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
                          ) : (
                            <Download className="w-4 h-4 text-muted-foreground" />
                          )}
                        </button>
                      )}

                      {/* Retry Button */}
                      {(job.status === 'failed' || job.status === 'cancelled') && (
                        <button
                          onClick={() => handleAction(job.job_id, 'retrying', () => onRetryJob(job.job_id))}
                          disabled={actionStates.has(job.job_id)}
                          className="p-2 hover:bg-green-100 dark:hover:bg-green-900/30 text-green-600 dark:text-green-400 rounded duration-300"
                        >
                          {actionStates.get(job.job_id) === 'retrying' ? (
                            <Loader2 className="w-4 h-4 animate-spin" />
                          ) : (
                            <RotateCcw className="w-4 h-4" />
                          )}
                        </button>
                      )}

                      {/* Archive/Unarchive Button */}
                      <button
                        onClick={() =>
                          handleAction(
                            job.job_id,
                            job.is_archived ? 'unarchiving' : 'archiving',
                            () => job.is_archived ? onUnarchiveJob(job.job_id) : onArchiveJob(job.job_id)
                          )
                        }
                        disabled={actionStates.has(job.job_id)}
                        className="p-2 hover:bg-accent rounded duration-300"
                      >
                        {actionStates.get(job.job_id) === 'archiving' || actionStates.get(job.job_id) === 'unarchiving' ? (
                          <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
                        ) : job.is_archived ? (
                          <ArchiveRestore className="w-4 h-4 text-muted-foreground" />
                        ) : (
                          <Archive className="w-4 h-4 text-muted-foreground" />
                        )}
                      </button>

                      {/* Edit Button */}
                      <button
                        onClick={() => handleRename(job.job_id, job.display_name || `Job ${job.job_id.slice(-8)}`)}
                        className="p-2 hover:bg-accent rounded duration-300"
                      >
                        <Edit2 className="w-4 h-4 text-muted-foreground" />
                      </button>

                      {/* Delete Button */}
                      <button
                        onClick={() => handleAction(job.job_id, 'deleting', () => onDeleteJob(job.job_id))}
                        disabled={actionStates.has(job.job_id)}
                        className="p-2 hover:bg-destructive/10 text-destructive rounded duration-300"
                      >
                        {actionStates.get(job.job_id) === 'deleting' ? (
                          <Loader2 className="w-4 h-4 animate-spin" />
                        ) : (
                          <Trash2 className="w-4 h-4" />
                        )}
                      </button>
                    </div>
                  </div>


                  {/* Job Metadata */}
                  <div className="flex items-center gap-2 text-xs text-muted-foreground mb-2">
                    <span className={`flex items-center gap-1 text-sm font-medium ${getStatusColor(job.status)}`}>
                      {getStatusIcon(job.status)}
                      {job.status.charAt(0).toUpperCase() + job.status.slice(1)}
                    </span>
                    {job.is_archived && (
                      <span className="text-xs bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400 px-2 py-1 rounded">
                        Archived
                      </span>
                    )}
                    <span>{formatDate(job.completion_timestamp || job.created_at)}</span>
                    <span>•</span>
                    <span>{job.text_length.toLocaleString()} chars</span>
                    {job.total_duration_seconds && (
                      <>
                        <span>•</span>
                        <span>{formatDuration(job.total_duration_seconds)}</span>
                      </>
                    )}
                    {job.audio_file_size && (
                      <>
                        <span>•</span>
                        <span>{formatFileSize(job.audio_file_size)}</span>
                      </>
                    )}
                    {job?.voice && (
                      <>
                        <span>•</span>
                        <span>Voice: {job?.voice}</span>
                      </>
                    )}
                    {(job?.retry_count && job?.retry_count > 0) ? (
                      <>
                        <span>•</span>
                        <span className="text-orange-600">Retry #{job?.retry_count}</span>
                      </>
                    ) : null}
                  </div>

                  {/* Text Preview */}
                  <div className="text-sm text-muted-foreground bg-muted rounded p-2 mb-2">
                    <p className="line-clamp-2 leading-relaxed">
                      {job.text_preview.length > 150
                        ? job.text_preview.slice(0, 150) + '...'
                        : job.text_preview}
                    </p>
                  </div>

                  {/* Voice Settings Summary */}
                  {job.parameters && (
                    <div className="flex flex-wrap gap-2 text-xs mb-2">
                      {job.parameters.exaggeration !== undefined && (
                        <span className="bg-primary/10 text-primary px-2 py-1 rounded">
                          Exag: {job.parameters.exaggeration}
                        </span>
                      )}
                      {job.parameters.cfg_weight !== undefined && (
                        <span className="bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-200 px-2 py-1 rounded">
                          CFG: {job.parameters.cfg_weight}
                        </span>
                      )}
                      {job.parameters.temperature !== undefined && (
                        <span className="bg-purple-100 dark:bg-purple-900/30 text-purple-800 dark:text-purple-200 px-2 py-1 rounded">
                          Temp: {job.parameters.temperature}
                        </span>
                      )}
                    </div>
                  )}

                  {/* Tags */}
                  {job.tags && job.tags.length > 0 && (
                    <div className="flex flex-wrap gap-1 mb-2">
                      {job.tags.map((tag, index) => (
                        <span
                          key={index}
                          className="text-xs bg-primary/10 text-primary px-2 py-1 rounded"
                        >
                          {tag}
                        </span>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}

          {/* Pagination */}
          {totalPages > 1 && (
            <div className="flex items-center justify-between mt-4 pt-4 border-t">
              <div className="text-sm text-muted-foreground">
                Page {currentPage + 1} of {totalPages} ({totalCount} total)
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={onPrevPage}
                  disabled={currentPage === 0}
                >
                  <ChevronLeft className="w-4 h-4" />
                  Previous
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={onNextPage}
                  disabled={currentPage >= totalPages - 1}
                >
                  Next
                  <ChevronRight className="w-4 h-4" />
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Clear History Confirmation Dialog */}
      {showClearConfirm && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card rounded-lg p-6 w-96 max-w-90vw">
            <h3 className="text-lg font-semibold text-foreground mb-4">Clear Long Text History</h3>
            <p className="text-muted-foreground mb-4">
              Are you sure you want to delete all {totalCount} long text jobs? This action cannot be undone and will remove all audio files.
            </p>
            <div className="flex justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => setShowClearConfirm(false)}
              >
                Cancel
              </Button>
              <Button
                variant="destructive"
                onClick={() => {
                  handleAction('history', 'clearing', onClearHistory);
                  setShowClearConfirm(false);
                }}
              >
                Clear All
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Bulk Action Confirmation Dialog */}
      {showBulkConfirm && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card rounded-lg p-6 w-96 max-w-90vw">
            <h3 className="text-lg font-semibold text-foreground mb-4">
              Confirm Bulk {showBulkConfirm.action.charAt(0).toUpperCase() + showBulkConfirm.action.slice(1)}
            </h3>
            <p className="text-muted-foreground mb-4">
              Are you sure you want to {showBulkConfirm.action} {showBulkConfirm.count} job{showBulkConfirm.count > 1 ? 's' : ''}?
              {showBulkConfirm.action === 'delete' && ' This action cannot be undone.'}
            </p>
            <div className="flex justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => setShowBulkConfirm(null)}
              >
                Cancel
              </Button>
              <Button
                variant={showBulkConfirm.action === 'delete' ? 'destructive' : 'default'}
                onClick={() => {
                  const { action } = showBulkConfirm;
                  switch (action) {
                    case 'delete':
                      handleBulkAction('delete', () => onBulkDelete(selectedJobs));
                      break;
                    case 'archive':
                      handleBulkAction('archive', () => onBulkArchive(selectedJobs));
                      break;
                    case 'retry':
                      handleBulkAction('retry', () => onBulkRetry(selectedJobs));
                      break;
                  }
                }}
              >
                {showBulkConfirm.action.charAt(0).toUpperCase() + showBulkConfirm.action.slice(1)}
              </Button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}