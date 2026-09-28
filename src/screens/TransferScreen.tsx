import { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  CheckCircle,
  HardDrive,
  Loader2,
  Monitor,
  Send,
  ShieldAlert,
  ShieldCheck,
  Wifi,
} from 'lucide-react';
import { Button } from '@components/ui/Button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@components/ui/Tabs';
import {
  errorMessage,
  getDiscoveredDevices,
  listPairedDevices,
  sendWorkspace,
  startDiscovery,
} from '@lib/ipc';
import { isTransferFinished } from '@model';
import type { DiscoveredDevice, PairedDevice, WorkspaceManifest } from '@model';
import { canReceive, scopeLabel } from '@lib/trustScopes';
import { useAppStore } from '@store/useAppStore';

/**
 * What is known about one machine, combining the two sources that each know
 * half of it.
 *
 * A discovered device knows it is reachable and which key it holds; a paired
 * device knows what we are allowed to do with it. Neither is sufficient: a
 * device seen on the network that was never paired is exactly the case where
 * sending would be wrong, and a paired device that is not on the network cannot
 * be reached. A single object holding both makes the "can we send?" question one
 * comparison rather than a lookup that silently yields nothing.
 */
interface Candidate {
  discovered: DiscoveredDevice | null;
  paired: PairedDevice | null;
}

/** Whether a workspace may be sent to this machine, and why not if it may not. */
function sendability(candidate: Candidate): { ok: boolean; reason?: string } {
  if (!candidate.discovered) {
    return { ok: false, reason: 'Not on the network right now' };
  }
  if (!candidate.discovered.static_public_key) {
    // Without a Noise static key there is no handshake to do, so this is not a
    // temporary condition -- offering a button would always fail.
    return { ok: false, reason: 'Advertised no key, so it cannot be authenticated' };
  }
  if (!candidate.paired) {
    return { ok: false, reason: 'Not paired' };
  }
  if (candidate.paired.revoked) {
    return { ok: false, reason: 'Paired, then revoked' };
  }
  if (!canReceive(candidate.paired.trust_scopes)) {
    return { ok: false, reason: 'Not allowed to receive workspaces' };
  }
  return { ok: true };
}

export function TransferScreen() {
  const navigate = useNavigate();
  const params = useParams<{ workspaceId: string }>();
  const queryClient = useQueryClient();
  const workspaceId = params.workspaceId ?? '';

  const setTransfer = useAppStore((s) => s.setTransfer);
  const manifest = useAppStore((s) => s.manifest);
  const captureWarnings = useAppStore((s) => s.captureWarnings);

  const [selectedId, setSelectedId] = useState<string | null>(null);

  // Discovery is started rather than only read, because a device that has
  // never been looked for is not in the list however long the user waits.
  useQuery({
    queryKey: ['discovery-start'],
    queryFn: startDiscovery,
    // Discovery is a one-shot kick-off; a refetch would re-announce the service.
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });

  const discovered = useQuery({
    queryKey: ['discovered-devices'],
    queryFn: getDiscoveredDevices,
    // Devices come and go. Polling is the honest way to show that, rather than
    // leaving a device on screen after it has gone to sleep.
    refetchInterval: 4000,
  });

  const paired = useQuery({ queryKey: ['paired-devices'], queryFn: listPairedDevices });

  const send = useMutation({
    mutationFn: (deviceId: string) => sendWorkspace(workspaceId, deviceId),
    onSuccess: (outcome) => {
      setTransfer(outcome.transfer);
      // The destination is a different machine, so its preflight is not this
      // app's to run. Navigating onward from a send would imply otherwise.
      void queryClient.invalidateQueries({ queryKey: ['workspaces'] });
    },
  });

  const candidates: Candidate[] = (() => {
    const found = discovered.data ?? [];
    const known = paired.data ?? [];
    const byId = new Map<string, Candidate>();
    for (const d of found) {
      byId.set(d.device_id, { discovered: d, paired: null });
    }
    for (const p of known) {
      const existing = byId.get(p.id);
      if (existing) {
        existing.paired = p;
      } else {
        byId.set(p.id, { discovered: null, paired: p });
      }
    }
    // Paired-and-present first: those are the ones the user most likely wants.
    return Array.from(byId.values()).sort((a, b) => {
      const aSendable = sendability(a).ok ? 0 : 1;
      const bSendable = sendability(b).ok ? 0 : 1;
      if (aSendable !== bSendable) {
        return aSendable - bSendable;
      }
      const aName = a.paired?.name ?? a.discovered?.name ?? '';
      const bName = b.paired?.name ?? b.discovered?.name ?? '';
      return aName.localeCompare(bName);
    });
  })();

  const selected = candidates.find((c) => c.paired?.id === selectedId) ?? null;
  const selectedVerdict = selected ? sendability(selected) : null;

  // Hoisted out of the JSX below. Inside the "send" card, the surrounding
  // `send.isSuccess &&` branch narrows `send.data` to a completed outcome, so a
  // sibling expression reading it again is typed `never` -- and a button whose
  // enablement depended on it would be checked against the wrong type.
  const outcome = send.data;
  const sendFinished = outcome !== undefined && isTransferFinished(outcome.transfer.status);

  return (
    <div className="space-y-6 max-w-3xl">
      <div>
        <p className="text-[13px] text-muted-foreground">
          Check what was captured, then choose where it goes.
        </p>
      </div>

      {captureWarnings.length > 0 && (
        <div
          role="alert"
          className="flex items-start gap-3 p-4 bg-warning-bg border border-warning-border rounded-lg"
        >
          <AlertTriangle className="w-5 h-5 text-warning-fg mt-0.5 flex-shrink-0" />
          <div>
            <h4 className="font-medium text-warning-fg">
              {captureWarnings.length} thing{captureWarnings.length === 1 ? '' : 's'} you asked
              for could not be captured
            </h4>
            <ul className="text-sm text-fg-muted mt-1 list-disc list-inside">
              {captureWarnings.map((warning, i) => (
                <li key={i}>{warning}</li>
              ))}
            </ul>
          </div>
        </div>
      )}

      {/* --- What was captured ------------------------------------------- */}
      <Card>
        <CardHeader>
          <CardTitle>Captured workspace</CardTitle>
          <CardDescription>
            The sealed manifest that will be sent. This is the whole of it.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Tabs defaultValue="summary">
            <TabsList>
              <TabsTrigger value="summary">Summary</TabsTrigger>
              <TabsTrigger value="manifest">Manifest</TabsTrigger>
            </TabsList>
            <TabsContent value="summary" className="space-y-3 pt-2">
              <div className="grid grid-cols-2 gap-3 text-sm">
                <Stat label="Name" value={manifest?.workspace.name ?? '(not loaded)'} />
                <Stat label="Captured" value={formatTime(manifest?.workspace.captured_at)} />
                <Stat
                  label="Projects"
                  value={String(manifest?.projects.length ?? 0)}
                />
                <Stat
                  label="Applications"
                  value={String(manifest?.applications.length ?? 0)}
                />
                <Stat
                  label="Requirements"
                  value={String(countRequirements(manifest))}
                />
                <Stat
                  label="Source device"
                  value={`${manifest?.workspace.source_device.os ?? '?'} ${
                    manifest?.workspace.source_device.os_version ?? ''
                  }`.trim()}
                />
              </div>
              {manifest && manifest.projects.length > 0 && (
                <ul className="text-sm space-y-1 mt-2">
                  {manifest.projects.map((p) => (
                    <li key={p.id} className="flex items-center gap-2">
                      <HardDrive className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                      <span>{p.name}</span>
                      {p.git && (
                        <span className="text-muted-foreground font-mono text-xs">
                          {p.git.branch}
                          {p.git.dirty_worktree ? ' · uncommitted changes' : ''}
                        </span>
                      )}
                    </li>
                  ))}
                </ul>
              )}
              {manifest && !manifest.policy.automatic_command_execution && (
                <p className="text-xs text-muted-foreground">
                  Commands, if any, are offered on the destination for you to run. None are run
                  automatically, here or there.
                </p>
              )}
            </TabsContent>
            <TabsContent value="manifest" className="pt-2">
              {manifest ? (
                <pre className="text-xs font-mono bg-muted rounded-lg p-3 overflow-auto max-h-96">
                  {JSON.stringify(manifest, null, 2)}
                </pre>
              ) : (
                <p className="text-sm text-muted-foreground">
                  The manifest is in the store for this session. If the app was restarted, open
                  the workspace from the list.
                </p>
              )}
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>

      {/* --- Where to send it -------------------------------------------- */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Destinations</CardTitle>
              <CardDescription>
                Only a machine that is on the network, paired, and allowed to receive can be sent
                this.
              </CardDescription>
            </div>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void discovered.refetch()}
              loading={discovered.isFetching}
            >
              Refresh
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          {discovered.isLoading && (
            <p className="text-sm text-muted-foreground">Looking for devices…</p>
          )}
          {discovered.isError && (
            <p role="alert" className="text-sm text-destructive">
              Discovery failed: {errorMessage(discovered.error)}
            </p>
          )}
          {!discovered.isLoading && !discovered.isError && candidates.length === 0 && (
            <div className="text-center py-8">
              <Monitor className="w-12 h-12 text-muted-foreground mx-auto mb-4" />
              <h3 className="font-medium">No other devices</h3>
              <p className="text-muted-foreground mt-1">
                Pair one from the Devices screen, or open this app on the other machine.
              </p>
            </div>
          )}
          {candidates.length > 0 && (
            <div className="space-y-3">
              {candidates.map((candidate) => {
                const verdict = sendability(candidate);
                const key = candidate.paired?.id ?? candidate.discovered?.device_id ?? '';
                const name = candidate.paired?.name ?? candidate.discovered?.name ?? 'Unknown device';
                return (
                  <button
                    key={key}
                    onClick={() => verdict.ok && setSelectedId(key)}
                    disabled={!verdict.ok}
                    aria-pressed={selectedId === key}
                    className={`w-full p-4 rounded-lg border transition-all text-left flex items-center gap-4 ${
                      selectedId === key
                        ? 'border-primary bg-primary/5'
                        : 'border-border hover:bg-muted/50'
                    } ${verdict.ok ? '' : 'opacity-60 cursor-not-allowed'}`}
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="font-medium truncate">{name}</span>
                        {candidate.discovered ? (
                          <Badge variant="success">
                            <Wifi className="w-3 h-3 mr-1" />
                            On network
                          </Badge>
                        ) : (
                          <Badge variant="secondary">
                            Offline
                          </Badge>
                        )}
                        {candidate.paired && !candidate.paired.revoked && (
                          <Badge variant="outline">
                            Paired
                          </Badge>
                        )}
                        {candidate.paired?.revoked && (
                          <Badge variant="danger">
                            Revoked
                          </Badge>
                        )}
                      </div>
                      <div className="flex items-center gap-4 text-sm text-muted-foreground mt-1 flex-wrap">
                        <span>
                          {candidate.paired?.os ?? candidate.discovered?.os ?? 'unknown OS'}
                        </span>
                        {candidate.paired && (
                          <span>
                            Allowed:{' '}
                            {candidate.paired.trust_scopes.map(scopeLabel).join(', ') || 'nothing'}
                          </span>
                        )}
                      </div>
                      {!verdict.ok && verdict.reason && (
                        <p className="text-xs text-muted-foreground mt-1">{verdict.reason}</p>
                      )}
                    </div>
                    {verdict.ok && selectedId !== key && (
                      <ShieldCheck className="w-5 h-5 text-muted-foreground flex-shrink-0" />
                    )}
                    {selectedId === key && (
                      <CheckCircle className="w-5 h-5 text-primary flex-shrink-0" />
                    )}
                  </button>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* --- The send itself --------------------------------------------- */}
      {selected && selectedVerdict?.ok && (
        <Card className="border-primary">
          <CardContent className="pt-6 space-y-4">
            <div className="flex items-center gap-3 p-4 bg-primary/5 rounded-lg">
              <Send className="w-6 h-6 text-primary flex-shrink-0" />
              <div>
                <h3 className="font-semibold">
                  Send to {selected.paired?.name ?? selected.discovered?.name}
                </h3>
                <p className="text-sm text-muted-foreground">
                  End to end encrypted over an authenticated direct connection. No server is
                  involved.
                </p>
              </div>
            </div>

            {send.isSuccess && (
              <div
                className={`p-4 rounded-lg border ${
                  send.data.succeeded
                    ? 'bg-success-bg border-success-border'
                    : 'bg-destructive/10 border-destructive/30'
                }`}
              >
                <div className="flex items-start gap-3">
                  {send.data.succeeded ? (
                    <CheckCircle className="w-5 h-5 text-success mt-0.5 flex-shrink-0" />
                  ) : (
                    <AlertTriangle className="w-5 h-5 text-destructive mt-0.5 flex-shrink-0" />
                  )}
                  <div>
                    <h4 className="font-medium">
                      {send.data.succeeded
                        ? 'Delivered'
                        : `Did not complete: ${send.data.transfer.status}`}
                    </h4>
                    <p className="text-sm text-muted-foreground mt-1">
                      {send.data.succeeded
                        ? `${send.data.bytesSent.toLocaleString()} bytes sent. Preflight runs on the destination, not here.`
                        : send.data.transfer.error ?? 'The destination did not acknowledge it.'}
                    </p>
                    {send.data.transfer.error && !send.data.succeeded && (
                      <p className="text-xs text-muted-foreground mt-1 font-mono">
                        {send.data.transfer.error}
                      </p>
                    )}
                  </div>
                </div>
              </div>
            )}

            {send.isError && (
              <div
                role="alert"
                className="flex items-start gap-3 p-4 bg-destructive/10 border border-destructive/30 rounded-lg"
              >
                <ShieldAlert className="w-5 h-5 text-destructive mt-0.5 flex-shrink-0" />
                <div>
                  <h4 className="font-medium text-destructive">The send could not start</h4>
                  <p className="text-sm text-destructive/90 mt-1">
                    {errorMessage(send.error)}
                  </p>
                </div>
              </div>
            )}

            <div className="flex justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => {
                  send.reset();
                  setSelectedId(null);
                }}
                disabled={send.isPending || send.isSuccess}
              >
                {send.isSuccess ? 'Back' : 'Cancel'}
              </Button>
              <Button
                onClick={() => selected.paired && send.mutate(selected.paired.id)}
                loading={send.isPending}
                disabled={send.isSuccess || !selected.paired || sendFinished}
              >
                {send.isPending ? (
                  <>
                    <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                    Sending…
                  </>
                ) : (
                  <>
                    <Send className="w-4 h-4 mr-2" />
                    Send workspace
                  </>
                )}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {/* --- The other path: this machine -------------------------------- */}
      <Card>
        <CardHeader>
          <CardTitle>Restore on this machine</CardTitle>
          <CardDescription>
            Skip the transfer. Check what is missing here, then restore.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-between gap-4">
            <p className="text-sm text-muted-foreground">
              Useful for a workspace you captured here, or one that was already sent to this
              machine. Preflight runs against what is installed here.
            </p>
            <Button
              variant="secondary"
              onClick={() => navigate(`/preflight/${workspaceId}`)}
              disabled={workspaceId.length === 0}
            >
              Continue
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border p-3">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="font-medium truncate">{value}</div>
    </div>
  );
}

function countRequirements(manifest: WorkspaceManifest | null): number {
  const requirements = manifest?.requirements;
  if (!requirements) {
    return 0;
  }
  // `environment` is a single object, not a list, so it contributes 0 here. It
  // is counted by its `presence_only` list instead, which is the part a user
  // would recognise as "things to check".
  const lists: unknown[][] = [
    requirements.applications,
    requirements.runtimes,
    requirements.cli_tools,
    requirements.identities,
    requirements.services,
    requirements.environment?.presence_only ?? [],
  ];
  return lists.reduce((total, list) => total + (Array.isArray(list) ? list.length : 0), 0);
}

function formatTime(value: string | undefined): string {
  if (!value) {
    return '(not loaded)';
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}
