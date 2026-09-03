import * as React from 'react';

import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

type LabButtonTone = 'success' | 'info' | 'danger' | 'secondary';

const LAB_BUTTON_TONES: Record<
  LabButtonTone,
  {
    className: string;
    variant?: React.ComponentProps<typeof Button>['variant'];
  }
> = {
  success: { className: 'op-button set' },
  info: { className: 'op-button get', variant: 'outline' },
  danger: { className: 'op-button delete', variant: 'destructive' },
  secondary: { className: 'op-button keys', variant: 'secondary' },
};

type LabButtonProps = React.ComponentProps<typeof Button> & {
  tone?: LabButtonTone;
};

function LabButton({ tone, className, variant, ...props }: LabButtonProps) {
  const toneConfig = tone ? LAB_BUTTON_TONES[tone] : undefined;

  return (
    <Button
      variant={variant ?? toneConfig?.variant}
      className={cn(toneConfig?.className, className)}
      {...props}
    />
  );
}

export { LabButton };
export type { LabButtonProps, LabButtonTone };
