import { useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useMutation, useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  FolderOpen,
  Globe,
  Key,
  Loader2,
  Monitor,
  Play,
  Shield,
  Terminal,
  Zap,
} from 'lucide-react';
import { Button } from '@components/ui/Button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { errorMessage, executeRestore, generateRestorePlan, getManifest, summarizeRestorePlan } from '@lib/ipc';
import type { RestoreAction, RestoreActionType, WorkspaceManifest } from '@model';
import { useAppStore } from '@store/useAppStore';

/** Where a logical bucket is placed on this machine, if the user chose one. */
type DestinationRoots = Record<string, string>;

function typeIcon(type: RestoreActionType) {
  switch (type) {
    case 'open_project':
      return <FolderOpen className="w-4 h-4 flex-shrink-0" />;
    case 'open_application':
      return <Monitor className="w-4 h-4 flex-shrink-0" />;
    case 'open_urls':
      return <Globe className="w-4 h-4 flex-shrink-0" />;
    case 'offer_command':
      return <Terminal className="w-4 h-4 flex-shrink-0" />;
    case 'check_git':
      return <Key className="w-4 h-4 flex-shrink-0" />;
    case 'map_path':
      return <Zap className="w-4 h-4 flex-shrink-0" />;
    default:
      return <Shield className="w-4 h-4 flex-shrink-0" />;
  }
}

/**
 * Whether a step would run something, rather than only record or open.
 *
 * Drives the "commands will run" warning. Getting this wrong in the cautious
 * direction is the safe failure: a warning that turns out to be unnecessary is
 * noise, and one that fails to appear is a surprise.
 */
function isExecutionStep(step: RestoreAction): boolean {
  return (
    step.action_type === 'offer_command' ||
    step.action_type === 'open_project' ||
    step.action_type === 'open_application' ||
    step.action_type === 'open_urls'
  );
}

/** The one string a user would want to see for a step's configuration. */
function summariseConfig(step: RestoreAction): string {
  const config = step.config;
  if (!config || typeof config !== 'object') {
    return '';
  }
  const parts: string[] = [];
  for (const [key, value] of Object.entries(config as Record<string, unknown>)) {
    if (value === null || value === undefined || value === '') {
      continue;
    }
    if (Array.isArray(value)) {
      if (value.length > 0) {
        parts.push(`${key}: ${value.join(', ')}`);
      }
    } else {
      parts.push(`${key}: ${String(value)}`);
    }
  }
  return parts.join('\n');
}

export function RestorePreviewScreen() {
  const navigate = useNavigate();
  const params = useParams<{ workspaceId: string }>();
  const workspaceId = params.workspaceId ?? '';

  const storeManifest = useAppStore((s) => s.manifest);
  const setRestorePlan = useAppStore((s) => s.setRestorePlan);
  const setRestoreReport = useAppStore((s) => s.setRestoreReport);

  const [expanded, setExpanded] = useState<string[]>([]);
  const [approvals, setApprovals] = useState<Record<string, boolean>>({});
  const [rootsText, setRootsText] = useState<DestinationRoots>({});

  const manifestQuery = useQuery({
    queryKey: ['manifest', workspaceId],
    queryFn: () => getManifest(workspaceId),
    enabled: workspaceId.length > 0,
  });
  const manifest: WorkspaceManifest | null = manifestQuery.data ?? storeManifest;

  /**
   * The destination roots the user typed, as JSON.
   *
   * Parsed leniently: a half-typed line is a line the user is still writing, and
   * failing the whole plan because of it would make the preview unusable while
   * they type. The backend's own error is shown if a value is not a path.
   */
  const rootsJson = useMemo(() => {
    const entries = Object.entries(rootsText).filter(([, v]) => v.trim().length > 0);
    return entries.length > 0 ? JSON.stringify(Object.fromEntries(entries)) : undefined;
  }, [rootsText]);

  const planQuery = useQuery({
    queryKey: ['restore-plan', workspaceId, rootsJson],
    queryFn: () => generateRestorePlan(manifest as WorkspaceManifest, rootsText),
    enabled: manifest !== null,
  });

  const plan = planQuery.data;

  const summaryQuery = useQuery({
    queryKey: ['restore-plan-summary', workspaceId, rootsJson],
    queryFn: () => summarizeRestorePlan(plan!),
    enabled: plan !== undefined,
  });

  const execute = useMutation({
    mutationFn: () => {
      if (!plan) {
        throw new Error('There is no plan to run');
      }
      // The run id is generated here, before the call, so the report screen has
      // an id to navigate to even if the user reloads mid-run.
      const runId = `restore-${workspaceId}-${Date.now()}`;
      return executeRestore({ runId, plan, approvals });
    },
    onSuccess: (report) => {
      setRestoreReport(report);
      navigate(`/restore-report/${report.run_id}`);
    },
  });

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

  if (planQuery.isLoading || manifestQuery.isLoading) {
    return (
      <div className="flex flex-col items-center justify-center py-16 space-y-3">
        <Loader2 className="w-10 h-10 animate-spin text-primary" />
        <p className="text-muted-foreground">Building a plan for this machine…</p>
      </div>
    );
  }

  if (planQuery.isError) {
    return (
      <div role="alert" className="p-4 bg-destructive/10 border border-destructive/30 rounded-lg">
        <h3 className="font-medium text-destructive">The plan could not be built</h3>
        <p className="text-sm text-destructive/90 mt-1">{errorMessage(planQuery.error)}</p>
        <Button variant="outline" className="mt-3" onClick={() => navigate(`/preflight/${workspaceId}`)}>
          Back to preflight
        </Button>
      </div>
    );
  }

  const steps = plan?.steps ?? [];

  /** A step runs unless the user explicitly declines an optional one. */
  const isApproved = (step: RestoreAction): boolean => {
    if (approvals[step.id] !== undefined) {
      return approvals[step.id] as boolean;
    }
    return step.approved || step.required;
  };

  const unapprovedRequired = steps.filter((s) => s.required && !isApproved(s));
  const approvedOptional = steps.filter((s) => !s.required && isApproved(s));
  const skippedOptional = steps.filter((s) => !s.required && !isApproved(s));
  const executionSteps = steps.filter((s) => isApproved(s) && isExecutionStep(s));

  const setApproval = (id: string, value: boolean) =>
    setApprovals((prev) => ({ ...prev, [id]: value }));

  const canExecute = steps.length > 0 && unapprovedRequired.length === 0 && !execute.isPending;

  return (
    <div className="space-y-6 max-w-4xl">
      <div>
        <p className="text-[13px] text-muted-foreground">
          What will happen on this machine, in order. Nothing runs until you choose.
        </p>
      </div>

      {/* --- Where files go ---------------------------------------------- */}
      <Card>
        <CardHeader>
          <CardTitle>Destination folders</CardTitle>
          <CardDescription>
            Each project's bucket on this machine, as a folder. Leave one out and its project is
            planned without a destination and reported as unplaced -- it is never written somewhere
            you did not name.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {[...new Set((manifest?.projects ?? []).map((p) => p.destination_location_id))].map(
            (bucket) => (
              <div key={bucket} className="flex items-center gap-3">
                <Badge variant="outline" className="font-mono flex-shrink-0 min-w-24 justify-center">
                  {bucket}
                </Badge>
                <input
                  type="text"
                  aria-label={`Destination folder for ${bucket}`}
                  placeholder="Leave empty to leave unplaced"
                  value={rootsText[bucket] ?? ''}
                  onChange={(e) => setRootsText((prev) => ({ ...prev, [bucket]: e.target.value }))}
                  className="flex-1 h-9 rounded-md border border-input bg-background px-3 text-sm font-mono"
                />
              </div>
            )
          )}
          {manifest && manifest.projects.length === 0 && (
            <p className="text-sm text-muted-foreground">
              This workspace has no projects, so there are no folders to choose.
            </p>
          )}
        </CardContent>
      </Card>

      {/* --- Notes from the planner -------------------------------------- */}
      {plan && plan.notes.length > 0 && (
        <Card className="border-warning-border bg-warning-bg">
          <CardContent className="flex items-start gap-3 p-4">
            <AlertTriangle className="w-5 h-5 text-warning-fg flex-shrink-0 mt-0.5" />
            <div>
              <h4 className="font-medium text-warning-fg">
                This build cannot express part of the workspace
              </h4>
              <ul className="text-sm text-fg-muted mt-1 list-disc list-inside space-y-0.5">
                {plan.notes.map((note, i) => (
                  <li key={i}>{note}</li>
                ))}
              </ul>
            </div>
          </CardContent>
        </Card>
      )}

      {/* --- The steps ---------------------------------------------------- */}
      <Card>
        <CardHeader>
          <CardTitle>Steps</CardTitle>
          <CardDescription>
            {steps.length === 0
              ? 'Nothing to do for this workspace.'
              : `${steps.length} step${steps.length === 1 ? '' : 's'}, in dependency order.`}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {steps.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              The planner produced no steps. If the workspace should have produced some, the notes
              above or below say why.
            </p>
          ) : (
            <div className="space-y-3">
              {steps.map((step, index) => {
                const approved = isApproved(step);
                const config = summariseConfig(step);
                return (
                  <div key={step.id} className="border rounded-lg overflow-hidden">
                    <div className="w-full p-4 flex items-center gap-3 text-left">
                      <span className="w-8 h-8 rounded-full bg-primary/10 flex items-center justify-center text-primary font-medium flex-shrink-0">
                        {index + 1}
                      </span>
                      <button
                        onClick={() =>
                          setExpanded((prev) =>
                            prev.includes(step.id)
                              ? prev.filter((s) => s !== step.id)
                              : [...prev, step.id]
                          )
                        }
                        aria-expanded={expanded.includes(step.id)}
                        className="flex items-center gap-2 flex-1 min-w-0 text-left"
                      >
                        {typeIcon(step.action_type)}
                        <span className="min-w-0">
                          <span className="font-medium truncate block">{step.description}</span>
                          <span className="text-xs text-muted-foreground font-mono block">
                            {step.action_type}
                            {step.adapter_id ? ` · via ${step.adapter_id}` : ''}
                          </span>
                        </span>
                        {expanded.includes(step.id) ? (
                          <ChevronDown className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                        ) : (
                          <ChevronRight className="w-4 h-4 text-muted-foreground flex-shrink-0" />
                        )}
                      </button>
                      <div className="flex items-center gap-2 flex-shrink-0">
                        {step.required ? (
                          <Badge>Required</Badge>
                        ) : (
                          <Badge variant="outline">Optional</Badge>
                        )}
                        <Button
                          variant={approved ? 'secondary' : 'outline'}
                          size="sm"
                          disabled={step.required}
                          onClick={() => setApproval(step.id, !approved)}
                          title={
                            step.required
                              ? 'Required steps run as part of the restore'
                              : approved
                                ? 'Skip this step'
                                : 'Run this step'
                          }
                        >
                          {approved ? 'Will run' : 'Skipped'}
                        </Button>
                      </div>
                    </div>

                    {expanded.includes(step.id) && (
                      <div className="px-4 pb-4 border-t bg-muted/30">
                        <div className="space-y-3 text-sm">
                          {config && (
                            <pre className="font-mono text-muted-foreground bg-background p-3 rounded overflow-x-auto text-xs whitespace-pre-wrap">
                              {config}
                            </pre>
                          )}
                          {step.dependencies.length > 0 && (
                            <p className="text-muted-foreground">
                              <strong>Depends on:</strong>{' '}
                              {step.dependencies
                                .map((d) => steps.find((s) => s.id === d)?.description ?? d)
                                .join(', ')}
                            </p>
                          )}
                          {!config && step.dependencies.length === 0 && (
                            <p className="text-muted-foreground">
                              This step takes no parameters.
                            </p>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* --- Summary ------------------------------------------------------ */}
      <Card className="bg-muted/50">
        <CardContent className="pt-6">
          <div className="grid gap-4 md:grid-cols-3">
            <div className="text-center p-4 rounded-lg bg-background">
              <div className="text-2xl font-bold text-primary">
                {steps.filter((s) => s.required).length}
              </div>
              <div className="text-sm text-muted-foreground">Required</div>
            </div>
            <div className="text-center p-4 rounded-lg bg-background">
              <div className="text-2xl font-bold text-success">{approvedOptional.length}</div>
              <div className="text-sm text-muted-foreground">Optional running</div>
            </div>
            <div className="text-center p-4 rounded-lg bg-background">
              <div className="text-2xl font-bold text-fg-subtle">{skippedOptional.length}</div>
              <div className="text-sm text-muted-foreground">Optional skipped</div>
            </div>
          </div>
          {summaryQuery.data && (
            <p className="text-sm text-muted-foreground text-center mt-3">
              {summaryQuery.data.opens_something} of {summaryQuery.data.total_steps} steps open
              something on this machine.
            </p>
          )}
        </CardContent>
      </Card>

      {/* --- Warnings ----------------------------------------------------- */}
      {unapprovedRequired.length > 0 && (
        <Card className="border-destructive bg-destructive/5">
          <CardContent className="flex items-start gap-3 p-4">
            <AlertTriangle className="w-5 h-5 text-destructive flex-shrink-0" />
            <div>
              <h4 className="font-medium text-destructive">
                {unapprovedRequired.length} required step
                {unapprovedRequired.length === 1 ? '' : 's'} will not run
              </h4>
              <p className="text-sm text-muted-foreground mt-1">
                A restore that skips a required step produces a workspace that looks restored and is
                not, so the button stays off until they are.
              </p>
            </div>
          </CardContent>
        </Card>
      )}

      {executionSteps.some((s) => s.action_type === 'offer_command') && (
        <Card className="border-warning-border bg-warning-bg">
          <CardContent className="flex items-start gap-3 p-4">
            <AlertTriangle className="w-5 h-5 text-warning-fg flex-shrink-0" />
            <div>
              <h4 className="font-medium text-warning-fg">Commands will be offered, not run</h4>
              <p className="text-sm text-fg-muted mt-1">
                A command step opens the command in a visible terminal for you to run yourself. This
                app never runs a command from a captured workspace on its own, and no workspace
                arrives able to make it.
              </p>
            </div>
          </CardContent>
        </Card>
      )}

      {execute.isError && (
        <div
          role="alert"
          className="flex items-start gap-3 p-4 bg-destructive/10 border border-destructive/30 rounded-lg"
        >
          <AlertTriangle className="w-5 h-5 text-destructive flex-shrink-0" />
          <div>
            <h4 className="font-medium text-destructive">The restore did not run</h4>
            <p className="text-sm text-destructive/90 mt-1">{errorMessage(execute.error)}</p>
          </div>
        </div>
      )}

      {/* --- Run it ------------------------------------------------------- */}
      <div className="flex justify-end gap-4">
        <Button
          variant="outline"
          onClick={() => {
            if (plan) {
              setRestorePlan({ runId: '', plan, summary: summaryQuery.data ?? null });
            }
            navigate(`/preflight/${workspaceId}`);
          }}
        >
          Back to Preflight
        </Button>
        <Button
          onClick={() => {
            if (plan) {
              setRestorePlan({ runId: '', plan, summary: summaryQuery.data ?? null });
            }
            execute.mutate();
          }}
          disabled={!canExecute}
        >
          {execute.isPending ? (
            <>
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
              Restoring…
            </>
          ) : (
            <>
              <Play className="w-4 h-4 mr-2" />
              Run the restore
            </>
          )}
        </Button>
      </div>
    </div>
  );
}
