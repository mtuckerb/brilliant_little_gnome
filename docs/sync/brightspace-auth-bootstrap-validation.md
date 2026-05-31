# Brightspace auth bootstrap validation

This note documents focused manual validation for the P2P `WireMsg::BootstrapCredentials` joiner/adoption path. It exists because the full path depends on paired Tauri windows, iroh transport, SQLite state, and a Brightspace HTTP probe; the validation endpoint must be mocked locally and must not call a real Brightspace/account/cloud service during CI or development review.

## Scope

The reviewed path is `tauri/src-tauri/src/p2p/engine.rs` handling of `WireMsg::BootstrapCredentials { host, cookie, uid, user_id }`.

Required invariant:

- the joiner must call `client.validate_session(&host, &cookie)` before `client.store_credentials(...)`;
- only `SessionValidation::Valid` may proceed to local storage and `auth-captured`;
- `SessionValidation::Invalid` and `SessionValidation::Inconclusive` must fail closed before storage;
- diagnostic logs and UI events must not include the cookie or key material.

## Local-only setup

Use a local stub HTTP server for the validation endpoint and point validation at it with `BRILLIANT_AUTH_VALIDATE_URL`. Do not use a production Brightspace tenant, account, auth link, cookie, or cloud sync service.

Example stub behavior:

- valid case: return `HTTP/1.1 200 OK`;
- invalid/expired case: return `HTTP/1.1 401 Unauthorized` or `HTTP/1.1 403 Forbidden`;
- unreachable/inconclusive case: point `BRILLIANT_AUTH_VALIDATE_URL` at a closed localhost port or stop the stub before bootstrap is received.

Use only synthetic cookie values such as `d2lSessionVal=test-session; d2lSecureSessionVal=test-secure`. Never paste a real Brightspace cookie into logs, terminal history, screenshots, or fixtures.

## Case 1: valid bootstrap credential is adopted

1. Start the local validation stub so the configured endpoint returns `200 OK`.
2. Set `BRILLIANT_AUTH_VALIDATE_URL` to the local stub endpoint.
3. Pair a seed device/session that sends `WireMsg::BootstrapCredentials` with a synthetic host and synthetic cookie.
4. Observe on the joiner:
   - `client.validate_session(&host, &cookie)` runs before storage;
   - `client.store_credentials(...)` succeeds only after validation returns `Valid`;
   - the app emits/observes the normal `auth-captured` event;
   - credentials are available locally for subsequent sync flows.
5. Inspect logs/events and confirm the synthetic cookie string is not printed.

Expected result: validation succeeds, bootstrap adoption proceeds, and no key material appears in logs or UI text.

## Case 2: invalid or expired bootstrap credential is rejected

1. Start the local validation stub so the configured endpoint returns `401 Unauthorized` or `403 Forbidden`.
2. Set `BRILLIANT_AUTH_VALIDATE_URL` to the local stub endpoint.
3. Pair a seed device/session that sends `WireMsg::BootstrapCredentials` with a synthetic host and synthetic cookie.
4. Observe on the joiner:
   - `client.validate_session(&host, &cookie)` returns `SessionValidation::Invalid`;
   - `client.store_credentials(...)` is not called after the invalid result;
   - no `auth-captured` event is emitted for the rejected bootstrap;
   - an `auth-share-blocked` event is emitted with the sanitized invalid-key message.
5. Confirm local stored Brightspace credentials remain unchanged and the synthetic cookie string is absent from logs/events.

Expected result: sync/adoption is blocked before storage, the user gets an actionable non-secret error, and no key material is logged.

## Case 3: unreachable or inconclusive validation is rejected

1. Configure `BRILLIANT_AUTH_VALIDATE_URL` to a closed localhost port, or stop the local validation stub before bootstrap arrives.
2. Pair a seed device/session that sends `WireMsg::BootstrapCredentials` with a synthetic host and synthetic cookie.
3. Observe on the joiner:
   - `client.validate_session(&host, &cookie)` returns `SessionValidation::Inconclusive`;
   - `client.store_credentials(...)` is not called after the inconclusive result;
   - no `auth-captured` event is emitted for the rejected bootstrap;
   - an `auth-share-blocked` event is emitted with the sanitized unreachable/inconclusive message.
4. Confirm logs contain only generic validation failure text and do not contain the probe URL if it embeds sensitive data, the Cookie header, or the synthetic cookie string.

Expected result: validation fails closed when the endpoint is unavailable or misconfigured.

## Reviewer checklist

- `engine.rs` has validation before `store_credentials` in the `BootstrapCredentials` branch.
- `Invalid` and `Inconclusive` branches return before storage.
- Failure branches emit only `SHARE_BLOCKED_INVALID` or `SHARE_BLOCKED_INCONCLUSIVE` text.
- No log line formats the cookie, Cookie header, or raw request/response body.
- Validation abstraction tests still pass: `cargo test --features p2p client::validation`.
- P2P build still passes: `cargo check --features p2p --quiet`.
