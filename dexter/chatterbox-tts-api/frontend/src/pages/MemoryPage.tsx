import React, { useState, useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  MemoryStick,
  Trash2,
  RefreshCw,
  AlertTriangle,
  MonitorSpeaker,
  HardDrive,
  Cpu,
  Zap,
  TrendingUp,
  TrendingDown,
  Activity,
  Settings,
  Lightbulb,
  Shield,
  Info
} from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card';
import { Button } from '../components/ui/button';
import { ChartContainer, ChartTooltip, ChartTooltipContent } from '../components/ui/chart';
import { Bar, BarChart, Line, LineChart, ResponsiveContainer, XAxis, YAxis } from 'recharts';
import { createTTSService } from '../services/tts';
import { useApiEndpoint } from '../hooks/useApiEndpoint';

interface MemoryInfo {
  cpu_memory_mb: number;
  cpu_memory_percent: number;
  gpu_memory_allocated_mb: number;
  gpu_memory_reserved_mb: number;
  gpu_memory_max_allocated_mb: number;
}

interface MemoryAlert {
  type: string;
  level: 'info' | 'warning' | 'error';
  message: string;
  threshold?: number;
  current?: number;
  suggestion?: string;
}

interface MemoryResponse {
  memory_info: MemoryInfo;
  request_counter: number;
  cleanup_performed: boolean;
  cuda_cache_cleared: boolean;
  collected_objects?: number;
  memory_info_after_cleanup?: MemoryInfo;
  cleanup_error?: string;
  alerts?: {
    has_alerts: boolean;
    alert_count: number;
    alerts: MemoryAlert[];
  };
}

interface MemoryRecommendation {
  type: string;
  priority: 'low' | 'medium' | 'high';
  title: string;
  description: string;
  actions: string[];
}

export default function MemoryPage() {
  const { apiBaseUrl } = useApiEndpoint();
  const queryClient = useQueryClient();
  const [memoryHistory, setMemoryHistory] = useState<Array<{ timestamp: string; cpu: number; gpu: number }>>([]);
  const [showRecommendations, setShowRecommendations] = useState(false);

  const ttsService = createTTSService(apiBaseUrl);

  // Real-time memory monitoring with alerts
  const { data: memoryData, isLoading, error, refetch } = useQuery<MemoryResponse>({
    queryKey: ['memory-info', apiBaseUrl],
    queryFn: () => ttsService.getMemoryWithAlerts(true),
    refetchInterval: 2000, // Update every 2 seconds
  });

  // Memory configuration
  const { data: memoryConfig } = useQuery({
    queryKey: ['memory-config', apiBaseUrl],
    queryFn: () => ttsService.getMemoryConfig(),
    refetchInterval: 30000, // Update every 30 seconds
  });

  // Memory recommendations
  const { data: recommendations, refetch: refetchRecommendations } = useQuery({
    queryKey: ['memory-recommendations', apiBaseUrl],
    queryFn: () => ttsService.getMemoryRecommendations(),
    enabled: showRecommendations,
    refetchInterval: showRecommendations ? 15000 : false, // Update every 15 seconds when visible
  });

  // Track memory history when data updates
  useEffect(() => {
    if (memoryData) {
      const now = new Date().toLocaleTimeString();
      setMemoryHistory(prev => {
        const newEntry = {
          timestamp: now,
          cpu: memoryData.memory_info.cpu_memory_mb,
          gpu: memoryData.memory_info.gpu_memory_allocated_mb || 0
        };

        // Keep last 20 entries
        const updated = [...prev, newEntry].slice(-20);
        return updated;
      });
    }
  }, [memoryData]);

  // Memory cleanup mutation
  const cleanupMutation = useMutation({
    mutationFn: async ({ forceCuda }: { forceCuda: boolean }) => {
      const response = await fetch(`${apiBaseUrl}/memory?cleanup=true&force_cuda_clear=${forceCuda}`, {
        method: 'GET'
      });
      if (!response.ok) {
        throw new Error(`Cleanup failed: ${response.status}`);
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['memory-info'] });
      queryClient.invalidateQueries({ queryKey: ['memory-recommendations'] });
    }
  });

  // Memory reset mutation
  const resetMutation = useMutation({
    mutationFn: async () => {
      const response = await fetch(`${apiBaseUrl}/memory/reset?confirm=true`, {
        method: 'POST'
      });
      if (!response.ok) {
        throw new Error(`Reset failed: ${response.status}`);
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['memory-info'] });
      queryClient.invalidateQueries({ queryKey: ['memory-recommendations'] });
      setMemoryHistory([]); // Clear history on reset
    }
  });

  const formatMemory = (mb: number) => {
    if (mb >= 1024) {
      return `${(mb / 1024).toFixed(1)}GB`;
    }
    return `${mb.toFixed(0)}MB`;
  };

  const getMemoryTrend = (values: number[]) => {
    if (values.length < 2) return 'stable';
    const recent = values.slice(-3);
    const avg = recent.reduce((a, b) => a + b, 0) / recent.length;
    const previous = values.slice(-6, -3);
    const prevAvg = previous.length > 0 ? previous.reduce((a, b) => a + b, 0) / previous.length : avg;

    if (avg > prevAvg * 1.1) return 'increasing';
    if (avg < prevAvg * 0.9) return 'decreasing';
    return 'stable';
  };

  const getAlertIcon = (level: string) => {
    switch (level) {
      case 'error': return <AlertTriangle className="w-4 h-4 text-red-500" />;
      case 'warning': return <AlertTriangle className="w-4 h-4 text-yellow-500" />;
      default: return <Info className="w-4 h-4 text-blue-500" />;
    }
  };

  const getPriorityColor = (priority: string) => {
    switch (priority) {
      case 'high': return 'text-red-600 dark:text-red-400';
      case 'medium': return 'text-yellow-600 dark:text-yellow-400';
      default: return 'text-blue-600 dark:text-blue-400';
    }
  };

  const cpuTrend = getMemoryTrend(memoryHistory.map(h => h.cpu));
  const gpuTrend = getMemoryTrend(memoryHistory.map(h => h.gpu));

  if (error) {
    return (
      <div className="container mx-auto px-4 py-8">
        <Card>
          <CardContent className="p-8 text-center">
            <AlertTriangle className="w-12 h-12 text-destructive mx-auto mb-4" />
            <h2 className="text-xl font-semibold text-destructive mb-2">Memory Monitoring Unavailable</h2>
            <p className="text-muted-foreground mb-4">
              Failed to connect to memory management endpoints.
            </p>
            <Button onClick={() => refetch()} variant="outline">
              <RefreshCw className="w-4 h-4 mr-2" />
              Retry Connection
            </Button>
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="container mx-auto px-4 py-8 space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center flex-col md:flex-row gap-2">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl md:text-3xl font-bold text-center md:text-left">Memory Management</h1>
          <p className="text-muted-foreground text-sm md:text-base text-center md:text-left">Monitor and manage backend memory usage</p>
        </div>
        <div className="flex gap-2 flex-col sm:flex-row">
          <Button
            onClick={() => {
              setShowRecommendations(!showRecommendations);
              if (!showRecommendations) {
                refetchRecommendations();
              }
            }}
            variant="outline"
            size="sm"
          >
            <Lightbulb className="w-4 h-4 mr-2" />
            {showRecommendations ? 'Hide' : 'Show'} Recommendations
          </Button>
          <Button
            onClick={() => refetch()}
            variant="outline"
            size="sm"
            disabled={isLoading}
          >
            <RefreshCw className={`w-4 h-4 mr-2 ${isLoading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      {/* Memory Alerts */}
      {memoryData?.alerts?.has_alerts && (
        <Card className="border-yellow-200 dark:border-yellow-800">
          <CardHeader>
            <CardTitle className="text-lg flex items-center gap-2 text-yellow-700 dark:text-yellow-300">
              <Shield className="w-5 h-5" />
              Memory Alerts ({memoryData.alerts.alert_count})
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              {memoryData.alerts.alerts.map((alert, index) => (
                <div key={index} className="flex items-start gap-3 p-3 bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 rounded-md">
                  {getAlertIcon(alert.level)}
                  <div className="flex-1">
                    <p className="text-sm font-medium text-yellow-800 dark:text-yellow-200">
                      {alert.message}
                    </p>
                    {alert.suggestion && (
                      <p className="text-xs text-yellow-700 dark:text-yellow-300 mt-1">
                        {alert.suggestion}
                      </p>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      {/* Current Memory Status */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">CPU Memory</CardTitle>
            <Cpu className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {memoryData ? formatMemory(memoryData.memory_info.cpu_memory_mb) : '...'}
            </div>
            <div className="flex items-center gap-1 text-xs text-muted-foreground">
              {cpuTrend === 'increasing' && <TrendingUp className="w-3 h-3 text-red-500" />}
              {cpuTrend === 'decreasing' && <TrendingDown className="w-3 h-3 text-green-500" />}
              {cpuTrend === 'stable' && <Activity className="w-3 h-3" />}
              <span className="capitalize">{cpuTrend}</span>
              {memoryData && (
                <span className="ml-1">({memoryData.memory_info.cpu_memory_percent.toFixed(1)}%)</span>
              )}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">GPU Memory</CardTitle>
            <MonitorSpeaker className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {memoryData ? formatMemory(memoryData.memory_info.gpu_memory_allocated_mb || 0) : '...'}
            </div>
            <div className="flex items-center gap-1 text-xs text-muted-foreground">
              {gpuTrend === 'increasing' && <TrendingUp className="w-3 h-3 text-red-500" />}
              {gpuTrend === 'decreasing' && <TrendingDown className="w-3 h-3 text-green-500" />}
              {gpuTrend === 'stable' && <Activity className="w-3 h-3" />}
              <div className="flex flex-row justify-between w-full">
                <span className="capitalize">{gpuTrend}</span>
                {memoryData?.memory_info.gpu_memory_reserved_mb && (
                  <span className="ml-1 flex-1 text-right">
                    Reserved: {formatMemory(memoryData.memory_info.gpu_memory_reserved_mb)}
                  </span>
                )}
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">Request Counter</CardTitle>
            <Zap className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {memoryData ? memoryData.request_counter : '...'}
            </div>
            <p className="text-xs text-muted-foreground">
              Total processed requests
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">GPU Peak</CardTitle>
            <HardDrive className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {memoryData ? formatMemory(memoryData.memory_info.gpu_memory_max_allocated_mb || 0) : '...'}
            </div>
            <p className="text-xs text-muted-foreground">
              Maximum allocated
            </p>
          </CardContent>
        </Card>
      </div>

      {/* Memory Recommendations */}
      {showRecommendations && recommendations && (
        <Card>
          <CardHeader>
            <CardTitle className="text-lg flex items-center gap-2">
              <Lightbulb className="w-5 h-5" />
              Memory Optimization Recommendations
              {recommendations.has_critical_issues && (
                <span className="ml-2 px-2 py-1 text-xs bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300 rounded">
                  Critical Issues
                </span>
              )}
            </CardTitle>
          </CardHeader>
          <CardContent>
            {recommendations.recommendation_count === 0 ? (
              <p className="text-muted-foreground text-center py-4">
                ✓ No recommendations at this time. Memory usage is optimal.
              </p>
            ) : (
              <div className="space-y-4">
                {recommendations.recommendations.map((rec: MemoryRecommendation, index: number) => (
                  <div key={index} className="p-4 border border-border rounded-lg">
                    <div className="flex items-start gap-3">
                      <div className={`font-medium text-sm ${getPriorityColor(rec.priority)}`}>
                        {rec.priority.toUpperCase()}
                      </div>
                      <div className="flex-1">
                        <h4 className="font-medium text-sm">{rec.title}</h4>
                        <p className="text-sm text-muted-foreground mt-1">{rec.description}</p>
                        <ul className="mt-2 space-y-1">
                          {rec.actions.map((action, actionIndex) => (
                            <li key={actionIndex} className="text-xs text-muted-foreground flex items-start gap-1">
                              <span>•</span>
                              <span>{action}</span>
                            </li>
                          ))}
                        </ul>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {/* Memory Trend Chart */}
      {memoryHistory.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-lg flex items-center gap-2">
              <Activity className="w-5 h-5" />
              Memory Usage Trends
            </CardTitle>
          </CardHeader>
          <CardContent>
            <ChartContainer config={{
              cpu: { label: "CPU Memory", color: "var(--color-chart-1)" },
              gpu: { label: "GPU Memory", color: "var(--color-chart-3)" }
            }} className="h-48">
              <LineChart data={memoryHistory}>
                <XAxis
                  dataKey="timestamp"
                  tick={{ fontSize: 10 }}
                  interval="preserveStartEnd"
                />
                <YAxis tick={{ fontSize: 10 }} />
                <Line
                  type="monotone"
                  dataKey="cpu"
                  stroke="var(--color-chart-1)"
                  strokeWidth={2}
                  dot={false}
                />
                <Line
                  type="monotone"
                  dataKey="gpu"
                  stroke="var(--color-chart-3)"
                  strokeWidth={2}
                  dot={false}
                />
                <ChartTooltip
                  content={<ChartTooltipContent
                    formatter={(value, name) => [formatMemory(value as number), name]}
                  />}
                />
              </LineChart>
            </ChartContainer>
          </CardContent>
        </Card>
      )}

      {/* Memory Operations */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Cleanup Operations */}
        <Card>
          <CardHeader>
            <CardTitle className="text-lg flex items-center gap-2">
              <Trash2 className="w-5 h-5" />
              Memory Cleanup
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <p className="text-sm text-muted-foreground">
              Perform garbage collection and free unused memory. CUDA cache clearing
              will free GPU memory but may slow down the next request.
            </p>

            <div className="space-y-2">
              <Button
                onClick={() => cleanupMutation.mutate({ forceCuda: false })}
                disabled={cleanupMutation.isPending}
                className="w-full"
                variant="outline"
              >
                {cleanupMutation.isPending ? (
                  <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Trash2 className="w-4 h-4 mr-2" />
                )}
                Basic Cleanup
              </Button>

              <Button
                onClick={() => cleanupMutation.mutate({ forceCuda: true })}
                disabled={cleanupMutation.isPending}
                className="w-full"
                variant="outline"
              >
                {cleanupMutation.isPending ? (
                  <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Zap className="w-4 h-4 mr-2" />
                )}
                Force CUDA Clear
              </Button>
            </div>

            {cleanupMutation.data && (
              <div className="p-3 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-md">
                <p className="text-sm text-green-800 dark:text-green-200">
                  ✓ Cleanup completed: {cleanupMutation.data.collected_objects} objects collected
                  {cleanupMutation.data.cuda_cache_cleared && " • CUDA cache cleared"}
                </p>
              </div>
            )}

            {cleanupMutation.error && (
              <div className="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
                <p className="text-sm text-red-800 dark:text-red-200">
                  ✗ Cleanup failed: {(cleanupMutation.error as Error).message}
                </p>
              </div>
            )}
          </CardContent>
        </Card>

        {/* Reset Operations */}
        <Card>
          <CardHeader>
            <CardTitle className="text-lg flex items-center gap-2">
              <AlertTriangle className="w-5 h-5 text-destructive" />
              Memory Reset
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <p className="text-sm text-muted-foreground">
              Reset all memory tracking and perform aggressive cleanup. This will
              clear request counters and GPU memory statistics.
            </p>

            <Button
              onClick={() => resetMutation.mutate()}
              disabled={resetMutation.isPending}
              className="w-full"
              variant="destructive"
            >
              {resetMutation.isPending ? (
                <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
              ) : (
                <AlertTriangle className="w-4 h-4 mr-2" />
              )}
              Reset Memory Tracking
            </Button>

            {resetMutation.data && (
              <div className="p-3 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-md">
                <p className="text-sm text-blue-800 dark:text-blue-200">
                  ✓ Reset completed: Counter {resetMutation.data.previous_request_counter} → 0
                  <br />
                  Collected: {resetMutation.data.collected_objects} objects
                  {resetMutation.data.cuda_stats_reset && " • GPU stats reset"}
                </p>
              </div>
            )}

            {resetMutation.error && (
              <div className="p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
                <p className="text-sm text-red-800 dark:text-red-200">
                  ✗ Reset failed: {(resetMutation.error as Error).message}
                </p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Memory Configuration & Detailed Information */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Configuration */}
        {memoryConfig && (
          <Card>
            <CardHeader>
              <CardTitle className="text-lg flex items-center gap-2">
                <Settings className="w-5 h-5" />
                Memory Configuration
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="space-y-4">
                <div>
                  <h4 className="font-medium mb-2">Current Settings</h4>
                  <div className="space-y-2 text-sm">
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Cleanup Interval:</span>
                      <span>{memoryConfig.config.memory_cleanup_interval} requests</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">CUDA Clear Interval:</span>
                      <span>{memoryConfig.config.cuda_cache_clear_interval} requests</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Memory Monitoring:</span>
                      <span>{memoryConfig.config.enable_memory_monitoring ? 'Enabled' : 'Disabled'}</span>
                    </div>
                  </div>
                </div>

                <div>
                  <h4 className="font-medium mb-2">Alert Thresholds</h4>
                  <div className="space-y-2 text-sm">
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">CPU Memory:</span>
                      <span>{memoryConfig.alert_thresholds.cpu_memory_percent}%</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">GPU Memory:</span>
                      <span>{formatMemory(memoryConfig.alert_thresholds.gpu_memory_mb)}</span>
                    </div>
                  </div>
                </div>

                <div>
                  <h4 className="font-medium mb-2">Device Information</h4>
                  <div className="space-y-2 text-sm">
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">CUDA Available:</span>
                      <span>{memoryConfig.device_info.cuda_available ? 'Yes' : 'No'}</span>
                    </div>
                    {memoryConfig.device_info.cuda_available && (
                      <div className="flex justify-between">
                        <span className="text-muted-foreground">GPU Devices:</span>
                        <span>{memoryConfig.device_info.cuda_device_count}</span>
                      </div>
                    )}
                  </div>
                </div>
              </div>
            </CardContent>
          </Card>
        )}

        {/* Detailed Memory Information */}
        {memoryData && (
          <Card>
            <CardHeader>
              <CardTitle className="text-lg flex items-center gap-2">
                <MemoryStick className="w-5 h-5" />
                Detailed Memory Information
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="space-y-4">
                <div>
                  <h4 className="font-medium mb-3">CPU Memory</h4>
                  <div className="space-y-2 text-sm">
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Used:</span>
                      <span>{formatMemory(memoryData.memory_info.cpu_memory_mb)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Percentage:</span>
                      <span>{memoryData.memory_info.cpu_memory_percent.toFixed(1)}%</span>
                    </div>
                  </div>
                </div>

                <div>
                  <h4 className="font-medium mb-3">GPU Memory</h4>
                  <div className="space-y-2 text-sm">
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Allocated:</span>
                      <span>{formatMemory(memoryData.memory_info.gpu_memory_allocated_mb || 0)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Reserved:</span>
                      <span>{formatMemory(memoryData.memory_info.gpu_memory_reserved_mb || 0)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Peak:</span>
                      <span>{formatMemory(memoryData.memory_info.gpu_memory_max_allocated_mb || 0)}</span>
                    </div>
                  </div>
                </div>
              </div>
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  );
} 