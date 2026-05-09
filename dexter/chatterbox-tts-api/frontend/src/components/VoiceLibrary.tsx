import React, { useState, useRef, useEffect } from 'react';
import { Trash2, Play, Pause, Upload, Edit2, Check, X, RefreshCw, Crown, Star, StarOff, Tag, Plus, Globe } from 'lucide-react';
import { Input } from './ui/input';
import { Card, CardContent, CardHeader, CardTitle } from './ui/card';
import { Button } from './ui/button';
import VoiceUploadModal from './VoiceUploadModal';
import type { VoiceSample } from '../types';
import { getLanguageByCode, getLanguageName, DEFAULT_LANGUAGE, getLanguageFlag } from '../constants/languages';

interface VoiceLibraryProps {
  voices: VoiceSample[];
  selectedVoice: VoiceSample | null;
  onVoiceSelect: (voice: VoiceSample | null) => void;
  onAddVoice: (file: File, customName?: string, language?: string) => Promise<VoiceSample>;
  onDeleteVoice: (voiceId: string) => Promise<void>;
  onRenameVoice: (voiceId: string, newName: string) => Promise<void>;
  onRefresh?: () => Promise<void>;
  isLoading: boolean;
  defaultVoice?: string | null;
  onSetDefaultVoice?: (voiceName: string) => Promise<boolean>;
  onClearDefaultVoice?: () => Promise<boolean>;
  onAddAlias?: (voiceName: string, alias: string) => Promise<boolean>;
  onRemoveAlias?: (voiceName: string, alias: string) => Promise<boolean>;
}

export default function VoiceLibrary({
  voices,
  selectedVoice,
  onVoiceSelect,
  onAddVoice,
  onDeleteVoice,
  onRenameVoice,
  onRefresh,
  isLoading,
  defaultVoice,
  onSetDefaultVoice,
  onClearDefaultVoice,
  onAddAlias,
  onRemoveAlias
}: VoiceLibraryProps) {

  const [playingVoice, setPlayingVoice] = useState<string | null>(null);
  const [editingVoice, setEditingVoice] = useState<string | null>(null);
  const [editName, setEditName] = useState('');
  const [showUploadModal, setShowUploadModal] = useState(false);
  const [addingAliasFor, setAddingAliasFor] = useState<string | null>(null);
  const [newAlias, setNewAlias] = useState('');
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

  const handleUploadVoice = async (file: File, customName?: string, language?: string) => {
    try {
      await onAddVoice(file, customName, language);
    } catch (error) {
      throw error; // Re-throw to let the modal handle the error display
    }
  };

  const handleRename = (voiceId: string, currentName: string) => {
    setEditingVoice(voiceId);
    setEditName(currentName);
  };

  const handleSaveRename = async () => {
    if (editingVoice && editName.trim()) {
      try {
        await onRenameVoice(editingVoice, editName.trim());
        setEditingVoice(null);
        setEditName('');
      } catch (error) {
        alert('Failed to rename voice. Please try again.');
      }
    }
  };

  const handleCancelRename = () => {
    setEditingVoice(null);
    setEditName('');
  };

  const handleDeleteVoice = async (voiceId: string) => {
    if (confirm('Are you sure you want to delete this voice?')) {
      try {
        await onDeleteVoice(voiceId);
      } catch (error) {
        alert('Failed to delete voice. Please try again.');
      }
    }
  };

  const handleSetAsDefault = async (voiceName: string) => {
    if (onSetDefaultVoice) {
      try {
        const success = await onSetDefaultVoice(voiceName);
        if (!success) {
          alert('Failed to set default voice. Please try again.');
        }
      } catch (error) {
        alert('Failed to set default voice. Please try again.');
      }
    }
  };

  const handleClearDefault = async () => {
    if (onClearDefaultVoice) {
      try {
        const success = await onClearDefaultVoice();
        if (!success) {
          alert('Failed to clear default voice. Please try again.');
        }
      } catch (error) {
        alert('Failed to clear default voice. Please try again.');
      }
    }
  };

  const handleAddAlias = (voiceId: string) => {
    setAddingAliasFor(voiceId);
    setNewAlias('');
  };

  const handleSaveAlias = async () => {
    if (addingAliasFor && newAlias.trim() && onAddAlias) {
      try {
        const success = await onAddAlias(addingAliasFor, newAlias.trim());
        if (success) {
          setAddingAliasFor(null);
          setNewAlias('');
        } else {
          alert('Failed to add alias. Please try again.');
        }
      } catch (error: any) {
        alert(error.message || 'Failed to add alias. Please try again.');
      }
    }
  };

  const handleCancelAlias = () => {
    setAddingAliasFor(null);
    setNewAlias('');
  };

  const handleRemoveAlias = async (voiceId: string, alias: string) => {
    if (onRemoveAlias) {
      try {
        const success = await onRemoveAlias(voiceId, alias);
        if (!success) {
          alert('Failed to remove alias. Please try again.');
        }
      } catch (error: any) {
        alert(error.message || 'Failed to remove alias. Please try again.');
      }
    }
  };

  const playPreview = (voice: VoiceSample) => {
    // If clicking the same voice that's currently playing/paused
    if (playingVoice === voice.id && currentAudioRef.current) {
      if (currentAudioRef.current.paused) {
        // Resume playback
        currentAudioRef.current.play().catch(error => {
          console.error('Error resuming voice:', error);
          alert('Failed to resume voice sample');
        });
      } else {
        // Pause playback (keep position)
        currentAudioRef.current.pause();
      }
      return;
    }

    // Stop any currently playing audio from a different voice
    if (currentAudioRef.current) {
      currentAudioRef.current.pause();
      currentAudioRef.current = null;
    }

    // Start playing the new audio
    const audio = new Audio(voice.audioUrl);
    currentAudioRef.current = audio;

    audio.onended = () => {
      setPlayingVoice(null);
      currentAudioRef.current = null;
    };

    audio.onerror = () => {
      setPlayingVoice(null);
      currentAudioRef.current = null;
      alert('Error playing voice sample');
    };

    audio.play().catch(error => {
      console.error('Error playing voice:', error);
      setPlayingVoice(null);
      currentAudioRef.current = null;
      alert('Failed to play voice sample');
    });

    setPlayingVoice(voice.id);
  };

  return (
    <>
      <Card>
        <CardHeader>
          <div className="flex justify-between items-center">
            <div className="flex items-center gap-2">
              <CardTitle>Voice Library</CardTitle>
              {defaultVoice && onClearDefaultVoice && (
                <button
                  onClick={handleClearDefault}
                  className="text-xs text-muted-foreground hover:text-foreground duration-300 flex items-center gap-1"
                  title="Clear default voice"
                >
                  <StarOff className="w-3 h-3" />
                  Clear Default
                </button>
              )}
            </div>
            <div className="flex items-center gap-2">
              {onRefresh && (
                <Button
                  onClick={onRefresh}
                  variant="outline"
                  size="sm"
                  className="px-3 py-2"
                  disabled={isLoading}
                >
                  <RefreshCw className={`w-4 h-4 ${isLoading ? 'animate-spin' : ''}`} />
                </Button>
              )}
              <Button
                onClick={() => setShowUploadModal(true)}
                className="bg-primary hover:bg-primary/90 text-primary-foreground px-4 py-2 rounded-lg text-sm flex items-center gap-2 font-semibold duration-300"
              >
                <Upload className="w-4 h-4" />
                Add Voice
              </Button>
            </div>
          </div>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="text-center py-8 text-muted-foreground">
              <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary mx-auto mb-2"></div>
              <p>Loading voices...</p>
            </div>
          ) : voices.length === 0 ? (
            <div className="text-center py-8 text-muted-foreground">
              <Upload className="mx-auto w-12 h-12 mb-2 opacity-50" />
              <p>No voices in library</p>
              <p className="text-sm">Upload voice samples to get started</p>
            </div>
          ) : (
            <div className="space-y-2 max-h-60 overflow-y-auto">
              {voices.map(voice => (
                <div
                  key={voice.id}
                  className={`p-3 border rounded-lg cursor-pointer duration-300 ${selectedVoice?.id === voice.id
                    ? 'border-primary bg-primary/10'
                    : 'border-border hover:border-accent-foreground/50 hover:bg-accent/50'
                    }`}
                  onClick={() => onVoiceSelect(voice)}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex-1 min-w-0">
                      {editingVoice === voice.id ? (
                        <div className="flex items-center gap-2">
                          <Input
                            type="text"
                            value={editName}
                            onChange={(e) => setEditName(e.target.value)}
                            className="flex-1 text-sm"
                            onClick={(e) => e.stopPropagation()}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter') handleSaveRename();
                              if (e.key === 'Escape') handleCancelRename();
                            }}
                            autoFocus
                          />
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              handleSaveRename();
                            }}
                            className="p-1 hover:bg-green-100 dark:hover:bg-green-900/30 text-green-600 dark:text-green-400 rounded transition-colors duration-300"
                          >
                            <Check className="w-4 h-4" />
                          </button>
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              handleCancelRename();
                            }}
                            className="p-1 hover:bg-destructive/10 text-destructive rounded transition-colors duration-300"
                          >
                            <X className="w-4 h-4" />
                          </button>
                        </div>
                      ) : (
                        <>
                          <div className="flex items-center gap-2">
                            <p className="font-medium text-foreground truncate">{voice.name}</p>
                            {defaultVoice === voice.name && (
                              <div className="flex-shrink-0" title="Default Voice">
                                <Crown className="w-4 h-4 text-yellow-500" />
                              </div>
                            )}
                          </div>
                          <div className="flex items-center gap-2 text-xs text-muted-foreground">
                            <span>{voice.uploadDate.toLocaleDateString()} • {(voice.file.size / 1024 / 1024).toFixed(1)}MB</span>
                            {voice.language && (
                              <div className="flex items-center gap-1 px-2 py-1 bg-accent/50 text-accent-foreground rounded-full">
                                {(getLanguageFlag(voice.language) && getLanguageFlag(voice.language) !== '') ? (
                                  <span className="text-xs font-medium">
                                    {getLanguageFlag(voice.language)}
                                  </span>
                                ) : (
                                  <Globe className="w-3 h-3" />
                                )}
                                <span className="text-xs font-medium">
                                  {getLanguageName(voice.language)}
                                </span>
                              </div>
                            )}
                          </div>

                          {/* Aliases display */}
                          {voice.aliases && voice.aliases.length > 0 && (
                            <div className="flex flex-wrap gap-1 mt-1">
                              {voice.aliases.map((alias, index) => (
                                <span
                                  key={index}
                                  className="inline-flex items-center gap-1 px-2 py-1 bg-primary/10 text-primary text-xs rounded-full"
                                  title={`Alias: ${alias}`}
                                >
                                  <Tag className="w-3 h-3" />
                                  {alias}
                                  {onRemoveAlias && (
                                    <button
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        handleRemoveAlias(voice.id, alias);
                                      }}
                                      className="ml-1 hover:bg-destructive/20 rounded-full p-0.5 duration-300 hover:text-destructive"
                                      title="Remove alias"
                                    >
                                      <X className="w-2.5 h-2.5" />
                                    </button>
                                  )}
                                </span>
                              ))}
                            </div>
                          )}

                          {/* Add alias input */}
                          {addingAliasFor === voice.id && (
                            <div className="flex items-center gap-2 mt-2">
                              <Input
                                type="text"
                                value={newAlias}
                                onChange={(e) => setNewAlias(e.target.value)}
                                placeholder="Enter alias..."
                                className="flex-1 text-xs h-8"
                                onClick={(e) => e.stopPropagation()}
                                onKeyDown={(e) => {
                                  if (e.key === 'Enter') handleSaveAlias();
                                  if (e.key === 'Escape') handleCancelAlias();
                                }}
                                autoFocus
                              />
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleSaveAlias();
                                }}
                                className="p-1 hover:bg-green-100 dark:hover:bg-green-900/30 text-green-600 dark:text-green-400 rounded transition-colors duration-300"
                                disabled={!newAlias.trim()}
                              >
                                <Check className="w-3 h-3" />
                              </button>
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleCancelAlias();
                                }}
                                className="p-1 hover:bg-destructive/10 text-destructive rounded transition-colors duration-300"
                              >
                                <X className="w-3 h-3" />
                              </button>
                            </div>
                          )}
                        </>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      {/* Default Voice Button - only show if we have the handler */}
                      {onSetDefaultVoice && defaultVoice !== voice.name && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleSetAsDefault(voice.name);
                          }}
                          className="p-1 hover:bg-yellow-100 dark:hover:bg-yellow-900/30 text-yellow-600 dark:text-yellow-400 rounded transition-colors duration-300"
                          title="Set as default voice"
                        >
                          <Star className="w-4 h-4" />
                        </button>
                      )}
                      {/* Add Alias Button - only show if we have the handler */}
                      {onAddAlias && addingAliasFor !== voice.id && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleAddAlias(voice.id);
                          }}
                          className="p-1 hover:bg-blue-100 dark:hover:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded transition-colors duration-300"
                          title="Add alias"
                        >
                          <Plus className="w-4 h-4" />
                        </button>
                      )}
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          playPreview(voice);
                        }}
                        className="p-1 hover:bg-accent rounded transition-colors duration-300"
                      >
                        {playingVoice === voice.id && currentAudioRef.current && !currentAudioRef.current.paused ? (
                          <Pause className="w-4 h-4 text-muted-foreground" />
                        ) : (
                          <Play className="w-4 h-4 text-muted-foreground" />
                        )}
                      </button>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          handleRename(voice.id, voice.name);
                        }}
                        className="p-1 hover:bg-accent rounded transition-colors duration-300"
                      >
                        <Edit2 className="w-4 h-4 text-muted-foreground" />
                      </button>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDeleteVoice(voice.id);
                        }}
                        className="p-1 hover:bg-destructive/10 text-destructive rounded transition-colors duration-300"
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Voice Upload Modal */}
      <VoiceUploadModal
        open={showUploadModal}
        onOpenChange={setShowUploadModal}
        onUpload={handleUploadVoice}
      />
    </>
  );
} 