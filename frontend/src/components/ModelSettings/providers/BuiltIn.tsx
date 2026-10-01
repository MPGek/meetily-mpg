import { BuiltInModelManager } from '@/components/BuiltInModelManager';

/** Built-in AI model picker section. */
export function BuiltInProvider({
  selectedModel,
  layout,
  onModelSelect,
}: {
  selectedModel: string;
  layout: 'inline' | 'dialog';
  onModelSelect: (model: string) => void;
}) {
  return (
    <div className="mt-6">
      <BuiltInModelManager
        selectedModel={selectedModel}
        layout={layout}
        onModelSelect={onModelSelect}
      />
    </div>
  );
}
