/// <reference types="vite/client" />

/**
 * The HTTP client for the StackHandoff cloud API (the NestJS service in
 * `~/Documents/stackhandoff-backend`).
 *
 * This is the frontend's *second* transport, and it is deliberately separate
 * from `@lib/ipc.ts`. `ipc.ts` names Tauri commands and speaks to the local Rust
 * core over the webview bridge; this module speaks JSON over `fetch` to a
 * server. Keeping them apart matters because the two have opposite failure
 * modes: a broken Tauri command is a local bug the user cannot route around,
 * while a broken cloud call is a network condition the UI must degrade to
 * (offline, signed out, expired token). The screens read the difference from
 * the types, not from a comment.
 *
 * The backend wraps every successful response in a `{ statusCode, message,
 * data, timestamp }` envelope and every failure in a `{ statusCode, message,
 * error, path, timestamp }` one. `request` unwraps the first and turns the
 * second into a {@link CloudApiError} carrying the status code, so callers deal
 * in domain objects and one error class rather than in envelopes.
 *
 * Nothing here is required for the local P2P flow. A signed-out user never
 * reaches this module, and every function is additive.
 */

export type PlanId = "free" | "basic" | "pro";

export interface CloudPlanLimits {
  maxDevices: number;
  maxWorkspaces: number;
  cloudSync: boolean;
  transferTtlHours: number;
  fullAppCloning: boolean;
  maxWorkspaceSizeMb: number;
}

export interface CloudPlan {
  id: PlanId;
  name: string;
  description: string;
  priceCents: number;
  currency: string;
  billingInterval: "none" | "month";
  features: string[];
  limits: CloudPlanLimits;
}

export interface CloudUser {
  id: string;
  email: string;
  name: string | null;
  plan: PlanId;
  emailConfirmed?: boolean;
}

export interface CloudSession {
  user: CloudUser;
  accessToken: string | null;
  refreshToken: string | null;
  expiresIn: number | null;
  tokenType: "bearer";
}

export interface CloudRegistration {
  user: CloudUser;
  requiresEmailConfirmation: boolean;
}

export interface CloudProfile extends CloudUser {
  deviceCount: number;
  maxDevices: number;
  isActive: boolean;
  onboardingComplete: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface CloudSubscription {
  userId: string;
  plan: PlanId;
  status: "active" | "cancelled" | "trialing";
  paymentProvider: string | null;
  startedAt: string;
  updatedAt: string;
  expiresAt: string | null;
  cancelledAt: string | null;
}

export type CloudDeviceStatus = "active" | "revoked";

export interface CloudDevice {
  id: string;
  userId: string;
  deviceName: string;
  deviceType: string;
  os: string;
  osVersion: string;
  appVersion: string;
  noiseKeyBase64: string;
  planAtRegistration: PlanId;
  status: CloudDeviceStatus;
  registeredAt: string;
  updatedAt: string;
  lastSeenAt: string | null;
  revokedAt: string | null;
}

export interface RegisterCloudDeviceInput {
  deviceName: string;
  deviceType: string;
  os: string;
  osVersion: string;
  appVersion: string;
  noiseKeyBase64: string;
}

export interface CloudWorkspace {
  id: string;
  userId: string;
  name: string;
  description: string | null;
  status: "active" | "archived";
  cloudRelayId: string | null;
  restoreStatus: "pending" | "completed" | "failed" | "revoked" | null;
  createdAt: string;
  updatedAt: string;
}

export type CloudTransferStatus =
  | "pending"
  | "uploading"
  | "ready"
  | "restoring"
  | "completed"
  | "failed"
  | "revoked"
  | "cancelled";

export interface CloudTransfer {
  id: string;
  userId: string;
  workspaceId: string;
  workspaceName: string;
  sourceDeviceId: string;
  destinationDeviceId: string | null;
  status: CloudTransferStatus;
  cloudRelayId: string;
  useCloudRelay: boolean;
  planAtTransfer: PlanId;
  sizeBytes: number;
  fileCount: number;
  failureReason: string | null;
  expiresAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface CreateCloudTransferInput {
  workspaceId: string;
  workspaceName: string;
  sourceDeviceId: string;
  destinationDeviceId?: string;
  sizeBytes?: number;
  fileCount?: number;
  useCloudRelay?: boolean;
}

/**
 * Base URL of the cloud API.
 *
 * Configurable through `VITE_CLOUD_API_URL` so a build can point at a deployed
 * environment; defaults to the local dev server, which is what `npm run dev` in
 * the backend listens on. Exported because the settings/verification surfaces
 * show it, and showing the URL the app actually uses is the point.
 */
export const CLOUD_API_BASE_URL = (
  (import.meta.env?.VITE_CLOUD_API_URL as string | undefined) ??
  "http://localhost:3000"
).replace(/\/+$/, "");

/** A non-2xx cloud response, or a transport failure, in one type. */
export class CloudApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly body?: unknown,
  ) {
    super(message);
    this.name = "CloudApiError";
  }
}

interface RequestOptions {
  method?: "GET" | "POST" | "PATCH" | "DELETE";
  body?: unknown;
  token?: string | null;
  signal?: AbortSignal;
}

/**
 * Pull the most useful string out of either envelope.
 *
 * The backend's validation errors arrive as `message: string[]`; collapsing them
 * to the first entry keeps a form usable, because a single field's message is
 * what the user has to act on.
 */
function messageOf(body: unknown, fallback: string): string {
  if (body && typeof body === "object" && "message" in body) {
    const raw = (body as { message: unknown }).message;
    if (Array.isArray(raw) && raw.length > 0) return String(raw[0]);
    if (typeof raw === "string" && raw.length > 0) return raw;
  }
  return fallback;
}

async function request<T>(
  path: string,
  options: RequestOptions = {},
): Promise<T> {
  const { method = "GET", body, token, signal } = options;

  let response: Response;
  try {
    response = await fetch(`${CLOUD_API_BASE_URL}${path}`, {
      method,
      headers: {
        Accept: "application/json",
        ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
      signal,
    });
  } catch (error) {
    // A network failure is not a 4xx: surface it as status 0 so callers can tell
    // "the API is unreachable" from "the API rejected you".
    throw new CloudApiError(
      error instanceof Error
        ? error.message
        : "The cloud API could not be reached",
      0,
    );
  }

  const text = await response.text();
  let payload: unknown = null;
  if (text) {
    try {
      payload = JSON.parse(text);
    } catch {
      payload = text;
    }
  }

  if (!response.ok) {
    throw new CloudApiError(
      messageOf(payload, `Request failed with status ${response.status}`),
      response.status,
      payload,
    );
  }

  // Unwrap the standard success envelope.
  if (payload && typeof payload === "object" && "data" in payload) {
    return (payload as { data: T }).data;
  }
  return payload as T;
}

// ---------------------------------------------------------------------------
// Health & auth
// ---------------------------------------------------------------------------

export interface CloudHealth {
  status: string;
  uptimeSeconds: number;
  timestamp: string;
}

export const getCloudHealth = (signal?: AbortSignal): Promise<CloudHealth> =>
  request("/health", { signal });

export const registerCloudAccount = (input: {
  email: string;
  password: string;
  name?: string;
}): Promise<CloudRegistration> =>
  request("/auth/register", { method: "POST", body: input });

export const loginCloud = (input: {
  email: string;
  password: string;
}): Promise<CloudSession> =>
  request("/auth/login", { method: "POST", body: input });

export const refreshCloudSession = (
  refreshToken: string,
): Promise<CloudSession> =>
  request("/auth/refresh", { method: "POST", body: { refreshToken } });

export const logoutCloud = (token: string): Promise<{ success: boolean }> =>
  request("/auth/logout", { method: "POST", token });

export const getCloudMe = (token: string): Promise<CloudProfile> =>
  request("/auth/me", { token });

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

export const getCloudProfile = (token: string): Promise<CloudProfile> =>
  request("/users/me", { token });

export const updateCloudProfile = (
  token: string,
  patch: { name?: string },
): Promise<CloudProfile> =>
  request("/users/me", { method: "PATCH", body: patch, token });

// ---------------------------------------------------------------------------
// Payments / plans
// ---------------------------------------------------------------------------

export const listCloudPlans = (signal?: AbortSignal): Promise<CloudPlan[]> =>
  request("/payments/plans", { signal });

export const getCloudSubscription = (
  token: string,
): Promise<CloudSubscription> => request("/payments/subscription", { token });

export const getCloudLimits = (token: string): Promise<CloudPlanLimits> =>
  request("/payments/limits", { token });

export const upgradeCloudPlan = (
  token: string,
  planId: Exclude<PlanId, "free">,
): Promise<CloudSubscription> =>
  request("/payments/upgrade", { method: "POST", body: { planId }, token });

export const cancelCloudPlan = (token: string): Promise<CloudSubscription> =>
  request("/payments/cancel", { method: "POST", token });

// ---------------------------------------------------------------------------
// PayPal checkout
// ---------------------------------------------------------------------------

/**
 * What the server is willing to tell the app about PayPal: whether checkout is
 * configured, which environment it runs against, and the currency. It contains
 * no credentials — not even the public client id — so nothing sensitive ever
 * lives in the frontend bundle.
 */
export interface CloudPayPalConfig {
  available: boolean;
  environment: "sandbox" | "live";
  currency: string;
}

/** The opener of a checkout: an order id and where to send the buyer. */
export interface CloudCheckout {
  orderId: string;
  planId: Exclude<PlanId, "free">;
  status: "created" | "captured" | "cancelled";
  approvalUrl: string;
  priceCents: number;
  currency: string;
}

export interface CloudCheckoutCapture {
  subscription: CloudSubscription;
  capture: {
    status: string;
    captureId: string | null;
    amountValue: string | null;
    currency: string | null;
    payerEmail: string | null;
    customId: string | null;
  };
}

export const getCloudPayPalConfig = (): Promise<CloudPayPalConfig> =>
  request("/payments/paypal/config");

export const createCloudCheckout = (
  token: string,
  planId: Exclude<PlanId, "free">,
): Promise<CloudCheckout> =>
  request("/payments/paypal/orders", {
    method: "POST",
    body: { planId },
    token,
  });

export const captureCloudCheckout = (
  token: string,
  orderId: string,
): Promise<CloudCheckoutCapture> =>
  request(`/payments/paypal/orders/${encodeURIComponent(orderId)}/capture`, {
    method: "POST",
    token,
  });

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

export const listCloudDevices = (token: string): Promise<CloudDevice[]> =>
  request("/devices", { token });

export const registerCloudDevice = (
  token: string,
  input: RegisterCloudDeviceInput,
): Promise<CloudDevice> =>
  request("/devices", { method: "POST", body: input, token });

export const heartbeatCloudDevice = (
  token: string,
  deviceId: string,
): Promise<CloudDevice> =>
  request(`/devices/${encodeURIComponent(deviceId)}/heartbeat`, {
    method: "POST",
    token,
  });

export const revokeCloudDevice = (
  token: string,
  deviceId: string,
): Promise<CloudDevice> =>
  request(`/devices/${encodeURIComponent(deviceId)}/revoke`, {
    method: "POST",
    token,
  });

// ---------------------------------------------------------------------------
// Workspaces
// ---------------------------------------------------------------------------

export const listCloudWorkspaces = (token: string): Promise<CloudWorkspace[]> =>
  request("/workspaces", { token });

export const createCloudWorkspace = (
  token: string,
  input: { name: string; description?: string },
): Promise<CloudWorkspace> =>
  request("/workspaces", { method: "POST", body: input, token });

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

export const listCloudTransfers = (token: string): Promise<CloudTransfer[]> =>
  request("/transfers", { token });

export const createCloudTransfer = (
  token: string,
  input: CreateCloudTransferInput,
): Promise<CloudTransfer> =>
  request("/transfers", { method: "POST", body: input, token });

export const cancelCloudTransfer = (
  token: string,
  transferId: string,
): Promise<CloudTransfer> =>
  request(`/transfers/${encodeURIComponent(transferId)}/cancel`, {
    method: "POST",
    token,
  });
