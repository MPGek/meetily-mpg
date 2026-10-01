import { Button } from '@/components/ui/button';
import { RefreshCw, Check, ChevronsUpDown } from 'lucide-react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import type { ModelConfig } from '@/lib/ipc/settings';
import { cn } from '@/lib/utils';
import type { ModelSettingsState } from '@/hooks/useModelSettings';

/**
 * Model combobox shown next to the provider select for every provider except
 * builtin-ai and custom-openai (hosted providers and Ollama share it).
 */
export function ModelCombobox({ settings }: { settings: ModelSettingsState }) {
  const {
    modelConfig,
    setModelConfig,
    modelComboboxOpen,
    setModelComboboxOpen,
    isLoadingOpenRouter,
    isLoadingOpenAI,
    isLoadingClaude,
    isLoadingGroq,
    modelOptions,
  } = settings;

  return (
    <Popover open={modelComboboxOpen} onOpenChange={setModelComboboxOpen} modal={true}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          role="combobox"
          aria-expanded={modelComboboxOpen}
          className="flex-1 max-w-[200px] justify-between font-normal"
        >
          <span className="truncate">
            {modelConfig.model || "Select model..."}
          </span>
          <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[250px] p-0" align="start">
        <Command>
          <CommandInput placeholder="Search models..." />
          <CommandList className="max-h-[300px]">
            {(modelConfig.provider === 'openrouter' && isLoadingOpenRouter) ||
             (modelConfig.provider === 'openai' && isLoadingOpenAI) ||
             (modelConfig.provider === 'claude' && isLoadingClaude) ||
             (modelConfig.provider === 'groq' && isLoadingGroq) ? (
              <div className="py-6 text-center text-sm text-muted-foreground">
                <RefreshCw className="mx-auto h-4 w-4 animate-spin mb-2" />
                Loading models...
              </div>
            ) : (
              <>
                <CommandEmpty>No models found.</CommandEmpty>
                <CommandGroup>
                  {modelOptions[modelConfig.provider]?.map((model) => (
                    <CommandItem
                      key={model}
                      value={model}
                      onSelect={(currentValue) => {
                        setModelConfig((prev: ModelConfig) => ({ ...prev, model: currentValue }));
                        setModelComboboxOpen(false);
                      }}
                    >
                      <Check
                        className={cn(
                          "mr-2 h-4 w-4",
                          modelConfig.model === model ? "opacity-100" : "opacity-0"
                        )}
                      />
                      <span className="truncate">{model}</span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
