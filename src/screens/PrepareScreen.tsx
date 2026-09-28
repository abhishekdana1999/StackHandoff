import { useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowRight,
  CheckCircle,
  Database,
  ExternalLink,
  HelpCircle,
  Key,
  Loader2,
  Monitor,
  Package,
  RefreshCw,
  Terminal,
  XCircle,
  Zap,
} from 'lucide-react';
import { Button } from '@components/ui/Button';
import { Card, CardContent, CardHeader, CardTitle } from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@components/ui/Tabs';
import { errorMessage, getManifest, rerunPreflightCheck, runPreflight } from '@lib/ipc';
import { isSatisfied } from '@model';
import type { PreflightCheck, PreflightReport, Requirements } from '@model';
import { useAppStore } from '@store/useAppStore';

/**
 * The groups, and where each check's adapter comes from.
 *
 * The grouping is over the *checks the engine ran*, not over a fixed list of
 * categories. A category the manifest never asked about has nothing to prepare,
 * and inventing one would put a heading on screen with nothing under it.
 */
const GROUPS = [
  { id: 'applications', label: 'Applications', icon: Monitor },
  { id: 'runtimes', label: 'Runtimes', icon: Zap },
  { id: 'tools', label: 'Tools', icon: Terminal },
  { id: 'identities', label: 'Accounts', icon: Key },
  { id: 'environment', label: 'Environment', icon: Database },
  { id: 'other', label: 'Other', icon: Package },
] as const;

type GroupId = (typeof GROUPS)[number]['id'];

function groupOf(check: PreflightCheck): GroupId {
  switch (check.adapter_id) {
    case 'runtime':
      return 'runtimes';
    case 'identity':
      return 'identities';
    case 'git':
      return 'tools';
    case 'environment':
      return 'environment';
    case 'vscode':
    case 'browser':
      return 'applications';
    default:
      return 'other';
  }
}

/**
 * Why a check is on this screen at all.
 *
 * The blueprint asks for "official installation links or the platform's package
 * manager suggestion" and "login buttons that launch the actual provider's
 * documented login flow". Neither can be produced by this app: it does not know
 * where a given tool should come from for this platform, and it must never hold
 * a credential. So what it does instead is hand over what the *adapter* recorded
 * — which is a link or a command the tool's own author published — and where
 * there is nothing, say so rather than inventing an install method.
 */
function actionKind(check: PreflightCheck): { label: string; tone: 'accent' | 'warning' } {
  const action = check.action;
  if (!action) {
    return { label: 'No automatic action available', tone: 'warning' };
  }
  if (action.url && action.command) {
    return { label: 'Open the docs, or run the command', tone: 'accent' };
  }
  if (action.url) {
    return { label: 'Official download or sign-in', tone: 'accent' };
  }
  if (action.command) {
    return { label: 'Run this yourself', tone: 'accent' };
  }
  return { label: 'No action recorded', tone: 'warning' };
}

export function PrepareScreen() {
  const navigate = useNavigate();
  const params = useParams<{ workspaceId: string }>();
  const queryClient = useQueryClient();
  const workspaceId = params.workspaceId ?? '';

  const storeReport = useAppStore((s) => s.preflight);
  const storeManifest = useAppStore((s) => s.manifest);
  const [activeGroup, setActiveGroup] = useState<string>('all');
  const [confirmed, setConfirmed] = useState<Set<string>>(new Set());

  const manifestQuery = useQuery({
    queryKey: ['manifest', workspaceId],
    queryFn: () => getManifest(workspaceId),
    enabled: workspaceId.length > 0,
  });
  const manifest = manifestQuery.data ?? storeManifest;

  const preflight = useQuery({
    queryKey: ['preflight', workspaceId, Array.from(confirmed).sort()],
    queryFn: () =>
      runPreflight({
        requirements: manifest?.requirements as unknown as Requirements,
        confirmedRequirements: Array.from(confirmed),
      }),
    enabled: manifest !== undefined && manifest !== null,
  });

  const report: PreflightReport | null = preflight.data ?? storeReport;

  const rerun = useMutation({
    mutationFn: (check: PreflightCheck) =>
      rerunPreflightCheck({ requirement_id: check.requirement_id, adapter: check.adapter_id }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['preflight', workspaceId] });
    },
  });

  /**
   * Only the things that need doing.
   *
   * A check that is already satisfied has nothing to prepare, and listing it
   * here would bury the three that matter under forty that are fine. The count
   * of them is kept for the summary, so nothing is hidden by being filtered.
   */
  const outstanding = useMemo(
    () => (report?.checks ?? []).filter((c) => !isSatisfied(c.status)),
    [report]
  );
  const satisfied = (report?.checks ?? []).length - outstanding.length;

  const grouped = useMemo(
    () =>
      GROUPS.map((group) => ({
        ...group,
        checks: outstanding.filter((c) => groupOf(c) === group.id),
      })).filter((g) => g.checks.length > 0),
    [outstanding]
  );

  if (manifestQuery.isError) {
    return (
      <div role="alert" className="p-4 bg-destructive/10 border border-destructive/30 rounded-lg">
        <h3 className="font-medium text-destructive">The workspace could not be read</h3>
        <p className="text-sm text-destructive/90 mt-1">{errorMessage(manifestQuery.error)}</p>
        <Button variant="outline" className="mt-3" onClick={() => navigate('/workspaces')}>
          Back to workspaces
        </Button>
      </div>
    );
  }

  if (!report) {
    return (
      <div className="flex flex-col items-center justify-center py-16 space-y-3">
        <Loader2 className="w-10 h-10 animate-spin text-primary" />
        <p className="text-muted-foreground">Checking this machine…</p>
        {preflight.isError && (
          <p role="alert" className="text-sm text-destructive max-w-md text-center">
            {errorMessage(preflight.error)}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-6 max-w-4xl">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="text-[13px] text-muted-foreground">
            {outstanding.length === 0
              ? 'Nothing is missing. Everything the workspace needs is already here.'
              : `${outstanding.length} thing${outstanding.length === 1 ? '' : 's'} to sort out before restoring.`}
          </p>
        </div>
        <div className="text-right flex-shrink-0">
          <div className="text-3xl font-bold text-success">{satisfied}</div>
          <div className="text-sm text-muted-foreground">
            of {report.checks.length} already ready
          </div>
        </div>
      </div>

      {outstanding.length > 0 && (
        <div className="p-4 bg-primary-soft border border-primary-soft-border rounded-lg">
          <h3 className="font-medium text-primary">This app installs and signs in to nothing</h3>
          <p className="text-sm text-fg-muted mt-1">
            For each item below it can show you where the tool's own authors say to get it, or
            print the command — but it will not run it, and it will never ask you for a password,
            an MFA code or a private key. Signing in has to happen in the provider's own flow.
          </p>
        </div>
      )}

      {outstanding.length === 0 ? (
        <Card>
          <CardContent className="py-12 text-center">
            <CheckCircle className="w-12 h-12 text-success mx-auto mb-4" />
            <h3 className="text-[15px] font-medium">Ready</h3>
            <p className="text-muted-foreground mt-1 max-w-sm mx-auto">
              Every check this workspace asked for passed on this machine.
            </p>
          </CardContent>
        </Card>
      ) : (
        <Tabs value={activeGroup} onValueChange={setActiveGroup} className="space-y-4">
          <TabsList className="flex flex-wrap gap-1 h-auto">
            <TabsTrigger value="all" className="px-2 py-1.5">
              All ({outstanding.length})
            </TabsTrigger>
            {grouped.map((group) => (
              <TabsTrigger key={group.id} value={group.id} className="px-2 py-1.5 text-xs">
                {group.label} ({group.checks.length})
              </TabsTrigger>
            ))}
          </TabsList>

          <TabsContent value="all" className="space-y-4">
            {grouped.map((group) => (
              <Card key={group.id}>
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-2">
                    <group.icon className="w-4 h-4" />
                    {group.label}
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-0 space-y-2">
                  {group.checks.map((check) => (
                    <PrepareRow
                      key={check.requirement_id}
                      check={check}
                      rerunning={
                        rerun.isPending && rerun.variables?.requirement_id === check.requirement_id
                      }
                      onRerun={() => rerun.mutate(check)}
                      isConfirmed={confirmed.has(check.requirement_id)}
                      onToggleConfirm={() =>
                        setConfirmed((prev) => {
                          const next = new Set(prev);
                          if (next.has(check.requirement_id)) {
                            next.delete(check.requirement_id);
                          } else {
                            next.add(check.requirement_id);
                          }
                          return next;
                        })
                      }
                    />
                  ))}
                </CardContent>
              </Card>
            ))}
          </TabsContent>

          {grouped.map((group) => (
            <TabsContent key={group.id} value={group.id}>
              <div className="space-y-2">
                {group.checks.map((check) => (
                  <PrepareRow
                    key={check.requirement_id}
                    check={check}
                    rerunning={
                      rerun.isPending && rerun.variables?.requirement_id === check.requirement_id
                    }
                    onRerun={() => rerun.mutate(check)}
                    isConfirmed={confirmed.has(check.requirement_id)}
                    onToggleConfirm={() =>
                      setConfirmed((prev) => {
                        const next = new Set(prev);
                        if (next.has(check.requirement_id)) {
                          next.delete(check.requirement_id);
                        } else {
                          next.add(check.requirement_id);
                        }
                        return next;
                      })
                    }
                  />
                ))}
              </div>
            </TabsContent>
          ))}
        </Tabs>
      )}

      {preflight.isError && (
        <p role="alert" className="text-sm text-destructive">
          The check run failed: {errorMessage(preflight.error)}
        </p>
      )}

      <div className="flex items-center justify-between border-t pt-4 gap-4">
        <Button variant="outline" onClick={() => navigate(`/preflight/${workspaceId}`)}>
          See every check
        </Button>
        <Button onClick={() => navigate(`/restore-preview/${workspaceId}`)}>
          Review the restore plan
          <ArrowRight className="w-4 h-4 ml-2" />
        </Button>
      </div>
    </div>
  );
}

function PrepareRow({
  check,
  onRerun,
  rerunning,
  onToggleConfirm,
  isConfirmed,
}: {
  check: PreflightCheck;
  onRerun: () => void;
  rerunning: boolean;
  onToggleConfirm: () => void;
  isConfirmed: boolean;
}) {
  const kind = actionKind(check);
  const action = check.action;
  return (
    <div className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted/50 transition-colors">
      <div className="mt-0.5 flex-shrink-0">
        {check.status === 'ready_account_mismatch' || check.status === 'login_required' ? (
          <XCircle className="w-4 h-4 text-danger" />
        ) : (
          <HelpCircle className="w-4 h-4 text-warning" />
        )}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-medium">{check.requirement_id}</span>
          {check.required ? (
            <Badge>Required</Badge>
          ) : (
            <Badge variant="outline">
              Optional
            </Badge>
          )}
          <Badge variant={kind.tone}>
            {kind.label}
          </Badge>
        </div>

        {check.evidence && (
          <p className="text-sm text-muted-foreground mt-1 font-mono break-words">
            {check.evidence}
          </p>
        )}
        {action?.description && (
          <p className="text-sm mt-1">{action.description}</p>
        )}

        <div className="mt-2 flex items-center gap-2 flex-wrap">
          {action?.url && (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => window.open(String(action.url), '_blank')}
            >
              <ExternalLink className="w-3 h-3 mr-1" />
              Open in browser
            </Button>
          )}
          {action?.command && (
            <>
              <pre className="text-xs font-mono bg-muted px-2 py-1 rounded flex-1 min-w-40 overflow-x-auto">
                {action.command}
              </pre>
              <Button
                variant="outline"
                size="sm"
                onClick={() => void navigator.clipboard?.writeText(String(action?.command))}
              >
                Copy
              </Button>
            </>
          )}
          {!action && (
            <p className="text-xs text-muted-foreground">
              No adapter recognised this requirement, so there is nothing to point you at. The
              check is reported as unresolved rather than assumed to be fine.
            </p>
          )}
        </div>

        <div className="mt-2 flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={onRerun} disabled={rerunning}>
            {rerunning ? (
              <Loader2 className="w-3 h-3 mr-1 animate-spin" />
            ) : (
              <RefreshCw className="w-3 h-3 mr-1" />
            )}
            I&apos;ve done it — check again
          </Button>
          <Button variant="ghost" size="sm" onClick={onToggleConfirm}>
            {isConfirmed ? 'Un-confirm' : 'Mark as done without checking'}
          </Button>
        </div>
        {isConfirmed && (
          <p className="text-xs text-primary mt-1">
            Marked as done by you, on your word. This is not a machine-verified result and is
            labelled as your assertion wherever it appears.
          </p>
        )}
      </div>
    </div>
  );
}
