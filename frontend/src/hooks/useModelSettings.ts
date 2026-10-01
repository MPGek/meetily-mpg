import { useState, useEffect, useRef } from 'react';
import {
  getApiKey,
  getAutoGenerateSetting,
  getCustomOpenaiConfig,
  getModelConfig,
  openExternalUrl,
  saveCustomOpenaiConfig,
  testCustomOpenaiConnection,
  type ModelConfig,
} from '@/lib/ipc/settings';
import {
  builtinAiListModels,
  getAnthropicModels,
  getGroqModels,
  getOllamaModels,
  getOpenaiModels,
  getOpenrouterModels,
  pullOllamaModel,
  type BuiltInModelInfo,
  type OllamaModel,
  type OpenRouterModel,
} from '@/lib/ipc/models';
import { useOllamaDownload } from '@/contexts/OllamaDownloadContext';
import { useConfig } from '@/contexts/ConfigContext';
import { isOllamaNotInstalledError } from '@/lib/utils';
import { toast } from 'sonner';

// Fallback models for when API fetch fails or no API key provided
const OPENAI_FALLBACK_MODELS = [
  'gpt-4o',
  'gpt-4o-mini',
  'gpt-4-turbo',
  'gpt-4',
  'gpt-3.5-turbo',
  'o1',
  'o1-mini',
  'o3',
  'o3-mini',
];

const CLAUDE_FALLBACK_MODELS = [
  'claude-sonnet-4-5-20250929',
  'claude-haiku-4-5-20251001',
  'claude-opus-4-5-20251101',
  'claude-3-5-sonnet-latest',
];

const GROQ_FALLBACK_MODELS = [
  'llama-3.3-70b-versatile',
  'llama-3.1-70b-versatile',
  'mixtral-8x7b-32768',
  'gemma2-9b-it',
];

export interface UseModelSettingsArgs {
  modelConfig: ModelConfig;
  setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;
  onSave: (config: ModelConfig) => void;
  skipInitialFetch?: boolean;
}

/** State, IPC and handlers behind the model settings panel (`ModelSettings/index.tsx`). */
export function useModelSettings({
  modelConfig: propsModelConfig,
  setModelConfig: propsSetModelConfig,
  onSave,
  skipInitialFetch = false,
}: UseModelSettingsArgs) {
  // Use ConfigContext if available, fallback to props for backward compatibility
  const configContext = useConfig();
  const modelConfig = configContext?.modelConfig || propsModelConfig;
  const setModelConfig = configContext?.setModelConfig || propsSetModelConfig;
  const providerApiKeys = configContext?.providerApiKeys;
  const updateProviderApiKey = configContext?.updateProviderApiKey;

  const [models, setModels] = useState<OllamaModel[]>([]);
  const [error, setError] = useState<string>('');
  const [apiKey, setApiKey] = useState<string | null>(modelConfig.apiKey || null);
  const [showApiKey, setShowApiKey] = useState<boolean>(false);
  const [isApiKeyLocked, setIsApiKeyLocked] = useState<boolean>(!!modelConfig.apiKey?.trim());
  const [isLockButtonVibrating, setIsLockButtonVibrating] = useState<boolean>(false);
  const [openRouterModels, setOpenRouterModels] = useState<OpenRouterModel[]>([]);
  const [, setOpenRouterError] = useState<string>('');
  const [isLoadingOpenRouter, setIsLoadingOpenRouter] = useState<boolean>(false);
  const [ollamaEndpoint, setOllamaEndpoint] = useState<string>(modelConfig.ollamaEndpoint || '');
  const [isLoadingOllama, setIsLoadingOllama] = useState<boolean>(false);
  const [lastFetchedEndpoint, setLastFetchedEndpoint] = useState<string>(modelConfig.ollamaEndpoint || '');
  const [endpointValidationState, setEndpointValidationState] = useState<'valid' | 'invalid' | 'none'>('none');
  const [hasAutoFetched, setHasAutoFetched] = useState<boolean>(false);
  const hasSyncedFromParent = useRef<boolean>(false);
  const hasLoadedInitialConfig = useRef<boolean>(false);
  const [, setAutoGenerateEnabled] = useState<boolean>(true); // Default to true
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [isEndpointSectionCollapsed, setIsEndpointSectionCollapsed] = useState<boolean>(true); // Collapsed by default
  const [ollamaNotInstalled, setOllamaNotInstalled] = useState<boolean>(false); // Track if Ollama is not installed

  // Custom OpenAI state
  const [customOpenAIEndpoint, setCustomOpenAIEndpoint] = useState<string>(modelConfig.customOpenAIEndpoint || '');
  const [customOpenAIModel, setCustomOpenAIModel] = useState<string>(modelConfig.customOpenAIModel || '');
  const [customOpenAIApiKey, setCustomOpenAIApiKey] = useState<string>(modelConfig.customOpenAIApiKey || '');
  const [customMaxTokens, setCustomMaxTokens] = useState<string>(modelConfig.maxTokens?.toString() || '');
  const [customTemperature, setCustomTemperature] = useState<string>(modelConfig.temperature?.toString() || '');
  const [customTopP, setCustomTopP] = useState<string>(modelConfig.topP?.toString() || '');
  const [isCustomOpenAIAdvancedOpen, setIsCustomOpenAIAdvancedOpen] = useState<boolean>(false);
  const [isTestingConnection, setIsTestingConnection] = useState<boolean>(false);

  // Combobox state
  const [modelComboboxOpen, setModelComboboxOpen] = useState<boolean>(false);

  // Dynamic model fetching state for OpenAI, Claude, and Groq
  const [openaiModels, setOpenaiModels] = useState<string[]>([]);
  const [claudeModels, setClaudeModels] = useState<string[]>([]);
  const [groqModels, setGroqModels] = useState<string[]>([]);
  const [isLoadingOpenAI, setIsLoadingOpenAI] = useState<boolean>(false);
  const [isLoadingClaude, setIsLoadingClaude] = useState<boolean>(false);
  const [isLoadingGroq, setIsLoadingGroq] = useState<boolean>(false);

  // Use global download context instead of local state
  const { isDownloading, getProgress, downloadingModels } = useOllamaDownload();

  // Built-in AI models state
  const [builtinAiModels, setBuiltinAiModels] = useState<BuiltInModelInfo[]>([]);

  // Cache models by endpoint to avoid refetching when reverting endpoint changes
  const modelsCache = useRef<Map<string, OllamaModel[]>>(new Map());

  // URL validation helper
  const validateOllamaEndpoint = (url: string): boolean => {
    if (!url.trim()) return true; // Empty is valid (uses default)
    try {
      const parsed = new URL(url);
      return parsed.protocol === 'http:' || parsed.protocol === 'https:';
    } catch {
      return false;
    }
  };

  // Debounced URL validation with visual feedback
  useEffect(() => {
    const timer = setTimeout(() => {
      const trimmed = ollamaEndpoint.trim();

      if (!trimmed) {
        setEndpointValidationState('none');
      } else if (validateOllamaEndpoint(trimmed)) {
        setEndpointValidationState('valid');
      } else {
        setEndpointValidationState('invalid');
      }
    }, 500); // 500ms debounce

    return () => clearTimeout(timer);
  }, [ollamaEndpoint]);

  // Auto-unlock when API key becomes empty, 
  useEffect(() => {
    const hasContent = !!apiKey?.trim();
    if (!hasContent) {
      setIsApiKeyLocked(false);
    }
  }, [apiKey]);

  const modelOptions: Record<string, string[]> = {
    ollama: models.map((model) => model.name),
    claude: claudeModels.length > 0 ? claudeModels : CLAUDE_FALLBACK_MODELS,
    groq: groqModels.length > 0 ? groqModels : GROQ_FALLBACK_MODELS,
    openai: openaiModels.length > 0 ? openaiModels : OPENAI_FALLBACK_MODELS,
    openrouter: openRouterModels.map((m) => m.id),
    'builtin-ai': builtinAiModels.map((m) => m.name),
    'custom-openai': customOpenAIModel ? [customOpenAIModel] : [], // User specifies model manually
  };

  const requiresApiKey =
    modelConfig.provider === 'claude' ||
    modelConfig.provider === 'groq' ||
    modelConfig.provider === 'openai' ||
    modelConfig.provider === 'openrouter';

  // Check if Ollama endpoint has changed but models haven't been fetched yet
  const ollamaEndpointChanged = modelConfig.provider === 'ollama' &&
    ollamaEndpoint.trim() !== lastFetchedEndpoint.trim();

  // Custom OpenAI validation
  const isCustomOpenAIInvalid = modelConfig.provider === 'custom-openai' && (
    !customOpenAIEndpoint.trim() ||
    !customOpenAIModel.trim()
  );

  const isDoneDisabled =
    (requiresApiKey && (!apiKey || (typeof apiKey === 'string' && !apiKey.trim()))) ||
    (modelConfig.provider === 'ollama' && ollamaEndpointChanged) ||
    isCustomOpenAIInvalid;

  useEffect(() => {
    const fetchModelConfig = async () => {
      // If parent component manages config, skip fetch and just mark as loaded
      if (skipInitialFetch) {
        hasLoadedInitialConfig.current = true;
        return;
      }

      try {
        const data = await getModelConfig();
        if (data && data.provider !== null) {
          setModelConfig(data);

          // Fetch API key if not included in response and provider requires it
          if (data.provider !== 'ollama' && !data.apiKey) {
            try {
              const apiKeyData = await getApiKey({
                provider: data.provider
              });
              data.apiKey = apiKeyData;
              setApiKey(apiKeyData);
            } catch (err) {
              console.error('Failed to fetch API key:', err);
            }
          }

          // Sync ollamaEndpoint state with fetched config
          if (data.ollamaEndpoint) {
            setOllamaEndpoint(data.ollamaEndpoint);
            // Don't set lastFetchedEndpoint here - it will be set after successful model fetch
          }
          hasLoadedInitialConfig.current = true; // Mark that initial config is loaded

          // Fetch Custom OpenAI config if that's the active provider
          if (data.provider === 'custom-openai') {
            try {
              const customConfig = await getCustomOpenaiConfig();
              if (customConfig) {
                setCustomOpenAIEndpoint(customConfig.endpoint || '');
                setCustomOpenAIModel(customConfig.model || '');
                setCustomOpenAIApiKey(customConfig.apiKey || '');
                setCustomMaxTokens(customConfig.maxTokens?.toString() || '');
                setCustomTemperature(customConfig.temperature?.toString() || '');
                setCustomTopP(customConfig.topP?.toString() || '');
              }
            } catch (err) {
              console.error('Failed to fetch custom OpenAI config:', err);
            }
          }
        }
      } catch (error) {
        console.error('Failed to fetch model config:', error);
        hasLoadedInitialConfig.current = true; // Mark as loaded even on error
      }
    };

    fetchModelConfig();
  }, [skipInitialFetch, setModelConfig]);

  // Fetch auto-generate setting on mount
  useEffect(() => {
    const fetchAutoGenerateSetting = async () => {
      try {
        const enabled = await getAutoGenerateSetting();
        setAutoGenerateEnabled(enabled);
        console.log('Auto-generate setting loaded:', enabled);
      } catch (err) {
        console.error('Failed to fetch auto-generate setting:', err);
        // Keep default value (true) on error
      }
    };

    fetchAutoGenerateSetting();
  }, []);

  // Sync ollamaEndpoint state when modelConfig.ollamaEndpoint changes from parent
  useEffect(() => {
    const endpoint = modelConfig.ollamaEndpoint || '';
    if (endpoint !== ollamaEndpoint) {
      setOllamaEndpoint(endpoint);
      // Don't set lastFetchedEndpoint here - only after successful model fetch
    }
    // Only mark as synced if we have a valid provider (prevents race conditions during init)
    if (modelConfig.provider) {
      hasSyncedFromParent.current = true; // Mark that we've received prop value
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- syncs from the parent value only; adding the local ollamaEndpoint would overwrite user edits as they type
  }, [modelConfig.ollamaEndpoint, modelConfig.provider]);

  // Sync custom OpenAI state from modelConfig (context or props)
  useEffect(() => {
    if (modelConfig.provider === 'custom-openai') {
      console.log('Syncing custom OpenAI fields from ConfigContext:', {
        endpoint: modelConfig.customOpenAIEndpoint,
        model: modelConfig.customOpenAIModel,
        hasApiKey: !!modelConfig.customOpenAIApiKey,
      });

      // Always sync from modelConfig (which comes from context if available)
      setCustomOpenAIEndpoint(modelConfig.customOpenAIEndpoint || '');
      setCustomOpenAIModel(modelConfig.customOpenAIModel || '');
      setCustomOpenAIApiKey(modelConfig.customOpenAIApiKey || '');
      setCustomMaxTokens(modelConfig.maxTokens?.toString() || '');
      setCustomTemperature(modelConfig.temperature?.toString() || '');
      setCustomTopP(modelConfig.topP?.toString() || '');
    }
  }, [
    modelConfig.provider,
    modelConfig.customOpenAIEndpoint,
    modelConfig.customOpenAIModel,
    modelConfig.customOpenAIApiKey,
    modelConfig.maxTokens,
    modelConfig.temperature,
    modelConfig.topP
  ]);

  // Reset hasAutoFetched flag and clear models when switching away from Ollama
  useEffect(() => {
    if (modelConfig.provider !== 'ollama') {
      setHasAutoFetched(false); // Reset flag so it can auto-fetch again if user switches back
      setModels([]); // Clear models list
      setError(''); // Clear any error state
      setOllamaNotInstalled(false); // Reset installation status
    }
  }, [modelConfig.provider]);

  // Handle endpoint changes - restore cached models or clear
  useEffect(() => {
    if (modelConfig.provider === 'ollama' &&
      ollamaEndpoint.trim() !== lastFetchedEndpoint.trim()) {

      // Check if we have cached models for this endpoint (including empty endpoint = default)
      const cachedModels = modelsCache.current.get(ollamaEndpoint.trim());

      if (cachedModels && cachedModels.length > 0) {
        // Restore cached models and update tracking
        setModels(cachedModels);
        setLastFetchedEndpoint(ollamaEndpoint.trim());
        setError('');
      } else {
        // No cache - clear models and allow refetch
        setHasAutoFetched(false);
        setModels([]);
        setError('');
      }
    }
  }, [ollamaEndpoint, lastFetchedEndpoint, modelConfig.provider]);

  // Sync local apiKey state when provider changes
  useEffect(() => {
    if (providerApiKeys && requiresApiKey && modelConfig.provider !== 'custom-openai') {
      const correctKey = providerApiKeys[modelConfig.provider as keyof typeof providerApiKeys];
      if (correctKey !== apiKey) {
        setApiKey(correctKey || '');
        setIsApiKeyLocked(!!correctKey?.trim());
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- syncs the key only when provider/keys change; adding the local apiKey would overwrite user edits as they type
  }, [modelConfig.provider, providerApiKeys, requiresApiKey]);

  // Manual fetch function for Ollama models
  const fetchOllamaModels = async (silent = false) => {
    const trimmedEndpoint = ollamaEndpoint.trim();

    // Validate URL if provided
    if (trimmedEndpoint && !validateOllamaEndpoint(trimmedEndpoint)) {
      const errorMsg = 'Invalid Ollama endpoint URL. Must start with http:// or https://';
      setError(errorMsg);
      if (!silent) {
        toast.error(errorMsg);
      }
      return;
    }

    setIsLoadingOllama(true);
    setError(''); // Clear previous errors

    try {
      const endpoint = trimmedEndpoint || null;
      const modelList = await getOllamaModels({ endpoint });
      setModels(modelList);
      setLastFetchedEndpoint(trimmedEndpoint); // Track successful fetch

      // Cache the fetched models for this endpoint
      modelsCache.current.set(trimmedEndpoint, modelList);

      // Successfully fetched models, Ollama is installed
      setOllamaNotInstalled(false);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to load Ollama models';
      setError(errorMsg);

      // Check if error indicates Ollama is not installed
      if (isOllamaNotInstalledError(errorMsg)) {
        setOllamaNotInstalled(true);
      } else {
        setOllamaNotInstalled(false);
      }

      if (!silent) {
        toast.error(errorMsg);
      }
      console.error('Error loading models:', err);
    } finally {
      setIsLoadingOllama(false);
    }
  };

  // Auto-fetch models on initial load only (not on endpoint changes)
  useEffect(() => {
    let mounted = true;

    const initialLoad = async () => {
      // Only auto-fetch on initial load if:
      // 1. Provider is ollama
      // 2. Haven't fetched yet
      // 3. Component is still mounted
      // If skipInitialFetch is true, fetch silently (no error toasts)
      if (modelConfig.provider === 'ollama' &&
        !hasAutoFetched &&
        mounted) {
        await fetchOllamaModels(skipInitialFetch); // Silent if skipInitialFetch=true
        setHasAutoFetched(true);
      }
    };

    initialLoad();

    return () => {
      mounted = false;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- initial fetch keyed on provider only (see trailing comment); fetchOllamaModels is recreated every render and hasAutoFetched is set by this effect
  }, [modelConfig.provider]); // Only depend on provider, NOT endpoint

  const loadOpenRouterModels = async () => {
    if (openRouterModels.length > 0) return; // Already loaded

    try {
      setIsLoadingOpenRouter(true);
      setOpenRouterError('');
      const data = await getOpenrouterModels();
      setOpenRouterModels(data);
    } catch (err) {
      console.error('Error loading OpenRouter models:', err);
      setOpenRouterError(
        err instanceof Error ? err.message : 'Failed to load OpenRouter models'
      );
    } finally {
      setIsLoadingOpenRouter(false);
    }
  };

  const loadBuiltinAiModels = async () => {
    if (builtinAiModels.length > 0) return; // Already loaded

    try {
      const data = await builtinAiListModels();
      setBuiltinAiModels(data);

      // Auto-select first available model if none selected
      if (data.length > 0 && !modelConfig.model) {
        const firstAvailable = data.find((m) => m.status?.type === 'available');
        if (firstAvailable) {
          setModelConfig((prev: ModelConfig) => ({ ...prev, model: firstAvailable.name }));
        }
      }
    } catch (err) {
      console.error('Error loading Built-in AI models:', err);
      toast.error('Failed to load Built-in AI models');
    }
  };

  // Fetch OpenAI models from API
  const loadOpenAIModels = async (key: string | null) => {
    if (!key?.trim()) {
      setOpenaiModels([]); // Will use fallback via modelOptions
      return;
    }
    setIsLoadingOpenAI(true);
    try {
      const data = await getOpenaiModels({ apiKey: key });
      setOpenaiModels(data.map((m) => m.id));
    } catch (err) {
      console.error('Error loading OpenAI models:', err);
      setOpenaiModels([]); // Will use fallback via modelOptions
    } finally {
      setIsLoadingOpenAI(false);
    }
  };

  // Fetch Anthropic (Claude) models from API
  const loadClaudeModels = async (key: string | null) => {
    if (!key?.trim()) {
      setClaudeModels([]); // Will use fallback via modelOptions
      return;
    }
    setIsLoadingClaude(true);
    try {
      const data = await getAnthropicModels({ apiKey: key });
      setClaudeModels(data.map((m) => m.id));
    } catch (err) {
      console.error('Error loading Claude models:', err);
      setClaudeModels([]); // Will use fallback via modelOptions
    } finally {
      setIsLoadingClaude(false);
    }
  };

  // Fetch Groq models from API
  const loadGroqModels = async (key: string | null) => {
    if (!key?.trim()) {
      setGroqModels([]); // Will use fallback via modelOptions
      return;
    }
    setIsLoadingGroq(true);
    try {
      const data = await getGroqModels({ apiKey: key });
      setGroqModels(data.map((m) => m.id));
    } catch (err) {
      console.error('Error loading Groq models:', err);
      setGroqModels([]); // Will use fallback via modelOptions
    } finally {
      setIsLoadingGroq(false);
    }
  };

  // Auto-fetch OpenAI models when provider is openai and we have an API key
  useEffect(() => {
    if (modelConfig.provider === 'openai' && apiKey?.trim()) {
      loadOpenAIModels(apiKey);
    }
  }, [modelConfig.provider, apiKey]);

  // Auto-fetch Claude models when provider is claude and we have an API key
  useEffect(() => {
    if (modelConfig.provider === 'claude' && apiKey?.trim()) {
      loadClaudeModels(apiKey);
    }
  }, [modelConfig.provider, apiKey]);

  // Auto-fetch Groq models when provider is groq and we have an API key
  useEffect(() => {
    if (modelConfig.provider === 'groq' && apiKey?.trim()) {
      loadGroqModels(apiKey);
    }
  }, [modelConfig.provider, apiKey]);

  // Restore cached model when async model lists become available
  useEffect(() => {
    const providerModels = modelOptions[modelConfig.provider];
    if (!providerModels || providerModels.length === 0) return;

    // If current model is already valid, nothing to do
    if (modelConfig.model && providerModels.includes(modelConfig.model)) return;

    // Try to restore from localStorage cache
    const map = JSON.parse(localStorage.getItem('providerModelMap') || '{}');
    const cachedModel = map[modelConfig.provider];
    if (cachedModel && providerModels.includes(cachedModel)) {
      setModelConfig((prev: ModelConfig) => ({ ...prev, model: cachedModel }));
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- modelOptions is a new object every render and setModelConfig would re-run the restore on every render; keyed on the model lists instead
  }, [models, openRouterModels, builtinAiModels, openaiModels, claudeModels, groqModels, modelConfig.provider]);

  const handleSave = async () => {
    // For custom-openai provider, save the custom config first
    if (modelConfig.provider === 'custom-openai') {
      try {
        await saveCustomOpenaiConfig({
          endpoint: customOpenAIEndpoint.trim(),
          apiKey: customOpenAIApiKey.trim() || null,
          model: customOpenAIModel.trim(),
          maxTokens: customMaxTokens ? parseInt(customMaxTokens, 10) : null,
          temperature: customTemperature ? parseFloat(customTemperature) : null,
          topP: customTopP ? parseFloat(customTopP) : null,
        });
        console.log('Custom OpenAI config saved successfully');
      } catch (err) {
        console.error('Failed to save custom OpenAI config:', err);
        toast.error('Failed to save custom OpenAI configuration');
        return;
      }
    }

    const updatedConfig = {
      ...modelConfig,
      apiKey: typeof apiKey === 'string' ? apiKey.trim() || null : null,
      ollamaEndpoint: modelConfig.provider === 'ollama'
        ? (ollamaEndpoint.trim() || null)
        : (modelConfig.ollamaEndpoint || null),
      // Include custom OpenAI fields
      customOpenAIEndpoint: modelConfig.provider === 'custom-openai' ? customOpenAIEndpoint.trim() : null,
      customOpenAIModel: modelConfig.provider === 'custom-openai' ? customOpenAIModel.trim() : null,
      customOpenAIApiKey: modelConfig.provider === 'custom-openai' && customOpenAIApiKey.trim() ? customOpenAIApiKey.trim() : null,
      maxTokens: modelConfig.provider === 'custom-openai' && customMaxTokens ? parseInt(customMaxTokens, 10) : null,
      temperature: modelConfig.provider === 'custom-openai' && customTemperature ? parseFloat(customTemperature) : null,
      topP: modelConfig.provider === 'custom-openai' && customTopP ? parseFloat(customTopP) : null,
      // For custom-openai, use the customOpenAIModel as the model field
      model: modelConfig.provider === 'custom-openai' ? customOpenAIModel.trim() : modelConfig.model,
    };
    setModelConfig(updatedConfig);
    console.log('ModelSettingsModal - handleSave - Updated ModelConfig:', updatedConfig);

    // Persist confirmed model choice to per-provider cache
    if (updatedConfig.model) {
      const map = JSON.parse(localStorage.getItem('providerModelMap') || '{}');
      map[updatedConfig.provider] = updatedConfig.model;
      localStorage.setItem('providerModelMap', JSON.stringify(map));
    }

    // Update provider-specific key in context
    if (updateProviderApiKey && updatedConfig.apiKey && updatedConfig.provider !== 'custom-openai') {
      updateProviderApiKey(updatedConfig.provider, updatedConfig.apiKey);
    }

    onSave(updatedConfig);
  };

  // Test custom OpenAI connection
  const testCustomOpenAIConnection = async () => {
    if (!customOpenAIEndpoint.trim() || !customOpenAIModel.trim()) {
      toast.error('Please enter endpoint URL and model name first');
      return;
    }

    setIsTestingConnection(true);
    try {
      const result = await testCustomOpenaiConnection({
        endpoint: customOpenAIEndpoint.trim(),
        apiKey: customOpenAIApiKey.trim() || null,
        model: customOpenAIModel.trim(),
      });
      toast.success(result.message || 'Connection successful!');
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      toast.error(errorMsg);
    } finally {
      setIsTestingConnection(false);
    }
  };

  const handleInputClick = () => {
    if (isApiKeyLocked) {
      setIsLockButtonVibrating(true);
      setTimeout(() => setIsLockButtonVibrating(false), 500);
    }
  };

  // Function to download recommended model
  const downloadRecommendedModel = async () => {
    const recommendedModel = 'gemma3:1b';

    // Prevent duplicate downloads (defense in depth - backend also checks)
    if (isDownloading(recommendedModel)) {
      toast.info(`${recommendedModel} is already downloading`, {
        description: `Progress: ${Math.round(getProgress(recommendedModel) || 0)}%`
      });
      return;
    }

    try {
      const endpoint = ollamaEndpoint.trim() || null;

      // The download will be tracked by the global context via events
      // Progress toasts are shown automatically by OllamaDownloadContext
      await pullOllamaModel({
        modelName: recommendedModel,
        endpoint
      });

      // Refresh the models list after successful download
      await fetchOllamaModels(true);

      // Note: Model is NOT auto-selected - user must explicitly choose it
      // This respects the database as the single source of truth
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to download model';
      console.error('Error downloading model:', err);

      // Check if Ollama is not installed and show appropriate error
      if (isOllamaNotInstalledError(errorMsg)) {
        toast.error('Ollama is not installed', {
          description: 'Please download and install Ollama before downloading models.',
          duration: 7000,
          action: {
            label: 'Download',
            onClick: () => openExternalUrl({ url: 'https://ollama.com/download' })
          }
        });
        // Update the installation status flag
        setOllamaNotInstalled(true);
      }
      // Other errors are handled by the context
    }
  };

  // Track previous downloading models to detect completions
  const previousDownloadingRef = useRef<Set<string>>(new Set());

  // Refresh models list when download completes
  useEffect(() => {
    const current = downloadingModels;
    const previous = previousDownloadingRef.current;

    // Check if any downloads completed (were in previous, not in current)
    for (const modelName of previous) {
      if (!current.has(modelName)) {
        // Download completed, refresh models list
        console.log(`[ModelSettingsModal] Download completed for ${modelName}, refreshing list`);
        fetchOllamaModels(true);
        break; // Only refresh once even if multiple completed
      }
    }

    // Update ref for next comparison
    previousDownloadingRef.current = new Set(current);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- fetchOllamaModels is recreated every render; the refresh must fire only when downloadingModels changes
  }, [downloadingModels]);

  // Filter Ollama models based on search query
  const filteredModels = models.filter((model) => {
    if (!searchQuery.trim()) return true;

    const query = searchQuery.toLowerCase();
    const isLoaded = modelConfig.model === model.name;
    const loadedText = isLoaded ? 'loaded' : '';

    return (
      model.name.toLowerCase().includes(query) ||
      model.size.toLowerCase().includes(query) ||
      loadedText.includes(query)
    );
  });

  return {
    modelConfig,
    setModelConfig,
    models,
    setModels,
    error,
    setError,
    apiKey,
    setApiKey,
    showApiKey,
    setShowApiKey,
    isApiKeyLocked,
    setIsApiKeyLocked,
    isLockButtonVibrating,
    isLoadingOpenRouter,
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
    customOpenAIEndpoint,
    setCustomOpenAIEndpoint,
    customOpenAIModel,
    setCustomOpenAIModel,
    customOpenAIApiKey,
    setCustomOpenAIApiKey,
    customMaxTokens,
    setCustomMaxTokens,
    customTemperature,
    setCustomTemperature,
    customTopP,
    setCustomTopP,
    isCustomOpenAIAdvancedOpen,
    setIsCustomOpenAIAdvancedOpen,
    isTestingConnection,
    modelComboboxOpen,
    setModelComboboxOpen,
    isLoadingOpenAI,
    isLoadingClaude,
    isLoadingGroq,
    isDownloading,
    getProgress,
    modelOptions,
    requiresApiKey,
    ollamaEndpointChanged,
    isDoneDisabled,
    fetchOllamaModels,
    loadOpenRouterModels,
    loadBuiltinAiModels,
    handleSave,
    testCustomOpenAIConnection,
    handleInputClick,
    downloadRecommendedModel,
    filteredModels,
  };
}

export type ModelSettingsState = ReturnType<typeof useModelSettings>;
