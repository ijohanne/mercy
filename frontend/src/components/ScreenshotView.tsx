'use client';

import { useState, useCallback } from 'react';
import { Button } from '@/components/ui/Button';
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/Card';

export default function ScreenshotView() {
  const [imgUrl, setImgUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const res = await fetch('/api/proxy/screenshot', { cache: 'no-store' });
      if (!res.ok) return;
      const blob = await res.blob();
      if (imgUrl) URL.revokeObjectURL(imgUrl);
      setImgUrl(URL.createObjectURL(blob));
    } finally {
      setLoading(false);
    }
  }, [imgUrl]);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center justify-between">
          Screenshot
          <Button onClick={refresh} variant="outline" size="sm" loading={loading}>
            Refresh
          </Button>
        </CardTitle>
      </CardHeader>
      <CardContent>
        {imgUrl ? (
          <img
            src={imgUrl}
            alt="Browser screenshot"
            className="w-full rounded-lg border border-border"
          />
        ) : (
          <p className="text-muted-foreground text-sm">
            Click Refresh to capture a screenshot.
          </p>
        )}
      </CardContent>
    </Card>
  );
}
