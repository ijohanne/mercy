'use client';

import { useState } from 'react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Label } from '@/components/ui/Label';
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/Card';
import { proxyPostJson } from '@/lib/api';
import type { StatusResponse } from '@/lib/api';

export default function ManualScanForm({
  status,
  onAction,
}: {
  status: StatusResponse | null;
  onAction: () => void;
}) {
  const [kingdom, setKingdom] = useState('');
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  const disabled =
    !status ||
    status.phase === 'preparing' ||
    status.manual_scan_kingdom != null;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!kingdom || disabled) return;
    setLoading(true);
    setMessage(null);
    try {
      const res = await proxyPostJson('scan-kingdom', { kingdom: Number(kingdom) });
      if (res.ok) {
        const data = await res.json();
        setMessage({ text: `Scan ${data.status} for kingdom ${kingdom}`, error: false });
        onAction();
      } else if (res.status === 409) {
        setMessage({ text: 'Cannot scan now (browser is preparing)', error: true });
      } else {
        setMessage({ text: `Error: ${res.status}`, error: true });
      }
    } catch {
      setMessage({ text: 'Request failed', error: true });
    } finally {
      setLoading(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Manual Scan</CardTitle>
      </CardHeader>
      <CardContent>
        {status?.manual_scan_kingdom != null && (
          <div className="mb-4 rounded-lg border border-warning/50 bg-warning/10 p-3 text-sm text-warning-foreground">
            Scanning kingdom {status.manual_scan_kingdom}...
          </div>
        )}
        <form onSubmit={handleSubmit} className="flex items-end gap-3">
          <div className="space-y-1">
            <Label htmlFor="scan-kingdom">Kingdom</Label>
            <Input
              id="scan-kingdom"
              value={kingdom}
              onChange={(e) => setKingdom(e.target.value)}
              placeholder="111"
              className="w-24"
            />
          </div>
          <Button type="submit" loading={loading} disabled={disabled}>
            Scan
          </Button>
        </form>
        {message && (
          <div
            className={`mt-3 rounded-lg border p-3 text-sm ${
              message.error
                ? 'border-destructive/50 bg-destructive/10 text-destructive'
                : 'border-border bg-muted/50 text-foreground'
            }`}
          >
            {message.text}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
