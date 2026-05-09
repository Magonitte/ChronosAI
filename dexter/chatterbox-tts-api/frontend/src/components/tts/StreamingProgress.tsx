import React from 'react';
import { Zap, Download, X, Pause, Info } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '../ui/card';
import { Button } from '../ui/button';
import type { StreamingProgress, AudioInfo } from '../../types';

interface StreamingProgressProps {
  isStreaming: boolean;
  progress: StreamingProgress | null;
  audioUrl: string | null;
  error: string | null;
  audioInfo: AudioInfo | null;
  onStop?: () => void;
  onClear?: () => void;
}

export default function StreamingProgressComponent({
  isStreaming,
  progress,
  audioUrl,
  error,
  audioInfo,
  onStop,
  onClear
}: StreamingProgressProps) {
  if (!isStreaming && !progress && !audioUrl && !error) {
    return null;
  }

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  const downloadAudio = () => {
    if (!audioUrl) return;

    const link = document.createElement('a');
    link.href = audioUrl;
    link.download = `chatterbox-streaming-${Date.now()}.wav`;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
  };

  // Calculate total audio duration estimate
  const getAudioDurationEstimate = () => {
    if (!progress?.totalBytes) return null;

    if (audioInfo) {
      const { sample_rate, channels, bits_per_sample } = audioInfo;
      const byteRate = sample_rate * channels * (bits_per_sample / 8);
      if (byteRate > 0) {
        return (progress.totalBytes / byteRate).toFixed(1);
      }
    }

    // Fallback estimate: WAV at 16kHz, 16-bit = ~32KB per second
    const estimatedSeconds = progress.totalBytes / (32 * 1024);
    return estimatedSeconds.toFixed(1);
  };

  return (
    <Card className="w-full">
      <CardContent className="">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            <Zap className={`w-4 h-4 ${isStreaming ? 'text-primary animate-pulse' : 'text-muted-foreground'}`} />
            <span className="text-sm font-medium">
              {isStreaming ? 'Streaming Audio...' : progress?.isComplete ? 'Streaming Complete' : 'Streaming Status'}
            </span>
            {progress?.chunksReceived && progress.chunksReceived > 1 && (
              <span className="text-xs bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 px-2 py-1 rounded">
                Multi-chunk
              </span>
            )}
          </div>
          <div className="flex items-center gap-2">
            {isStreaming && onStop && (
              <Button
                onClick={onStop}
                variant="outline"
                size="sm"
                className="h-7 px-2 text-xs"
              >
                <Pause className="w-3 h-3 mr-1" />
                Stop
              </Button>
            )}
            {audioUrl && (
              <Button
                onClick={downloadAudio}
                variant="outline"
                size="sm"
                className="h-7 px-2 text-xs"
              >
                <Download className="w-3 h-3 mr-1" />
                Download
              </Button>
            )}
            {onClear && !isStreaming && (
              <Button
                onClick={onClear}
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs text-muted-foreground hover:text-destructive"
              >
                <X className="w-3 h-3" />
              </Button>
            )}
          </div>
        </div>

        {/* Error Display */}
        {error && (
          <div className="mb-3 p-2 bg-destructive/10 border border-destructive/20 rounded-md">
            <div className="text-xs text-destructive font-medium">Streaming Error</div>
            <div className="text-xs text-destructive/80 mt-1">{error}</div>
          </div>
        )}

        {/* Progress Information */}
        {progress && (
          <div className="space-y-2">
            <div className="flex justify-between text-xs text-muted-foreground">
              <span>Chunks: {progress.chunksReceived}</span>
              <span>Size: {formatBytes(progress.totalBytes)}</span>
              {getAudioDurationEstimate() && (
                <span>~{getAudioDurationEstimate()}s</span>
              )}
            </div>

            {(progress.currentChunk !== undefined && progress.totalChunks !== undefined) && (
              <div className="space-y-1">
                <div className="flex justify-between text-xs text-muted-foreground">
                  <span>Progress: {progress.currentChunk}/{progress.totalChunks}</span>
                  <span>{Math.round((progress.currentChunk / progress.totalChunks) * 100)}%</span>
                </div>
                <div className="w-full bg-muted rounded-full h-1.5">
                  <div
                    className="bg-primary h-1.5 rounded-full transition-all duration-300"
                    style={{ width: `${(progress.currentChunk / progress.totalChunks) * 100}%` }}
                  />
                </div>
              </div>
            )}

            {/* Real-time streaming status */}
            {isStreaming && (
              <div className="flex items-center gap-2 text-xs">
                <div className="flex items-center gap-1 text-primary/70">
                  <div className="w-2 h-2 bg-primary rounded-full animate-pulse"></div>
                  <span>Playing in real-time...</span>
                </div>
                {progress.chunksReceived > 1 && (
                  <span className="text-green-600 dark:text-green-400">
                    ✓ Multi-chunk streaming active
                  </span>
                )}
              </div>
            )}

            {progress.isComplete && !isStreaming && (
              <div className="space-y-1">
                <div className="text-xs text-green-600 dark:text-green-400">
                  ✓ Streaming completed successfully
                </div>
                {progress.chunksReceived > 1 && (
                  <div className="text-xs text-blue-600 dark:text-blue-400">
                    <Info className="w-3 h-3 inline mr-1" />
                    {progress.chunksReceived} chunks merged into final audio
                  </div>
                )}
              </div>
            )}
          </div>
        )}

        {/* Final Audio Player */}
        {audioUrl && !isStreaming && (
          <div className="mt-3 pt-3 border-t">
            <div className="text-xs text-muted-foreground mb-2">
              Final Audio
              {progress?.chunksReceived && progress.chunksReceived > 1 && (
                <span className="text-blue-600 dark:text-blue-400">
                  {' '}({progress.chunksReceived} chunks merged)
                </span>
              )}
            </div>
            <audio controls className="w-full h-8">
              <source src={audioUrl} type="audio/wav" />
              Your browser does not support the audio element.
            </audio>
          </div>
        )}
      </CardContent>
    </Card>
  );
} 