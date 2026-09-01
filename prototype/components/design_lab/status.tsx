import * as React from 'react';

import { cn } from '@/lib/utils';

import {
  LAB_BADGE_VARIANT_CLASSES,
  LAB_STATUS_PILL_CLASSES,
  type LabBadgeVariant,
  type LabStatusTone,
} from './tokens';

type LabStatusOrbProps = React.ComponentProps<'span'> & {
  offline?: boolean;
};

function LabStatusOrb({
  offline = false,
  className,
  ...props
}: LabStatusOrbProps) {
  return (
    <span
      data-slot="lab-status-orb"
      className={cn('status-orb', offline && 'offline', className)}
      {...props}
    />
  );
}

type LabStatusPillProps = React.ComponentProps<'span'> & {
  tone?: LabStatusTone;
};

function LabStatusPill({
  tone = 'neutral',
  className,
  ...props
}: LabStatusPillProps) {
  return (
    <span
      data-slot="lab-status-pill"
      className={cn('tiny-status', LAB_STATUS_PILL_CLASSES[tone], className)}
      {...props}
    />
  );
}

type LabBadgeTone =
  | 'idle'
  | 'running'
  | 'completed'
  | 'stopped'
  | 'interrupted'
  | 'pass'
  | 'fail'
  | 'waiting';

type LabBadgeProps = React.ComponentProps<'span'> & {
  variant: LabBadgeVariant;
  tone: LabBadgeTone;
};

function LabBadge({ variant, tone, className, ...props }: LabBadgeProps) {
  const toneClass = variant === 'experiment' ? `status-${tone}` : tone;

  return (
    <span
      data-slot="lab-badge"
      className={cn(LAB_BADGE_VARIANT_CLASSES[variant], toneClass, className)}
      {...props}
    />
  );
}

export { LabBadge, LabStatusOrb, LabStatusPill };
export type {
  LabBadgeProps,
  LabBadgeTone,
  LabStatusOrbProps,
  LabStatusPillProps,
};
