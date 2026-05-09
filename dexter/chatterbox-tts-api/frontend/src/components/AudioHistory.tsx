import React, { useState, useRef, useEffect } from 'react';
import { Play, Pause, Download, Trash2, Edit2, Check, X, History, Clock, Settings, FileText } from 'lucide-react';
import { Input } from './ui/input';
import type { AudioRecord } from '../types';
import { Card, CardAction, CardContent } from './ui/card';
import { formatDuration } from '../lib/utils';

interface AudioHistoryProps {
  audioHistory: AudioRecord[];
  onDeleteAudioRecord: (recordId: string) => Promise<void>;
  onRenameAudioRecord: (recordId: string, newName: string) => void;
  onClearHistory: () => Promise<void>;
  onRestoreSettings: (settings: { exaggeration: number; cfgWeight: number; temperature: number }) => void;
  onRestoreText: (text: string) => void;
  isLoading: boolean;
}

export default function AudioHistory({
  audioHistory,
  onDeleteAudioRecord,
  onRenameAudioRecord,
  onClearHistory,
  onRestoreSettings,
  onRestoreText,
  isLoading
}: AudioHistoryProps) {

  const [playingAudio, setPlayingAudio] = useState<string | null>(null);
  const [editingRecord, setEditingRecord] = useState<string | null>(null);
  const [editName, setEditName] = useState('');
  const [showClearConfirm, setShowClearConfirm] = useState(false);
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

  const handlePlayPause = (record: AudioRecord) => {
    // If clicking the same audio that's currently playing/paused
    if (playingAudio === record.id && currentAudioRef.current) {
      if (currentAudioRef.current.paused) {
        // Resume playback
        currentAudioRef.current.play().catch(error => {
          console.error('Error resuming audio:', error);
          alert('Failed to resume audio');
        });
      } else {
        // Pause playback (keep position)
        currentAudioRef.current.pause();
      }
      return;
    }

    // Stop any currently playing audio from a different record
    if (currentAudioRef.current) {
      currentAudioRef.current.pause();
      currentAudioRef.current = null;
    }

    // Start playing the new audio
    const audio = new Audio(record.audioUrl);
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

    audio.play().catch(error => {
      console.error('Error playing audio:', error);
      setPlayingAudio(null);
      currentAudioRef.current = null;
      alert('Failed to play audio');
    });

    setPlayingAudio(record.id);
  };

  const handleDownload = (record: AudioRecord) => {
    const a = document.createElement('a');
    a.href = record.audioUrl;
    a.download = `${record.name}.wav`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
  };

  const handleRename = (recordId: string, currentName: string) => {
    setEditingRecord(recordId);
    setEditName(currentName);
  };

  const handleSaveRename = () => {
    if (editingRecord && editName.trim()) {
      onRenameAudioRecord(editingRecord, editName.trim());
      setEditingRecord(null);
      setEditName('');
    }
  };

  const handleCancelRename = () => {
    setEditingRecord(null);
    setEditName('');
  };

  const handleDelete = async (recordId: string) => {
    if (confirm('Are you sure you want to delete this audio record?')) {
      try {
        await onDeleteAudioRecord(recordId);
      } catch (error) {
        alert('Failed to delete audio record. Please try again.');
      }
    }
  };

  const handleClearHistory = async () => {
    if (confirm('Are you sure you want to clear all audio history? This cannot be undone.')) {
      try {
        await onClearHistory();
        setShowClearConfirm(false);
      } catch (error) {
        alert('Failed to clear history. Please try again.');
      }
    }
  };

  const handleRestoreSettings = (record: AudioRecord) => {
    onRestoreSettings({
      exaggeration: record.settings.exaggeration,
      cfgWeight: record.settings.cfgWeight,
      temperature: record.settings.temperature
    });
  };

  const handleRestoreText = (record: AudioRecord) => {
    onRestoreText(record.settings.text);
  };

  const getDurationDisplay = (record: AudioRecord) => {
    // Use actual duration if available
    if (record.duration && record.duration > 0) {
      return formatDuration(record.duration);
    }
    
    // Fallback to word-based estimation for records without duration
    const wordCount = record.settings.text.split(' ').length;
    const estimatedSeconds = (wordCount / 150) * 60;
    if (estimatedSeconds < 60) {
      return `~${Math.ceil(estimatedSeconds)}s`;
    }
    return `~${Math.ceil(estimatedSeconds / 60)}m`;
  };

  return (
    <>
      <Card>
        <CardContent>
          <div className="flex justify-between items-center mb-4">
            <div className="flex items-center gap-2">
              <History className="w-5 h-5 text-muted-foreground" />
              <h3 className="text-lg font-semibold text-foreground">Audio History</h3>
              <span className="text-sm text-muted-foreground">
                ({audioHistory.length} recordings)
              </span>
            </div>
            {audioHistory.length > 0 && (
              <button
                onClick={() => setShowClearConfirm(true)}
                className="text-sm text-destructive hover:text-destructive/80 duration-300"
              >
                Clear All
              </button>
            )}
          </div>

          {isLoading ? (
            <div className="text-center py-8 text-muted-foreground">
              <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary mx-auto mb-2"></div>
              <p>Loading history...</p>
            </div>
          ) : audioHistory.length === 0 ? (
            <div className="text-center py-8 text-muted-foreground">
              <Clock className="mx-auto w-12 h-12 mb-2 opacity-50" />
              <p>No audio history</p>
              <p className="text-sm">Generated audio will appear here</p>
            </div>
          ) : (
            <div className="space-y-3 max-h-96 overflow-y-auto">
              {audioHistory.map(record => (
                <div
                  key={record.id}
                  className="group p-4 border border-border rounded-lg hover:border-accent-foreground/30 hover:bg-accent/30 duration-300"
                >
                  <div className="flex items-start justify-between">
                    <div className="flex-1 min-w-0">
                      {editingRecord === record.id ? (
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
                          <h4 className="text-xl/normal font-medium text-foreground truncate">{record.name}</h4>
                        </>
                      )}
                    </div>
                    <div className="flex items-center gap-1 ml-4">
                      {/* Hover-only buttons for restoring data */}
                      <button
                        onClick={() => handleRestoreText(record)}
                        className="p-2 hover:bg-primary/10 text-primary rounded duration-300 opacity-0 group-hover:opacity-100"
                        title="Restore text to input"
                      >
                        <FileText className="w-4 h-4" />
                      </button>
                      <button
                        onClick={() => handleRestoreSettings(record)}
                        className="p-2 hover:bg-green-100 dark:hover:bg-green-900/30 text-green-600 dark:text-green-400 rounded duration-300 opacity-0 group-hover:opacity-100"
                        title="Restore settings"
                      >
                        <Settings className="w-4 h-4" />
                      </button>

                      {/* Regular always-visible buttons */}
                      <button
                        onClick={() => handlePlayPause(record)}
                        className="p-2 hover:bg-accent rounded duration-300"
                      >
                        {playingAudio === record.id && currentAudioRef.current && !currentAudioRef.current.paused ? (
                          <Pause className="w-4 h-4 text-muted-foreground" />
                        ) : (
                          <Play className="w-4 h-4 text-muted-foreground" />
                        )}
                      </button>
                      <button
                        onClick={() => handleDownload(record)}
                        className="p-2 hover:bg-accent rounded duration-300"
                      >
                        <Download className="w-4 h-4 text-muted-foreground" />
                      </button>
                      <button
                        onClick={() => handleRename(record.id, record.name)}
                        className="p-2 hover:bg-accent rounded duration-300"
                      >
                        <Edit2 className="w-4 h-4 text-muted-foreground" />
                      </button>
                      <button
                        onClick={() => handleDelete(record.id)}
                        className="p-2 hover:bg-destructive/10 text-destructive rounded duration-300"
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </div>
                  </div>

                  <div className="flex items-center gap-2 text-xs text-muted-foreground p-2">
                    <span>{record.createdAt.toLocaleDateString()}</span>
                    <span>•</span>
                    <span>{record.createdAt.toLocaleTimeString()}</span>
                    <span>•</span>
                    <span>{getDurationDisplay(record)}</span>
                    {record.settings.voiceName && (
                      <>
                        <span>•</span>
                        <span>{record.settings.voiceName}</span>
                      </>
                    )}
                  </div>

                  {/* Text Preview */}
                  <div className="text-sm text-muted-foreground bg-muted rounded p-2 mb-2">
                    <p className="line-clamp-2 leading-relaxed">
                      {record.settings.text.length > 120
                        ? record.settings.text.slice(0, 120) + '...'
                        : record.settings.text}
                    </p>
                  </div>

                  {/* Settings Summary */}
                  <div className="flex flex-wrap gap-2 text-xs">
                    <span className="bg-primary/10 text-primary px-2 py-1 rounded">
                      Exag: {record.settings.exaggeration}
                    </span>
                    <span className="bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-200 px-2 py-1 rounded">
                      CFG: {record.settings.cfgWeight}
                    </span>
                    <span className="bg-purple-100 dark:bg-purple-900/30 text-purple-800 dark:text-purple-200 px-2 py-1 rounded">
                      Temp: {record.settings.temperature}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Clear Confirmation Dialog */}
      {showClearConfirm && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-card rounded-lg p-6 w-96 max-w-90vw">
            <h3 className="text-lg font-semibold text-foreground mb-4">Clear Audio History</h3>
            <p className="text-muted-foreground mb-4">
              Are you sure you want to delete all {audioHistory.length} audio recordings? This action cannot be undone.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowClearConfirm(false)}
                className="px-4 py-2 text-muted-foreground hover:text-foreground duration-300"
              >
                Cancel
              </button>
              <button
                onClick={handleClearHistory}
                className="px-4 py-2 bg-destructive hover:bg-destructive/90 text-destructive-foreground rounded duration-300"
              >
                Clear All
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
} 