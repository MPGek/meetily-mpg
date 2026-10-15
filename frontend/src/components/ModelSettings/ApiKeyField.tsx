import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Lock, Unlock, Eye, EyeOff } from 'lucide-react';

/** Masked API key input with lock/unlock and show/hide toggles (hosted providers). */
export function ApiKeyField({
  value,
  locked,
  visible,
  vibrating,
  onChange,
  onLockedClick,
  onToggleLock,
  onToggleVisible,
}: {
  value: string | null;
  locked: boolean;
  visible: boolean;
  vibrating: boolean;
  onChange: (value: string) => void;
  onLockedClick: () => void;
  onToggleLock: () => void;
  onToggleVisible: () => void;
}) {
  return (
    <div>
      <Label>API Key</Label>
      <div className="relative mt-1">
        <Input
          type={visible ? 'text' : 'password'}
          value={value || ''}
          onChange={(e) => onChange(e.target.value)}
          disabled={locked}
          placeholder="Enter your API key"
          className="pr-24"
        />
        {locked && value?.trim() && (
          <div
            onClick={onLockedClick}
            className="absolute inset-0 flex items-center justify-center bg-muted/50 rounded-md cursor-not-allowed"
          />
        )}
        <div className="absolute inset-y-0 right-0 pr-1 flex items-center space-x-1">
          {value?.trim() && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={onToggleLock}
              className={vibrating ? 'animate-vibrate text-red-500' : ''}
              title={locked ? 'Unlock to edit' : 'Lock to prevent editing'}
            >
              {locked ? <Lock /> : <Unlock />}
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={onToggleVisible}
          >
            {visible ? <EyeOff /> : <Eye />}
          </Button>
        </div>
      </div>
    </div>
  );
}
