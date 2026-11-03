# PayPal checkout — UI states

Captured against the local app (`npm run dev` → `http://localhost:1420/cloud`) and
the live NestJS API with the **sandbox** PayPal credentials in `.env.dev`.
Regenerate with `node scripts/screenshot-cloud-paypal.mjs` (both servers up).

| File | State | Provenance |
| --- | --- | --- |
| `01-checkout-button.png` | The payment-click point: the signed-in plans grid with the **Switch to Basic** button (opens a PayPal order). | real API call |
| `02-approval-window-opened.png` | After the click: the order was created and the PayPal approval window was opened; the app shows the "Payment window opened" hint. | real API call (real sandbox order) |
| `02b-paypal-hosted-checkout.png` | The PayPal **sandbox** hosted checkout page for that order (buyer logs in / pays here; auto-renewal aside, this is where the money moves). | real PayPal page |
| `03-cancelled-return.png` | Buyer returned via `?paypal=cancelled` — "Checkout cancelled, no charge was made". | real app + real return URL |
| `04-capture-error-return.png` | Capture of an un-approved order fails (PayPal 422 → API 409) and the app surfaces the error notice. Seen in practice only if the buyer's return is malformed. | real backend + real PayPal rejection |
| `05-success-payment-confirmed.png` | "Payment confirmed — you're now on Basic." **NOTE: the capture response here is simulated** (the app's network layer faked the `COMPLETED` capture) because completing a real sandbox payment requires logging in with a PayPal sandbox **buyer** account. The success *UI* is genuine; a real payment produces the same notice. | simulated capture |

To see the real success notice end-to-end: open a checkout in the app, complete
the payment in the PayPal sandbox window with a buyer account, and return — the
app captures the order and shows `05-success-payment-confirmed.png`.