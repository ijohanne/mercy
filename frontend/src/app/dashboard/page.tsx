'use client';

import { useState, useEffect, useCallback } from 'react';
import StatusPanel from '@/components/StatusPanel';
import ActionButtons from '@/components/ActionButtons';
import ExchangeList from '@/components/ExchangeList';
import ScreenshotView from '@/components/ScreenshotView';
import GotoForm from '@/components/GotoForm';
import ManualScanForm from '@/components/ManualScanForm';
import { proxyGet } from '@/lib/api';
import type { StatusResponse, Exchange } from '@/lib/api';

export default function DashboardPage() {
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [exchanges, setExchanges] = useState<Exchange[]>([]);

  const fetchData = useCallback(async () => {
    try {
      const [statusRes, exchangesRes] = await Promise.all([
        proxyGet('status'),
        proxyGet('exchanges'),
      ]);
      if (statusRes.ok) setStatus(await statusRes.json());
      if (exchangesRes.ok) setExchanges(await exchangesRes.json());
    } catch {
      // Backend might be down
    }
  }, []);

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 2000);
    return () => clearInterval(interval);
  }, [fetchData]);

  return (
    <div className="space-y-6">
      <StatusPanel status={status} />
      <ActionButtons status={status} onAction={fetchData} />
      <div className="grid gap-6 lg:grid-cols-2">
        <ExchangeList exchanges={exchanges} />
        <div className="space-y-6">
          <ScreenshotView />
          <GotoForm />
          <ManualScanForm status={status} onAction={fetchData} />
        </div>
      </div>
    </div>
  );
}
