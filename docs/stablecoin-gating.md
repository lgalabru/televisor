# Stablecoin gating — plan

**Goal:** let users discover IPTV providers that accept x402/MPP and subscribe (monthly, day pass, or pay-per-stream) straight from televisor; let provider teams gate their existing Xtream panel with a pay proxy they run themselves — no panel changes.

**Key idea:** Xtream auths everything with a username/password in the URL, and panels natively support accounts with an expiry + max-connections. So **payment satisfied → an Xtream credential scoped to what was paid for**. A $6 month = 30-day account; a $0.30 match pass = 3-hour/1-connection account. Only the *purchase* needs a 402-capable client (televisor, an agent, or a checkout page) — the resulting credentials work in any IPTV app. Entitlement, concurrency, and revocation are all enforced by the panel itself.

**What already exists in pay:** `pay-proxy` (Pingora 402 data plane), MPP session channels with voucher metering (per-minute billing), MPP subscription intent (challenge ✅, activation verify pending Rust SDK port — TS has it). Missing: the Xtream adapter server-side, and the payment flow in televisor.

---

## Phase 1 — credential vending gateway (prove the economics)

*One-shot x402 `exact` payments per period — works with pay today. No video traffic through the gateway; revocation = account expiry.*

### Gateway (`crates/gateway`, deps on pay-core)
- [ ] Scaffold axum service on pay-core's `PaymentGate`
- [ ] Provisioning trait `provision(tier) -> Account` with two impls:
  - [ ] XUI.one reseller API (create user with expiry/max_connections)
  - [ ] Static pre-provisioned account pool (panels without an API)
- [ ] `GET /subscribe?tier=` → 402 challenge → on payment, provision + return `{ host, username, password, expires_at }`
- [ ] Tier config (TOML): price, period, max_connections (e.g. `month/$6`, `day/$0.80`, `pass-3h/$0.30`)
- [ ] Checkout web page for non-402 clients (pay in browser → credentials on screen)
- [ ] `/.well-known/televisor.json` manifest (name, tiers, pricing, chains, checkout URL)

### televisor client
- [ ] `televisor providers` — browse/search the registry
- [ ] `televisor subscribe <provider> [--tier]` — drive the 402 via `pay fetch` subprocess (TouchID approval for free), store returned credentials as a normal account, print expiry
- [ ] MCP tools: `list_providers`, `subscribe_provider`
- [ ] Surface expiry warnings (`status`, and before `play` when < 24h left)

### Discovery / registry
- [ ] `media/iptv` category in the pay catalog with structured pricing metadata
- [ ] Manifest schema doc for gateway operators

### Dogfood
- [ ] Run gateway against a panel we control (devnet USDC first)
- [ ] End-to-end: `televisor providers` → `subscribe` → `play` with real payment
- [ ] Short operator doc (incl. merchant-of-record/compliance note: tooling is content-neutral, the operator answers for their panel's content)

## Phase 2 — inline pay-proxy gate (streaming payments)

*True pay-per-minute via MPP sessions; televisor-native.*

- [ ] pay-proxy Xtream adapter: session token rides in the username/password slots; gateway swaps in upstream credentials
- [ ] Per-wallet **ephemeral upstream accounts** (not a shared pool — see evidence below: catch-up redirects leak upstream creds in the URL path)
- [ ] MPP session per stream: televisor opens the channel on `play`, closes it on VLC exit (we already hold the PID)
- [ ] Gateway inactivity timeout as fallback close (video bypasses the gateway, so non-televisor clients degrade to time-boxed passes — acceptable)
- [ ] Recurring MPP subscriptions once pay-kit's Rust verify lands (501 today)

## Open questions

- [ ] Panel targets: XUI.one first? abstract others behind the provisioning trait?
- [ ] Gateway home: televisor repo (`crates/gateway`, leaning yes) vs pay repo as a "vertical" example
- [ ] Pay integration: `pay` CLI subprocess (leaning yes for v1) vs linking pay-core client
- [ ] Refunds/disputes: v1 = short tiers bound the blast radius + catalog delisting; enough?

---

<details>
<summary>Appendix: evidence from a live panel (2026-07-16)</summary>

- Trial = panel account with `exp_date` epoch + `max_connections: 1` — the entitlement primitive already exists; that's what reseller trial bots provision.
- Live playback `GET /live/USER/PASS/ID.m3u8` → 302 to the media server with an **opaque token** → video can bypass a gateway entirely.
- Catch-up `GET /timeshift/USER/PASS/DUR/START/ID.m3u8` → 302 whose path **contains the raw credentials** → why Mode B needs per-wallet upstream accounts.
- `get.php` (M3U export) returns HTTP 884 on trials — treat M3U export as optional in gateways.

</details>
