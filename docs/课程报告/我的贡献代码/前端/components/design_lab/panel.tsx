import * as React from 'react';

import { cn } from '@/lib/utils';

type LabPanelProps = React.ComponentProps<'article'>;

function LabPanel({ className, ...props }: LabPanelProps) {
  return (
    <article
      data-slot="lab-panel"
      className={cn('panel', className)}
      {...props}
    />
  );
}

type LabPanelHeaderProps = {
  icon: React.ReactNode;
  eyebrow: React.ReactNode;
  title: React.ReactNode;
  action?: React.ReactNode;
  tone?: string;
  className?: string;
};

function LabPanelHeader({
  icon,
  eyebrow,
  title,
  action,
  tone,
  className,
}: LabPanelHeaderProps) {
  return (
    <div className={cn('panel-heading', className)}>
      <div>
        <span className={cn('panel-kicker', tone)}>
          {icon}
          {eyebrow}
        </span>
        <h2>{title}</h2>
      </div>
      {action}
    </div>
  );
}

export { LabPanel, LabPanelHeader };
export type { LabPanelProps, LabPanelHeaderProps };
