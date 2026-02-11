'use client';

import { useState } from 'react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Label } from '@/components/ui/Label';
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/Card';

export default function GotoForm() {
  const [k, setK] = useState('');
  const [x, setX] = useState('');
  const [y, setY] = useState('');
  const [loading, setLoading] = useState(false);
  const [imgUrl, setImgUrl] = useState<string | null>(null);

  async function handleGoto(e: React.FormEvent) {
    e.preventDefault();
    if (!k || !x || !y) return;
    setLoading(true);
    try {
      const res = await fetch(`/api/proxy/goto?k=${k}&x=${x}&y=${y}`, { cache: 'no-store' });
      if (!res.ok) return;
      const blob = await res.blob();
      if (imgUrl) URL.revokeObjectURL(imgUrl);
      setImgUrl(URL.createObjectURL(blob));
    } finally {
      setLoading(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Navigate</CardTitle>
      </CardHeader>
      <CardContent>
        <form onSubmit={handleGoto} className="space-y-4">
          <div className="grid grid-cols-3 gap-2 sm:flex sm:items-end sm:gap-3">
            <div className="space-y-1">
              <Label htmlFor="goto-k">K</Label>
              <Input id="goto-k" value={k} onChange={(e) => setK(e.target.value)} placeholder="111" />
            </div>
            <div className="space-y-1">
              <Label htmlFor="goto-x">X</Label>
              <Input id="goto-x" value={x} onChange={(e) => setX(e.target.value)} placeholder="512" />
            </div>
            <div className="space-y-1">
              <Label htmlFor="goto-y">Y</Label>
              <Input id="goto-y" value={y} onChange={(e) => setY(e.target.value)} placeholder="512" />
            </div>
            <Button type="submit" loading={loading} className="col-span-3 sm:col-span-1">
              Go
            </Button>
          </div>
        </form>
        {imgUrl && (
          <img
            src={imgUrl}
            alt="Goto screenshot"
            className="mt-4 w-full rounded-lg border border-border"
          />
        )}
      </CardContent>
    </Card>
  );
}
