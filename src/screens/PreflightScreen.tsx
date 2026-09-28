import { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CheckCircle,
  ChevronRight,
  Database,
  ExternalLink,
  HelpCircle,
  Key,
  Loader2,
  Monitor,
  RefreshCw,
  Shield,
  ShieldCheck,
  Terminal,
  XCircle,
  Zap,
} from 'lucide-react';
import { Button } from '@components/ui/Button';
import { Card, CardContent, CardHeader, CardTitle } from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@components/ui/Tabs';
import { errorMessage, getManifest, rerunPreflightCheck, runPreflight } from '@lib/ipc';
import { isMachineVerified, isSatisfied, isUnresolved } from '@model';
import type { CheckStatus, PreflightCheck, PreflightReport, Requirements } from '@model';
import { useAppStore } from '@store/useAppStore';

/**
 * Which group a requirement belongs to, for the tab layout.
 *
 * Derived from the requirement itself rather than stored, because the manifest
 * is the only authority on what a workspace needs. A category the engine never
 * reported would be a claim the app cannot back up, and the blueprint asks for
 * a check list that is the checks that actually ran.
 */
const CATEGORIES = [
  { id: 'applications', label: 'Applications', icon: Monitor },
  { id: 'runtimes', label: 'Runtimes', icon: Zap },
  { id: 'cli_tools', label: 'Tools', icon: Terminal },
  { id: 'identities', label: 'Accounts', icon: Key },
  { id: 'environment', label: 'Environment', icon: Database },
  { id: 'services', label: 'Services', icon: Shield },
] as const;

type CategoryId = (typeof CATEGORIES)[number]['id'];

function categoryOf(check: PreflightCheck): CategoryId {
  switch (check.adapter_id) {
    case 'runtime':
      return 'runtimes';
    case 'identity':
      return 'identities';
    case 'vscode':
    case 'browser':
      return 'applications';
    case 'git':
      return 'cli_tools';
    default:
      return 'services';
  }
}

function statusIcon(status: CheckStatus) {
  if (isMachineVerified(status)) {
    return <CheckCircle className="w-4 h-4 text-success flex-shrink-0" />;
  }
  if (status === 'ready_user_confirmed') {
    // Deliberately not a plain check mark. The blueprint requires a
    // user-asserted result to be *labelled* as one, and an icon that looks the
    // same as a machine-verified one is a claim the app cannot support.
    return <ShieldCheck className="w-4 h-4 text-neutral flex-shrink-0" />;
  }
  if (status === 'login_required' || status === 'ready_account_mismatch') {
    return <XCircle className="w-4 h-4 text-danger flex-shrink-0" />;
  }
  if (isUnresolved(status)) {
    return <HelpCircle className="w-4 h-4 text-warning flex-shrink-0" />;
  }
  return <HelpCircle className="w-4 h-4 text-fg-subtle flex-shrink-0" />;
}

function statusLabel(status: CheckStatus): string {
  switch (status) {
    case 'ready_verified':
      return 'Ready';
    case 'ready_user_confirmed':
      return 'Ready — you confirmed';
    case 'ready_account_mismatch':
      return 'Account mismatch';
    case 'login_required':
      return 'Sign-in required';
    case 'configured_unverified':
      return 'Present, unverified';
    case 'not_applicable':
      return 'Not applicable';
    default:
      // An unrecognised status is shown as itself rather than as a pass. A
      // newer backend build can add a variant, and calling it "ready" would
      // report readiness nobody established.
      return String(status);
  }
}

/**
 * Map a check status to a badge.
 *
 * The three families are chosen so that green, amber and red each keep their
 * one meaning across the whole app:
 *
 *   - `success` — the machine verified this itself.
 *   - `accent`  — the user asserted it, and said so. Deliberately *not* green:
 *     `ready_user_confirmed` is true because somebody clicked, not because
 *     anything was measured, and colouring it the same as a machine-verified
 *     check would make the two indistinguishable at a glance. It is the one
 *     place the indigo accent doubles as a status.
 *   - `warning` — needs the user: signed out, or an account that is not the one
 *     the workspace expects. Amber, not red: nothing has failed, and red here
 *     would overstate it.
 *   - `neutral` — we could not determine this either way.
 */
function statusVariant(status: CheckStatus) {
  if (isMachineVerified(status)) {
    return 'success' as const;
  }
  if (status === 'ready_user_confirmed') {
    return 'accent' as const;
  }
  if (status === 'login_required' || status === 'ready_account_mismatch') {
    return 'warning' as const;
  }
  if (isUnresolved(status)) {
    return 'warning' as const;
  }
  return 'neutral' as const;
}

export function PreflightScreen() {
  const navigate = useNavigate();
  const params = useParams<{ workspaceId: string }>();
  const queryClient = useQueryClient();
  const workspaceId = params.workspaceId ?? '';

  const storeReport = useAppStore((s) => s.preflight);
  const setPreflight = useAppStore((s) => s.setPreflight);
  const manifest = useAppStore((s) => s.manifest);

  const [activeCategory, setActiveCategory] = useState<string>('all');
  const [confirmed, setConfirmed] = useState<Set<string>>(new Set());

  // The manifest is fetched rather than taken from the store so the screen
  // works when opened directly, or after a restart. The store copy is the
  // fallback while the fetch is in flight, not the source of truth.
  const manifestQuery = useQuery({
    queryKey: ['manifest', workspaceId],
    queryFn: () => getManifest(workspaceId),
    enabled: workspaceId.length > 0,
  });

  const effectiveManifest = manifestQuery.data ?? manifest;

  const preflight = useQuery({
    queryKey: ['preflight', workspaceId, Array.from(confirmed).sort()],
    queryFn: () =>
      runPreflight({
        requirements: effectiveManifest?.requirements as unknown as Requirements,
        confirmedRequirements: Array.from(confirmed),
      }),
    enabled: effectiveManifest !== null && effectiveManifest !== undefined,
  });

  const report: PreflightReport | null = preflight.data ?? storeReport;

  const rerun = useMutation({
    mutationFn: (check: PreflightCheck) =>
      rerunPreflightCheck({ requirement_id: check.requirement_id, adapter: check.adapter_id }),
    onSuccess: () => {
      // A rerun may have changed the machine, so the report is refetched rather
      // than patched in place: a check that was the only thing blocking
      // readiness has to be able to unblock it.
      void queryClient.invalidateQueries({ queryKey: ['preflight', workspaceId] });
    },
  });

  if (manifestQuery.isError) {
    return (
      <div role="alert" className="p-4 bg-destructive/10 border border-destructive/30 rounded-lg">
        <h3 className="font-medium text-destructive">The workspace could not be read</h3>
        <p className="text-sm text-destructive/90 mt-1">
          {errorMessage(manifestQuery.error)}
        </p>
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

  const checks = report.checks;
  const blocking = checks.filter((c) => c.required && !isSatisfied(c.status));
  const readiness = report.overall_readiness;

  const grouped = (list: PreflightCheck[]) =>
    CATEGORIES.map((category) => ({
      ...category,
      checks: list.filter((c) => categoryOf(c) === category.id),
    })).filter((g) => g.checks.length > 0);

  return (
    <div className="space-y-6 max-w-4xl">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="text-[13px] text-muted-foreground">
            What this machine is missing before{' '}
            {effectiveManifest?.workspace.name ?? 'the workspace'} can be restored.
          </p>
        </div>
        <div className="text-right flex-shrink-0">
          <div
            className={`text-3xl font-bold ${
              readiness === 100 ? 'text-success' : blocking.length > 0 ? 'text-warning-fg' : ''
            }`}
          >
            {readiness}%
          </div>
          <div className="text-sm text-muted-foreground">
            {report.required_satisfied} of {report.required_total} required
          </div>
        </div>
      </div>

      {blocking.length > 0 && (
        <div className="p-4 bg-warning-bg border border-warning-border rounded-lg">
          <h3 className="font-medium text-warning-fg">
            {blocking.length} required item{blocking.length === 1 ? '' : 's'} not satisfied
          </h3>
          <p className="text-sm text-fg-muted mt-1">
            You can restore anyway — anything not satisfied is reported as skipped, not silently
            treated as done. Fixing them first is what makes a restore come out complete.
          </p>
        </div>
      )}

      <Tabs value={activeCategory} onValueChange={setActiveCategory} className="space-y-4">
        <TabsList className="grid w-full grid-cols-4 md:grid-cols-7 gap-1">
          <TabsTrigger value="all" className="px-2 py-1.5">
            All
          </TabsTrigger>
          {CATEGORIES.map((category) => {
            const Icon = category.icon;
            return (
              <TabsTrigger key={category.id} value={category.id} className="px-2 py-1.5 text-xs">
                <span className="flex items-center gap-1">
                  <Icon className="w-3 h-3" />
                  {category.label}
                </span>
              </TabsTrigger>
            );
          })}
        </TabsList>

        <TabsContent value="all" className="space-y-4">
          {grouped(checks).map((group) => {
            const Icon = group.icon;
            const ready = group.checks.filter((c) => isSatisfied(c.status)).length;
            return (
              <Card key={group.id} className="overflow-hidden">
                <CardHeader className="pb-2">
                  <div className="flex items-center gap-2">
                    <Icon className="w-4 h-4" />
                    <CardTitle>{group.label}</CardTitle>
                    <Badge variant="outline">
                      {ready}/{group.checks.length} ready
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent className="pt-0">
                  <div className="space-y-2">
                    {group.checks.map((check) => (
                      <CheckRow
                        key={check.requirement_id}
                        check={check}
                        rerunning={rerun.isPending && rerun.variables?.requirement_id === check.requirement_id}
                        onRerun={() => rerun.mutate(check)}
                        onConfirm={() =>
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
                        isConfirmed={confirmed.has(check.requirement_id)}
                      />
                    ))}
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </TabsContent>

        {CATEGORIES.map((category) => {
          const list = checks.filter((c) => categoryOf(c) === category.id);
          if (list.length === 0) {
            return null;
          }
          return (
            <TabsContent key={category.id} value={category.id}>
              <div className="space-y-2">
                {list.map((check) => (
                  <CheckRow
                    key={check.requirement_id}
                    check={check}
                    rerunning={
                      rerun.isPending && rerun.variables?.requirement_id === check.requirement_id
                    }
                    onRerun={() => rerun.mutate(check)}
                    onConfirm={() =>
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
                    isConfirmed={confirmed.has(check.requirement_id)}
                  />
                ))}
              </div>
            </TabsContent>
          );
        })}
      </Tabs>

      {preflight.isError && (
        <p role="alert" className="text-sm text-destructive">
          The preflight run failed: {errorMessage(preflight.error)}
        </p>
      )}

      <div className="flex items-center justify-between border-t pt-4 gap-4">
        <p className="text-sm text-muted-foreground">
          {report.required_satisfied}/{report.required_total} required checks satisfied.
        </p>
        <div className="flex gap-2">
          <Button
            variant="outline"
            onClick={() => {
              setPreflight(report);
              navigate(`/prepare/${workspaceId}`);
            }}
          >
            <RefreshCw className="w-4 h-4 mr-2" />
            Continue
          </Button>
          <Button
            onClick={() => {
              setPreflight(report);
              navigate(`/restore-preview/${workspaceId}`);
            }}
          >
            Review the restore plan
            <ChevronRight className="w-4 h-4 ml-2" />
          </Button>
        </div>
      </div>
    </div>
  );
}

function CheckRow({
  check,
  onRerun,
  rerunning,
  onConfirm,
  isConfirmed,
}: {
  check: PreflightCheck;
  onRerun: () => void;
  rerunning: boolean;
  onConfirm: () => void;
  isConfirmed: boolean;
}) {
  return (
    <div className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted/50 transition-colors">
      <div className="mt-0.5">{statusIcon(check.status)}</div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-medium truncate">{check.requirement_id}</span>
          <Badge variant={statusVariant(check.status)}>
            {statusLabel(check.status)}
          </Badge>
          {check.required && (
            <Badge variant="outline">
              Required
            </Badge>
          )}
        </div>
        {check.evidence && (
          <p className="text-sm text-muted-foreground mt-1 font-mono break-words">
            {check.evidence}
          </p>
        )}
        {check.action && (
          <div className="mt-2 flex items-center gap-2 flex-wrap">
            {check.action.url && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => window.open(String(check.action?.url), '_blank')}
              >
                <ExternalLink className="w-3 h-3 mr-1" />
                {String(check.action.description || 'Open')}
              </Button>
            )}
            {check.action.command && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => void navigator.clipboard?.writeText(String(check.action?.command))}
              >
                Copy: {String(check.action.command)}
              </Button>
            )}
          </div>
        )}
        <div className="mt-2 flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={onRerun}
            disabled={rerunning}
            title="Check this again now"
          >
            {rerunning ? (
              <Loader2 className="w-3 h-3 mr-1 animate-spin" />
            ) : (
              <RefreshCw className="w-3 h-3 mr-1" />
            )}
            Re-check
          </Button>
          {check.status !== 'ready_verified' && check.status !== 'not_applicable' && (
            <Button variant="ghost" size="sm" onClick={onConfirm}>
              {isConfirmed ? 'Un-confirm' : "I've done this"}
            </Button>
          )}
        </div>
        {isConfirmed && (
          <p className="text-xs text-primary mt-1">
            Marked as done by you. This is not the same as the app verifying it, and it is labelled
            as your assertion wherever it appears.
          </p>
        )}
      </div>
    </div>
  );
}
