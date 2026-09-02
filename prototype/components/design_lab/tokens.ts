export const LAB_ACCENT_CLASSES = {
  cyan: 'cyan',
  violet: 'violet',
  green: 'green',
  blue: 'blue',
  orange: 'orange',
} as const;

export type LabAccent = keyof typeof LAB_ACCENT_CLASSES;

export const LAB_STATUS_PILL_CLASSES = {
  neutral: '',
  online: 'ok',
  offline: 'bad',
  warning: 'warn',
} as const;

export type LabStatusTone = keyof typeof LAB_STATUS_PILL_CLASSES;

export const LAB_BADGE_VARIANT_CLASSES = {
  experiment: 'experiment-badge',
  proof: 'proof-badge',
} as const;

export type LabBadgeVariant = keyof typeof LAB_BADGE_VARIANT_CLASSES;

export const LAB_BENCHMARK_SERIES_COLORS = [
  'var(--benchmark-series-a)',
  'var(--benchmark-series-b)',
  'var(--benchmark-series-c)',
] as const;
