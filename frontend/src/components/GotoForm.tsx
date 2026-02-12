'use client';

import { useState } from 'react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Label } from '@/components/ui/Label';
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/Card';
import { proxyGet } from '@/lib/api';

interface DetectResult {
  found: boolean;
  threshold: number;
  pixel_x: number | null;
  pixel_y: number | null;
  score: number | null;
  game_dx: number | null;
  game_dy: number | null;
}

export default function GotoForm() {
  const [k, setK] = useState('');
  const [x, setX] = useState('');
  const [y, setY] = useState('');
  const [loading, setLoading] = useState(false);
  const [imgUrl, setImgUrl] = useState<string | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [detectResult, setDetectResult] = useState<DetectResult | null>(null);

  async function handleGoto(e: React.FormEvent) {
    e.preventDefault();
    if (!k || !x || !y) return;
    setLoading(true);
    setDetectResult(null);
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

  const [detectError, setDetectError] = useState<string | null>(null);

  async function handleDetect() {
    setDetecting(true);
    setDetectResult(null);
    setDetectError(null);
    try {
      const res = await proxyGet('detect');
      if (res.status === 400) {
        setDetectError('No screenshot available — use Go or Refresh first.');
        return;
      }
      if (!res.ok) return;
      const data: DetectResult = await res.json();
      setDetectResult(data);
    } finally {
      setDetecting(false);
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

        <div className="mt-4">
          <Button onClick={handleDetect} variant="outline" loading={detecting}>
            Detect match
          </Button>
        </div>

        {detectError && (
          <div className="mt-3 rounded-lg border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
            {detectError}
          </div>
        )}

        {detectResult && (
          <div className="mt-3 rounded-lg border border-border bg-muted/50 p-3 text-sm font-mono">
            {detectResult.score != null ? (
              <div className="space-y-1">
                <div>
                  {detectResult.found ? (
                    <span className="text-green-500 font-bold">MATCH</span>
                  ) : (
                    <span className="text-muted-foreground">No match</span>
                  )}
                  {' '}(threshold: {detectResult.threshold.toFixed(2)})
                </div>
                <div>
                  Best score: <span className={detectResult.found ? 'text-green-500 font-bold' : 'text-red-500'}>{detectResult.score.toFixed(4)}</span>
                </div>
                <div>Pixel: ({detectResult.pixel_x}, {detectResult.pixel_y})</div>
                <div>Offset from center: ({detectResult.game_dx}, {detectResult.game_dy}) game units</div>
                {detectResult.found && k && x && y && (
                  <div className="text-muted-foreground">
                    Est. coords: K:{k} X:{Number(x) + detectResult.game_dx!} Y:{Number(y) + detectResult.game_dy!}
                  </div>
                )}
              </div>
            ) : (
              <span className="text-muted-foreground">No template match at all</span>
            )}
          </div>
        )}

        {imgUrl && (
          <div className="mt-4 space-y-2">
            <div className="flex justify-end">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  const a = document.createElement('a');
                  a.href = imgUrl;
                  a.download = `goto_k${k}_${x}_${y}.png`;
                  a.click();
                }}
              >
                Download
              </Button>
            </div>
            <img
              src={imgUrl}
              alt="Goto screenshot"
              className="w-full rounded-lg border border-border"
            />
          </div>
        )}
      </CardContent>
    </Card>
  );
}
