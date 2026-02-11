export default function DashboardLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-screen bg-background">
      <header className="border-b border-border bg-card px-4 py-3 sm:px-6">
        <h1 className="text-lg font-semibold text-foreground">Mercy Admin</h1>
      </header>
      <main className="mx-auto max-w-5xl p-4 sm:p-6">{children}</main>
    </div>
  );
}
