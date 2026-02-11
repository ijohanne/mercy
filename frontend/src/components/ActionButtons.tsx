'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button } from '@/components/ui/Button';
import { proxyPost } from '@/lib/api';
import type { StatusResponse } from '@/lib/api';

export default function ActionButtons({
  status,
  onAction,
}: {
  status: StatusResponse | null;
  onAction: () => void;
}) {
  const router = useRouter();
  const [loading, setLoading] = useState<string | null>(null);
  const phase = status?.phase || 'idle';

  async function doAction(action: string) {
    setLoading(action);
    try {
      await proxyPost(action);
      onAction();
    } finally {
      setLoading(null);
    }
  }

  async function handleLogout() {
    await fetch('/api/auth/logout', { method: 'POST' });
    router.push('/login');
  }

  return (
    <div className="flex flex-wrap gap-2">
      <Button
        onClick={() => doAction('prepare')}
        disabled={phase !== 'idle'}
        loading={loading === 'prepare'}
        variant="outline"
      >
        Prepare
      </Button>
      <Button
        onClick={() => doAction('start')}
        disabled={phase !== 'ready' && phase !== 'idle' && phase !== 'paused'}
        loading={loading === 'start'}
      >
        {phase === 'paused' ? 'Resume' : 'Start'}
      </Button>
      <Button
        onClick={() => doAction('pause')}
        disabled={phase !== 'scanning'}
        loading={loading === 'pause'}
        variant="secondary"
      >
        Pause
      </Button>
      <Button
        onClick={() => doAction('stop')}
        disabled={phase !== 'scanning' && phase !== 'paused'}
        loading={loading === 'stop'}
        variant="destructive"
      >
        Stop
      </Button>
      <Button
        onClick={() => doAction('logout')}
        disabled={phase === 'scanning' || phase === 'preparing'}
        loading={loading === 'logout'}
        variant="outline"
      >
        Kill Browser
      </Button>
      <div className="flex-1" />
      <Button onClick={handleLogout} variant="ghost">
        Sign out
      </Button>
    </div>
  );
}
