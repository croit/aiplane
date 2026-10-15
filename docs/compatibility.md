<!--
SPDX-License-Identifier: AGPL-3.0-only
Copyright (C) 2026 croit GmbH
-->

# Stable identifiers

Some names in AIplane look older than the product. They identify existing
storage or are shared with separately updated clients, so they are part of
active contracts and must not be renamed casually.

- `/var/lib/gateway`, `gateway.sqlite`, the `gateway` container user, and the `systemd-gateway` volume name identify the runtime storage of existing installations. Renaming one starts a second, empty store beside the database.
- Browser-extension message names and the `gateways` browser storage key are shared between the extension and the server, which are updated independently.
- `x-gateway-affinity`, `X-Gateway-Backend`, and `x-gateway-tool-rounds` are client-facing HTTP fields. The affinity header is also accepted as `x-aiplane-affinity`, which wins when a request carries both.
- The `llmgw-` ComfyUI output prefix is part of the generated-file contract.

AIplane refuses to start while any environment variable starting with `GATEWAY_` is set, naming each one, so a forgotten old name fails loudly instead of being ignored.

Everything else uses the AIplane names: environment variables are `AIPLANE_*`,
the binary is `/app/aiplane`, images and the Helm chart are published as
`ghcr.io/croit/aiplane*` and `oci://ghcr.io/croit/charts/aiplane`, and the Helm
session Secret key is `AIPLANE_SESSION_KEY`.
