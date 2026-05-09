import React from 'react';
import { Settings, RotateCcw } from 'lucide-react';
import { Slider } from '../ui/slider';
import { Card, CardContent } from '../ui/card';

interface AdvancedSettingsProps {
  showAdvanced: boolean;
  onToggle: () => void;
  exaggeration: number;
  onExaggerationChange: (value: number) => void;
  cfgWeight: number;
  onCfgWeightChange: (value: number) => void;
  temperature: number;
  onTemperatureChange: (value: number) => void;
  onResetToDefaults: () => void;
  isDefault: boolean;
}

export default function AdvancedSettings({
  showAdvanced,
  onToggle,
  exaggeration,
  onExaggerationChange,
  cfgWeight,
  onCfgWeightChange,
  temperature,
  onTemperatureChange,
  onResetToDefaults,
  isDefault
}: AdvancedSettingsProps) {
  return (
    <Card className="">
      <CardContent>
        <div className="flex items-center justify-between mb-4">
          <button
            onClick={onToggle}
            className="flex items-center gap-2 text-sm font-medium text-foreground hover:text-foreground/80 transition-colors duration-300"
          >
            <Settings className="w-4 h-4" />
            Advanced Settings
            <span className="text-xs bg-muted text-muted-foreground px-2 py-1 rounded">
              {showAdvanced ? 'Hide' : 'Show'}
            </span>
          </button>

          {showAdvanced && !isDefault && (
            <button
              onClick={onResetToDefaults}
              className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors duration-300"
            >
              <RotateCcw className="w-3 h-3" />
              Reset to Defaults
            </button>
          )}
        </div>

        {showAdvanced && (
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-foreground mb-1">
                Exaggeration: {exaggeration}
              </label>
              <Slider
                min={0.25}
                max={2.0}
                step={0.05}
                value={[exaggeration]}
                onValueChange={(values) => onExaggerationChange(values[0])}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground">Controls emotion intensity</p>
            </div>

            <div>
              <label className="block text-sm font-medium text-foreground mb-1">
                Pace (CFG Weight): {cfgWeight}
              </label>
              <Slider
                min={0.0}
                max={1.0}
                step={0.05}
                value={[cfgWeight]}
                onValueChange={(values) => onCfgWeightChange(values[0])}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground">Controls speech pace</p>
            </div>

            <div>
              <label className="block text-sm font-medium text-foreground mb-1">
                Temperature: {temperature}
              </label>
              <Slider
                min={0.05}
                max={2.0}
                step={0.05}
                value={[temperature]}
                onValueChange={(values) => onTemperatureChange(values[0])}
                className="w-full"
              />
              <p className="text-xs text-muted-foreground">Controls randomness/creativity</p>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
} 