import * as React from 'react';

import { Progress } from '@/components/ui/progress';

type LabProgressProps = React.ComponentProps<typeof Progress>;

function LabProgress({ className, ...props }: LabProgressProps) {
  return <Progress className={className} {...props} />;
}

export { LabProgress };
export type { LabProgressProps };
