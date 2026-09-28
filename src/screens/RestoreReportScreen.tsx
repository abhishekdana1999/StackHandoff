import { useMemo } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import {
  CheckCircle,
  ChevronRight,
  FileDown,
  FolderOpen,
  Globe,
  HelpCircle,
  Monitor,
  Terminal,
  XCircle,
  Zap,
} from 'lucide-react';
import { Button } from '@components/ui/Button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@components/ui/Table';
import { getWorkspaceRestoreRuns } from '@lib/ipc';
import { useQuery } from '@tanstack/react-query';
import type { ActionResult, ActionStatus, RestoreExecutionReport } from '@model';
import { useAppStore } from '@store/useAppStore';

/**
 * What a result's status means for the user.
 *
 * `manual` gets its own wording rather than being folded into a failure or a
 * success. A command that was printed for the user to run *is* the correct
 * outcome for a command step -- the app's job was to present it, not execute it
 * -- and calling it a success would claim the command ran.
 */
function statusLabel(status: ActionStatus): string {
  switch (status) {
    case 'success':
      return 'Done';
    case 'failed':
      return 'Failed';
    case 'skipped':
      return 'Skipped';
    case 'manual':
      return 'For you to run';
    default:
      return String(status);
  }
}

function statusVariant(status: ActionStatus) {
  switch (status) {
    case 'success':
      return 'success' as const;
    case 'failed':
      return 'danger' as const;
    case 'skipped':
      // Neutral rather than green: a skipped step is not an achievement, and
      // colouring it as one would overstate what happened.
      return 'neutral' as const;
    default:
      // `manual` — the app stopped and handed this one to the user.
      return 'accent' as const;
  }
}

function statusIcon(status: ActionStatus) {
  switch (status) {
    case 'success':
      return <CheckCircle className="w-4 h-4 text-success flex-shrink-0" />;
    case 'failed':
      return <XCircle className="w-4 h-4 text-danger flex-shrink-0" />;
    case 'manual':
      return <HelpCircle className="w-4 h-4 text-neutral flex-shrink-0" />;
    default:
      return <HelpCircle className="w-4 h-4 text-fg-subtle flex-shrink-0" />;
  }
}

function typeIcon(type: string) {
  switch (type) {
    case 'open_project':
      return <FolderOpen className="w-3 h-3" />;
    case 'open_application':
      return <Monitor className="w-3 h-3" />;
    case 'open_urls':
      return <Globe className="w-3 h-3" />;
    case 'offer_command':
      return <Terminal className="w-3 h-3" />;
    case 'map_path':
      return <Zap className="w-3 h-3" />;
    case 'extract_files':
      return <FileDown className="w-3 h-3" />;
    default:
      return <CheckCircle className="w-3 h-3" />;
  }
}

export function RestoreReportScreen() {
  const navigate = useNavigate();
  const params = useParams<{ restoreRunId: string }>();
  const runId = params.restoreRunId ?? '';

  const storeReport = useAppStore((s) => s.restoreReport);
  const storePlan = useAppStore((s) => s.restorePlan);

  /**
   * A report is not a query.
   *
   * The executor returns it once, and there is no command to read it back by id
   * -- so the run history below is the only thing that survives a reload, and it
   * carries a summary, not per-step results. Saying so is better than a
   * fabricated "not found".
   */
  const history = useQuery({
    queryKey: ['restore-runs', storeReport?.workspace_id ?? ''],
    queryFn: () => getWorkspaceRestoreRuns(storeReport?.workspace_id ?? ''),
    enabled: Boolean(storeReport?.workspace_id),
  });

  const report: RestoreExecutionReport | null = storeReport;
  const stepsById = useMemo(() => {
    const map = new Map<string, string>();
    for (const step of storePlan?.steps ?? []) {
      map.set(step.id, step.description);
    }
    return map;
  }, [storePlan]);

  if (!report) {
    return (
      <div className="space-y-6 max-w-4xl">
        <Card>
          <CardContent className="py-10 text-center space-y-3">
            <HelpCircle className="w-10 h-10 text-muted-foreground mx-auto" />
            <p className="font-medium">This restore's results are not available</p>
            <p className="text-sm text-muted-foreground max-w-md mx-auto">
              Run <span className="font-mono">{runId || '(no run id)'}</span> finished in a
              session that has since ended. Per-step results are held in memory only -- this build
              records that a run happened but not what each step reported -- so a report cannot be
              reopened after a reload. The run history is below.
            </p>
            {history.data && history.data.length > 0 ? (
              <RunHistory runs={history.data} />
            ) : (
              <Button variant="outline" onClick={() => navigate('/workspaces')}>
                Back to workspaces
              </Button>
            )}
          </CardContent>
        </Card>
      </div>
    );
  }

  const results: ActionResult[] = report.results;
  const count = (status: ActionStatus) => results.filter((r) => r.status === status).length;
  const totalDuration = results.reduce((sum, r) => sum + r.duration_ms, 0);
  const failed = count('failed');

  return (
    <div className="space-y-6 max-w-4xl">
      <div>
        <p className="text-[13px] text-muted-foreground">
          Run <span className="font-mono text-sm">{report.run_id}</span> finished at{' '}
          {new Date(report.completed_at).toLocaleString()}.
        </p>
      </div>

      <div className="grid gap-4 md:grid-cols-4">
        <Summary value={count('success')} label="Done" tone="text-success" />
        <Summary value={failed} label="Failed" tone="text-danger" />
        <Summary value={count('skipped')} label="Skipped" tone="text-fg-subtle" />
        <Summary value={count('manual')} label="For you to run" tone="text-primary" />
      </div>

      {results.length === 0 ? (
        <Card>
          <CardContent className="py-8 text-center">
            <p className="text-muted-foreground">
              The run produced no results. Every step was declined, or there were none to run.
            </p>
          </CardContent>
        </Card>
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>Steps</CardTitle>
            <CardDescription>
              {totalDuration.toLocaleString()} ms of adapter work in total.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-8"></TableHead>
                  <TableHead>Step</TableHead>
                  <TableHead>Result</TableHead>
                  <TableHead className="w-24 text-right">Time</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {results.map((result) => (
                  <TableRow key={result.action_id}>
                    <TableCell>{statusIcon(result.status)}</TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <span className="text-muted-foreground flex-shrink-0">
                          {typeIcon(stepsById.get(result.action_id) ? guessType(result.action_id) : '')}
                        </span>
                        <div className="min-w-0">
                          <div className="font-medium truncate">
                            {stepsById.get(result.action_id) ?? result.action_id}
                          </div>
                          <div className="text-xs text-muted-foreground">{result.message}</div>
                          {result.details && (
                            <pre className="text-xs font-mono text-muted-foreground mt-1 whitespace-pre-wrap break-words">
                              {result.details}
                            </pre>
                          )}
                        </div>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant={statusVariant(result.status)}>
                        {statusLabel(result.status)}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right text-xs text-muted-foreground font-mono">
                      {result.duration_ms} ms
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {report.notes.length > 0 && (
        <Card className="border-warning-border bg-warning-bg">
          <CardContent className="p-4">
            <h4 className="font-medium text-warning-fg">Not done</h4>
            <ul className="text-sm text-fg-muted mt-1 list-disc list-inside space-y-0.5">
              {report.notes.map((note, i) => (
                <li key={i}>{note}</li>
              ))}
            </ul>
          </CardContent>
        </Card>
      )}

      {history.data && history.data.length > 0 && <RunHistory runs={history.data} />}

      <div className="flex justify-end gap-3">
        <Button variant="outline" onClick={() => navigate('/workspaces')}>
          Back to workspaces
        </Button>
        {report.workspace_id && (
          <Button
            onClick={() => {
              useAppStore.getState().clearRestore();
              navigate(`/restore-preview/${report.workspace_id}`);
            }}
          >
            Run it again
            <ChevronRight className="w-4 h-4 ml-2" />
          </Button>
        )}
      </div>
    </div>
  );
}

function Summary({ value, label, tone }: { value: number; label: string; tone: string }) {
  return (
    <div className="text-center p-4 rounded-lg border">
      <div className={`text-2xl font-bold ${tone}`}>{value}</div>
      <div className="text-sm text-muted-foreground">{label}</div>
    </div>
  );
}

function RunHistory({
  runs,
}: {
  runs: { id: string; started_at: string; status: string; completed_at: string | null }[];
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Past runs</CardTitle>
        <CardDescription>Recorded from the database, so this survives a restart.</CardDescription>
      </CardHeader>
      <CardContent>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Run</TableHead>
              <TableHead>Started</TableHead>
              <TableHead>Status</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {runs.map((run) => (
              <TableRow key={run.id}>
                <TableCell className="font-mono text-xs">{run.id}</TableCell>
                <TableCell className="text-sm">{new Date(run.started_at).toLocaleString()}</TableCell>
                <TableCell>
                  <Badge variant={run.status === 'completed' ? 'success' : 'outline'}>
                    {run.status}
                  </Badge>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}

/**
 * A best guess at a step's type from its id, for the row icon.
 *
 * Only used when the plan is no longer in the store. Guessing from a step *id*
 * would be nonsense, so this reads the id only to decide an icon and the label
 * beside it is the honest one: the id itself.
 */
function guessType(actionId: string): string {
  if (actionId.includes('url')) return 'open_urls';
  if (actionId.includes('command') || actionId.includes('terminal')) return 'offer_command';
  if (actionId.includes('project') || actionId.includes('code')) return 'open_project';
  return '';
}
