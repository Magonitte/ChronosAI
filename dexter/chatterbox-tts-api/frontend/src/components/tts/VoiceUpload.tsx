import React, { useRef } from 'react';
import { Upload, Mic } from 'lucide-react';
import { Card, CardContent } from '../ui/card';

interface VoiceUploadProps {
  voiceFile: File | null;
  onFileChange: (file: File | null) => void;
}

export default function VoiceUpload({ voiceFile, onFileChange }: VoiceUploadProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleFileUpload = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    onFileChange(file || null);
  };

  const handleClick = () => {
    fileInputRef.current?.click();
  };

  return (
    <Card className="mb-6">
      <CardContent>
        <label className="block text-sm font-medium text-foreground mb-2">
          <Mic className="inline w-4 h-4 mr-1" />
          Custom Voice (Optional)
        </label>
        <div
          className="border-2 border-dashed border-border rounded-lg p-4 text-center cursor-pointer hover:border-primary transition-colors duration-300"
          onClick={handleClick}
        >
          <Upload className="mx-auto w-8 h-8 text-muted-foreground mb-2" />
          {voiceFile ? (
            <div>
              <p className="text-sm text-foreground">Selected: {voiceFile.name}</p>
              <p className="text-xs text-muted-foreground">Click to change</p>
            </div>
          ) : (
            <div>
              <p className="text-sm text-foreground">Click to upload your voice sample</p>
              <p className="text-xs text-muted-foreground">MP3, WAV, FLAC, M4A, OGG (max 10MB)</p>
            </div>
          )}
        </div>
        <input
          ref={fileInputRef}
          type="file"
          accept=".mp3,.wav,.flac,.m4a,.ogg"
          onChange={handleFileUpload}
          className="hidden"
        />
      </CardContent>
    </Card>
  );
} 