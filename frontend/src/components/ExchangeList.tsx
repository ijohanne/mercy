'use client';

import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/Card';
import type { Exchange } from '@/lib/api';

export default function ExchangeList({ exchanges }: { exchanges: Exchange[] }) {
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
                  <th className="pb-2 pr-4 font-medium">Kingdom</th>
                  <th className="pb-2 pr-4 font-medium">X</th>
                  <th className="pb-2 pr-4 font-medium">Y</th>
                  <th className="pb-2 font-medium">Found at</th>
                </tr>
              </thead>
              <tbody>
                {exchanges.map((ex, i) => (
                  <tr key={i} className="border-b border-border/50 last:border-0">
                    <td className="py-2 pr-4">{ex.kingdom}</td>
                    <td className="py-2 pr-4">{ex.x}</td>
                    <td className="py-2 pr-4">{ex.y}</td>
                    <td className="py-2 text-muted-foreground">
                      {new Date(ex.found_at).toLocaleString()}
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
