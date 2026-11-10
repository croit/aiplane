# Monitoring

AIplane serves process health on `/healthz` and `/readyz` (see
[deployment](deployment.md)) and Prometheus metrics on `GET /metrics`.
The metrics endpoint is off until an administrator switches it on.

## Switch on the metrics endpoint

1. Open **Administration → Settings → Access** (`/admin/settings?tab=access`)
   and find the **Prometheus metrics** card.
2. Set at least one guard:
   - **Scrape token** (`metrics.token`): a random secret that Prometheus sends
     as `Authorization: Bearer <token>`. It is stored encrypted and is
     write-only in the settings page.
   - **Allowed IPs** (`metrics.allowed_ips`): addresses or CIDR networks, such
     as `10.0.0.0/8, 2001:db8::/32`, that the scraping client must come from.
     An entry that is not an address or a network is refused on Save, with
     the entry and the reason named under the field.
3. Turn on **Serve /metrics** (`metrics.enabled`) and save the card.

The change applies to the next scrape; no restart is needed. With both guards
set, a scrape must pass both.

| Situation | Response |
|---|---|
| Switched off | The answer an unknown path gets: `404` |
| Switched on, but no token and no allowed IPs | The same; the endpoint never answers unguarded |
| Allowed IPs set, client address not in them | `403` |
| Allowed IPs stored in a form the gateway cannot read (only a hand-edited database row) | `403` for every scrape, and one error in the log when the settings load; saving the card again fixes it |
| Token set, `Authorization: Bearer` missing or wrong | `401` |
| Every configured guard passes | `200`, `text/plain; version=0.0.4` |

While the endpoint is not served, nothing distinguishes `/metrics` from a path
that does not exist. The `403` and `401` refusals use the gateway's JSON error
envelope. The address is checked before the token. The `Bearer` scheme name
matches in any case; the token must match exactly. The endpoint uses no browser
session and sends no CORS headers.

## Behind a reverse proxy

The allowed IP list is matched against the client address that AIplane
resolves for every request. It is the TCP peer unless that peer is listed in
`AIPLANE_TRUSTED_PROXIES` (see [environment](../reference/environment.md)), in
which case the forwarded client from `X-Forwarded-For` is used. Without
`AIPLANE_TRUSTED_PROXIES`, every scrape that passes through a reverse proxy or
ingress arrives from the proxy's address. An allowed IP list then admits either
all clients of the proxy or none. A forwarded header from a peer that is not a
trusted proxy is ignored.

## Prometheus configuration

```yaml
scrape_configs:
  - job_name: aiplane
    metrics_path: /metrics
    scheme: https
    authorization:
      type: Bearer
      credentials_file: /etc/prometheus/aiplane-metrics-token
    static_configs:
      - targets: ["aiplane.example.com"]
```

Use `credentials: "<token>"` instead of `credentials_file` to put the token in
the configuration directly. Without a token, omit the `authorization` block
and rely on the allowed IP list.

On Kubernetes, a Prometheus inside the cluster can scrape the AIplane Service
directly (target `aiplane.<namespace>.svc:<service port>`, `scheme: http`). The
client address is then the Prometheus pod's address, so the allowed IP list
names the pod network, or the scrape uses the token alone. Scraping through the
Ingress needs `trustedProxies` set as described in
[the client IP behind the ingress](../kubernetes.md#the-client-ip-behind-the-ingress).
The chart does not create a `ServiceMonitor`.

## Metrics

Every backend metric is labelled `pool` and `backend` with the configured pool
and backend names. Backend URLs, API keys, users and models never appear in
the output.

| Metric | Type | Meaning |
|---|---|---|
| `aiplane_build_info{version}` | gauge | Always `1`; the `version` label is the running build's version. |
| `aiplane_backend_up` | gauge | `1` when the backend's last health probe succeeded, otherwise `0`. |
| `aiplane_backend_enabled` | gauge | `1` when the backend may take traffic, `0` when it is switched off for maintenance under Models & routing. |
| `aiplane_backend_auth_failed` | gauge | `1` when the backend rejected the gateway's credentials on the last health probe. |
| `aiplane_backend_inflight` | gauge | Requests the gateway has in flight at the backend right now. |
| `aiplane_backend_dispatched_total` | counter | Requests dispatched to the backend since it was loaded: at process start, or when a topology change was saved. |
| `aiplane_usage_records_dropped_total` | counter | Usage records dropped since the process started because the usage writer could not keep up. Usage and cost reports miss these requests. |

The metrics reflect the running upstream registry. A pool or backend edit
appears as soon as it is saved in Models & routing, which reloads every
backend: its dispatched counter restarts at zero, and it reports up until its
next health probe. Both counters also restart at zero when the process
restarts; Prometheus `rate()` and `increase()` handle such resets. A backend
can serve requests only when both `aiplane_backend_up` and
`aiplane_backend_enabled` are `1`. Per-user and per-model usage is reported in
the application's usage pages, not here.
