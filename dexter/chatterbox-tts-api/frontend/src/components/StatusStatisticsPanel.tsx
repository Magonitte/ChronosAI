import React from 'react';
import { BarChart3, Clock, CheckCircle, XCircle, Activity, MonitorSpeaker, HardDrive } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from './ui/card';
import { ChartContainer, ChartTooltip, ChartTooltipContent } from './ui/chart';
import { Bar, BarChart, Cell, Pie, PieChart, ResponsiveContainer } from 'recharts';
import type { TTSStatistics } from '../types';

interface StatusStatisticsPanelProps {
  statistics: TTSStatistics | undefined;
  isLoading: boolean;
  hasError: boolean;
}

export default function StatusStatisticsPanel({
  statistics,
  isLoading,
  hasError
}: StatusStatisticsPanelProps) {

  if (hasError) {
    return (
      <Card>
        <CardContent className="p-4">
          <div className="flex items-center gap-2 text-destructive">
            <XCircle className="w-4 h-4" />
            <span className="text-sm">Failed to load statistics</span>
          </div>
        </CardContent>
      </Card>
    );
  }

  if (isLoading || !statistics) {
    return (
      <Card>
        <CardContent className="p-4">
          <div className="flex items-center gap-2 text-muted-foreground">
            <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-primary" />
            <span className="text-sm">Loading statistics...</span>
          </div>
        </CardContent>
      </Card>
    );
  }

  const formatMemory = (mb: number) => {
    if (mb >= 1024) {
      return `${(mb / 1024).toFixed(1)}GB`;
    }
    return `${mb.toFixed(0)}MB`;
  };

  const formatDuration = (seconds: number) => {
    if (seconds < 60) {
      return `${seconds.toFixed(1)}s`;
    }
    return `${(seconds / 60).toFixed(1)}m`;
  };

  // Chart data preparation
  const successRateData = [
    {
      name: 'Completed',
      value: statistics.completed_requests,
      fill: 'var(--color-chart-2)'
    },
    {
      name: 'Errors',
      value: statistics.error_requests,
      fill: 'var(--color-chart-5)'
    }
  ];

  const memoryData = statistics.current_memory ? [
    {
      name: 'CPU',
      value: statistics.current_memory.cpu_memory_mb,
      fill: 'var(--color-chart-1)'
    },
    {
      name: 'GPU',
      value: statistics.current_memory.gpu_memory_allocated_mb,
      fill: 'var(--color-chart-3)'
    }
  ] : [];

  const chartConfig = {
    completed: {
      label: "Completed",
      color: "var(--color-chart-2)",
    },
    errors: {
      label: "Errors",
      color: "var(--color-chart-5)",
    },
    cpu: {
      label: "CPU Memory",
      color: "var(--color-chart-1)",
    },
    gpu: {
      label: "GPU Memory",
      color: "var(--color-chart-3)",
    },
  };

  return (
    <Card className="w-full max-w-4xl">
      <CardHeader className="pb-3">
        <CardTitle className="text-sm font-medium flex items-center gap-2">
          <BarChart3 className="w-4 h-4" />
          Processing Statistics
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Overview Stats */}
        <div className="grid grid-cols-2 gap-4">
          <div className="space-y-1">
            <div className="text-xs text-muted-foreground">Total Requests</div>
            <div className="text-xl font-bold text-foreground">
              {statistics.total_requests}
            </div>
          </div>
          <div className="space-y-1">
            <div className="text-xs text-muted-foreground">Success Rate</div>
            <div className="text-xl font-bold text-foreground">
              {statistics.success_rate.toFixed(1)}%
            </div>
          </div>
        </div>

        {/* Success Rate Chart */}
        {statistics.total_requests > 0 && (statistics.completed_requests > 0 || statistics.error_requests > 0) && (
          <div className="space-y-2">
            <div className="text-xs text-muted-foreground font-medium">Request Breakdown</div>
            <div className="flex items-center gap-4">
              <ChartContainer config={chartConfig} className="h-24 w-24 flex-shrink-0">
                <PieChart>
                  <Pie
                    data={successRateData}
                    dataKey="value"
                    nameKey="name"
                    cx="50%"
                    cy="50%"
                    innerRadius={20}
                    outerRadius={40}
                    strokeWidth={2}
                  >
                    {successRateData.map((entry, index) => (
                      <Cell key={`cell-${index}`} fill={entry.fill} />
                    ))}
                  </Pie>
                  <ChartTooltip content={<ChartTooltipContent />} />
                </PieChart>
              </ChartContainer>
              <div className="flex-1 space-y-2">
                <div className="flex items-center gap-2">
                  <CheckCircle className="w-4 h-4 text-green-600 dark:text-green-400" />
                  <div>
                    <div className="text-xs text-muted-foreground">Completed</div>
                    <div className="text-sm font-medium text-foreground">
                      {statistics.completed_requests}
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <XCircle className="w-4 h-4 text-destructive" />
                  <div>
                    <div className="text-xs text-muted-foreground">Errors</div>
                    <div className="text-sm font-medium text-foreground">
                      {statistics.error_requests}
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Performance Metrics */}
        <div className="border-t pt-3">
          <div className="flex items-center gap-2 mb-2">
            <Clock className="w-4 h-4 text-muted-foreground" />
            <span className="text-xs text-muted-foreground font-medium">Performance</span>
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1">
              <div className="text-xs text-muted-foreground">Avg Duration</div>
              <div className="text-sm font-medium text-foreground">
                {formatDuration(statistics.average_duration_seconds)}
              </div>
            </div>
            <div className="space-y-1">
              <div className="text-xs text-muted-foreground">Avg Text Length</div>
              <div className="text-sm font-medium text-foreground">
                {Math.round(statistics.average_text_length)} chars
              </div>
            </div>
          </div>
        </div>

        {/* Memory Usage Chart */}
        {statistics.current_memory && memoryData.length > 0 && (
          <div className="border-t pt-3">
            <div className="flex items-center gap-2 mb-3">
              <MonitorSpeaker className="w-4 h-4 text-muted-foreground" />
              <span className="text-xs text-muted-foreground font-medium">Memory Usage</span>
            </div>
            <ChartContainer config={chartConfig} className="h-20 w-full">
              <BarChart data={memoryData} layout="horizontal">
                <Bar dataKey="value" radius={2} />
                <ChartTooltip
                  content={<ChartTooltipContent
                    formatter={(value, name) => [formatMemory(value as number), name]}
                  />}
                />
              </BarChart>
            </ChartContainer>
            <div className="flex items-center justify-between text-xs text-muted-foreground mt-2">
              <div className="flex items-center gap-1">
                <div className="w-2 h-2 rounded-full bg-[hsl(var(--chart-1))]" />
                <span>CPU: {formatMemory(statistics.current_memory.cpu_memory_mb)}</span>
              </div>
              <div className="flex items-center gap-1">
                <div className="w-2 h-2 rounded-full bg-[hsl(var(--chart-3))]" />
                <span>GPU: {formatMemory(statistics.current_memory.gpu_memory_allocated_mb)}</span>
              </div>
            </div>
          </div>
        )}

        {/* Processing Status */}
        <div className="border-t pt-3">
          <div className="flex items-center gap-2">
            <div className={`w-2 h-2 rounded-full ${statistics.is_processing
              ? 'bg-green-500 animate-pulse'
              : 'bg-muted-foreground'
              }`} />
            <span className="text-xs text-muted-foreground">
              {statistics.is_processing ? 'Processing active' : 'Idle'}
            </span>
          </div>
        </div>
      </CardContent>
    </Card>
  );
} 