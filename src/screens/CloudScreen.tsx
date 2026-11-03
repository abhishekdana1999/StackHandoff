import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Cloud,
  CreditCard,
  Laptop,
  Loader2,
  LogOut,
  Monitor,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Zap,
} from "lucide-react";
import { Button } from "@components/ui/Button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@components/ui/Card";
import { Badge } from "@components/ui/Badge";
import { Input } from "@components/ui/Input";
import { Label } from "@components/ui/Label";
import { Separator } from "@components/ui/Separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@components/ui/Tabs";
import {
  CLOUD_API_BASE_URL,
  cancelCloudPlan,
  cancelCloudTransfer,
  captureCloudCheckout,
  createCloudCheckout,
  getCloudLimits,
  getCloudPayPalConfig,
  getCloudProfile,
  getCloudSubscription,
  listCloudDevices,
  listCloudPlans,
  listCloudTransfers,
  loginCloud,
  logoutCloud,
  registerCloudAccount,
  registerCloudDevice,
  revokeCloudDevice,
  type CloudDevice,
  type CloudPlan,
  type CloudPlanLimits,
  type CloudTransfer,
  type PlanId,
} from "@lib/cloudApi";
import { getAppVersion, getDeviceIdentity, getPlatform } from "@lib/ipc";
import { useCloudStore } from "@store/useCloudStore";

/**
 * The cloud account surface: plans, sign-in, subscription and the cloud-side
 * inventory of devices and transfers.
 *
 * The local P2P product works without any of this, so the screen is written to
 * be honest about that: signed out, it is a pricing page with a sign-in form,
 * and it says so rather than pretending the app is crippled. Signed in, it
 * shows only what the cloud backend actually knows — the account, the plan and
 * its limits, the registered devices and the relay transfers — and leaves the
 * local workspaces and pairings to the screens that own them.
 *
 * Every value is a real backend field. Where that is a limit (for example
 * `maxDevices: 0` on Free) the UI states the number rather than hiding it,
 * because "0 devices" is the accurate reason cloud device registration fails on
 * the free plan.
 */

const PLAN_RANK: Record<PlanId, number> = { free: 0, basic: 1, pro: 2 };

function formatPrice(cents: number, interval: string): string {
  if (cents === 0) return "Free";
  const amount = `$${(cents / 100).toFixed(2)}`;
  return interval === "month" ? `${amount}/mo` : amount;
}

function describeLimit(value: number): string {
  return value < 0 ? "Unlimited" : String(value);
}

function errorText(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

/** The pricing comparison. Public: it needs no session. */
function PlanGrid({
  plans,
  currentPlan,
  onChoose,
  pendingPlan,
  canChoose,
  checkoutAvailable = true,
}: {
  plans: CloudPlan[];
  currentPlan: PlanId | null;
  onChoose: (planId: Exclude<PlanId, "free">) => void;
  pendingPlan: Exclude<PlanId, "free"> | null;
  canChoose: boolean;
  /** False hides the PayPal checkout buttons until the server has credentials. */
  checkoutAvailable?: boolean;
}) {
  return (
    <div className="grid gap-4 md:grid-cols-3">
      {plans.map((plan) => {
        const isCurrent = currentPlan === plan.id;
        const isDowngrade =
          currentPlan !== null && PLAN_RANK[plan.id] < PLAN_RANK[currentPlan];
        return (
          <Card
            key={plan.id}
            className={isCurrent ? "border-primary" : undefined}
          >
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle className="text-[14px]">{plan.name}</CardTitle>
                {isCurrent && <Badge variant="accent">Current plan</Badge>}
              </div>
              <CardDescription className="mt-1">
                {plan.description}
              </CardDescription>
              <p className="pt-2 text-lg font-semibold">
                {formatPrice(plan.priceCents, plan.billingInterval)}
              </p>
            </CardHeader>
            <CardContent className="space-y-3">
              <ul className="space-y-1.5 text-[13px] text-fg-muted">
                {plan.features.map((feature) => (
                  <li key={feature} className="flex gap-2">
                    <ShieldCheck
                      className="mt-0.5 h-3.5 w-3.5 shrink-0 text-success"
                      aria-hidden="true"
                    />
                    <span>{feature}</span>
                  </li>
                ))}
              </ul>
              <Separator />
              <dl className="grid grid-cols-2 gap-y-1 text-[12px] text-fg-subtle">
                <dt>Devices</dt>
                <dd className="text-right text-fg-muted">
                  {describeLimit(plan.limits.maxDevices)}
                </dd>
                <dt>Workspaces</dt>
                <dd className="text-right text-fg-muted">
                  {describeLimit(plan.limits.maxWorkspaces)}
                </dd>
                <dt>Cloud relay</dt>
                <dd className="text-right text-fg-muted">
                  {plan.limits.cloudSync
                    ? `Yes · ${plan.limits.transferTtlHours}h`
                    : "No"}
                </dd>
                <dt>Full app cloning</dt>
                <dd className="text-right text-fg-muted">
                  {plan.limits.fullAppCloning ? "Yes" : "No"}
                </dd>
              </dl>

              {canChoose &&
                plan.id !== "free" &&
                !isCurrent &&
                !isDowngrade &&
                checkoutAvailable && (
                  <Button
                    className="w-full"
                    loading={pendingPlan === plan.id}
                    onClick={() => onChoose(plan.id as Exclude<PlanId, "free">)}
                  >
                    <Zap className="mr-1.5 h-3.5 w-3.5" aria-hidden="true" />
                    Switch to {plan.name}
                  </Button>
                )}
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}

function SignedOut({
  plans,
  plansLoading,
  plansError,
}: {
  plans: CloudPlan[];
  plansLoading: boolean;
  plansError: unknown;
}) {
  const setSession = useCloudStore((s) => s.setSession);
  const [mode, setMode] = useState<"login" | "register">("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");

  const login = useMutation({
    mutationFn: () => loginCloud({ email, password }),
    onSuccess: (session) => setSession(session),
  });

  // Registration returns a profile but no tokens in this backend, so a
  // successful signup is immediately followed by a login. Doing it in one
  // mutation keeps the button's pending state honest across both calls.
  const register = useMutation({
    mutationFn: async () => {
      await registerCloudAccount({
        email,
        password,
        name: name.trim() || undefined,
      });
      return loginCloud({ email, password });
    },
    onSuccess: (session) => setSession(session),
  });

  const active = mode === "login" ? login : register;
  const canSubmit = email.trim().length > 0 && password.length > 0;

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Cloud className="h-4 w-4 text-primary" aria-hidden="true" />
            <CardTitle className="text-[14px]">Cloud account</CardTitle>
          </div>
          <CardDescription>
            The local peer-to-peer handoff needs no account. Sign in only to use
            the cloud relay, beyond your own network.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Tabs
            value={mode}
            onValueChange={(value) => {
              setMode(value as "login" | "register");
              active.reset();
            }}
          >
            <TabsList>
              <TabsTrigger value="login">Sign in</TabsTrigger>
              <TabsTrigger value="register">Create account</TabsTrigger>
            </TabsList>

            <TabsContent value="login">
              <div className="space-y-3 pt-2">
                <div className="space-y-1.5">
                  <Label htmlFor="cloud-email">Email</Label>
                  <Input
                    id="cloud-email"
                    type="email"
                    autoComplete="email"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="cloud-password">Password</Label>
                  <Input
                    id="cloud-password"
                    type="password"
                    autoComplete="current-password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                  />
                </div>
              </div>
            </TabsContent>

            <TabsContent value="register">
              <div className="space-y-3 pt-2">
                <div className="space-y-1.5">
                  <Label htmlFor="cloud-name">Name (optional)</Label>
                  <Input
                    id="cloud-name"
                    autoComplete="name"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="cloud-register-email">Email</Label>
                  <Input
                    id="cloud-register-email"
                    type="email"
                    autoComplete="email"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="cloud-register-password">Password</Label>
                  <Input
                    id="cloud-register-password"
                    type="password"
                    autoComplete="new-password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                  />
                  <p className="text-[11px] text-fg-subtle">
                    At least 6 characters, with an uppercase letter, a lowercase
                    letter, a digit and a symbol.
                  </p>
                </div>
              </div>
            </TabsContent>
          </Tabs>

          {active.isError && (
            <p role="alert" className="mt-3 text-[13px] text-danger">
              {errorText(active.error)}
            </p>
          )}

          <Button
            className="mt-4 w-full"
            loading={active.isPending}
            disabled={!canSubmit}
            onClick={() => active.mutate()}
          >
            {mode === "login" ? "Sign in" : "Create account"}
          </Button>
        </CardContent>
      </Card>

      <div>
        <h2 className="mb-3 text-[13px] font-semibold">Plans</h2>
        {plansError ? (
          <div
            role="alert"
            className="rounded-lg border border-danger-border bg-danger-bg p-3"
          >
            <p className="text-[13px] text-danger-fg">
              {errorText(plansError)}
            </p>
          </div>
        ) : plansLoading ? (
          <Card>
            <CardContent className="py-10 text-center">
              <Loader2 className="mx-auto h-6 w-6 animate-spin text-primary" />
              <p className="mt-2 text-[13px] text-fg-muted">Loading plans…</p>
            </CardContent>
          </Card>
        ) : (
          <PlanGrid
            plans={plans}
            currentPlan={null}
            onChoose={() => {}}
            pendingPlan={null}
            canChoose={false}
          />
        )}
      </div>
    </div>
  );
}

function SubscriptionCard({
  plan,
  subscriptionStatus,
  onCancel,
  cancelling,
}: {
  plan: PlanId;
  subscriptionStatus: string | null;
  onCancel: () => void;
  cancelling: boolean;
}) {
  const paid = plan !== "free";
  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <CreditCard className="h-4 w-4 text-primary" aria-hidden="true" />
            <CardTitle className="text-[14px]">Subscription</CardTitle>
          </div>
          <Badge variant={paid ? "accent" : "secondary"}>
            {plan.charAt(0).toUpperCase() + plan.slice(1)}
          </Badge>
        </div>
        <CardDescription>
          {paid
            ? `Your cloud relay transfers expire after this plan's time-to-live.`
            : "Cloud relay transfers are disabled on the free plan. Upgrade to move a workspace between networks."}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-wrap items-center justify-between gap-3">
        <span className="text-[13px] text-fg-muted">
          Status: {subscriptionStatus ?? "active"}
        </span>
        {paid && (
          <Button variant="outline" loading={cancelling} onClick={onCancel}>
            Cancel subscription
          </Button>
        )}
      </CardContent>
    </Card>
  );
}

function LimitsCard({ limits }: { limits: CloudPlanLimits }) {
  const rows: Array<[string, string]> = [
    ["Paired devices", describeLimit(limits.maxDevices)],
    ["Workspaces", describeLimit(limits.maxWorkspaces)],
    [
      "Cloud relay",
      limits.cloudSync
        ? `Enabled · ${limits.transferTtlHours}h TTL`
        : "Disabled",
    ],
    ["Full app cloning", limits.fullAppCloning ? "Enabled" : "Disabled"],
    ["Max workspace size", `${limits.maxWorkspaceSizeMb} MB`],
  ];
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-[14px]">Plan limits</CardTitle>
      </CardHeader>
      <CardContent>
        <dl className="space-y-1.5 text-[13px]">
          {rows.map(([label, value]) => (
            <div key={label} className="flex items-center justify-between">
              <dt className="text-fg-muted">{label}</dt>
              <dd className="font-medium">{value}</dd>
            </div>
          ))}
        </dl>
      </CardContent>
    </Card>
  );
}

function DeviceRow({
  device,
  onRevoke,
  revoking,
}: {
  device: CloudDevice;
  onRevoke: (id: string) => void;
  revoking: boolean;
}) {
  const revoked = device.status === "revoked";
  return (
    <div className="flex flex-wrap items-start justify-between gap-3 rounded-lg border border-border p-3">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <Monitor
            className="h-3.5 w-3.5 shrink-0 text-fg-muted"
            aria-hidden="true"
          />
          <span className="truncate text-[13px] font-medium">
            {device.deviceName}
          </span>
          <Badge variant={revoked ? "danger" : "success"}>
            {revoked ? "Revoked" : "Active"}
          </Badge>
        </div>
        <p className="mt-1 text-[12px] text-fg-subtle">
          {device.os} {device.osVersion} · App v{device.appVersion} · registered{" "}
          {new Date(device.registeredAt).toLocaleDateString()}
        </p>
      </div>
      {!revoked && (
        <Button
          variant="ghost"
          size="icon"
          title={`Revoke ${device.deviceName}`}
          loading={revoking}
          onClick={() => onRevoke(device.id)}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      )}
    </div>
  );
}

function CloudDevicesCard({
  devices,
  token,
  limits,
  onChanged,
}: {
  devices: CloudDevice[];
  token: string;
  limits: CloudPlanLimits | undefined;
  onChanged: () => void;
}) {
  const [deviceName, setDeviceName] = useState("");
  const identity = useQuery({
    queryKey: ["cloud-device-identity"],
    queryFn: getDeviceIdentity,
  });
  const platform = useQuery({
    queryKey: ["cloud-platform"],
    queryFn: getPlatform,
    staleTime: Infinity,
  });
  const version = useQuery({
    queryKey: ["cloud-app-version"],
    queryFn: getAppVersion,
    staleTime: Infinity,
  });

  const register = useMutation({
    mutationFn: () =>
      registerCloudDevice(token, {
        deviceName: deviceName.trim(),
        deviceType: platform.data ?? "unknown",
        os: platform.data ?? "unknown",
        osVersion: "unknown",
        appVersion: version.data ?? "0.0.0",
        noiseKeyBase64: identity.data?.noisePublicKeyB64 ?? "",
      }),
    onSuccess: () => {
      setDeviceName("");
      onChanged();
    },
  });

  const revoke = useMutation({
    mutationFn: (id: string) => revokeCloudDevice(token, id),
    onSuccess: onChanged,
  });

  const atLimit =
    limits !== undefined &&
    limits.maxDevices >= 0 &&
    devices.filter((d) => d.status === "active").length >= limits.maxDevices;

  const canRegister =
    deviceName.trim().length > 0 &&
    Boolean(identity.data?.noisePublicKeyB64) &&
    !atLimit &&
    !register.isPending;

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Laptop className="h-4 w-4 text-primary" aria-hidden="true" />
          <CardTitle className="text-[14px]">Cloud devices</CardTitle>
        </div>
        <CardDescription>
          Devices registered with your account for relayed transfers. Distinct
          from locally paired devices, which the Devices screen owns.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        {devices.length === 0 ? (
          <p className="text-[13px] text-fg-muted">
            No devices registered yet.
          </p>
        ) : (
          <div className="space-y-2">
            {devices.map((device) => (
              <DeviceRow
                key={device.id}
                device={device}
                revoking={revoke.isPending && revoke.variables === device.id}
                onRevoke={(id) => revoke.mutate(id)}
              />
            ))}
          </div>
        )}

        <Separator />

        {atLimit ? (
          <p className="text-[12px] text-warning-fg">
            Your plan allows {describeLimit(limits?.maxDevices ?? 0)} devices.
            Upgrade to register another.
          </p>
        ) : (
          <div className="flex flex-wrap items-end gap-2">
            <div className="min-w-[180px] flex-1 space-y-1.5">
              <Label htmlFor="cloud-device-name">Register this device</Label>
              <Input
                id="cloud-device-name"
                placeholder="e.g. MacBook Pro"
                value={deviceName}
                onChange={(e) => setDeviceName(e.target.value)}
              />
            </div>
            <Button
              disabled={!canRegister}
              loading={register.isPending}
              onClick={() => register.mutate()}
            >
              Register
            </Button>
          </div>
        )}

        {register.isError && (
          <p role="alert" className="text-[13px] text-danger">
            {errorText(register.error)}
          </p>
        )}
        {revoke.isError && (
          <p role="alert" className="text-[13px] text-danger">
            {errorText(revoke.error)}
          </p>
        )}
      </CardContent>
    </Card>
  );
}

function CloudTransfersCard({
  transfers,
  token,
  onChanged,
}: {
  transfers: CloudTransfer[];
  token: string;
  onChanged: () => void;
}) {
  const cancel = useMutation({
    mutationFn: (id: string) => cancelCloudTransfer(token, id),
    onSuccess: onChanged,
  });

  const cancellable = (status: CloudTransfer["status"]) =>
    status !== "completed" &&
    status !== "cancelled" &&
    status !== "revoked" &&
    status !== "failed";

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Cloud className="h-4 w-4 text-primary" aria-hidden="true" />
          <CardTitle className="text-[14px]">Cloud transfers</CardTitle>
        </div>
        <CardDescription>
          Workspace handoffs stored on the relay for this account.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-2">
        {transfers.length === 0 ? (
          <p className="text-[13px] text-fg-muted">No cloud transfers yet.</p>
        ) : (
          transfers.map((transfer) => (
            <div
              key={transfer.id}
              className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border p-3"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="truncate text-[13px] font-medium">
                    {transfer.workspaceName}
                  </span>
                  <Badge
                    variant={
                      transfer.status === "completed"
                        ? "success"
                        : transfer.status === "failed"
                          ? "danger"
                          : transfer.status === "cancelled" ||
                              transfer.status === "revoked"
                            ? "neutral"
                            : "accent"
                    }
                  >
                    {transfer.status}
                  </Badge>
                </div>
                <p className="mt-1 font-mono text-[11px] text-fg-subtle">
                  {transfer.cloudRelayId} ·{" "}
                  {(transfer.sizeBytes / 1024).toFixed(1)} KB ·{" "}
                  {transfer.fileCount} files
                </p>
              </div>
              {cancellable(transfer.status) && (
                <Button
                  variant="outline"
                  size="sm"
                  loading={cancel.isPending && cancel.variables === transfer.id}
                  onClick={() => cancel.mutate(transfer.id)}
                >
                  Cancel
                </Button>
              )}
            </div>
          ))
        )}
        {cancel.isError && (
          <p role="alert" className="text-[13px] text-danger">
            {errorText(cancel.error)}
          </p>
        )}
      </CardContent>
    </Card>
  );
}

function SignedIn({ plans, token }: { plans: CloudPlan[]; token: string }) {
  const queryClient = useQueryClient();
  const session = useCloudStore((s) => s.session);
  const clearSession = useCloudStore((s) => s.clearSession);
  const setUser = useCloudStore((s) => s.setUser);

  const paypalConfig = useQuery({
    queryKey: ["cloud", "paypal", "config"],
    queryFn: () => getCloudPayPalConfig(),
    staleTime: 5 * 60 * 1000,
  });
  const [notice, setNotice] = useState<{
    kind: "success" | "error" | "cancelled";
    text: string;
  } | null>(null);
  const [checkoutOpen, setCheckoutOpen] = useState(false);

  const profile = useQuery({
    queryKey: ["cloud", "profile", token],
    queryFn: () => getCloudProfile(token),
  });
  const subscription = useQuery({
    queryKey: ["cloud", "subscription", token],
    queryFn: () => getCloudSubscription(token),
  });
  const limits = useQuery({
    queryKey: ["cloud", "limits", token],
    queryFn: () => getCloudLimits(token),
  });
  const devices = useQuery({
    queryKey: ["cloud", "devices", token],
    queryFn: () => listCloudDevices(token),
  });
  const transfers = useQuery({
    queryKey: ["cloud", "transfers", token],
    queryFn: () => listCloudTransfers(token),
  });

  const refreshPlanData = () => {
    void queryClient.invalidateQueries({
      queryKey: ["cloud", "subscription", token],
    });
    void queryClient.invalidateQueries({
      queryKey: ["cloud", "limits", token],
    });
    void queryClient.invalidateQueries({
      queryKey: ["cloud", "profile", token],
    });
  };

  // Checkout: the server creates a PayPal order (never exposing credentials)
  // and the buyer pays in PayPal; on return the capture below grants the plan.
  const checkout = useMutation({
    mutationFn: (planId: Exclude<PlanId, "free">) =>
      createCloudCheckout(token, planId),
    onSuccess: (session) => {
      window.open(session.approvalUrl, "_blank", "noopener");
      setCheckoutOpen(true);
      setNotice(null);
    },
    onError: (error) => {
      setNotice({ kind: "error", text: errorText(error) });
    },
  });

  const capture = useMutation({
    mutationFn: (orderId: string) => captureCloudCheckout(token, orderId),
    onSuccess: (result) => {
      setUser({
        ...(profile.data ?? session!.user),
        plan: result.subscription.plan,
      });
      refreshPlanData();
      const name =
        plans.find((p) => p.id === result.subscription.plan)?.name ??
        result.subscription.plan;
      setNotice({
        kind: "success",
        text: `Payment confirmed — you're now on ${name}.`,
      });
      setCheckoutOpen(false);
    },
    onError: (error) => {
      setNotice({ kind: "error", text: errorText(error) });
    },
  });

  // PayPal redirects the buyer back to `.../cloud?paypal=success&token=<orderId>`
  // (or `?paypal=cancelled`) after approval. Handle that on mount: capture the
  // approved order, or surface the cancellation. The URL is cleaned afterwards
  // so a reload does not capture twice.
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const outcome = params.get("paypal");
    if (outcome === "cancelled") {
      setNotice({
        kind: "cancelled",
        text: "Checkout cancelled — no charge was made.",
      });
      window.history.replaceState({}, "", window.location.pathname);
    } else if (outcome === "success") {
      const orderId = params.get("token") ?? "";
      window.history.replaceState({}, "", window.location.pathname);
      if (orderId) {
        setNotice(null);
        capture.mutate(orderId);
      } else {
        setNotice({
          kind: "error",
          text: "Checkout return was missing the order id.",
        });
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cancel = useMutation({
    mutationFn: () => cancelCloudPlan(token),
    onSuccess: (result) => {
      setUser({ ...(profile.data ?? session!.user), plan: result.plan });
      refreshPlanData();
    },
  });

  const signOut = useMutation({
    mutationFn: () => logoutCloud(token).catch(() => ({ success: true })),
    onSettled: () => {
      clearSession();
      queryClient.removeQueries({ queryKey: ["cloud"] });
    },
  });

  const plan =
    subscription.data?.plan ??
    profile.data?.plan ??
    session?.user.plan ??
    "free";

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <Cloud className="h-4 w-4 text-primary" aria-hidden="true" />
              <CardTitle className="text-[14px]">
                {session?.user.name ?? session?.user.email}
              </CardTitle>
            </div>
            <Button
              variant="outline"
              size="sm"
              loading={signOut.isPending}
              onClick={() => signOut.mutate()}
            >
              <LogOut className="mr-1.5 h-3.5 w-3.5" aria-hidden="true" />
              Sign out
            </Button>
          </div>
          <CardDescription>{session?.user.email}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {profile.isLoading ? (
            <div className="flex items-center gap-2 text-[13px] text-fg-muted">
              <Loader2
                className="h-3.5 w-3.5 animate-spin"
                aria-hidden="true"
              />
              Loading account…
            </div>
          ) : profile.isError ? (
            <p role="alert" className="text-[13px] text-danger">
              {errorText(profile.error)}
            </p>
          ) : (
            <SubscriptionCard
              plan={plan}
              subscriptionStatus={subscription.data?.status ?? null}
              cancelling={cancel.isPending}
              onCancel={() => cancel.mutate()}
            />
          )}
          {cancel.isError && (
            <p role="alert" className="text-[13px] text-danger">
              {errorText(cancel.error)}
            </p>
          )}
        </CardContent>
      </Card>

      <div className="grid gap-4 md:grid-cols-2">
        {limits.data && <LimitsCard limits={limits.data} />}
        {devices.data && (
          <CloudDevicesCard
            devices={devices.data}
            token={token}
            limits={limits.data}
            onChanged={() => {
              void queryClient.invalidateQueries({
                queryKey: ["cloud", "devices", token],
              });
              void queryClient.invalidateQueries({
                queryKey: ["cloud", "profile", token],
              });
            }}
          />
        )}
      </div>

      {transfers.data && (
        <CloudTransfersCard
          transfers={transfers.data}
          token={token}
          onChanged={() => {
            void queryClient.invalidateQueries({
              queryKey: ["cloud", "transfers", token],
            });
          }}
        />
      )}

      <div>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-[13px] font-semibold">Plans</h2>
          <span
            className="flex items-center gap-1.5 text-[11px] text-fg-subtle"
            title={CLOUD_API_BASE_URL}
          >
            <RefreshCw className="h-3 w-3" aria-hidden="true" />
            {CLOUD_API_BASE_URL}
          </span>
        </div>
        <PlanGrid
          plans={plans}
          currentPlan={plan}
          pendingPlan={
            checkout.isPending
              ? (checkout.variables as Exclude<PlanId, "free">)
              : null
          }
          canChoose
          checkoutAvailable={paypalConfig.data?.available ?? false}
          onChoose={(planId) => checkout.mutate(planId)}
        />
        {paypalConfig.data && !paypalConfig.data.available && (
          <p className="mt-2 text-[12px] text-fg-subtle">
            PayPal checkout is not configured on the server yet.
          </p>
        )}
        {checkoutOpen && (
          <p className="mt-2 text-[12px] text-fg-muted">
            Payment window opened — complete it in PayPal, then come back here
            and your plan updates automatically.
          </p>
        )}
        {notice && (
          <p
            role="status"
            className={
              notice.kind === "success"
                ? "mt-3 text-[13px] text-success"
                : notice.kind === "cancelled"
                  ? "mt-3 text-[13px] text-fg-muted"
                  : "mt-3 text-[13px] text-danger"
            }
          >
            {notice.text}
          </p>
        )}
      </div>
    </div>
  );
}

export function CloudScreen() {
  const session = useCloudStore((s) => s.session);
  const plans = useQuery({
    queryKey: ["cloud", "plans"],
    queryFn: ({ signal }) => listCloudPlans(signal),
    staleTime: 5 * 60 * 1000,
  });

  return (
    <div className="space-y-6">
      <p className="text-[13px] text-fg-muted">
        Cloud relay, account and paid plans. Everything on this screen is
        additive: the local peer-to-peer handoff works without signing in.
      </p>

      {session ? (
        <SignedIn plans={plans.data ?? []} token={session.accessToken} />
      ) : (
        <SignedOut
          plans={plans.data ?? []}
          plansLoading={plans.isLoading}
          plansError={plans.isError ? plans.error : null}
        />
      )}
    </div>
  );
}
