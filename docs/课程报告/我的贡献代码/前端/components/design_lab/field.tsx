import * as React from 'react';

import { cn } from '@/lib/utils';

type LabFieldProps = React.ComponentProps<'label'> & {
  label: React.ReactNode;
  children: React.ReactNode;
};

function LabField({ label, children, className, ...props }: LabFieldProps) {
  return (
    <label
      data-slot="lab-field"
      className={cn('lab-field', className)}
      {...props}
    >
      <span>{label}</span>
      {children}
    </label>
  );
}

export { LabField };
export type { LabFieldProps };
