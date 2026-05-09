import React, { useState, useRef, useCallback } from 'react';
import { Upload, X, Check, Loader2, Globe } from 'lucide-react';
import { Input } from './ui/input';
import { Button } from './ui/button';
import { Modal, ModalContent, ModalHeader, ModalTitle, ModalFooter } from './modal';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './ui/select';
import { LANGUAGE_OPTIONS, DEFAULT_LANGUAGE, getLanguageDisplayName } from '../constants/languages';

interface VoiceUploadModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onUpload: (file: File, customName?: string, language?: string) => Promise<void>;
}

type UploadState = 'idle' | 'uploading' | 'success' | 'error';

export default function VoiceUploadModal({ open, onOpenChange, onUpload }: VoiceUploadModalProps) {
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [customName, setCustomName] = useState('');
  const [selectedLanguage, setSelectedLanguage] = useState<string>(DEFAULT_LANGUAGE);
  const [uploadState, setUploadState] = useState<UploadState>('idle');
  const [isDragOver, setIsDragOver] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleClose = useCallback(() => {
    if (uploadState === 'uploading') return; // Prevent closing during upload

    setSelectedFile(null);
    setCustomName('');
    setSelectedLanguage(DEFAULT_LANGUAGE);
    setUploadState('idle');
    setIsDragOver(false);
    setErrorMessage('');
    onOpenChange(false);
  }, [uploadState, onOpenChange]);

  const handleFileSelect = useCallback((file: File) => {
    // Validate file type
    const allowedTypes = ['.mp3', '.wav', '.flac', '.m4a', '.ogg'];
    const fileExtension = '.' + file.name.split('.').pop()?.toLowerCase();

    if (!allowedTypes.includes(fileExtension)) {
      setErrorMessage('Please select a valid audio file (MP3, WAV, FLAC, M4A, OGG)');
      return;
    }

    // Validate file size (max 10MB)
    if (file.size > 10 * 1024 * 1024) {
      setErrorMessage('File size must be less than 10MB');
      return;
    }

    setSelectedFile(file);
    setErrorMessage('');

    // Auto-populate name from filename (without extension) if no custom name set
    if (!customName.trim()) {
      const nameWithoutExtension = file.name.replace(/\.[^/.]+$/, '');
      setCustomName(nameWithoutExtension);
    }
  }, [customName]);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);

    const files = Array.from(e.dataTransfer.files);
    if (files.length > 0) {
      handleFileSelect(files[0]); // Only handle first file for now
    }
  }, [handleFileSelect]);

  const handleFileInputChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0) {
      handleFileSelect(files[0]);
    }
  }, [handleFileSelect]);

  const handleUpload = useCallback(async () => {
    if (!selectedFile) return;

    setUploadState('uploading');
    setErrorMessage('');

    try {
      await onUpload(selectedFile, customName.trim() || undefined, selectedLanguage);
      setUploadState('success');

      // Auto-close after success
      setTimeout(() => {
        handleClose();
      }, 1500);
    } catch (error) {
      setUploadState('error');

      // Extract error message from API response
      let errorMsg = 'Upload failed. Please try again.';
      if (error instanceof Error) {
        errorMsg = error.message;
      }
      // Handle axios errors with response data
      if (error && typeof error === 'object' && 'response' in error) {
        const axiosError = error as any;
        if (axiosError.response?.data?.error?.message) {
          errorMsg = axiosError.response.data.error.message;
        }
      }

      setErrorMessage(errorMsg);
    }
  }, [selectedFile, customName, selectedLanguage, onUpload, handleClose]);

  const handleClickUpload = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const renderDropZone = () => {
    if (selectedFile && uploadState !== 'idle') {
      return (
        <div className="border-2 border-dashed border-border rounded-lg p-8 text-center">
          <div className="flex flex-col items-center gap-4">
            {uploadState === 'uploading' && (
              <>
                <Loader2 className="w-12 h-12 text-primary animate-spin" />
                <div className="space-y-2">
                  <p className="text-lg font-medium text-foreground">Uploading...</p>
                  <p className="text-sm text-muted-foreground">{selectedFile.name}</p>
                </div>
              </>
            )}

            {uploadState === 'success' && (
              <>
                <div className="w-12 h-12 bg-green-100 dark:bg-green-900/30 rounded-full flex items-center justify-center">
                  <Check className="w-6 h-6 text-green-600 dark:text-green-400" />
                </div>
                <div className="space-y-2">
                  <p className="text-lg font-medium text-foreground">Upload Complete!</p>
                  <p className="text-sm text-muted-foreground">Voice "{customName}" has been added to your library</p>
                </div>
              </>
            )}

            {uploadState === 'error' && (
              <>
                <div className="w-12 h-12 bg-destructive/10 rounded-full flex items-center justify-center">
                  <X className="w-6 h-6 text-destructive" />
                </div>
                <div className="space-y-2">
                  <p className="text-lg font-medium text-foreground">Upload Failed</p>
                  <p className="text-sm text-destructive">{errorMessage}</p>
                </div>
              </>
            )}
          </div>
        </div>
      );
    }

    return (
      <div
        className={`border-2 border-dashed rounded-lg p-8 text-center cursor-pointer transition-colors duration-300 ${isDragOver
          ? 'border-primary bg-primary/5'
          : 'border-border hover:border-primary hover:bg-primary/5'
          }`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={handleClickUpload}
      >
        <Upload className="mx-auto w-12 h-12 text-muted-foreground mb-4" />
        {selectedFile ? (
          <div className="space-y-2">
            <p className="text-lg font-medium text-foreground">Selected: {selectedFile.name}</p>
            <p className="text-sm text-muted-foreground">
              {(selectedFile.size / 1024 / 1024).toFixed(1)}MB • Click to change
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            <p className="text-lg font-medium text-foreground">
              {isDragOver ? 'Drop your voice file here' : 'Drag & drop your voice file'}
            </p>
            <p className="text-sm text-muted-foreground">
              Or click to browse • MP3, WAV, FLAC, M4A, OGG (max 10MB)
            </p>
          </div>
        )}
      </div>
    );
  };

  return (
    <>
      <Modal open={open} onOpenChange={handleClose}>
        <ModalContent className="">
          <ModalHeader>
            <ModalTitle>Add Voice to Library</ModalTitle>
          </ModalHeader>

          <div className="p-6 pt-0 space-y-6">
            {/* Upload Area */}
            {renderDropZone()}

            {/* Error Message */}
            {errorMessage && uploadState === 'idle' && (
              <div className="text-sm text-destructive bg-destructive/10 rounded p-3">
                {errorMessage}
              </div>
            )}

            {/* Name Input */}
            {selectedFile && uploadState === 'idle' && (
              <div className="space-y-4">
                <div className="space-y-2">
                  <label className="text-sm font-medium text-foreground">
                    Voice Name
                  </label>
                  <Input
                    type="text"
                    value={customName}
                    onChange={(e) => setCustomName(e.target.value)}
                    placeholder="Enter a name for this voice..."
                    className="w-full"
                  />
                  <p className="text-xs text-muted-foreground">
                    This name will appear in your voice library
                  </p>
                </div>

                {/* Language Selection */}
                <div className="space-y-2">
                  <label className="text-sm font-medium text-foreground flex items-center gap-2">
                    <Globe className="w-4 h-4" />
                    Language
                  </label>
                  <Select value={selectedLanguage} onValueChange={setSelectedLanguage}>
                    <SelectTrigger className="w-full">
                      <SelectValue placeholder="Select language" />
                    </SelectTrigger>
                    <SelectContent>
                      {LANGUAGE_OPTIONS.map((option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">
                    Select the language this voice will be used for
                  </p>
                </div>
              </div>
            )}
          </div>

          {uploadState === 'idle' && (
            <ModalFooter>
              <Button
                variant="outline"
                onClick={handleClose}
              >
                Cancel
              </Button>
              <Button
                onClick={handleUpload}
                disabled={!selectedFile || !customName.trim()}
              >
                Add Voice
              </Button>
            </ModalFooter>
          )}
        </ModalContent>
      </Modal>

      {/* Hidden file input */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".mp3,.wav,.flac,.m4a,.ogg"
        onChange={handleFileInputChange}
        className="hidden"
      />
    </>
  );
} 