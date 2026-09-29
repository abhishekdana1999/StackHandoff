import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowDownToLine,
  ChevronRight,
  Clock,
  Download,
  GitBranch,
  HardDrive,
  Monitor,
  Plus,
  Trash2,
  Upload,
  X,
} from 'lucide-react';
import { EmptyStateCard } from '@components/EmptyState';
import { Button } from '@components/ui/Button';
import { Card, CardContent } from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { Dialog, DialogContent } from '@components/ui/Dialog';
import { Input } from '@components/ui/Input';
import { Label } from '@components/ui/Label';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@components/ui/Tabs';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@components/ui/Table';
import {
  deleteWorkspace,
  dismissIncomingTransfer,
  errorMessage,
  getIncomingTransfers,
  getTransferHistory,
  getWorkspaceRestoreRuns,
  getWorkspaceSnapshots,
  listWorkspaces,
} from '@lib/ipc';
import type { IncomingTransfer, WorkspaceRecord } from '@model';
import { useAppStore } from '@store/useAppStore';

/**
 * A workspace's status, as the database records it.
 *
 * The column is free text, so this maps what the backend can currently write and
 * falls back to showing the value rather than guessing. A new status from a newer
 * build must render as itself; calling it "ready" would be a claim nothing
 * established.
 */
function statusBadge(status: string) {
  switch (status) {
    case 'captured':
      // Provenance, not a verdict. Indigo, because the accent is the app's own
      // voice — and because green here would collide with the meaning green
      // carries everywhere else in the preflight and restore lists.
      return <Badge variant="accent">Captured</Badge>;
    case 'received':
      // Not "Transferred", and deliberately not the same badge as "Captured". A
      // workspace that arrived on this machine was not transferred *by* this
      // machine, and the distinction is the whole reason the source device's
      // name is on the row. Neutral rather than indigo, so a row that came from
      // somewhere else reads differently at a glance.
      return <Badge variant="neutral">Received</Badge>;
    case 'transferred':
      return <Badge variant="success">Transferred</Badge>;
    case 'restored':
      return <Badge variant="success">Restored</Badge>;
    case 'failed':
      return <Badge variant="danger">Failed</Badge>;
    default:
      return <Badge variant="outline">{status}</Badge>;
  }
}

function relative(dateStr: string): string {
  const then = new Date(dateStr).getTime();
  if (Number.isNaN(then)) {
    return dateStr;
  }
  const diff = Date.now() - then;
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return 'Just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(dateStr).toLocaleDateString();
}

/**
 * How often the window asks the backend what has arrived.
 *
 * Polling, not an event, because receiving has to work while this window is
 * closed — the accept loop is a backend task, not something the UI starts. A
 * transfer that completes with the app in the background is stored regardless,
 * and this interval is only how long the banner takes to notice it. Four seconds
 * is a compromise: a workspace that arrived thirty seconds ago and is not yet
 * listed looks exactly like a transfer that failed.
 *
 * Exported so the test can wait on the real value. A test that hardcoded `4000`
 * would keep passing if this changed, and would then be testing an interval the
 * app no longer uses.
 */
export const ARRIVALS_POLL_MS = 4000;

/**
 * What other devices tried to send, and what happened to it.
 *
 * A refusal is shown as prominently as an acceptance, and with its reason,
 * because the sender is watching the other machine. A silently dropped transfer
 * is the failure mode this whole path was written to remove: the sender's
 * `send_workspace` reports "completed" as soon as the bytes are delivered, so if
 * the receiving machine says nothing, the only honest place left for the truth is
 * here.
 */
function ArrivalBanner({
  arrival,
  onDismiss,
  onOpen,
}: {
  arrival: IncomingTransfer;
  onDismiss: () => void;
  onOpen: () => void;
}) {
  return (
    <div
      // A live region, because this appears without the user doing anything and
      // the screen reader should say so. `assertive` on a refusal, which is the
      // case that needs attention; the accepted case is `polite`.
      role="status"
      aria-live={arrival.accepted ? 'polite' : 'assertive'}
      data-testid={`arrival-${arrival.transfer_id}`}
      data-accepted={arrival.accepted ? 'true' : 'false'}
      className={`flex items-start gap-3 p-4 rounded-lg border ${
        arrival.accepted
          ? 'bg-success/10 border-success/30'
          : 'bg-destructive/10 border-destructive/30'
      }`}
    >
      <ArrowDownToLine
        className={`w-5 h-5 mt-0.5 flex-shrink-0 ${
          arrival.accepted ? 'text-success' : 'text-destructive'
        }`}
      />
      <div className="flex-1 min-w-0">
        {arrival.accepted ? (
          <>
            <p className="font-medium">
              '{arrival.workspace_name}' arrived from {arrival.sender_device_name}
            </p>
            <p className="text-sm text-muted-foreground mt-0.5">
              Stored on this machine, sealed with this machine's key. It is ready for
              preflight whenever you are.
            </p>
          </>
        ) : (
          <>
            <p className="font-medium text-destructive">
              A workspace from {arrival.sender_device_name} was not accepted
            </p>
            <p className="text-sm text-destructive/90 mt-0.5">
              {arrival.refusal_reason}
            </p>
          </>
        )}
        <button
          type="button"
          onClick={onOpen}
          className="text-sm underline underline-offset-4 mt-1.5"
        >
          {arrival.accepted ? 'Open preflight' : 'See the workspace list'}
        </button>
      </div>
      <Button variant="ghost" size="icon" title="Dismiss" onClick={onDismiss}>
        <X className="w-4 h-4" />
        <span className="sr-only">Dismiss this notification</span>
      </Button>
    </div>
  );
}

export function WorkspacesScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  type WorkspaceTab = 'workspaces' | 'history';
  const [activeTab, setActiveTab] = useState<WorkspaceTab>('workspaces');
  const [selected, setSelected] = useState<WorkspaceRecord | null>(null);
  const [showCaptureDialog, setShowCaptureDialog] = useState(false);
  const [newWorkspaceName, setNewWorkspaceName] = useState('');

  const setDraftName = useAppStore((s) => s.setDraftName);
  const clearDraft = useAppStore((s) => s.clearDraft);

  const workspaces = useQuery({
    queryKey: ['workspaces'],
    queryFn: () => listWorkspaces(),
  });

  const snapshots = useQuery({
    queryKey: ['snapshots', selected?.id ?? ''],
    queryFn: () => getWorkspaceSnapshots(selected!.id),
    enabled: selected !== null,
  });

  const runs = useQuery({
    queryKey: ['restore-runs', selected?.id ?? ''],
    queryFn: () => getWorkspaceRestoreRuns(selected!.id),
    enabled: selected !== null,
  });

  const transfers = useQuery({
    queryKey: ['transfer-history', selected?.id ?? ''],
    queryFn: () => getTransferHistory(selected!.id),
    enabled: selected !== null,
  });

  const incoming = useQuery({
    queryKey: ['incoming-transfers'],
    queryFn: () => getIncomingTransfers(),
    refetchInterval: ARRIVALS_POLL_MS,
  });

  // The workspace list is fetched once when the screen mounts. A workspace that
  // arrives while this screen is open is stored by the backend without this
  // window knowing, so the only evidence it exists is the arrival banner —
  // and until it is refetched, the list above still shows the pre-arrival
  // state. That was the "I can see the green banner, my workspace is just not
  // in the list" gap; it only disappeared when the user navigated away and
  // back, which remounted the screen. The arrivals poll is the signal that
  // something new and accepted landed, so that is when the list refetches.
  const seenArrivals = useRef<Set<string>>(new Set());
  useEffect(() => {
    const arrivals = incoming.data ?? [];
    const landed = arrivals.filter(
      (a) => a.accepted && a.workspace_id && !seenArrivals.current.has(a.transfer_id)
    );
    for (const arrival of arrivals) {
      seenArrivals.current.add(arrival.transfer_id);
    }
    if (landed.length > 0) {
      void queryClient.invalidateQueries({ queryKey: ['workspaces'] });
    }
  }, [incoming.data, queryClient]);

  const dismiss = useMutation({
    mutationFn: (transferId: string) => dismissIncomingTransfer(transferId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['incoming-transfers'] });
    },
  });

  const remove = useMutation({
    mutationFn: (id: string) => deleteWorkspace(id),
    onSuccess: (_result, deletedId) => {
      // Only drop out of the history tab if it was showing the row that just
      // went away. Leaving it up would show history for a workspace the user
      // deleted, reachable only by reloading.
      if (selected?.id === deletedId) {
        setSelected(null);
        setActiveTab('workspaces');
      }
      void queryClient.invalidateQueries({ queryKey: ['workspaces'] });
    },
  });

  const handleTabChange = (value: string) => {
    if (value === 'workspaces' || value === 'history') {
      setActiveTab(value);
    }
  };

  const startCapture = () => {
    if (!newWorkspaceName.trim()) {
      return;
    }
    // The name is typed here and the selection is built on the next screen, so
    // it is handed over through the store. A stale draft from a capture the user
    // abandoned is cleared on the way in.
    clearDraft();
    setDraftName(newWorkspaceName.trim());
    setShowCaptureDialog(false);
    setNewWorkspaceName('');
    navigate('/capture');
  };

  const list = workspaces.data ?? [];
  const arrivals = incoming.data ?? [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <p className="text-[13px] text-muted-foreground">
          Captured work sessions, ready to send or to resume on this machine.
        </p>
        <Button onClick={() => setShowCaptureDialog(true)}>
          <Plus className="w-4 h-4 mr-2" />
          Capture New Workspace
        </Button>
      </div>

      {arrivals.length > 0 && (
        <div className="space-y-2">
          {arrivals.map((arrival) => (
            <ArrivalBanner
              key={arrival.transfer_id}
              arrival={arrival}
              onDismiss={() => dismiss.mutate(arrival.transfer_id)}
              onOpen={() => {
                // An accepted arrival has a workspace to work on, so it goes
                // straight to preflight -- that is the next step, and the only
                // thing a user wanting to use what just arrived would do next. A
                // refusal has no workspace, so it goes to the list, which is where
                // the absence of one is visible.
                if (arrival.accepted && arrival.workspace_id) {
                  navigate(`/preflight/${arrival.workspace_id}`);
                } else {
                  setActiveTab('workspaces');
                }
              }}
            />
          ))}
        </div>
      )}

      <Tabs value={activeTab} onValueChange={handleTabChange} className="space-y-4">
        <TabsList>
          <TabsTrigger value="workspaces">Workspaces ({list.length})</TabsTrigger>
          {selected && <TabsTrigger value="history">History: {selected.name}</TabsTrigger>}
        </TabsList>

        <TabsContent value="workspaces">
          {workspaces.isLoading ? (
            <Card>
              <CardContent className="py-12 text-center text-muted-foreground">
                Loading workspaces…
              </CardContent>
            </Card>
          ) : workspaces.isError ? (
            <div
              role="alert"
              className="p-4 bg-destructive/10 border border-destructive/30 rounded-lg"
            >
              <h3 className="font-medium text-destructive">The workspace list could not be read</h3>
              <p className="text-sm text-destructive/90 mt-1">
                {errorMessage(workspaces.error)}
              </p>
            </div>
          ) : list.length === 0 ? (
            <EmptyStateCard
              icon={<Monitor className="h-5 w-5" />}
              title="No workspaces captured yet"
              action={
                <Button onClick={() => setShowCaptureDialog(true)}>
                  <Plus className="mr-1.5 h-3.5 w-3.5" aria-hidden="true" />
                  Capture Workspace
                </Button>
              }
            >
              Capture your first workspace: choose the projects, applications and URLs to include,
              and it becomes a manifest you can send or restore here.
            </EmptyStateCard>
          ) : (
            <div className="space-y-4">
              {list.map((workspace) => (
                <Card
                  key={workspace.id}
                  data-testid={`workspace-${workspace.id}`}
                  className="hover:shadow-md transition-shadow cursor-pointer"
                  onClick={() => navigate(`/transfer/${workspace.id}`)}
                >
                  <CardContent className="py-4">
                    <div className="flex items-center gap-4">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className="font-medium truncate">{workspace.name}</span>
                          {statusBadge(workspace.status)}
                        </div>
                        <div className="flex items-center gap-4 text-sm text-muted-foreground mt-1 flex-wrap">
                          <span className="flex items-center gap-1">
                            <Clock className="w-3 h-3" />
                            {relative(workspace.captured_at)}
                          </span>
                          <span className="flex items-center gap-1">
                            <HardDrive className="w-3 h-3" />
                            schema v{workspace.schema_version}
                          </span>
                          <span className="font-mono text-xs" title="SHA-256 of the sealed manifest">
                            {workspace.manifest_digest.slice(0, 12)}
                          </span>
                        </div>
                      </div>
                      <div className="flex items-center gap-2 flex-shrink-0">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={(e) => {
                            e.stopPropagation();
                            setSelected(workspace);
                            setActiveTab('history');
                          }}
                        >
                          History
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={(e) => {
                            e.stopPropagation();
                            navigate(`/preflight/${workspace.id}`);
                          }}
                        >
                          Preflight
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          title="Delete this workspace"
                          onClick={(e) => {
                            e.stopPropagation();
                            remove.mutate(workspace.id);
                          }}
                        >
                          <Trash2 className="w-4 h-4" />
                        </Button>
                        <ChevronRight className="w-5 h-5 text-muted-foreground" />
                      </div>
                    </div>
                  </CardContent>
                </Card>
              ))}
            </div>
          )}
        </TabsContent>

        {selected && (
          <TabsContent value="history">
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <div>
                  <h2 className="text-[15px] font-semibold">{selected.name}</h2>
                  <p className="text-sm text-muted-foreground font-mono">
                    {selected.id}
                  </p>
                </div>
                <Button
                  variant="ghost"
                  onClick={() => {
                    setSelected(null);
                    setActiveTab('workspaces');
                  }}
                >
                  Back to the list
                </Button>
              </div>

              <Card>
                <CardContent className="pt-6">
                  <h3 className="font-medium mb-3 flex items-center gap-2">
                    <Upload className="w-4 h-4" />
                    Snapshots
                  </h3>
                  {snapshots.isLoading ? (
                    <p className="text-sm text-muted-foreground">Loading…</p>
                  ) : (snapshots.data ?? []).length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                      No snapshots recorded. A snapshot is written when this workspace is sent.
                    </p>
                  ) : (
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>Taken</TableHead>
                          <TableHead>Size</TableHead>
                          <TableHead>Status</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {(snapshots.data ?? []).map((snapshot) => (
                          <TableRow key={snapshot.id}>
                            <TableCell className="text-sm">
                              {new Date(snapshot.captured_at).toLocaleString()}
                            </TableCell>
                            <TableCell className="text-sm font-mono">
                              {formatBytes(snapshot.size_bytes)}
                            </TableCell>
                            <TableCell>
                              <Badge variant="outline">{snapshot.transfer_status}</Badge>
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardContent className="pt-6">
                  <h3 className="font-medium mb-3 flex items-center gap-2">
                    <Download className="w-4 h-4" />
                    Restore runs
                  </h3>
                  {runs.isLoading ? (
                    <p className="text-sm text-muted-foreground">Loading…</p>
                  ) : (runs.data ?? []).length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                      Never restored on this machine.
                    </p>
                  ) : (
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>Started</TableHead>
                          <TableHead>Status</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {(runs.data ?? []).map((run) => (
                          <TableRow key={run.id}>
                            <TableCell className="text-sm">
                              {new Date(run.started_at).toLocaleString()}
                            </TableCell>
                            <TableCell>
                              <Badge
                                variant={
                                  run.status === 'completed'
                                    ? 'success'
                                    : run.status === 'failed'
                                      ? 'destructive'
                                      : 'outline'
                                }
                              >
                                {run.status}
                              </Badge>
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardContent className="pt-6">
                  <h3 className="font-medium mb-1 flex items-center gap-2">
                    <ArrowDownToLine className="w-4 h-4" />
                    Transfers
                  </h3>
                  <p className="text-sm text-muted-foreground mb-3">
                    Written by both machines at each end of a transfer, so this is
                    where a workspace that went missing gets answered. A send the
                    other machine refused is listed here with its reason.
                  </p>
                  {transfers.isLoading ? (
                    <p className="text-sm text-muted-foreground">Loading…</p>
                  ) : (transfers.data ?? []).length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                      No transfers recorded for this workspace.
                    </p>
                  ) : (
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>Direction</TableHead>
                          <TableHead>Started</TableHead>
                          <TableHead>Status</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {(transfers.data ?? []).map((transfer) => (
                          <TableRow key={transfer.id}>
                            <TableCell className="text-sm">
                              {transfer.source_device_name} →{' '}
                              {transfer.destination_device_name}
                            </TableCell>
                            <TableCell className="text-sm">
                              {new Date(transfer.started_at).toLocaleString()}
                            </TableCell>
                            <TableCell>
                              <Badge
                                variant={
                                  transfer.status === 'completed'
                                    ? 'success'
                                    : transfer.status === 'failed'
                                      ? 'destructive'
                                      : 'outline'
                                }
                              >
                                {transfer.status}
                              </Badge>
                              {transfer.error && (
                                <p className="text-xs text-destructive/90 mt-1 max-w-xs">
                                  {transfer.error}
                                </p>
                              )}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardContent className="pt-6 space-y-2">
                  <h3 className="font-medium flex items-center gap-2">
                    <GitBranch className="w-4 h-4" />
                    Integrity
                  </h3>
                  <p className="text-sm text-muted-foreground">
                    The manifest is stored sealed, and the digest below is over those exact bytes.
                    Two devices holding the same digest hold the same workspace.
                  </p>
                  <p className="text-xs font-mono break-all bg-muted p-2 rounded">
                    {selected.manifest_digest}
                  </p>
                  <p className="text-xs text-muted-foreground font-mono break-all">
                    {selected.encrypted_manifest_path}
                  </p>
                </CardContent>
              </Card>
            </div>
          </TabsContent>
        )}
      </Tabs>

      <Dialog open={showCaptureDialog} onOpenChange={setShowCaptureDialog}>
        <DialogContent>
          <div className="space-y-4">
            <div>
              <h2 className="text-[15px] font-semibold">New workspace</h2>
              <p className="text-sm text-muted-foreground">
                Give it a name you will recognise on the other machine. The next screen chooses
                what goes in it.
              </p>
            </div>
            <div className="space-y-2">
              <Label htmlFor="new-workspace-name">Name</Label>
              <Input
                id="new-workspace-name"
                value={newWorkspaceName}
                onChange={(e) => setNewWorkspaceName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    startCapture();
                  }
                }}
                placeholder="e.g. Welcome Rewards, payments branch"
                autoFocus
              />
            </div>
            <div className="flex justify-end gap-2">
              <Button variant="outline" onClick={() => setShowCaptureDialog(false)}>
                Cancel
              </Button>
              <Button onClick={startCapture} disabled={!newWorkspaceName.trim()}>
                Continue
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
