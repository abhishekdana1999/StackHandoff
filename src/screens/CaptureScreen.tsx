import { useMemo, useState, useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { useMutation, useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  CheckCircle,
  ChevronDown,
  ChevronUp,
  FolderOpen,
  Globe,
  Info,
  Loader2,
  Monitor,
  Terminal,
  Variable,
  LayoutDashboard,
} from 'lucide-react';
import { Button } from '@components/ui/Button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { Input } from '@components/ui/Input';
import { Label } from '@components/ui/Label';
import { Textarea } from '@components/ui/Textarea';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@components/ui/Tabs';
import { Switch } from '@components/ui/Switch';
import { captureWorkspace, discoverApplications, errorMessage, listProjectRoots } from '@lib/ipc';
import { emptySelection, useAppStore } from '@store/useAppStore';
import type {
  AppIntent,
  ApprovedCommand,
  ApplicationCategory,
  CaptureSelection,
  DiscoveredApplication,
  ProjectCandidate,
} from '@model';

/**
 * The adapters whose captured context the user can include.
 *
 * `git` is deliberately absent: it runs automatically whenever a project is
 * selected, because a project's git state is the project rather than an optional
 * extra. Offering it as a toggle would imply a project can be captured without
 * its branch and remote, which would be a worse manifest.
 */
const OPTIONAL_ADAPTERS = [
  {
    id: 'vscode',
    label: 'Editor (VS Code)',
    icon: <Monitor className="w-5 h-5" />,
    description: 'Open folders and installed extensions. Settings and any auth state are not read.',
  },
  {
    id: 'terminal',
    label: 'Terminal',
    icon: <Terminal className="w-5 h-5" />,
    description:
      'Working directories for the selected projects, and any commands you approve below. Command history is never read.',
  },
] as const;

const SECTION_IDS = ['projects', 'adapters', 'applications', 'browser', 'env', 'commands'] as const;
type SectionId = (typeof SECTION_IDS)[number];

/** Split a pasted block into individual URLs, one per line or comma. */
function parseUrlList(raw: string): string[] {
  return raw
    .split(/[\n,]/)
    .map((u) => u.trim())
    .filter((u) => u.length > 0);
}

/**
 * Parse the approved-command textarea.
 *
 * Each line is `label | command`, or just a command with an empty label. A line
 * that is neither is dropped rather than becoming a half-formed command: an
 * approved command is offered for execution on the destination, so accepting a
 * fragment would put something unusable in front of the user there.
 */
function parseCommandList(raw: string): ApprovedCommand[] {
  return raw
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const [label, command] = line.includes('|')
        ? [line.slice(0, line.indexOf('|')).trim(), line.slice(line.indexOf('|') + 1).trim()]
        : ['', line];
      return { label, command, workingDirectory: null };
    })
    .filter((c) => c.command.length > 0);
}

export function CaptureScreen() {
  const navigate = useNavigate();
  const draft = useAppStore((s) => s.draft);
  const setDraftName = useAppStore((s) => s.setDraftName);
  const setCapture = useAppStore((s) => s.setCapture);
  const clearDraft = useAppStore((s) => s.clearDraft);

  const [expanded, setExpanded] = useState<string[]>(['projects', 'adapters', 'applications']);
  const [selectedProjectIds, setSelectedProjectIds] = useState<Set<string>>(new Set());
  const [selectedAdapters, setSelectedAdapters] = useState<Set<string>>(new Set());
  const [urlText, setUrlText] = useState('');
  const [envText, setEnvText] = useState('');
  const [commandText, setCommandText] = useState('');
  const [error, setError] = useState<string | null>(null);

  // Application discovery state
  const [discoveredApps, setDiscoveredApps] = useState<DiscoveredApplication[]>([]);
  const [selectedAppIds, setSelectedAppIds] = useState<Set<string>>(new Set());
  const [selectedAppFolders, setSelectedAppFolders] = useState<Map<string, Set<string>>>(new Map());
  const [discoveryLoading, setDiscoveryLoading] = useState(false);
  const [discoveryError, setDiscoveryError] = useState<string | null>(null);
  const [browserTabWarnings, setBrowserTabWarnings] = useState<string[]>([]);
  const [browserTabOptIn, setBrowserTabOptIn] = useState(false);
  const [browserTabDiscoveryLoading, setBrowserTabDiscoveryLoading] = useState(false);
  const discoveryRequestId = useRef(0);

  const scan = useQuery({ queryKey: ['project-scan'], queryFn: listProjectRoots });

  const capture = useMutation({
    mutationFn: (selection: CaptureSelection) => captureWorkspace(draft.name, selection),
    onSuccess: (result) => {
      setError(null);
      setCapture({
        workspaceId: result.manifest.workspace.id,
        manifest: result.manifest,
        warnings: result.warnings,
      });
      clearDraft();
      navigate(`/transfer/${result.manifest.workspace.id}`);
    },
    onError: (e) => setError(errorMessage(e)),
  });

  const handleBrowserTabOptIn = (enabled: boolean) => {
    setBrowserTabOptIn(enabled);
    setDiscoveryError(null);
    if (!enabled) {
      setBrowserTabWarnings([]);
      setSelectedAppIds((prev) => new Set(
        Array.from(prev).filter((id) =>
          discoveredApps.find((app) => app.id === id)?.category !== 'browser'
        )
      ));
      setSelectedAppFolders((prev) => new Map(
        Array.from(prev).filter(([id]) =>
          discoveredApps.find((app) => app.id === id)?.category !== 'browser'
        )
      ));
    }
  };

  const projects = scan.data?.projects ?? [];
  const browserUrls = useMemo(() => parseUrlList(urlText), [urlText]);
  const discoveredBrowserUrls = useMemo(() =>
    discoveredApps
      .filter((a) => a.category === 'browser')
      .flatMap((a) => a.openFolders.map((f) => f.path)),
    [discoveredApps],
  );
  const appIntents = useMemo(() =>
    Array.from(selectedAppIds)
      .map((appId) => discoveredApps.find((a) => a.id === appId))
      .filter((a): a is DiscoveredApplication => !!a)
      .map((a) => ({
        id: a.id,
        name: a.name,
        category: a.category,
        hasAdapter: a.hasAdapter,
        adapterId: a.adapterId,
        selectedFolders: Array.from(selectedAppFolders.get(a.id) || new Set()),
      })),
    [discoveredApps, selectedAppIds, selectedAppFolders],
  );
  const envNames = useMemo(
    () =>
      envText
        .split('\n')
        .map((e) => e.trim())
        .filter((e) => e.length > 0),
    [envText]
  );
  const commands = useMemo(() => parseCommandList(commandText), [commandText]);

  const chosenProjects = useMemo(
    () => projects.filter((p) => selectedProjectIds.has(p.id)),
    [projects, selectedProjectIds]
  );

  // Refresh folder/app matches when projects change, and refresh browser tabs
  // whenever the user changes the explicit browser-discovery opt-in.
  useEffect(() => {
    const requestId = ++discoveryRequestId.current;
    const discover = async () => {
      setDiscoveryLoading(true);
      setDiscoveryError(null);
      if (browserTabOptIn) {
        setBrowserTabDiscoveryLoading(true);
      }
      try {
        const result = await discoverApplications({ includeBrowserTabs: browserTabOptIn });
        if (requestId !== discoveryRequestId.current) return;
        setDiscoveredApps(result.applications);
        setBrowserTabWarnings(browserTabOptIn ? result.warnings : []);
        // Auto-select apps that have folders matching selected projects
        const projectPaths = new Set(chosenProjects.map((p) => p.path));
        const initialSelectedApps = new Set<string>();
        const initialSelectedFolders = new Map<string, Set<string>>();

        for (const app of result.applications) {
          const matchingFolders = app.openFolders.filter((f) => projectPaths.has(f.path));
          if (matchingFolders.length > 0) {
            initialSelectedApps.add(app.id);
            initialSelectedFolders.set(app.id, new Set(matchingFolders.map((f) => f.path)));
          }
        }

        setSelectedAppIds(initialSelectedApps);
        setSelectedAppFolders(initialSelectedFolders);
      } catch (e) {
        if (requestId === discoveryRequestId.current) {
          setDiscoveryError(errorMessage(e));
        }
      } finally {
        if (requestId === discoveryRequestId.current) {
          setDiscoveryLoading(false);
          setBrowserTabDiscoveryLoading(false);
        }
      }
    };
    discover();
    return () => {
      discoveryRequestId.current += 1;
    };
  }, [browserTabOptIn, chosenProjects]);

  // Handle browser tab opt-in
  const selectedAppFoldersForCapture = useMemo(() => {
    const folders: { appId: string; path: string; name: string }[] = [];
    for (const [appId, paths] of selectedAppFolders) {
      const app = discoveredApps.find((a) => a.id === appId);
      if (app) {
        for (const path of paths) {
          const folder = app.openFolders.find((f) => f.path === path);
          if (folder) {
            folders.push({ appId, path, name: folder.name });
          }
        }
      }
    }
    return folders;
  }, [discoveredApps, selectedAppFolders]);

  const selectedCount =
    chosenProjects.length +
    selectedAdapters.size +
    browserUrls.length +
    envNames.length +
    commands.length +
    selectedAppIds.size +
    selectedAppFoldersForCapture.length;

  const nothingSelected = selectedCount === 0;
  const canCapture = draft.name.trim().length > 0 && !nothingSelected && !capture.isPending;

  const toggle = (set: Set<string>, value: string) => {
    const next = new Set(set);
    if (next.has(value)) {
      next.delete(value);
    } else {
      next.add(value);
    }
    return next;
  };

  const toggleApp = (appId: string) => {
    setSelectedAppIds((prev) => toggle(prev, appId));
    // When deselecting an app, clear its folder selections
    setSelectedAppFolders((prev) => {
      const next = new Map(prev);
      if (!next.has(appId)) {
        next.delete(appId);
      }
      return next;
    });
  };

  const toggleAppFolder = (appId: string, folderPath: string) => {
    setSelectedAppFolders((prev) => {
      const next = new Map(prev);
      let appFolders = next.get(appId);
      if (!appFolders) {
        appFolders = new Set();
        next.set(appId, appFolders);
      }
      if (appFolders.has(folderPath)) {
        appFolders.delete(folderPath);
      } else {
        appFolders.add(folderPath);
      }
      if (appFolders.size === 0) {
        next.delete(appId);
      }
      return next;
    });
  };

  const handleCapture = () => {
    // Build app intents from selected apps and their folders
    const appIntents: AppIntent[] = Array.from(selectedAppIds).map((appId) => {
      const app = discoveredApps.find((a) => a.id === appId);
      const selectedFolders = selectedAppFolders.get(appId) || new Set();
      return {
        id: appId,
        name: app?.name || appId,
        category: app?.category || 'other',
        hasAdapter: app?.hasAdapter || false,
        adapterId: app?.adapterId || null,
        selectedFolders: Array.from(selectedFolders),
      };
    });

    // Build discovered browser URLs from browser apps
    const discoveredBrowserUrls = discoveredApps
      .filter((a) => a.category === 'browser')
      .flatMap((a) => a.openFolders.map((f) => f.path));

    const selection: CaptureSelection = {
      ...emptySelection(),
      projects: chosenProjects.map((p) => ({
        id: p.id,
        name: p.name,
        sourcePath: p.path,
        destinationLocationId: 'code',
      })),
      includeApplications: Array.from(selectedAdapters).sort(),
      browserUrls,
      discoveredBrowserUrls,
      appIntents,
      terminalDirs: selectedAdapters.has('terminal')
        ? chosenProjects.map((p) => p.path)
        : [],
      terminalCommands: selectedAdapters.has('terminal') ? commands : [],
      envVarNames: envNames,
    };
    capture.mutate(selection);
  };

  const toggleSection = (id: string) =>
    setExpanded((prev) => (prev.includes(id) ? prev.filter((s) => s !== id) : [...prev, id]));

  const sectionHeader = (id: SectionId, label: string, icon: React.ReactNode, count: string) => {
    const isOpen = expanded.includes(id);
    return (
      <button
        onClick={() => toggleSection(id)}
        aria-expanded={isOpen}
        className="w-full px-4 py-3 flex items-center gap-3 bg-muted/50 hover:bg-muted transition-colors text-left"
      >
        <span className="text-muted-foreground">{icon}</span>
        <span className="font-medium">{label}</span>
        <span className="ml-auto text-sm text-muted-foreground">{count}</span>
        {isOpen ? (
          <ChevronUp className="w-4 h-4 text-muted-foreground" />
        ) : (
          <ChevronDown className="w-4 h-4 text-muted-foreground" />
        )}
      </button>
    );
  };

  const getCategoryLabel = (category: ApplicationCategory) => {
    switch (category) {
      case 'editor':
        return 'Editors';
      case 'ide':
        return 'IDEs';
      case 'terminal':
        return 'Terminals';
      case 'browser':
        return 'Browsers';
      case 'database':
        return 'Database Tools';
      case 'design':
        return 'Design Tools';
      default:
        return 'Other';
    }
  };

  // Group apps by category
  const appsByCategory = useMemo(() => {
    const grouped = new Map<ApplicationCategory, DiscoveredApplication[]>();
    for (const app of discoveredApps) {
      const existing = grouped.get(app.category) || [];
      existing.push(app);
      grouped.set(app.category, existing);
    }
    return grouped;
  }, [discoveredApps]);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-[13px] text-muted-foreground">
            Choose what to include. Nothing is read that is not on this list.
          </p>
        </div>
        <Badge variant="outline" className="text-sm">
          {selectedCount} item{selectedCount === 1 ? '' : 's'} selected
        </Badge>
      </div>

      <div className="space-y-2">
        <Label htmlFor="capture-name">Workspace name</Label>
        <Input
          id="capture-name"
          placeholder="e.g. Welcome Rewards, payments branch"
          value={draft.name}
          onChange={(e) => setDraftName(e.target.value)}
          autoFocus
        />
      </div>

      {error && (
        <div
          role="alert"
          className="flex items-start gap-3 p-4 bg-destructive/10 border border-destructive/30 rounded-lg"
        >
          <AlertTriangle className="w-5 h-5 text-destructive mt-0.5 flex-shrink-0" />
          <div>
            <h4 className="font-medium text-destructive">The capture did not finish</h4>
            <p className="text-sm text-destructive/90 mt-1">{error}</p>
          </div>
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>What to include</CardTitle>
          <CardDescription>
            Every item you do not select here is not read from this machine.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Tabs defaultValue="selection">
            <TabsList>
              <TabsTrigger value="selection">Select items</TabsTrigger>
              <TabsTrigger value="summary">Selection summary</TabsTrigger>
            </TabsList>

            <TabsContent value="selection">
              <div className="space-y-4 mt-4">
                {/* --- Projects ------------------------------------------- */}
                <div className="border rounded-lg overflow-hidden">
                  {sectionHeader(
                    'projects',
                    'Projects',
                    <FolderOpen className="w-5 h-5" />,
                    `${chosenProjects.length}/${projects.length}`
                  )}
                  {expanded.includes('projects') && (
                    <div className="p-4 space-y-3">
                      {scan.isLoading && (
                        <p className="text-sm text-muted-foreground">Looking for projects…</p>
                      )}
                      {scan.isError && (
                        <div role="alert" className="text-sm text-destructive">
                          The project scan failed: {errorMessage(scan.error)}
                        </div>
                      )}
                      {scan.isSuccess && projects.length === 0 && (
                        <div className="text-sm text-muted-foreground space-y-2">
                          <p>No project folders were found in the configured locations.</p>
                          {scan.data.missingRoots.length > 0 && (
                            <p>
                              These folders do not exist:{' '}
                              <span className="font-mono">{scan.data.missingRoots.join(', ')}</span>.
                              Change them in Settings.
                            </p>
                          )}
                        </div>
                      )}
                      {projects.map((project) => (
                        <ProjectRow
                          key={project.id}
                          project={project}
                          selected={selectedProjectIds.has(project.id)}
                          onToggle={() =>
                            setSelectedProjectIds((prev) => toggle(prev, project.id))
                          }
                        />
                      ))}
                    </div>
                  )}
                </div>

                {/* --- Applications (Discovered) ----------------------------------- */}
                <div className="border rounded-lg overflow-hidden">
                  {sectionHeader(
                    'applications',
                    'Applications',
                    <LayoutDashboard className="w-5 h-5" />,
                    `${selectedAppIds.size} app${selectedAppIds.size === 1 ? '' : 's'} · ${selectedAppFoldersForCapture.length} folder${selectedAppFoldersForCapture.length === 1 ? '' : 's'}`
                  )}
                  {expanded.includes('applications') && (
                    <div className="p-4 space-y-3">
                      {discoveryLoading && (
                        <div className="flex items-center gap-3 p-3 text-sm text-muted-foreground">
                          <Loader2 className="w-5 h-5 animate-spin" />
                          Discovering running applications…
                        </div>
                      )}
                      {discoveryError && (
                        <div role="alert" className="text-sm text-destructive">
                          Discovery failed: {discoveryError}
                        </div>
                      )}
                      {discoveryLoading === false && discoveredApps.length === 0 && (
                        <p className="text-sm text-muted-foreground">
                          No running applications were detected. Make sure your editors,
                          terminals, and browsers are open.
                        </p>
                      )}
                      {discoveryLoading === false && (
                        <>
                          {/* Browser tab opt-in */}
                          <div className="flex items-center gap-3 p-3 rounded-lg border bg-muted/50">
                            <Globe className="w-5 h-5 text-muted-foreground" />
                            <div className="flex-1">
                              <Label className="font-medium">Browser tab discovery (opt-in)</Label>
                              <p className="text-xs text-muted-foreground mt-0.5">
                                Enable to read open Safari tabs (macOS may ask you to allow StackHandoff
                                in Automation settings) and tabs from Chromium browsers started with
                                remote debugging. Only http/https URLs are captured; installed browsers
                                with no readable open tabs are not listed.
                              </p>
                            </div>
                            <Switch
                              checked={browserTabOptIn}
                              onCheckedChange={handleBrowserTabOptIn}
                              disabled={browserTabDiscoveryLoading}
                            />
                            {browserTabDiscoveryLoading && (
                              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
                            )}
                          </div>
                          {browserTabWarnings.length > 0 && (
                            <div
                              role="status"
                              className="text-sm text-warning-fg bg-warning-bg border border-warning-border rounded-lg p-3"
                            >
                              {browserTabWarnings.map((warning) => (
                                <p key={warning}>{warning}</p>
                              ))}
                            </div>
                          )}

                          {/* Applications grouped by category */}
                          {Array.from(appsByCategory.entries()).map(([category, apps]) => (
                            <div key={category} className="space-y-2">
                              <h4 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                                {getCategoryLabel(category)}
                              </h4>
                              {apps.map((app) => (
                                <ApplicationRow
                                  key={app.id}
                                  app={app}
                                  selected={selectedAppIds.has(app.id)}
                                  selectedFolders={selectedAppFolders.get(app.id) || new Set()}
                                  onToggleApp={toggleApp}
                                  onToggleFolder={toggleAppFolder}
                                  hasAdapter={app.hasAdapter}
                                  adapterId={app.adapterId}
                                  isLoading={discoveryLoading}
                                />
                              ))}
                            </div>
                          ))}
                        </>
                      )}
                    </div>
                  )}
                </div>

                {/* --- Adapters ------------------------------------------ */}
                <div className="border rounded-lg overflow-hidden">
                  {sectionHeader(
                    'adapters',
                    'Applications',
                    <Monitor className="w-5 h-5" />,
                    `${selectedAdapters.size}/${OPTIONAL_ADAPTERS.length}`
                  )}
                  {expanded.includes('adapters') && (
                    <div className="p-4 space-y-3">
                      {OPTIONAL_ADAPTERS.map((adapter) => (
                        <div
                          key={adapter.id}
                          className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted/50 transition-colors"
                        >
                          <input
                            type="checkbox"
                            id={`adapter-${adapter.id}`}
                            checked={selectedAdapters.has(adapter.id)}
                            onChange={() =>
                              setSelectedAdapters((prev) => toggle(prev, adapter.id))
                            }
                            className="mt-1 h-4 w-4 rounded border-hairline text-primary accent-primary"
                          />
                          <label htmlFor={`adapter-${adapter.id}`} className="flex-1 cursor-pointer">
                            <span className="font-medium">{adapter.label}</span>
                            <p className="text-sm text-muted-foreground mt-0.5">
                              {adapter.description}
                            </p>
                          </label>
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                {/* --- Browser URLs -------------------------------------- */}
                <div className="border rounded-lg overflow-hidden">
                  {sectionHeader(
                    'browser',
                    'Browser URLs',
                    <Globe className="w-5 h-5" />,
                    `${browserUrls.length} manual + ${discoveredBrowserUrls.length} discovered`
                  )}
                  {expanded.includes('browser') && (
                    <div className="p-4 space-y-2">
                      <Label htmlFor="capture-urls">One URL per line (manual entry)</Label>
                      <Textarea
                        id="capture-urls"
                        rows={5}
                        placeholder={'http://localhost:3000\nhttps://github.com/acme/api'}
                        value={urlText}
                        onChange={(e) => setUrlText(e.target.value)}
                      />
                      {discoveredBrowserUrls.length > 0 && (
                        <div className="space-y-2">
                          <Label>Discovered from browser tabs (opt-in)</Label>
                          <div className="max-h-40 overflow-y-auto space-y-1">
                            {discoveredBrowserUrls.map((url, i) => (
                              <div
                                key={i}
                                className="text-sm font-mono text-muted-foreground p-2 bg-muted rounded truncate"
                              >
                                {url}
                              </div>
                            ))}
                          </div>
                          <p className="text-xs text-muted-foreground">
                            These URLs were discovered from open browser tabs. Enable/disable browser
                            tab discovery above to refresh.
                          </p>
                        </div>
                      )}
                      <p className="text-xs text-muted-foreground">
                        This app does not read your browser history or profile. You paste the URLs
                        you want to carry across, and they are reopened on the destination.
                      </p>
                    </div>
                  )}
                </div>

                {/* --- Environment names --------------------------------- */}
                <div className="border rounded-lg overflow-hidden">
                  {sectionHeader(
                    'env',
                    'Environment variable names',
                    <Variable className="w-5 h-5" />,
                    `${envNames.length} name${envNames.length === 1 ? '' : 's'}`
                  )}
                  {expanded.includes('env') && (
                    <div className="p-4 space-y-2">
                      <Label htmlFor="capture-env">One variable name per line</Label>
                      <Textarea
                        id="capture-env"
                        rows={4}
                        placeholder={'SUPABASE_URL\nAPI_BASE_URL'}
                        value={envText}
                        onChange={(e) => setEnvText(e.target.value)}
                      />
                      <p className="text-xs text-muted-foreground">
                        Only the names are carried. The destination is asked whether each one is
                        set; no value is ever read, and none is sent.
                      </p>
                    </div>
                  )}
                </div>

                {/* --- Approved commands --------------------------------- */}
                <div className="border rounded-lg overflow-hidden">
                  {sectionHeader(
                    'commands',
                    'Commands to offer',
                    <Terminal className="w-5 h-5" />,
                    `${commands.length} command${commands.length === 1 ? '' : 's'}`
                  )}
                  {expanded.includes('commands') && (
                    <div className="p-4 space-y-2">
                      <Label htmlFor="capture-commands">
                        One per line, as <span className="font-mono">label | command</span>
                      </Label>
                      <Textarea
                        id="capture-commands"
                        rows={4}
                        placeholder={'Dev server | pnpm dev\nMigrations | supabase db reset'}
                        value={commandText}
                        onChange={(e) => setCommandText(e.target.value)}
                        disabled={!selectedAdapters.has('terminal')}
                      />
                      {!selectedAdapters.has('terminal') ? (
                        <p className="text-xs text-muted-foreground">
                          Select the Terminal application above to offer commands. Nothing you type
                          here is ever run automatically: the destination is shown these and you
                          decide there.
                        </p>
                      ) : (
                        <p className="text-xs text-muted-foreground">
                          These are offered on the destination for you to run. Nothing is replayed
                          automatically, and shell history is never read.
                        </p>
                      )}
                    </div>
                  )}
                </div>
              </div>
            </TabsContent>

            <TabsContent value="summary">
              <div className="mt-4 space-y-4">
                <div className="p-4 bg-muted rounded-lg">
                  <pre className="text-xs font-mono overflow-x-auto text-muted-foreground max-h-96">
                    {JSON.stringify(
                      {
                        workspace: { name: draft.name || '(not named yet)' },
                        projects: chosenProjects.map((p) => ({
                          name: p.name,
                          branch: p.git?.branch ?? null,
                          dirty: p.git?.dirty ?? null,
                          // The path stays on this machine; the manifest will carry
                          // only a redacted hint. Showing the full path here is
                          // correct because this screen is on this machine.
                          source_path: p.path,
                        })),
                        applications: Array.from(selectedAdapters).sort(),
                        browser_urls: browserUrls,
                        discovered_browser_urls: discoveredBrowserUrls,
                        app_intents: appIntents,
                        env_var_names: envNames,
                        terminal_commands: selectedAdapters.has('terminal') ? commands : [],
                      },
                      null,
                      2
                    )}
                  </pre>
                </div>
                <div className="p-4 bg-warning-bg border border-warning-border rounded-lg">
                  <div className="flex items-start gap-3">
                    <Info className="w-5 h-5 text-warning-fg mt-0.5 flex-shrink-0" />
                    <div>
                      <h4 className="font-medium text-warning-fg">What is never captured</h4>
                      <ul className="text-sm text-fg-muted mt-1 list-disc list-inside space-y-0.5">
                        <li>Environment variable values, tokens, cookies, and private keys</li>
                        <li>Absolute paths, which are replaced with a redacted hint</li>
                        <li>Credentials in repository remotes</li>
                        <li>Uncommitted git state — the .git internals and the index stay
                            behind; the working-tree files themselves are captured as files</li>
                        <li>Shell history, which is never read at all</li>
                      </ul>
                      <p className="text-sm text-fg-muted mt-2">
                        This is what you asked to capture. Selected project files travel too,
                        including non-Git folders, minus the denylist: no .git, no dependencies,
                        build output or caches, no .env or keys,
                        no databases or logs, and nothing over the size caps — any skips are
                        listed in the capture warnings. The exact manifest is shown on the next
                        screen, after it has been built and sealed.
                      </p>
                    </div>
                  </div>
                </div>
              </div>
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>

      <div className="flex justify-end gap-4 items-center">
        {nothingSelected && (
          <p className="text-sm text-muted-foreground mr-auto">
            Select at least one thing to capture.
          </p>
        )}
        <Button variant="outline" onClick={() => navigate('/workspaces')}>
          Cancel
        </Button>
        <Button onClick={handleCapture} disabled={!canCapture}>
          {capture.isPending ? (
            <>
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
              Capturing…
            </>
          ) : (
            <>
              <CheckCircle className="w-4 h-4 mr-2" />
              Capture workspace
            </>
          )}
        </Button>
      </div>
    </div>
  );
}

function ProjectRow({
  project,
  selected,
  onToggle,
}: {
  project: ProjectCandidate;
  selected: boolean;
  onToggle: () => void;
}) {
  return (
    <div className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted/50 transition-colors">
      <input
        type="checkbox"
        id={`project-${project.id}`}
        checked={selected}
        onChange={onToggle}
        className="mt-1 h-4 w-4 rounded border-hairline text-primary accent-primary"
      />
      <label htmlFor={`project-${project.id}`} className="flex-1 cursor-pointer min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-medium truncate">{project.name}</span>
          {project.git?.dirty && <Badge variant="warning">Uncommitted changes</Badge>}
          {!project.isGitRepo && <Badge variant="outline">Not a git repository</Badge>}
        </div>
        <p className="text-xs text-muted-foreground mt-0.5 font-mono truncate">{project.path}</p>
        {project.git && (
          <p className="text-xs text-muted-foreground mt-1">
            {project.git.branch}
            {project.git.commit ? ` @ ${project.git.commit.slice(0, 7)}` : ''}
            {project.git.remoteHint ? ` · ${project.git.remoteHint}` : ''}
          </p>
        )}
      </label>
    </div>
  );
}

interface ApplicationRowProps {
  app: DiscoveredApplication;
  selected: boolean;
  selectedFolders: Set<string>;
  onToggleApp: (appId: string) => void;
  onToggleFolder: (appId: string, folderPath: string) => void;
  hasAdapter: boolean;
  adapterId: string | null;
  isLoading: boolean;
}

function ApplicationRow({
  app,
  selected,
  selectedFolders,
  onToggleApp,
  onToggleFolder,
  hasAdapter,
  adapterId,
  isLoading,
}: ApplicationRowProps) {
  return (
    <div className="border rounded-lg overflow-hidden">
      <div className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted/50 transition-colors">
        <input
          type="checkbox"
          id={`app-${app.id}`}
          checked={selected}
          onChange={() => onToggleApp(app.id)}
          disabled={isLoading}
          className="mt-1 h-4 w-4 rounded border-hairline text-primary accent-primary"
        />
        <div className="flex-1 cursor-pointer min-w-0">
          <label htmlFor={`app-${app.id}`} className="flex-1 cursor-pointer">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="font-medium truncate">{app.name}</span>
              {hasAdapter && adapterId && (
                <Badge variant="outline" className="text-xs">
                  Adapter: {adapterId}
                </Badge>
              )}
              {!hasAdapter && (
                <Badge variant="secondary" className="text-xs">
                  Manual
                </Badge>
              )}
              {app.executablePath && (
                <span className="text-xs text-muted-foreground font-mono truncate max-w-[200px]">
                  {app.executablePath}
                </span>
              )}
            </div>
            {app.openFolders.length > 0 && (
              <p className="text-xs text-muted-foreground mt-1">
                {app.openFolders.length} open folder{app.openFolders.length === 1 ? '' : 's'}
              </p>
            )}
          </label>
          {selected && app.openFolders.length > 0 && (
            <div className="mt-2 ml-9 space-y-1 border-l-2 border-primary/20 pl-3">
              {app.openFolders.map((folder) => (
                <div
                  key={folder.path}
                  className="flex items-center gap-2 p-1.5 rounded hover:bg-muted/50 transition-colors"
                >
                  <input
                    type="checkbox"
                    id={`app-folder-${app.id}-${folder.path}`}
                    checked={selectedFolders.has(folder.path)}
                    onChange={() => onToggleFolder(app.id, folder.path)}
                    className="h-3.5 w-3.5 rounded border-hairline text-primary accent-primary"
                  />
                  <label
                    htmlFor={`app-folder-${app.id}-${folder.path}`}
                    className="flex-1 cursor-pointer min-w-0 text-sm"
                  >
                    <span className="font-medium truncate">{folder.name}</span>
                    <span className="text-xs text-muted-foreground font-mono ml-1">
                      {folder.path}
                    </span>
                    {folder.isGitRepo && (
                      <span className="ml-2 flex items-center gap-1 text-xs text-muted-foreground">
                        <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground/30" />
                        {folder.gitBranch ? ` {folder.gitBranch}` : ''}
                        {folder.gitDirty && <Badge variant="warning" className="text-[10px]">Dirty</Badge>}
                      </span>
                    )}
                  </label>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
