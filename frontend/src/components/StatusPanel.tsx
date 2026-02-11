'use client';

import { Badge } from '@/components/ui/Badge';
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/Card';
import type { StatusResponse } from '@/lib/api';

const phaseVariant: Record<string, 'default' | 'secondary' | 'success' | 'warning' | 'destructive'> = {
  idle: 'secondary',
  preparing: 'warning',
  ready: 'default',
  scanning: 'success',
  paused: 'warning',
};

export default function StatusPanel({ status }: { status: StatusResponse | null }) {
  if (!status) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Status</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-muted-foreground text-sm">Loading...</p>
        </CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center justify-between">
          Status
          <Badge variant={phaseVariant[status.phase] || 'secondary'}>
            {status.phase}
          </Badge>
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-2 gap-4 text-sm">
          <div>
            <p className="text-muted-foreground">Kingdom</p>
            <p className="font-medium">{status.current_kingdom ?? '---'}</p>
          </div>
          <div>
            <p className="text-muted-foreground">Exchanges found</p>
            <p className="font-medium">{status.exchanges_found}</p>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
