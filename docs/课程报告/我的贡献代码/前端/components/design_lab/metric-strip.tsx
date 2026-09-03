import * as React from 'react';

import { cn } from '@/lib/utils';

import { LAB_ACCENT_CLASSES, type LabAccent } from './tokens';

type LabMetricItem = {
  id?: string;
  label: React.ReactNode;
  value: React.ReactNode;
  suffix?: React.ReactNode;
  accent?: LabAccent;
};

type LabMetricProps = LabMetricItem & {
  className?: string;
};

function LabMetric({
  label,
  value,
  suffix,
  accent = 'green',
  className,
}: LabMetricProps) {
  return (
    <div data-slot="lab-metric" className={className}>
      <span className={cn('metric-dot', LAB_ACCENT_CLASSES[accent])} />
      <p>
        <small>{label}</small>
        <strong>{value}</strong>
        {suffix && <em>{suffix}</em>}
      </p>
    </div>
  );
}

type LabMetricStripProps = React.ComponentProps<'div'> & {
  metrics: readonly LabMetricItem[];
};

function LabMetricStrip({ metrics, className, ...props }: LabMetricStripProps) {
  return (
    <div
      className={cn('metric-strip', className)}
      data-count={metrics.length}
      {...props}
    >
      {metrics.map((metric, index) => (
        <LabMetric key={metric.id ?? index} {...metric} />
      ))}
    </div>
  );
}

export { LabMetric, LabMetricStrip };
export type { LabMetricItem, LabMetricProps, LabMetricStripProps };
