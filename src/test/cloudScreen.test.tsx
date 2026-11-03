/**
 * The cloud screen, from the user's side.
 *
 * Two states matter and they must not bleed into each other: signed out is a
 * pricing page plus a sign-in form, signed in is the account. The bug this
 * guards against is the one every auth screen gets wrong once — showing the
 * signed-in chrome while logged out, or promising plans that need a session
 * before the user has one.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CloudScreen } from "@screens/CloudScreen";
import { useCloudStore } from "@store/useCloudStore";
import * as cloudApi from "@lib/cloudApi";
import * as ipc from "@lib/ipc";
import type { CloudPlan, CloudSession } from "@lib/cloudApi";

vi.mock("@lib/cloudApi", () => ({
  CLOUD_API_BASE_URL: "http://localhost:3000",
  listCloudPlans: vi.fn(),
  loginCloud: vi.fn(),
  registerCloudAccount: vi.fn(),
  getCloudProfile: vi.fn(),
  getCloudSubscription: vi.fn(),
  getCloudLimits: vi.fn(),
  getCloudPayPalConfig: vi.fn(),
  listCloudDevices: vi.fn(),
  listCloudTransfers: vi.fn(),
  logoutCloud: vi.fn(),
  upgradeCloudPlan: vi.fn(),
  createCloudCheckout: vi.fn(),
  captureCloudCheckout: vi.fn(),
  cancelCloudPlan: vi.fn(),
  registerCloudDevice: vi.fn(),
  revokeCloudDevice: vi.fn(),
  cancelCloudTransfer: vi.fn(),
}));

vi.mock("@lib/ipc", () => ({
  getDeviceIdentity: vi.fn(),
  getPlatform: vi.fn(),
  getAppVersion: vi.fn(),
}));

const plans: CloudPlan[] = [
  {
    id: "free",
    name: "Free",
    description: "Local handoff only.",
    priceCents: 0,
    currency: "USD",
    billingInterval: "none",
    features: ["Local P2P handoff"],
    limits: {
      maxDevices: 0,
      maxWorkspaces: 1,
      cloudSync: false,
      transferTtlHours: 0,
      fullAppCloning: false,
      maxWorkspaceSizeMb: 512,
    },
  },
  {
    id: "basic",
    name: "Basic",
    description: "Cloud sync.",
    priceCents: 999,
    currency: "USD",
    billingInterval: "month",
    features: ["Cloud sync beyond the local network"],
    limits: {
      maxDevices: 2,
      maxWorkspaces: 3,
      cloudSync: true,
      transferTtlHours: 24,
      fullAppCloning: false,
      maxWorkspaceSizeMb: 512,
    },
  },
  {
    id: "pro",
    name: "Pro",
    description: "Full cloning.",
    priceCents: 1999,
    currency: "USD",
    billingInterval: "month",
    features: ["Full app cloning"],
    limits: {
      maxDevices: 3,
      maxWorkspaces: -1,
      cloudSync: true,
      transferTtlHours: 168,
      fullAppCloning: true,
      maxWorkspaceSizeMb: 512,
    },
  },
];

const SESSION: CloudSession = {
  user: { id: "u1", email: "ada@example.com", name: "Ada", plan: "free" },
  accessToken: "access-1",
  refreshToken: "refresh-1",
  expiresIn: 3600,
  tokenType: "bearer",
};

function renderScreen() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <CloudScreen />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  window.localStorage.clear();
  useCloudStore.setState({ session: null });
  vi.clearAllMocks();
  vi.mocked(ipc.getDeviceIdentity).mockResolvedValue({
    signingPublicKeyB64: "c2ln",
    noisePublicKeyB64: "AAAAAAAAAAAAAAAAAAAAAA==",
    fingerprint: "fp",
  });
  vi.mocked(ipc.getPlatform).mockResolvedValue("macos");
  vi.mocked(ipc.getAppVersion).mockResolvedValue("0.1.3");
  vi.mocked(cloudApi.listCloudPlans).mockResolvedValue(plans);
  vi.mocked(cloudApi.loginCloud).mockResolvedValue(SESSION);
  vi.mocked(cloudApi.registerCloudAccount).mockResolvedValue({
    user: SESSION.user,
    requiresEmailConfirmation: false,
  });
  vi.mocked(cloudApi.getCloudProfile).mockResolvedValue({
    ...SESSION.user,
    deviceCount: 0,
    maxDevices: 0,
    isActive: true,
    onboardingComplete: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  });
  vi.mocked(cloudApi.getCloudSubscription).mockResolvedValue({
    userId: "u1",
    plan: "free",
    status: "active",
    paymentProvider: null,
    startedAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    expiresAt: null,
    cancelledAt: null,
  });
  vi.mocked(cloudApi.getCloudLimits).mockResolvedValue(plans[0].limits);
  vi.mocked(cloudApi.getCloudPayPalConfig).mockResolvedValue({
    available: true,
    environment: "sandbox",
    currency: "USD",
  });
  vi.mocked(cloudApi.listCloudDevices).mockResolvedValue([]);
  vi.mocked(cloudApi.listCloudTransfers).mockResolvedValue([]);
  vi.mocked(cloudApi.createCloudCheckout).mockResolvedValue({
    orderId: "ORDER-1",
    planId: "basic",
    status: "created",
    approvalUrl: "https://www.sandbox.paypal.com/checkoutnow?token=ORDER-1",
    priceCents: 999,
    currency: "USD",
  });
  vi.mocked(cloudApi.captureCloudCheckout).mockResolvedValue({
    subscription: {
      userId: "u1",
      plan: "basic",
      status: "active",
      paymentProvider: "paypal",
      startedAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
      expiresAt: null,
      cancelledAt: null,
    },
    capture: {
      status: "COMPLETED",
      captureId: "cap-1",
      amountValue: "9.99",
      currency: "USD",
      payerEmail: "buyer@example.com",
      customId: "u1",
    },
  });
  vi.mocked(cloudApi.upgradeCloudPlan).mockResolvedValue({
    userId: "u1",
    plan: "basic",
    status: "active",
    paymentProvider: "manual",
    startedAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    expiresAt: null,
    cancelledAt: null,
  });
  vi.mocked(cloudApi.logoutCloud).mockResolvedValue({ success: true });
});

describe("CloudScreen signed out", () => {
  it("shows the pricing comparison and the sign-in form", async () => {
    renderScreen();

    expect(await screen.findByText("Basic")).toBeInTheDocument();
    expect(screen.getByText("Pro")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Sign in" })).toBeInTheDocument();
    expect(
      screen.getByRole("tab", { name: "Create account" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Email")).toBeInTheDocument();
  });

  it("does not offer a plan change before there is a session", async () => {
    renderScreen();
    await screen.findByText("Basic");
    expect(
      screen.queryByRole("button", { name: /Switch to Basic/i }),
    ).not.toBeInTheDocument();
  });

  it("signs in, stores the session, and reveals the account", async () => {
    renderScreen();

    fireEvent.change(await screen.findByLabelText("Email"), {
      target: { value: "ada@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "Str0ng!Pass" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    await waitFor(() => {
      expect(cloudApi.loginCloud).toHaveBeenCalledWith({
        email: "ada@example.com",
        password: "Str0ng!Pass",
      });
    });
    expect(useCloudStore.getState().session?.accessToken).toBe("access-1");

    expect(
      await screen.findByRole("button", { name: /Sign out/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("tab", { name: "Sign in" }),
    ).not.toBeInTheDocument();
  });

  it("surfaces a rejected sign-in", async () => {
    vi.mocked(cloudApi.loginCloud).mockRejectedValue(
      new Error("Invalid email or password"),
    );
    renderScreen();

    fireEvent.change(await screen.findByLabelText("Email"), {
      target: { value: "ada@example.com" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "nope" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Invalid email or password",
    );
  });
});

describe("CloudScreen signed in", () => {
  beforeEach(() => {
    useCloudStore.getState().setSession(SESSION);
  });

  it("shows the account and the plan limits from the backend", async () => {
    renderScreen();

    expect(
      await screen.findByRole("button", { name: /Sign out/i }),
    ).toBeInTheDocument();
    expect(screen.getByText("ada@example.com")).toBeInTheDocument();
    // Limits copy comes from the Free plan fixture.
    expect(await screen.findByText("Plan limits")).toBeInTheDocument();
    expect(screen.getByText("Paired devices")).toBeInTheDocument();
    expect(screen.getAllByText("Disabled").length).toBeGreaterThan(0);
  });

  it("opens a PayPal checkout when a paid plan is chosen", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    renderScreen();

    const upgrade = await screen.findByRole("button", {
      name: /Switch to Basic/i,
    });
    fireEvent.click(upgrade);

    await waitFor(() => {
      expect(cloudApi.createCloudCheckout).toHaveBeenCalledWith(
        "access-1",
        "basic",
      );
    });
    expect(open).toHaveBeenCalledWith(
      "https://www.sandbox.paypal.com/checkoutnow?token=ORDER-1",
      "_blank",
      "noopener",
    );
    expect(screen.getByText(/Payment window opened/)).toBeInTheDocument();
  });

  it("captures the approved order after the PayPal return redirect", async () => {
    window.history.replaceState(
      {},
      "",
      "/cloud?paypal=success&token=ORDER-1&planId=basic",
    );
    renderScreen();

    await waitFor(() => {
      expect(cloudApi.captureCloudCheckout).toHaveBeenCalledWith(
        "access-1",
        "ORDER-1",
      );
    });
    expect(await screen.findByText(/Payment confirmed/)).toBeInTheDocument();
    // The URL is cleaned so a reload does not capture twice.
    expect(window.location.search).toBe("");
  });

  it("shows a cancellation notice when the buyer returns cancelled", async () => {
    window.history.replaceState({}, "", "/cloud?paypal=cancelled");
    renderScreen();

    expect(await screen.findByText(/Checkout cancelled/)).toBeInTheDocument();
    expect(window.location.search).toBe("");
  });

  it("hides the checkout buttons while the server has no PayPal credentials", async () => {
    vi.mocked(cloudApi.getCloudPayPalConfig).mockResolvedValue({
      available: false,
      environment: "sandbox",
      currency: "USD",
    });
    renderScreen();

    expect(
      await screen.findByText(/PayPal checkout is not configured/),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Switch to Basic/i }),
    ).not.toBeInTheDocument();
  });

  it("signs out and clears the stored session", async () => {
    renderScreen();

    fireEvent.click(await screen.findByRole("button", { name: /Sign out/i }));

    await waitFor(() => {
      expect(useCloudStore.getState().session).toBeNull();
    });
    expect(
      await screen.findByRole("tab", { name: "Sign in" }),
    ).toBeInTheDocument();
  });
});

// The PayPal return-redirect tests manipulate window.location; restore it so
// no URL (or window.open spy) leaks into the next case.
afterEach(() => {
  window.history.replaceState({}, "", "/");
  vi.restoreAllMocks();
});
