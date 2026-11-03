/**
 * The cloud client's contract with the backend.
 *
 * The backend answers in envelopes — `{ data }` on success, `{ message }` on
 * failure — so the two things worth pinning are that success unwraps to the
 * domain object and failure unwraps to a thrown {@link CloudApiError}. Both are
 * easy to get subtly wrong (return the envelope, lose the status code) and
 * neither shows up in a screen test that mocks the client.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  CloudApiError,
  CLOUD_API_BASE_URL,
  captureCloudCheckout,
  createCloudCheckout,
  getCloudPayPalConfig,
  getCloudProfile,
  listCloudPlans,
  loginCloud,
} from "@lib/cloudApi";

interface FakeResponse {
  ok: boolean;
  status: number;
  text: () => Promise<string>;
}

function respond(body: unknown, status = 200): FakeResponse {
  return {
    ok: status >= 200 && status < 300,
    status,
    text: async () => (body === undefined ? "" : JSON.stringify(body)),
  };
}

const fetchMock = vi.fn();

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("cloudApi request", () => {
  it("targets the configured base URL and defaults to the local dev server", () => {
    expect(CLOUD_API_BASE_URL).toBe("http://localhost:3000");
  });

  it("unwraps the success envelope to the domain value", async () => {
    fetchMock.mockResolvedValue(
      respond({ statusCode: 200, message: "Success", data: [{ id: "free" }] }),
    );

    const plans = await listCloudPlans();
    expect(plans).toEqual([{ id: "free" }]);
    expect(fetchMock).toHaveBeenCalledWith(
      `${CLOUD_API_BASE_URL}/payments/plans`,
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("sends the bearer token when one is given", async () => {
    fetchMock.mockResolvedValue(
      respond({
        data: { id: "u1", email: "a@b.com", name: null, plan: "free" },
      }),
    );

    await getCloudProfile("token-123");

    const headers = fetchMock.mock.calls[0][1]?.headers as Record<
      string,
      string
    >;
    expect(headers.Authorization).toBe("Bearer token-123");
  });

  it("posts JSON with a content type", async () => {
    fetchMock.mockResolvedValue(respond({ data: { accessToken: "a" } }));

    await loginCloud({ email: "a@b.com", password: "Str0ng!Pass" });

    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe(`${CLOUD_API_BASE_URL}/auth/login`);
    expect(init?.method).toBe("POST");
    expect(init?.body).toBe(
      JSON.stringify({ email: "a@b.com", password: "Str0ng!Pass" }),
    );
    expect((init?.headers as Record<string, string>)["Content-Type"]).toBe(
      "application/json",
    );
  });

  it("throws a CloudApiError carrying the status and the first validation message", async () => {
    fetchMock.mockResolvedValue(
      respond(
        {
          statusCode: 400,
          error: "Bad Request",
          message: [
            "password must be longer than 6 characters",
            "name is required",
          ],
        },
        400,
      ),
    );

    await expect(
      loginCloud({ email: "a@b.com", password: "x" }),
    ).rejects.toMatchObject({
      name: "CloudApiError",
      status: 400,
      message: "password must be longer than 6 characters",
    });
  });

  it("throws a CloudApiError with status 0 when the network is unreachable", async () => {
    fetchMock.mockRejectedValue(new Error("connect ECONNREFUSED"));

    await expect(listCloudPlans()).rejects.toBeInstanceOf(CloudApiError);
    await expect(listCloudPlans()).rejects.toMatchObject({
      status: 0,
      message: "connect ECONNREFUSED",
    });
  });

  it("tolerates an empty response body", async () => {
    fetchMock.mockResolvedValue(respond(undefined, 204));
    await expect(getCloudProfile("t")).resolves.toBeNull();
  });

  it("fetches the credential-free PayPal config", async () => {
    const config = { available: true, environment: "sandbox", currency: "USD" };
    fetchMock.mockResolvedValue(respond(config));

    await expect(getCloudPayPalConfig()).resolves.toEqual(config);
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining("/payments/paypal/config"),
      expect.objectContaining({
        headers: expect.not.objectContaining({
          Authorization: expect.anything(),
        }),
      }),
    );
  });

  it("creates a checkout order for a paid plan", async () => {
    const session = {
      orderId: "ORDER-1",
      planId: "basic",
      status: "created",
      approvalUrl: "https://www.sandbox.paypal.com/checkoutnow?token=ORDER-1",
      priceCents: 999,
      currency: "USD",
    };
    fetchMock.mockResolvedValue(respond(session));

    await expect(createCloudCheckout("t", "basic")).resolves.toEqual(session);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({ planId: "basic" });
    expect(init.headers).toMatchObject({ Authorization: "Bearer t" });
  });

  it("captures a checkout order and returns the new subscription", async () => {
    const capture = {
      subscription: { plan: "pro", status: "active" },
      capture: { status: "COMPLETED", captureId: "cap-1" },
    };
    fetchMock.mockResolvedValue(respond(capture));

    await expect(captureCloudCheckout("t", "ORDER-ABC/1")).resolves.toEqual(
      capture,
    );
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining("/payments/paypal/orders/ORDER-ABC%2F1/capture"),
      expect.objectContaining({ method: "POST" }),
    );
  });
});
