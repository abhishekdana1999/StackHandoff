import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  Check,
  Copy,
  Edit,
  Key,
  Loader2,
  Plus,
  QrCode,
  Shield,
  ShieldCheck,
  Trash2,
  Wifi,
  WifiOff,
} from 'lucide-react';
import { EmptyStateCard } from '@components/EmptyState';
import { Button } from '@components/ui/Button';
import { Card, CardContent } from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { Input } from '@components/ui/Input';
import { Label } from '@components/ui/Label';
import { Dialog, DialogContent } from '@components/ui/Dialog';
import {
  createPairingInvitation,
  deletePairedDevice,
  errorMessage,
  getDeviceIdentity,
  getDiscoveredDevices,
  getSafetyNumber,
  listPairedDevices,
  startDiscovery,
  updatePairedDevice,
  verifyPairing,
} from '@lib/ipc';
import type { DiscoveredDevice, PairedDevice, RequestedTrustScope } from '@model';
import { scopeLabel, TRUST_SCOPES } from '@lib/trustScopes';

/** The scopes the backend accepts, with the plain-English meaning of each. */
// Both scope vocabularies and their labels come from one table. See the header of
// that module for the bug this prevents.
const SCOPES = TRUST_SCOPES;

/** What the user is doing in the pairing dialog. */
type PairMode = 'choose' | 'safety' | 'invite';

export function DevicesScreen() {
  const queryClient = useQueryClient();

  const [dialogOpen, setDialogOpen] = useState(false);
  const [mode, setMode] = useState<PairMode>('choose');
  const [remoteKey, setRemoteKey] = useState('');
  const [remoteName, setRemoteName] = useState('');
  // Short form: this is the *request* direction, and `parse_trust_scopes`
  // accepts `'receive' | 'send' | 'files' | 'clipboard'`. What comes back on a
  // `PairedDevice` afterwards is the kebab-case spelling instead, which is why
  // there are two scope types.
  const [scopes, setScopes] = useState<RequestedTrustScope[]>(
    SCOPES.filter((s) => s.requested === 'receive' || s.requested === 'send').map(
      (s) => s.requested
    )
  );
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');

  // Discovery is kicked off, not merely read: a device that was never looked
  // for is not in the list however long the user waits.
  useQuery({ queryKey: ['discovery-start'], queryFn: startDiscovery, staleTime: Infinity });

  const identity = useQuery({ queryKey: ['device-identity'], queryFn: getDeviceIdentity });
  const paired = useQuery({ queryKey: ['paired-devices'], queryFn: listPairedDevices });
  const discovered = useQuery({
    queryKey: ['discovered-devices'],
    queryFn: getDiscoveredDevices,
    refetchInterval: 5000,
  });

  const onlineIds = new Set(
    (discovered.data ?? [])
      .filter((d) => d.static_public_key.length > 0)
      .map((d) => d.device_id)
  );

  const invalidation = () => {
    void queryClient.invalidateQueries({ queryKey: ['paired-devices'] });
    void queryClient.invalidateQueries({ queryKey: ['discovered-devices'] });
  };

  const invite = useMutation({ mutationFn: (name: string) => createPairingInvitation(name) });

  const computeSafetyNumber = useMutation({
    mutationFn: (key: string) => getSafetyNumber(key),
    onSuccess: () => {
      setMode('safety');
    },
  });

  // Sends the number this screen displayed rather than one the user retyped.
  // The comparison still has to happen -- between the two screens, out loud --
  // but the app cannot verify that a human did it, so it does not pretend to
  // verify it. What it *can* verify is that the key being paired is the key the
  // displayed number was derived from, and that check stays: it is in the Rust
  // command, and the frontend cannot talk the backend into pairing a different
  // key.
  const pair = useMutation({
    mutationFn: () =>
      verifyPairing({
        remoteNoiseKeyB64: remoteKey,
        deviceName: remoteName,
        confirmedSafetyNumber: computeSafetyNumber.data?.number ?? '',
        trustScopes: scopes,
      }),
    onSuccess: () => {
      invalidation();
      closeDialog();
    },
  });

  const rename = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => updatePairedDevice(id, name),
    onSuccess: () => {
      setRenaming(null);
      setRenameValue('');
      invalidation();
    },
  });

  const remove = useMutation({
    mutationFn: (id: string) => deletePairedDevice(id),
    onSuccess: invalidation,
  });

  function closeDialog() {
    setDialogOpen(false);
    setMode('choose');
    setRemoteKey('');
    setRemoteName('');
    pair.reset();
    computeSafetyNumber.reset();
  }

  /** A device on the network that has a key and is not already paired. */
  const pairable: DiscoveredDevice[] = (discovered.data ?? []).filter(
    (d) =>
      d.static_public_key.length > 0 &&
      d.device_id !== identity.data?.fingerprint &&
      !(paired.data ?? []).some((p) => p.id === d.device_id && !p.revoked)
  );

  const pairedList: PairedDevice[] = paired.data ?? [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <p className="text-[13px] text-muted-foreground">
          Machines this app will exchange workspaces with, over an authenticated connection.
        </p>
        <Button onClick={() => setDialogOpen(true)}>
          <Plus className="w-4 h-4 mr-2" />
          Pair a device
        </Button>
      </div>

      {/* --- Paired list --------------------------------------------------- */}
      {paired.isLoading ? (
        <Card>
          <CardContent className="py-12 text-center">
            <Loader2 className="w-8 h-8 animate-spin text-primary mx-auto mb-2" />
            <p className="text-muted-foreground">Loading devices…</p>
          </CardContent>
        </Card>
      ) : paired.isError ? (
        <div role="alert" className="p-4 bg-destructive/10 border border-destructive/30 rounded-lg">
          <h3 className="font-medium text-destructive">The device list could not be read</h3>
          <p className="text-sm text-destructive/90 mt-1">{errorMessage(paired.error)}</p>
        </div>
      ) : pairedList.length === 0 ? (
        <EmptyStateCard
          icon={<Shield className="h-5 w-5" />}
          title="No paired devices yet"
          action={
            <Button onClick={() => setDialogOpen(true)}>
              <Plus className="mr-1.5 h-3.5 w-3.5" aria-hidden="true" />
              Pair the first device
            </Button>
          }
        >
          Pair another machine running this app. Until one is paired, a workspace cannot be sent
          anywhere.
        </EmptyStateCard>
      ) : (
        <div className="space-y-4">
          {pairedList.map((device) => {
            const isOnline = !device.revoked && onlineIds.has(device.id);
            return (
              <Card key={device.id} className={device.revoked ? 'opacity-70' : ''}>
                <CardContent className="pt-6">
                  <div className="flex flex-wrap items-start justify-between gap-4">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-3 flex-wrap">
                        {renaming === device.id ? (
                          <div className="flex items-center gap-2">
                            <Input
                              value={renameValue}
                              onChange={(e) => setRenameValue(e.target.value)}
                              onKeyDown={(e) => {
                                if (e.key === 'Enter' && renameValue.trim()) {
                                  rename.mutate({ id: device.id, name: renameValue.trim() });
                                } else if (e.key === 'Escape') {
                                  setRenaming(null);
                                }
                              }}
                              className="h-8"
                              autoFocus
                              aria-label={`New name for ${device.name}`}
                            />
                            <Button
                              size="sm"
                              loading={rename.isPending}
                              disabled={!renameValue.trim()}
                              onClick={() =>
                                rename.mutate({ id: device.id, name: renameValue.trim() })
                              }
                            >
                              Save
                            </Button>
                          </div>
                        ) : (
                          <>
                            <h3 className="font-semibold text-[15px] truncate">{device.name}</h3>
                            <Badge variant={isOnline ? 'success' : 'secondary'}>
                              {isOnline ? (
                                <>
                                  <Wifi className="w-3 h-3 mr-1" />
                                  On network
                                </>
                              ) : (
                                <>
                                  <WifiOff className="w-3 h-3 mr-1" />
                                  Not reachable
                                </>
                              )}
                            </Badge>
                            {device.revoked && (
                              <Badge variant="danger">
                                <AlertTriangle className="w-3 h-3 mr-1" />
                                Revoked
                              </Badge>
                            )}
                          </>
                        )}
                      </div>

                      <div className="flex flex-wrap items-center gap-4 mt-2 text-sm text-muted-foreground">
                        <span className="font-mono text-xs">{device.fingerprint}</span>
                        <span>
                          {device.os} {device.os_version}
                        </span>
                        <span>App v{device.app_version}</span>
                        <span>Paired {new Date(device.created_at).toLocaleString()}</span>
                        {device.last_seen && (
                          <span>Last seen {new Date(device.last_seen).toLocaleString()}</span>
                        )}
                      </div>

                      <div className="mt-2 flex flex-wrap gap-2">
                        {device.trust_scopes.map((scope) => (
                          <Badge key={scope} variant="outline">
                            {scopeLabel(scope)}
                          </Badge>
                        ))}
                        {device.trust_scopes.length === 0 && (
                          <Badge variant="warning">
                            Allowed nothing
                          </Badge>
                        )}
                      </div>

                      <p className="text-xs text-muted-foreground mt-2">
                        This is a fingerprint of the key the connection is authenticated with. It is
                        not the machine's Ed25519 identity key, which this build advertises but does
                        not verify.
                      </p>
                    </div>

                    <div className="flex items-center gap-2 flex-shrink-0">
                      <Button
                        variant="ghost"
                        size="icon"
                        title="Rename this device"
                        disabled={device.revoked}
                        onClick={() => {
                          setRenaming(device.id);
                          setRenameValue(device.name);
                        }}
                      >
                        <Edit className="w-4 h-4" />
                      </Button>
                      {!device.revoked && (
                        <Button
                          variant="ghost"
                          size="icon"
                          title="Forget this device"
                          onClick={() => {
                            if (
                              window.confirm(
                                `Forget ${device.name}? The pairing is removed, along with the workspaces it sent to this machine and its transfer records.`
                              )
                            ) {
                              remove.mutate(device.id);
                            }
                          }}
                        >
                          <Trash2 className="w-4 h-4" />
                        </Button>
                      )}
                    </div>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}

      {(remove.isError || rename.isError) && (
        <div role="alert" className="p-4 bg-destructive/10 border border-destructive/30 rounded-lg">
          <p className="text-sm text-destructive">
            {errorMessage(remove.error ?? rename.error)}
          </p>
        </div>
      )}

      {/* --- Pairing dialog ----------------------------------------------- */}
      <Dialog open={dialogOpen} onOpenChange={(open) => (open ? setDialogOpen(true) : closeDialog())}>
        <DialogContent className="max-w-lg max-h-[90vh] overflow-y-auto">
          {mode === 'choose' && (
            <div className="space-y-5">
              <div>
                <h2 className="text-[15px] font-semibold">Pair a device</h2>
                <p className="text-sm text-muted-foreground mt-1">
                  Both machines need this app open and on the same network, or you need the other
                  machine's pairing code.
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="this-device-name">This device&apos;s name</Label>
                <Input
                  id="this-device-name"
                  placeholder="e.g. MacBook Pro"
                  value={remoteName}
                  onChange={(e) => setRemoteName(e.target.value)}
                  autoFocus
                />
                <p className="text-xs text-muted-foreground">
                  Shown to the person at the other machine so they know what they are pairing with.
                </p>
              </div>

              {pairable.length > 0 && (
                <div className="space-y-2">
                  <Label>Devices on the network</Label>
                  {pairable.map((d) => (
                    <button
                      key={d.device_id}
                      onClick={() => {
                        setRemoteKey(d.static_public_key);
                        setRemoteName(remoteName || d.name);
                        computeSafetyNumber.mutate(d.static_public_key);
                      }}
                      className="w-full p-3 rounded-lg border hover:bg-muted/50 text-left flex items-center gap-3"
                    >
                      <Wifi className="w-4 h-4 text-success flex-shrink-0" />
                      <div className="min-w-0">
                        <div className="font-medium truncate">{d.name}</div>
                        <div className="text-xs text-muted-foreground font-mono">
                          {d.device_id}
                        </div>
                      </div>
                    </button>
                  ))}
                </div>
              )}

              <div className="space-y-2">
                <Label htmlFor="remote-key">Or paste the other machine&apos;s key</Label>
                <Input
                  id="remote-key"
                  placeholder="Paste a stackhandoff://pair?... link, or the bare base64 key"
                  value={remoteKey}
                  onChange={(e) => setRemoteKey(e.target.value)}
                  className="font-mono text-xs"
                />
                <Button
                  variant="secondary"
                  className="w-full"
                  loading={computeSafetyNumber.isPending}
                  disabled={!remoteKey.trim()}
                  onClick={() => computeSafetyNumber.mutate(remoteKey)}
                >
                  <ShieldCheck className="w-4 h-4 mr-2" />
                  Check the safety number
                </Button>
                {computeSafetyNumber.isError && (
                  <p role="alert" className="text-sm text-destructive">
                    {errorMessage(computeSafetyNumber.error)}
                  </p>
                )}
              </div>

              <div className="space-y-2">
                <Label>What this device is allowed to do</Label>
                <div className="space-y-1">
                  {SCOPES.map((scope) => (
                    <label
                      key={scope.requested}
                      className="flex items-start gap-2 cursor-pointer"
                    >
                      <input
                        type="checkbox"
                        checked={scopes.includes(scope.requested)}
                        onChange={(e) =>
                          setScopes((prev) =>
                            e.target.checked
                              ? [...prev, scope.requested]
                              : prev.filter((s) => s !== scope.requested)
                          )
                        }
                        className="rounded border-input mt-1"
                      />
                      <span className="text-sm">
                        <span className="font-medium">{scope.label}</span>{' '}
                        <span className="text-muted-foreground">— {scope.detail}</span>
                      </span>
                    </label>
                  ))}
                </div>
                {scopes.length === 0 && (
                  <p className="text-xs text-destructive">
                    Choose at least one. A device allowed nothing can be paired, but nothing can be
                    done with it.
                  </p>
                )}
              </div>

              <Button variant="outline" className="w-full" onClick={() => setMode('invite')}>
                <QrCode className="w-4 h-4 mr-2" />
                Show my pairing code instead
              </Button>
            </div>
          )}

          {mode === 'safety' && (
            <div className="space-y-5">
              <div>
                <h2 className="text-[15px] font-semibold">Compare this number out loud</h2>
                <p className="text-sm text-muted-foreground mt-1">
                  Read it to the person at the other device. Both screens show the same digits for
                  the same pair of machines. If they differ, stop — do not pair.
                </p>
              </div>

              {computeSafetyNumber.data && (
                <div className="p-4 bg-muted rounded-lg text-center">
                  <div className="font-mono text-xl tracking-wider break-all">
                    {computeSafetyNumber.data.number}
                  </div>
                  <p className="text-xs text-muted-foreground mt-2">
                    {computeSafetyNumber.data.note}
                  </p>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="mt-2"
                    onClick={() => void navigator.clipboard?.writeText(computeSafetyNumber.data!.number)}
                  >
                    <Copy className="w-3 h-3 mr-1" />
                    Copy
                  </Button>
                </div>
              )}

              <div className="rounded-lg border border-warning-border bg-warning-bg p-3">
                <p className="text-sm font-medium">Before you pair, look at the other screen</p>
                <ol className="mt-2 space-y-1 text-sm text-muted-foreground list-decimal list-inside">
                  <li>On the other machine, open Devices → Pair this device</li>
                  <li>Select this machine, so both screens show a safety number</li>
                  <li>
                    Check the digits match, then come back here and continue
                  </li>
                </ol>
                <p className="mt-2 text-xs text-muted-foreground">
                  Both screens show the same number for the same two machines, so if they differ,
                  something is substituting one device for the other. Stop and do not pair.
                </p>
              </div>

              {pair.isError && (
                <div
                  role="alert"
                  className="p-3 bg-destructive/10 border border-destructive/30 rounded-lg"
                >
                  <p className="text-sm text-destructive">{errorMessage(pair.error)}</p>
                </div>
              )}

              <div className="flex gap-2">
                <Button variant="outline" onClick={() => setMode('choose')}>
                  Back
                </Button>
                <Button
                  className="flex-1"
                  loading={pair.isPending}
                  disabled={scopes.length === 0}
                  onClick={() => pair.mutate()}
                >
                  <Check className="w-4 h-4 mr-2" />
                  The numbers match — pair
                </Button>
              </div>
            </div>
          )}

          {mode === 'invite' && (
            <div className="space-y-5">
              <div>
                <h2 className="text-[15px] font-semibold">Your pairing code</h2>
                <p className="text-sm text-muted-foreground mt-1">
                  Give this to the person at the other machine. It carries your public keys, and it
                  expires in five minutes.
                </p>
              </div>

              {invite.isError && (
                <div role="alert" className="p-3 bg-destructive/10 border border-destructive/30 rounded-lg">
                  <p className="text-sm text-destructive">{errorMessage(invite.error)}</p>
                </div>
              )}

              {!invite.data && (
                <Button
                  className="w-full"
                  loading={invite.isPending}
                  onClick={() => invite.mutate(remoteName.trim() || 'This device')}
                >
                  <Key className="w-4 h-4 mr-2" />
                  Generate a pairing code
                </Button>
              )}

              {invite.data && (
                <>
                  <div className="p-4 bg-muted rounded-lg">
                    <div className="text-sm font-medium mb-2">Code</div>
                    <div className="flex items-center gap-2">
                      <code className="flex-1 font-mono text-lg tracking-widest bg-background px-3 py-2 rounded border">
                        {invite.data.code}
                      </code>
                      <Button
                        variant="outline"
                        size="icon"
                        title="Copy the code"
                        onClick={() => void navigator.clipboard?.writeText(invite.data!.code)}
                      >
                        <Copy className="w-4 h-4" />
                      </Button>
                    </div>
                    <p className="text-xs text-muted-foreground mt-2">
                      Expires {new Date(invite.data.expires_at).toLocaleTimeString()}.
                    </p>
                  </div>

                  <div className="space-y-2">
                    <div className="flex items-center justify-between">
                      <Label htmlFor="qr-data">Or copy this link</Label>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => void navigator.clipboard?.writeText(invite.data!.qr_data)}
                      >
                        <Copy className="w-3 h-3 mr-1" />
                        Copy
                      </Button>
                    </div>
                    <p
                      id="qr-data"
                      className="text-xs font-mono bg-muted p-2 rounded break-all"
                    >
                      {invite.data.qr_data}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      Paste it into the &ldquo;paste the other machine&apos;s key&rdquo; box on the
                      other device. This build has no camera access, so there is no QR image to
                      scan — the link is the whole of it.
                    </p>
                  </div>

                  <div className="p-3 bg-primary-soft border border-primary-soft-border rounded-lg">
                    <p className="text-sm text-primary">
                      Once they have paired, the safety numbers must be compared. A code alone does
                      not pair anything.
                    </p>
                  </div>
                </>
              )}

              <Button variant="outline" className="w-full" onClick={() => setMode('choose')}>
                Back
              </Button>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
