import { Monitor, Loader2 } from 'lucide-react';

export function LoadingScreen() {
  return (
    <div className="flex h-screen items-center justify-center bg-background">
      <div className="flex flex-col items-center gap-6 text-center">
        <div className="w-16 h-16 rounded-xl bg-primary/10 flex items-center justify-center">
          <Monitor className="w-8 h-8 text-primary" />
        </div>
        <div className="flex flex-col gap-2">
          <h1 className="text-2xl font-bold text-foreground">Workspace Clone</h1>
          <p className="text-muted-foreground">Initializing...</p>
        </div>
        <Loader2 className="w-8 h-8 text-primary animate-spin" />
      </div>
    </div>
  );
}