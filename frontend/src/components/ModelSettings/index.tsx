import type { ModelConfig } from '@/lib/ipc/settings';
import { getCustomOpenaiConfig } from '@/lib/ipc/settings';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';
import { useModelSettings } from '@/hooks/useModelSettings';
import { ApiKeyField } from './ApiKeyField';
import { ModelCombobox } from './ModelCombobox';
import { CustomOpenAIProvider } from './providers/CustomOpenAI';
import { OllamaProvider } from './providers/Ollama';
import { BuiltInProvider } from './providers/BuiltIn';

export interface ModelSettingsModalProps {
  modelConfig: ModelConfig;
  setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;
  onSave: (config: ModelConfig) => void;
  skipInitialFetch?: boolean; // Optional: skip fetching config from backend if parent manages it
  layout?: 'inline' | 'dialog';
}

export function ModelSettingsModal({
  modelConfig: propsModelConfig,
  setModelConfig: propsSetModelConfig,
  onSave,
  skipInitialFetch = false,
  layout = 'inline',
}: ModelSettingsModalProps) {
  const settings = useModelSettings({
    modelConfig: propsModelConfig,
    setModelConfig: propsSetModelConfig,
    onSave,
    skipInitialFetch,
  });
  const {
    modelConfig,
    setModelConfig,
    setError,
    apiKey,
    setApiKey,
    showApiKey,
    setShowApiKey,
    isApiKeyLocked,
    setIsApiKeyLocked,
    isLockButtonVibrating,
    setCustomOpenAIEndpoint,
    setCustomOpenAIModel,
    setCustomOpenAIApiKey,
    setCustomMaxTokens,
    setCustomTemperature,
    setCustomTopP,
    modelOptions,
    requiresApiKey,
    isDoneDisabled,
    loadOpenRouterModels,
    loadBuiltinAiModels,
    handleSave,
    handleInputClick,
  } = settings;

  return (
    <div>
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-lg font-semibold">Model Settings</h3>
      </div>

      <div className="space-y-4">
        <div>
          <Label>Summarization Model</Label>
          <div className="flex space-x-2 mt-1">
            <Select
              value={modelConfig.provider}
              onValueChange={(value) => {
                const provider = value as ModelConfig['provider'];

                // Clear error state when switching providers
                setError('');

                // Save current provider's model to localStorage before switching
                const map = JSON.parse(localStorage.getItem('providerModelMap') || '{}');
                if (modelConfig.model) {
                  map[modelConfig.provider] = modelConfig.model;
                  localStorage.setItem('providerModelMap', JSON.stringify(map));
                }

                // Try to restore cached model for the new provider
                const savedModel = map[provider];
                const providerModels = modelOptions[provider];
                const defaultModel = providerModels && providerModels.length > 0
                  ? providerModels[0]
                  : '';
                const model = (savedModel && providerModels?.includes(savedModel))
                  ? savedModel
                  : defaultModel;

                setModelConfig({
                  ...modelConfig,
                  provider,
                  model,
                });
                // API key is now synced automatically via useEffect watching providerApiKeys

                // Load OpenRouter models only when OpenRouter is selected
                if (provider === 'openrouter') {
                  loadOpenRouterModels();
                }

                // Load Built-in AI models when selected
                if (provider === 'builtin-ai') {
                  loadBuiltinAiModels();
                }

                // Load custom OpenAI config when selected
                if (provider === 'custom-openai') {
                  getCustomOpenaiConfig().then((config) => {
                    if (config) {
                      setCustomOpenAIEndpoint(config.endpoint || '');
                      setCustomOpenAIModel(config.model || '');
                      setCustomOpenAIApiKey(config.apiKey || '');
                      setCustomMaxTokens(config.maxTokens?.toString() || '');
                      setCustomTemperature(config.temperature?.toString() || '');
                      setCustomTopP(config.topP?.toString() || '');
                    }
                  }).catch((err) => {
                    console.error('Failed to load custom OpenAI config:', err);
                  });
                }
              }}
            >
              <SelectTrigger>
                <SelectValue placeholder="Select provider" />
              </SelectTrigger>
              <SelectContent className="max-h-64 overflow-y-auto">
                <SelectItem value="builtin-ai">Built-in AI (Offline, No API needed)</SelectItem>
                <SelectItem value="claude">Claude</SelectItem>
                <SelectItem value="custom-openai">Custom Server (OpenAI)</SelectItem>
                <SelectItem value="groq">Groq</SelectItem>
                <SelectItem value="ollama">Ollama</SelectItem>
                <SelectItem value="openai">OpenAI</SelectItem>
                <SelectItem value="openrouter">OpenRouter</SelectItem>
              </SelectContent>
            </Select>

            {modelConfig.provider !== 'builtin-ai' && modelConfig.provider !== 'custom-openai' && (
              <ModelCombobox settings={settings} />
            )}
          </div>
        </div>

        {/* Custom OpenAI Configuration Section */}
        {modelConfig.provider === 'custom-openai' && (
          <CustomOpenAIProvider settings={settings} />
        )}

        {requiresApiKey && (
          <ApiKeyField
            value={apiKey}
            locked={isApiKeyLocked}
            visible={showApiKey}
            vibrating={isLockButtonVibrating}
            onChange={setApiKey}
            onLockedClick={handleInputClick}
            onToggleLock={() => setIsApiKeyLocked(!isApiKeyLocked)}
            onToggleVisible={() => setShowApiKey(!showApiKey)}
          />
        )}

        {modelConfig.provider === 'ollama' && <OllamaProvider settings={settings} />}

        {/* Built-in AI Models Section */}
        {modelConfig.provider === 'builtin-ai' && (
          <BuiltInProvider
            selectedModel={modelConfig.model}
            layout={layout}
            onModelSelect={(model) =>
              setModelConfig((prev: ModelConfig) => ({ ...prev, model }))
            }
          />
        )}
      </div>

      {/* Auto-generate summaries toggle */}
      {/* <div className="mt-6 pt-6 border-t border-gray-200">
        <div className="flex items-center justify-between">
          <div className="flex-1">
            <Label htmlFor="auto-generate" className="text-base font-medium">
              Auto-generate summaries
            </Label>
            <p className="text-sm text-muted-foreground mt-1">
              Automatically generate summary when opening meetings without one
            </p>
          </div>
          <Switch
            id="auto-generate"
            checked={autoGenerateEnabled}
            onCheckedChange={setAutoGenerateEnabled}
          />
        </div>
      </div> */}

      <div className="mt-6 flex justify-end">
        <Button
          className={cn(
            'px-4 text-sm font-medium text-white rounded-md focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500',
            isDoneDisabled ? 'bg-gray-400 cursor-not-allowed' : 'bg-blue-600 hover:bg-blue-700'
          )}
          onClick={handleSave}
          disabled={isDoneDisabled}
        >
          Save
        </Button>
      </div>
    </div>
  );
}
