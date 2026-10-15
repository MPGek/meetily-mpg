import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { ScrollArea } from '@/components/ui/scroll-area';
import { RefreshCw, CheckCircle2, XCircle, ChevronDown, ChevronUp, Download, ExternalLink } from 'lucide-react';
import type { ModelConfig } from '@/lib/ipc/settings';
import { openExternalUrl } from '@/lib/ipc/settings';
import { cn } from '@/lib/utils';
import type { ModelSettingsState } from '@/hooks/useModelSettings';

/** Ollama endpoint section plus the available-models list. */
export function OllamaProvider({ settings }: { settings: ModelSettingsState }) {
  const {
    modelConfig,
    setModelConfig,
    models,
    setModels,
    error,
    setError,
    ollamaEndpoint,
    setOllamaEndpoint,
    isLoadingOllama,
    lastFetchedEndpoint,
    endpointValidationState,
    searchQuery,
    setSearchQuery,
    isEndpointSectionCollapsed,
    setIsEndpointSectionCollapsed,
    ollamaNotInstalled,
    isDownloading,
    getProgress,
    ollamaEndpointChanged,
    fetchOllamaModels,
    downloadRecommendedModel,
    filteredModels,
  } = settings;

  return (
    <>
      <div>
        <div
          className="flex items-center justify-between cursor-pointer py-2"
          onClick={() => setIsEndpointSectionCollapsed(!isEndpointSectionCollapsed)}
        >
          <Label className="cursor-pointer">Custom Endpoint (optional)</Label>
          {isEndpointSectionCollapsed ? (
            <ChevronDown className="h-4 w-4 text-muted-foreground" />
          ) : (
            <ChevronUp className="h-4 w-4 text-muted-foreground" />
          )}
        </div>

        {!isEndpointSectionCollapsed && (
          <>
            <p className="text-sm text-muted-foreground mt-1 mb-2">
              Leave empty or enter a custom endpoint (e.g., http://x.yy.zz:11434)
            </p>
            <div className="flex gap-2 mt-1">
              <div className="relative flex-1">
                <Input
                  type="url"
                  value={ollamaEndpoint}
                  onChange={(e) => {
                    setOllamaEndpoint(e.target.value);
                    // Clear models and errors when endpoint changes to avoid showing stale data
                    if (e.target.value.trim() !== lastFetchedEndpoint.trim()) {
                      setModels([]);
                      setError(''); // Clear error state
                    }
                  }}
                  placeholder="http://localhost:11434"
                  className={cn(
                    "pr-10",
                    endpointValidationState === 'invalid' && "border-red-500"
                  )}
                />
                {endpointValidationState === 'valid' && (
                  <CheckCircle2 className="absolute right-3 top-1/2 -translate-y-1/2 h-5 w-5 text-green-500" />
                )}
                {endpointValidationState === 'invalid' && (
                  <XCircle className="absolute right-3 top-1/2 -translate-y-1/2 h-5 w-5 text-red-500" />
                )}
              </div>
              <Button
                type="button"
                size={'sm'}
                onClick={() => fetchOllamaModels()}
                disabled={isLoadingOllama}
                variant="outline"
                className="whitespace-nowrap"
              >
                {isLoadingOllama ? (
                  <>
                    <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                    Fetching...
                  </>
                ) : (
                  <>
                    <RefreshCw className="mr-2 h-4 w-4" />
                    Fetch Models
                  </>
                )}
              </Button>
            </div>
            {ollamaEndpointChanged && !error && (
              <Alert className="mt-3 border-yellow-500 bg-yellow-50">
                <AlertDescription className="text-yellow-800">
                  Endpoint changed. Please click &quot;Fetch Models&quot; to load models from the new endpoint before saving.
                </AlertDescription>
              </Alert>
            )}
          </>
        )}
      </div>
      <div>
        <div className="flex items-center justify-between mb-4">
          <h4 className="text-sm font-bold">Available Ollama Models</h4>
          {lastFetchedEndpoint && models.length > 0 && (
            <div className="flex items-center gap-2 text-sm">
              <span className="text-muted-foreground">Using:</span>
              <code className="px-2 py-1 bg-muted rounded text-xs">
                {lastFetchedEndpoint || 'http://localhost:11434'}
              </code>
            </div>
          )}
        </div>
        {models.length > 0 && (
          <div className="mb-4">
            <Input
              placeholder="Search models..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full"
            />
          </div>
        )}
        {isLoadingOllama ? (
          <div className="text-center py-8 text-muted-foreground">
            <RefreshCw className="mx-auto h-8 w-8 animate-spin mb-2" />
            Loading models...
          </div>
        ) : models.length === 0 ? (
          <div className="space-y-3">
            {ollamaNotInstalled ? (
              /* Show Ollama download link when not installed */
              <div className="space-y-4">
                <Alert className="border-orange-500 bg-orange-50">
                  <AlertDescription className="text-orange-800">
                    Ollama is not installed or not running. Please download and install Ollama to use local models.
                  </AlertDescription>
                </Alert>
                <Button
                  variant="default"
                  size="sm"
                  onClick={() => openExternalUrl({ url: 'https://ollama.com/download' })}
                  className="w-full bg-blue-600 hover:bg-blue-700"
                >
                  <ExternalLink className="mr-2 h-4 w-4" />
                  Download Ollama
                </Button>
                <div className="text-sm text-muted-foreground text-center">
                  After installing Ollama, restart this application and click &quot;Fetch Models&quot; to continue.
                </div>
              </div>
            ) : (
              /* Show model download option when Ollama is installed but no models */
              <>
                <Alert className="mb-4">
                  <AlertDescription>
                    {ollamaEndpointChanged
                      ? 'Endpoint changed. Click "Fetch Models" to load models from the new endpoint.'
                      : 'No models found. Download a recommended model or click "Fetch Models" to load available Ollama models.'}
                  </AlertDescription>
                </Alert>
                {!ollamaEndpointChanged && (
                  <div className="space-y-3">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={downloadRecommendedModel}
                      disabled={isDownloading('gemma3:1b')}
                      className="w-full"
                    >
                      {isDownloading('gemma3:1b') ? (
                        <>
                          <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                          Downloading gemma3:1b...
                        </>
                      ) : (
                        <>
                          <Download className="mr-2 h-4 w-4" />
                          Download gemma3:1b (Recommended, ~800MB)
                        </>
                      )}
                    </Button>

                    {/* Show progress for gemma3:1b download */}
                    {isDownloading('gemma3:1b') && getProgress('gemma3:1b') !== undefined && (
                      <div className="bg-white rounded-md border p-3">
                        <div className="flex items-center justify-between mb-2">
                          <span className="text-sm font-medium text-blue-600">Downloading gemma3:1b</span>
                          <span className="text-sm font-semibold text-blue-600">
                            {Math.round(getProgress('gemma3:1b')!)}%
                          </span>
                        </div>
                        <div className="w-full h-2 bg-gray-200 rounded-full overflow-hidden">
                          <div
                            className="h-full bg-gradient-to-r from-blue-500 to-blue-600 rounded-full transition-all duration-300"
                            style={{ width: `${getProgress('gemma3:1b')}%` }}
                          />
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </>
            )}
          </div>
        ) : !ollamaEndpointChanged && (
          <ScrollArea className="max-h-[calc(100vh-450px)] overflow-y-auto pr-4">
            {filteredModels.length === 0 ? (
              <Alert>
                <AlertDescription>
                  No models found matching &quot;{searchQuery}&quot;. Try a different search term.
                </AlertDescription>
              </Alert>
            ) : (
              <div className="grid gap-4">
                {filteredModels.map((model) => {
                  const progress = getProgress(model.name);
                  const modelIsDownloading = isDownloading(model.name);

                  return (
                    <div
                      key={model.id}
                      className={cn(
                        'bg-card p-2 m-0 rounded-md border transition-colors',
                        modelConfig.model === model.name
                          ? 'ring-1 ring-blue-500 border-blue-500 background-blue-100'
                          : 'hover:bg-muted/50',
                        !modelIsDownloading && 'cursor-pointer'
                      )}
                      onClick={() => {
                        if (!modelIsDownloading) {
                          setModelConfig((prev: ModelConfig) => ({ ...prev, model: model.name }))
                        }
                      }}
                    >
                      <div>
                        <b className="font-bold">{model.name}&nbsp;</b>
                        <span className="text-muted-foreground">with a size of </span>
                        <span className="font-mono font-bold text-sm">{model.size}</span>
                      </div>

                      {/* Progress bar for downloading models */}
                      {modelIsDownloading && progress !== undefined && (
                        <div className="mt-3 pt-3 border-t border-gray-200">
                          <div className="flex items-center justify-between mb-2">
                            <span className="text-sm font-medium text-blue-600">Downloading...</span>
                            <span className="text-sm font-semibold text-blue-600">{Math.round(progress)}%</span>
                          </div>
                          <div className="w-full h-2 bg-gray-200 rounded-full overflow-hidden">
                            <div
                              className="h-full bg-gradient-to-r from-blue-500 to-blue-600 rounded-full transition-all duration-300"
                              style={{ width: `${progress}%` }}
                            />
                          </div>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </ScrollArea>
        )}
      </div>
    </>
  );
}
