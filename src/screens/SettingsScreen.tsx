import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  Check,
  Database,
  Download,
  FolderOpen,
  Loader2,
  Monitor,
  Plus,
  RefreshCw,
  Shield,
  Trash2,
  Upload,
  Wifi,
  X,
} from 'lucide-react';
import { Button } from '@components/ui/Button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@components/ui/Card';
import { Separator } from '@components/ui/Separator';
import { Label } from '@components/ui/Label';
import { Input } from '@components/ui/Input';
import { Badge } from '@components/ui/Badge';
import { AppearanceControl } from '@components/AppearanceControl';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@components/ui/Tabs';
import {
  deleteSetting,
  errorMessage,
  exportSettings,
  getAppVersion,
  getDeviceIdentity,
  getDeviceKeyExists,
  getPlatform,
  importSettings,
  listPairedDevices,
  listProjectRoots,
  listSettings,
  listWorkspaces,
  setProjectRoots,
} from '@lib/ipc';

type SettingsTab = 'device' | 'capture' | 'data';

/**
 * Why this screen has three tabs and not five.
 *
 * It used to offer theme, language, update checking, start-minimised, telemetry,
 * crash reporting, database encryption, data retention, LAN discovery, relay
 * preference, log level and debug mode. Every one of them was a React toggle
 * that changed nothing: the save button called a command that does not exist,
 * and five of the buttons said "will be implemented in Phase 1" in an `alert`.
 *
 * A switch that appears to work and does not is worse than an absent one,
 * because a user who sets "include clipboard: on" has been told something about
 * what will be captured that is not true. So the screen now shows only the
 * settings this build really reads, and says plainly where the rest went.
 */
export function SettingsScreen() {
  const [activeTab, setActiveTab] = useState<SettingsTab>('device');
  const handleTabChange = (value: string) => {
    if (value === 'device' || value === 'capture' || value === 'data') {
      setActiveTab(value);
    }
  };

  return (
    <div className="space-y-6 max-w-3xl">
      <p className="text-[13px] text-muted-foreground">
        What this build actually reads, and nothing that it does not.
      </p>

      <Tabs value={activeTab} onValueChange={handleTabChange} className="space-y-6">
        <TabsList className="grid w-full grid-cols-3 gap-1">
          <TabsTrigger value="device" className="px-3 py-2">
            This device
          </TabsTrigger>
          <TabsTrigger value="capture" className="px-3 py-2">
            Capture
          </TabsTrigger>
          <TabsTrigger value="data" className="px-3 py-2">
            Data
          </TabsTrigger>
        </TabsList>

        <TabsContent value="device">
          <DeviceTab />
        </TabsContent>
        <TabsContent value="capture">
          <CaptureTab />
        </TabsContent>
        <TabsContent value="data">
          <DataTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}

// ---------------------------------------------------------------------------
// This device
// ---------------------------------------------------------------------------

function DeviceTab() {
  const identity = useQuery({ queryKey: ['device-identity'], queryFn: getDeviceIdentity });
  const hasKeys = useQuery({ queryKey: ['device-key-exists'], queryFn: getDeviceKeyExists });
  const version = useQuery({ queryKey: ['app-version'], queryFn: getAppVersion });
  const platform = useQuery({ queryKey: ['platform'], queryFn: getPlatform });
  const paired = useQuery({ queryKey: ['paired-devices'], queryFn: listPairedDevices });

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Monitor className="w-5 h-5" />
            <CardTitle>Identity</CardTitle>
            {hasKeys.data === false && (
              <Badge variant="warning">No keys yet — created on first capture</Badge>
            )}
          </div>
          <CardDescription>
            The identity a peer pairs with. Read the fingerprint aloud when pairing; a number
            that does not match on both screens means something is intercepting the connection.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {identity.isError ? (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(identity.error)}
            </p>
          ) : (
            <>
              <KeyValue label="Fingerprint (over the connection key)" mono>
                {identity.data?.fingerprint ?? '—'}
              </KeyValue>
              <KeyValue label="Connection key (X25519, Noise)" mono>
                {identity.data?.noisePublicKeyB64 ?? '—'}
              </KeyValue>
              <KeyValue label="Signing key (Ed25519)" mono>
                {identity.data?.signingPublicKeyB64 ?? '—'}
              </KeyValue>
            </>
          )}
          <p className="text-xs text-muted-foreground">
            The fingerprint is a fingerprint of the <em>connection</em> key, not the signing key.
            Only the connection key is what a handshake authenticates and what a peer holds, so it
            is the only thing an identity can honestly be derived from. The signing key is
            advertised in a pairing invitation and is not verified by this build.
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Shield className="w-5 h-5" />
            <CardTitle>Where your keys are</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="space-y-2 text-sm text-muted-foreground">
          <p>
            In the operating system's credential store — Keychain on macOS, Credential Manager on
            Windows — under the service name <span className="font-mono">stackhandoff</span>, in
            three separate entries: the identity key, the Noise key, and the key that seals
            manifests at rest. Each is written only to its own entry.
          </p>
          <p>
            They are never written to the database, to a manifest, or to a transfer. A manifest
            records project paths, branches, application names and environment variable{' '}
            <em>names</em>; no value of any secret is ever read into one.
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Wifi className="w-5 h-5" />
            <CardTitle>Paired devices</CardTitle>
            <Badge variant="outline">{(paired.data ?? []).length}</Badge>
          </div>
          <CardDescription>
            Manage these from the Devices screen, where pairing and the safety-number check live.
          </CardDescription>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground">
          {paired.data && paired.data.length > 0 ? (
            <ul className="space-y-1">
              {paired.data.map((d) => (
                <li key={d.id} className="flex items-center gap-2">
                  <span className={d.revoked ? 'line-through' : ''}>{d.name}</span>
                  {d.revoked && <Badge variant="danger">Revoked</Badge>}
                  <span className="text-xs font-mono text-muted-foreground">{d.fingerprint}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p>None yet.</p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Build</CardTitle>
        </CardHeader>
        <CardContent className="space-y-1 text-sm">
          <KeyValue label="Version">{version.data ?? '—'}</KeyValue>
          <KeyValue label="Platform">{platform.data ?? '—'}</KeyValue>
        </CardContent>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

/**
 * The directories a capture scans for Git repositories.
 *
 * This is the one capture setting that is a real setting: the adapter runs
 * against whatever is here, and changing it changes what the next capture sees.
 */
function CaptureTab() {
  const queryClient = useQueryClient();
  const scan = useQuery({ queryKey: ['project-scan'], queryFn: listProjectRoots });

  const [draft, setDraft] = useState<string[] | null>(null);
  const [newRoot, setNewRoot] = useState('');

  // Seeded from the query once, then owned by the user. Re-seeding on every refetch
  // would throw away half-typed input the moment the scan refreshed.
  useEffect(() => {
    if (draft === null && scan.data) {
      setDraft(scan.data.roots);
    }
  }, [scan.data, draft]);

  const save = useMutation({
    mutationFn: (roots: string[]) => setProjectRoots(roots),
    onSuccess: (roots) => {
      setDraft(roots);
      setNewRoot('');
      void queryClient.invalidateQueries({ queryKey: ['project-scan'] });
    },
  });

  const roots = draft ?? scan.data?.roots ?? [];
  const dirty =
    draft !== null &&
    JSON.stringify([...draft].sort()) !== JSON.stringify([...(scan.data?.roots ?? [])].sort());

  if (scan.isError) {
    return (
      <div role="alert" className="p-4 bg-destructive/10 border border-destructive/30 rounded-lg">
        <h3 className="font-medium text-destructive">The project folders could not be scanned</h3>
        <p className="text-sm text-destructive/90 mt-1">{errorMessage(scan.error)}</p>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <FolderOpen className="w-5 h-5" />
            <CardTitle>Project folders</CardTitle>
          </div>
          <CardDescription>
            A capture looks for Git repositories in these folders, one level deep. Change the
            list to match where you keep your work.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {scan.isLoading ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="w-4 h-4 animate-spin" />
              Scanning…
            </div>
          ) : (
            <>
              <div className="space-y-1">
                {roots.map((root) => (
                  <div key={root} className="flex items-center gap-2">
                    <span className="flex-1 font-mono text-sm truncate">{root}</span>
                    {(scan.data?.missingRoots ?? []).includes(root) && (
                      <Badge variant="warning">
                        <AlertTriangle className="w-3 h-3 mr-1" />
                        Not found
                      </Badge>
                    )}
                    <Button
                      variant="ghost"
                      size="icon"
                      title="Remove this folder"
                      onClick={() => setDraft((prev) => (prev ?? []).filter((r) => r !== root))}
                    >
                      <X className="w-4 h-4" />
                    </Button>
                  </div>
                ))}
                {roots.length === 0 && (
                  <p className="text-sm text-muted-foreground">
                    No folders configured, so a capture would find no projects.
                  </p>
                )}
              </div>

              <Separator />

              <div className="flex items-end gap-2">
                <div className="flex-1 space-y-1">
                  <Label htmlFor="new-root">Add a folder</Label>
                  <Input
                    id="new-root"
                    value={newRoot}
                    onChange={(e) => setNewRoot(e.target.value)}
                    placeholder="~/projects"
                    className="font-mono text-sm"
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && newRoot.trim()) {
                        setDraft((prev) =>
                          prev && !prev.includes(newRoot.trim()) ? [...prev, newRoot.trim()] : prev
                        );
                        setNewRoot('');
                      }
                    }}
                  />
                </div>
                <Button
                  variant="secondary"
                  disabled={!newRoot.trim()}
                  onClick={() => {
                    setDraft((prev) =>
                      prev && !prev.includes(newRoot.trim()) ? [...prev, newRoot.trim()] : prev
                    );
                    setNewRoot('');
                  }}
                >
                  <Plus className="w-4 h-4" />
                  Add
                </Button>
              </div>
            </>
          )}

          <div className="flex justify-end gap-2">
            {dirty && (
              <Button variant="outline" onClick={() => setDraft(scan.data?.roots ?? [])}>
                Discard
              </Button>
            )}
            <Button
              loading={save.isPending}
              disabled={!dirty || roots.length === 0}
              onClick={() => save.mutate(roots)}
            >
              <Check className="w-4 h-4 mr-2" />
              Save and rescan
            </Button>
          </div>

          {save.isError && (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(save.error)}
            </p>
          )}
        </CardContent>
      </Card>

      {scan.data && (
        <Card>
          <CardHeader>
            <CardTitle>What the scan found</CardTitle>
            <CardDescription>
              {scan.data.projects.length} repositor{scan.data.projects.length === 1 ? 'y' : 'ies'}
              {scan.data.projects.length === 0 ? ' — nothing to capture yet.' : '.'}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-2">
            {scan.data.missingRoots.length > 0 && (
              <div className="p-3 bg-warning-bg border border-warning-border rounded-lg">
                <p className="text-sm text-warning-fg font-medium">
                  {scan.data.missingRoots.length} configured folder
                  {scan.data.missingRoots.length === 1 ? '' : 's'} do not exist
                </p>
                <ul className="text-xs text-fg-muted font-mono mt-1 list-disc list-inside">
                  {scan.data.missingRoots.map((r) => (
                    <li key={r}>{r}</li>
                  ))}
                </ul>
                <p className="text-xs text-fg-muted mt-1">
                  These are reported rather than silently skipped, so an empty project list is
                  distinguishable from looking in the wrong place.
                </p>
              </div>
            )}
            {scan.data.unreadableRoots.length > 0 && (
              <div className="p-3 bg-danger-bg border border-danger-border rounded-lg">
                <p className="text-sm text-danger-fg font-medium">
                  {scan.data.unreadableRoots.length} folder
                  {scan.data.unreadableRoots.length === 1 ? '' : 's'} could not be read
                </p>
                <p className="text-xs text-fg-muted font-mono mt-1">
                  {scan.data.unreadableRoots.join(', ')}
                </p>
                <p className="text-xs text-fg-muted mt-1">
                  A permissions problem, not a missing folder. Grant this app access in System
                  Settings → Privacy &amp; Security → Files and Folders.
                </p>
              </div>
            )}
            {scan.data.projects.length > 0 && (
              <ul className="text-sm space-y-1 max-h-64 overflow-y-auto">
                {scan.data.projects.map((p) => (
                  <li key={p.path ?? p.id} className="flex items-center gap-2">
                    <Badge variant="outline" className="text-xs font-mono">
                      {p.isGitRepo ? 'git' : 'plain'}
                    </Badge>
                    <span className="truncate">{p.name}</span>
                    <span className="text-xs text-muted-foreground font-mono truncate">
                      {p.path}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void scan.refetch()}
              loading={scan.isFetching}
            >
              <RefreshCw className="w-3 h-3 mr-1" />
              Rescan
            </Button>
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader>
          <CardTitle>What a capture always includes</CardTitle>
          <CardDescription>
            Not toggles — this build captures exactly this, every time.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ul className="text-sm text-muted-foreground space-y-1 list-disc list-inside">
            <li>Repository path, current branch, remote, and whether the worktree is dirty</li>
            <li>Which applications you selected, with their detected versions</li>
            <li>
              Browser URLs you pasted in — this build does not read your browser, so there is
              nothing to opt out of
            </li>
            <li>Terminal working directories and the commands the adapters suggested</li>
            <li>
              Environment variable <em>names</em> only. Values are never read, so a workspace
              cannot carry a credential
            </li>
            <li>
              The manifest is sealed with this device's local key before it is written to disk,
              and its digest is recorded alongside
            </li>
          </ul>
        </CardContent>
      </Card>
    </div>
  );
}


// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

function DataTab() {
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: ['settings'], queryFn: listSettings });
  const workspaces = useQuery({ queryKey: ['workspaces'], queryFn: () => listWorkspaces() });
  const paired = useQuery({ queryKey: ['paired-devices'], queryFn: listPairedDevices });

  const [importText, setImportText] = useState('');
  const [notice, setNotice] = useState<string | null>(null);

  const doExport = useMutation({
    mutationFn: exportSettings,
    onSuccess: (data) => {
      const keys = Object.keys(data);
      setNotice(
        keys.length === 0
          ? 'There is nothing stored to export yet.'
          : `Exported ${keys.length} setting${keys.length === 1 ? '' : 's'}: ${keys.join(', ')}`
      );
      void navigator.clipboard?.writeText(JSON.stringify(data, null, 2));
    },
  });

  const doImport = useMutation({
    mutationFn: (text: string) => {
      const parsed: unknown = JSON.parse(text);
      if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
        throw new Error('Expected a JSON object of settings.');
      }
      return importSettings(parsed as Record<string, string>);
    },
    onSuccess: () => {
      setImportText('');
      setNotice('Settings imported.');
      void queryClient.invalidateQueries({ queryKey: ['settings'] });
    },
    onError: (error) => setNotice(`Import failed: ${errorMessage(error)}`),
  });

  const removeSetting = useMutation({
    mutationFn: (key: string) => deleteSetting(key),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['settings'] });
    },
  });

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Database className="w-5 h-5" />
            <CardTitle>What is stored on this machine</CardTitle>
          </div>
          <CardDescription>
            A local SQLite database. Nothing leaves it except the sealed manifest you explicitly
            send to a paired device.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-1 text-sm">
          <KeyValue label="Workspaces">
            {workspaces.isLoading ? '…' : String(workspaces.data?.length ?? 0)}
          </KeyValue>
          <KeyValue label="Paired devices">
            {paired.isLoading ? '…' : String(paired.data?.length ?? 0)}
          </KeyValue>
          <KeyValue label="Settings">
            {settings.isLoading ? '…' : String(settings.data?.length ?? 0)}
          </KeyValue>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Settings in the database</CardTitle>
          <CardDescription>
            {settings.data && settings.data.length > 0
              ? 'Each row is read by the code, not just recorded.'
              : 'Nothing stored yet. The capture folder list is the first thing to be written.'}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {settings.data?.map(([key, value]) => (
            <div key={key} className="flex items-center gap-2 text-sm">
              <span className="font-mono min-w-40">{key}</span>
              <span className="flex-1 font-mono text-xs text-muted-foreground truncate">
                {value}
              </span>
              <Button
                variant="ghost"
                size="icon"
                title={`Forget ${key}`}
                onClick={() => removeSetting.mutate(key)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))}

          <Separator />

          <div className="space-y-2">
            <Label htmlFor="import-settings">Import settings</Label>
            <textarea
              id="import-settings"
              value={importText}
              onChange={(e) => setImportText(e.target.value)}
              rows={4}
              placeholder='{"project_roots": "[\"~/projects\"]"}'
              className="w-full rounded-md border border-input bg-background p-3 text-xs font-mono"
            />
            <div className="flex gap-2">
              <Button
                variant="secondary"
                loading={doImport.isPending}
                disabled={!importText.trim()}
                onClick={() => doImport.mutate(importText)}
              >
                <Upload className="w-4 h-4" />
                Import
              </Button>
              <Button
                variant="outline"
                loading={doExport.isPending}
                onClick={() => doExport.mutate()}
              >
                <Download className="w-4 h-4" />
                Export (to clipboard)
              </Button>
            </div>
            {notice && <p className="text-sm text-muted-foreground">{notice}</p>}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Appearance</CardTitle>
          <CardDescription>
            Both modes are designed, not one derived from the other by inverting it, so the
            window looks deliberate either way. The choice is remembered on this machine.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="max-w-sm">
            <AppearanceControl />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Settings this build does not have</CardTitle>
          <CardDescription>
            Listed so the absence is a fact rather than an oversight. Each was a switch on this
            screen that changed nothing.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ul className="text-sm text-muted-foreground space-y-1 list-disc list-inside">
            <li>Language — every string in the app is English, with no translation layer behind it</li>
            <li>
              Update checking and start-minimised — no updater or tray behaviour exists yet
            </li>
            <li>
              Telemetry and crash reporting — this build sends nothing anywhere, and adding a
              switch would imply it might
            </li>
            <li>
              Database encryption — manifests are sealed individually with a key from the OS
              credential store; the database itself is not encrypted
            </li>
            <li>
              Data retention — nothing is ever deleted on a timer; delete a workspace from the list
            </li>
            <li>
              LAN discovery and relay preferences — discovery is always on, and a relay does not
              exist in this build
            </li>
            <li>Log level and debug mode — logging is configured in Rust, not from here</li>
          </ul>
        </CardContent>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------

function KeyValue({
  label,
  children,
  mono,
}: {
  label: string;
  children: React.ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className={mono ? 'font-mono text-xs break-all' : 'text-sm break-all'}>
        {children}
      </span>
    </div>
  );
}
