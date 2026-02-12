'use client';

import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/Card';
import type { Exchange } from '@/lib/api';

export default function ExchangeList({ exchanges }: { exchanges: Exchange[] }) {
  function downloadScreenshot(index: number, ex: Exchange) {
    const a = document.createElement('a');
    a.href = `/api/proxy/exchanges/${index}/screenshot`;
    a.download = `exchange_k${ex.kingdom}_${ex.x}_${ex.y}.png`;
    a.click();
  }

  function copyCoords(ex: Exchange) {
    navigator.clipboard.writeText(`K:${ex.kingdom} X:${ex.x} Y:${ex.y}`);
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Exchanges ({exchanges.length})</CardTitle>
      </CardHeader>
      <CardContent>
        {exchanges.length === 0 ? (
          <p className="text-muted-foreground text-sm">No exchanges found yet.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border text-left text-muted-foreground">
                  <th className="pb-2 pr-4 font-medium">Coords</th>
                  <th className="pb-2 pr-4 font-medium">Status</th>
                  <th className="pb-2 pr-4 font-medium">Scan time</th>
                  <th className="pb-2 pr-4 font-medium">Found at</th>
                  <th className="pb-2 font-medium"></th>
                </tr>
              </thead>
              <tbody>
                {exchanges.map((ex, i) => (
                  <tr
                    key={i}
                    className={`border-b border-border/50 last:border-0${!ex.confirmed ? ' opacity-60' : ''}`}
                  >
                    <td className="py-2 pr-4">
                      <button
                        type="button"
                        onClick={() => copyCoords(ex)}
                        className="font-mono text-xs hover:underline cursor-pointer"
                        title="Click to copy"
                      >
                        K:{ex.kingdom} X:{ex.x} Y:{ex.y}
                      </button>
                    </td>
                    <td className="py-2 pr-4">
                      {ex.confirmed ? (
                        <span className="inline-block rounded bg-green-900/40 px-1.5 py-0.5 text-xs text-green-400">
                          Confirmed
                        </span>
                      ) : (
                        <span className="inline-block rounded bg-yellow-900/40 px-1.5 py-0.5 text-xs text-yellow-400">
                          Estimate
                        </span>
                      )}
                    </td>
                    <td className="py-2 pr-4 text-muted-foreground">
                      {ex.scan_duration_secs != null
                        ? `${Math.round(ex.scan_duration_secs)}s`
                        : '\u2014'}
                    </td>
                    <td className="py-2 pr-4 text-muted-foreground">
                      {new Date(ex.found_at).toLocaleString()}
                    </td>
                    <td className="py-2">
                      <button
                        type="button"
                        onClick={() => downloadScreenshot(i, ex)}
                        className="text-xs text-primary hover:underline"
                      >
                        Screenshot
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
