import React from 'react';
import { X, Zap, Volume2 } from 'lucide-react';
import { Textarea } from '../ui/textarea';
import { Card, CardContent } from '../ui/card';

interface TextInputProps {
  value: string;
  onChange: (value: string) => void;
  onClear: () => void;
  hasText: boolean;
  maxLength?: number;
  placeholder?: string;
  isStreamingEnabled?: boolean;
  onToggleStreaming?: () => void;
}

export default function TextInput({
  value,
  onChange,
  onClear,
  hasText,
  maxLength = 3000,
  placeholder = "Enter the text you want to convert to speech...",
  isStreamingEnabled = false,
  onToggleStreaming
}: TextInputProps) {
  return (
    <Card className="">
      <CardContent>
        <div className="flex items-center justify-between mb-2">
          <label className="block text-sm font-medium text-foreground">
            Text to Convert
          </label>
          <div className="flex items-center gap-2">
            {/* Streaming Toggle */}
            {onToggleStreaming && (
              <button
                onClick={onToggleStreaming}
                className={`flex items-center gap-1 text-xs px-2 py-1 rounded-md transition-colors duration-200 ${isStreamingEnabled
                  ? 'bg-primary/20 text-primary border border-primary/30'
                  : 'bg-muted text-muted-foreground hover:bg-muted/80 border border-border'
                  }`}
                title={isStreamingEnabled ? 'Streaming enabled - audio will play in real-time' : 'Enable streaming for real-time audio'}
              >
                {isStreamingEnabled ? <Zap className="w-3 h-3" /> : <Volume2 className="w-3 h-3" />}
                {isStreamingEnabled ? 'Streaming' : 'Standard'}
              </button>
            )}
            {/* Clear Button */}
            {hasText && (
              <button
                onClick={onClear}
                className="flex items-center gap-1 text-xs text-muted-foreground hover:text-destructive duration-300"
                title="Clear text"
              >
                <X className="w-3 h-3" />
                Clear
              </button>
            )}
          </div>
        </div>

        {/* Streaming Info */}
        {isStreamingEnabled && onToggleStreaming && (
          <div className="mb-2 p-2 bg-primary/5 border border-primary/20 rounded-md">
            <div className="flex items-start gap-2">
              <Zap className="w-4 h-4 text-primary mt-0.5 flex-shrink-0" />
              <div className="text-xs text-primary">
                <div className="font-medium">Streaming Mode Enabled</div>
                <div className="text-primary/70 mt-1">
                  Audio will play in real-time as it's generated. You'll also get a downloadable file when complete.
                </div>
              </div>
            </div>
          </div>
        )}

        <Textarea
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="h-32"
          placeholder={placeholder}
        // maxLength={maxLength}
        />
        <div className="text-right text-sm text-muted-foreground mt-1">
          {value.length}/{maxLength} characters
        </div>
      </CardContent>
    </Card>
  );
} 