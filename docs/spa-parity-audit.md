# SPA parity audit

This checklist tracks the Svelte migration against the production UI at
`https://llm.croit.io`. A page is complete only when the production and local
versions expose the same information and actions, follow the same information
hierarchy, and match at desktop and mobile breakpoints. Intentional differences
must be recorded beside the page before its checkbox is closed.

For every page:

- [x] Inventory visible data, empty/error/loading states, forms, disclosures,
      destructive confirmations, live updates, and navigation.
- [x] Compare desktop layout at the same viewport and theme.
- [x] Compare mobile layout at the same viewport and theme.
- [x] Add state-based unit/integration coverage for data and mutations.
- [x] Add or extend browser assertions for the critical user journey.
- [x] Verify all user-visible strings exist in all six Fluent catalogs.

## Shared shell

- [x] Sidebar navigation, section disclosure state, active item, conversations,
      identity controls, language/theme controls, feedback, and mobile drawer.
  - [x] Desktop route groups match production: legacy defaults and persisted
        disclosure state, route icons, non-scrolling primary navigation, and a
        dedicated scrolling conversation region.
  - [x] The runtime source offer and exact build version are again linked from
        the footer; the public build metadata route preserves the deploy-time
        `GATEWAY_SOURCE_URL` override.
- [x] Signed-out redirect and return-to behavior.
- [x] Loading, forbidden, and API-error surfaces.

## User pages

- [x] `/` — resolves the caller's latest conversation, creating only their
      first conversation when none exists, then replaces the landing URL.
- [x] `/chat` — uses the same latest-or-first landing behavior as production;
      starting another conversation remains the sidebar's explicit `+` action.
- [x] `/chat/[id]` — conversation, streaming turns, tools, attachments, voice,
      canvas, sharing, pin/fork/delete actions, and reconnect states.
  - [x] Header and empty-composer hierarchy: new-conversation title, export,
        compliance-aware chat model, transcription model, optional spoken-
        reply voice, share/fork, thinking effort, dictation, live voice, attach,
        send/stop, and selected-file feedback are restored and verified at
        desktop and 390 px widths.
  - [x] Conversation capability contract carries built-in tools, connected
        integrations, skills, descriptions, categories, and explicit
        Off/Auto/On state; the picker groups, searches, bulk-updates, and pins.
        The API contract and browser interaction are covered by tests.
  - [x] Transcript parity: compaction boundary, inline edit/retry confirmation,
        copy controls, rich tool/media output, and all failure/live states.
  - [x] Docked document canvas and complete conversation-assets surface,
        including revision history, hand edits, and persisted resizing.
  - [x] Desktop/mobile visual recheck after transcript and canvas restoration;
        the 390 px canvas is an in-viewport overlay without horizontal scroll.
- [x] `/memory` — memory list and mutations. Production and local were
      compared with empty and populated data. The three kind sections,
      always-editable rows, create/update/delete controls, 2,000-character
      bound, empty states, translations, and 390 px behavior now match; domain,
      route, and browser coverage pass.
- [x] `/scheduled` — schedules, enablement, run state, and mutations. Production
      and local were compared at desktop width, and the complete hourly/daily/
      weekly/monthly/advanced builder was exercised at 390 px. Model compliance,
      timezone previews, tool policy, conversation reuse, pause/resume, separate
      edit flow, delete confirmation, next/last-run details, translations, JSON
      contract, domain tests, OpenAPI drift, build, and browser coverage pass.
- [x] `/webhooks` — webhook list, secrets, run state, and mutations. Production
      and local were compared at desktop width; the create/edit flow and 390 px
      layout were exercised in-browser. Synchronous mode, tool security warning,
      conversation reuse, compliance-aware models, one-time full URL reveal,
      pause/resume, secret rotation, deletion, last-fire state, owner-scoped run
      payloads, history, and exact reruns are restored across reusable form/row/
      subpage components. API, OpenAPI, translation, unit, build, lint, and all
      48 browser checks pass.
- [x] `/integrations` — connector authorization and callback states. Production
      and local were compared with populated connectors at desktop width, and a
      static-token connection plus transport failure was exercised at 390 px.
      Global/operator-provided connectors, setup and reconnect states, OAuth,
      no-auth and token connection controls, branded identities, live tool
      discovery errors, counts, expandable read/write tool details, and the
      per-tool/bulk Always/Ask/Off policies are restored through reusable card,
      tool-list, and mode-picker components. API authorization, persistence,
      OpenAPI, translation, unit, build, lint, and all 48 browser checks pass.
- [x] `/skills` — personal skills, upload/edit/delete, and visibility states.
      Production and local were compared in Chrome at desktop width, and the
      complete authoring journey was exercised at 390 px. The master-detail
      rail, explicit `.skill` archive upload, inline create/edit workflow,
      rendered and sanitized Markdown detail, metadata, download, delete, and
      disabled/empty/loading/error states are restored through reusable rail,
      editor, and detail components. The JSON authoring contract now derives a
      missing request name from the manifest and returns the complete manifest
      for editing. Route integration, OpenAPI, translation, unit, build, lint,
      and all 48 browser checks pass.
- [x] `/tools` — tool catalog, grouping, and preferences. Production and local
      were compared in Chrome with the copied account data, including the
      conditional precise-location surface. The location grant gate, stored
      sharing/accuracy state, browser permission flow, stop-sharing action,
      category order, human descriptions, underlying function names, account
      toggles, and empty/error/saving states are restored through reusable
      location and toggle-section components. The location journey and mobile
      overflow are covered in-browser; API state, authorization, OpenAPI,
      translations, build, type-check, lint, and all 48 browser checks pass.
- [x] `/tokens` — token creation, one-time secret display, scopes, and
      revocation. Production and local were compared in Chrome with the copied
      account data, including notifications, usage, account identity, compact
      calendar dates, action help, and the always-visible MCP policy. The
      mobile journey creates a token, changes master and individual tool
      access, changes MCP policy, restricts models, adds a quota, revokes, and
      removes it without horizontal overflow. The owner-scoped read model,
      timezone handling, errors, API authorization, translations, unit and
      integration tests, build, lint, and all 48 browser checks pass.
- [x] `/usage` — production and local were compared in Chrome in both personal
      and all-user scopes. Period/source/backend/token URL filters, admin scope,
      metrics and unpriced-model warnings, timezone-aware quota refreshes,
      compact totals, cost, error state, and all five breakdown tables are
      restored through reusable components. API authorization, translations,
      unit/type/build checks, populated mobile interactions, and all 48 browser
      checks pass.

## Admin pages

- [x] `/admin/users` — production and local were compared in Chrome. The
      combined identity cell, OIDC groups, resolved gateway roles, joined
      date, current-user marker, impersonation gate/action, and recent audit
      trail are restored. The complete read model, mobile containment,
      translations, type/build checks, route test, and all 52 browser checks
      pass.
- [x] `/admin/tokens` — production and local were compared in Chrome with
      populated token data. Token identity and owner, active/expired/revoked
      states, complete lifecycle dates, request/token/cost accounting, owner
      model scopes, quotas, and the independent operator model restriction are
      restored. The inline restriction editor preserves the unrestricted/list
      distinction and was exercised at 390 px without page overflow. Shared
      formatting helpers, domain tests, route integration coverage,
      translations, type/build checks, and all 53 browser checks pass.
- [x] `/admin/groups` — production and local were compared in Chrome with
      populated groups. The always-visible create form and complete inline
      editor for every group are restored, including immutable names,
      descriptions, admin/default flags, raw OIDC mappings, tool grants, skill
      grants, option suggestions, deletion, and empty/error states. A reusable
      group-form component preserves the production hierarchy at desktop and
      390 px; all six catalogs, the full route round trip, build/type checks,
      and all 54 browser checks pass.
- [x] `/admin/upstreams` — pool/backend topology, health, models, aliases,
      compliance, RBAC, fallbacks, editors, live updates, connection testing,
      model-id insertion, failure diagnosis, and the DB-vs-runtime apply diff.
      Production and local were compared at desktop and 390 px mobile widths;
      route integration, domain unit, catalog, build, and browser coverage pass.
- [x] `/admin/models` — production and local were compared in Chrome with
      multiple model kinds and aliases. Feature defaults, web-search settings,
      kind/name/configuration filters, pricing units, context windows,
      reasoning styles and budgets/efforts, capability tri-states, fallback
      models, sampling TOML, and clear overrides are restored. Reusable cards
      and row editors preserve the desktop hierarchy and 390 px containment;
      route round trips, auth gates, domain tests, all six catalogs, build/type
      checks, and all 55 browser checks pass.
- [x] `/rag` — collections, sources, profiles, indexing, and OAuth states.
      The production and local pages were compared with populated collection
      data at desktop width and at 390 px. Collection creation/editing,
      versioned and aggregate sources, provider authentication, access groups,
      sync URLs, ref editing/removal/rebuild, primary refs, and index logs are
      represented in the JSON API and reusable collection components. Route,
      domain, browser, translation, type-check, and build coverage pass.
- [x] `/rag/profiles` — extraction-profile creation, editing, deletion, and
      collection usage. Built-in and custom profile states, versioning,
      extraction prompts, typed field JSON, deletion protection, and re-index
      guidance were compared at desktop and mobile widths and covered by the
      same RAG API and browser checks.
- [x] `/admin/skills` — production and local were compared with populated
      skill bundles. The master-detail rail, upload, selected file tree, source
      and directory-access states, rendered `SKILL.md`, description, bundle
      count, archive download, deletion, effective wildcard/direct grants, and
      checkbox access editor are restored. The editor validates real gateway
      groups and preserves inherited all-skills access. Reusable components,
      all six catalogs, domain and route tests, a 390 px browser workflow, and
      a 1400×950 visual check pass.
- [x] `/admin/connectors` — production and local were compared in Chrome with
      the full add editor open and with a populated catalog. The complete
      definition editor, scope/authentication variants, OAuth client JSON and
      discovery overrides, redirect URI, encrypted-secret preservation,
      access groups, operational/provenance badges, setup gating, restore,
      enable/disable, deletion, and the per-connector audit page are restored.
      Provider-aware OAuth guidance again covers Google Workspace DCR, Google,
      GitHub, Slack, and custom providers instead of collapsing them into a
      generic redirect notice.
      Shared form/card components, all six catalogs, domain and route tests, a
      complete 390 px CRUD/audit workflow, all 57 browser checks, and a
      1400×950 visual recheck pass.
- [x] `/admin/comfyui` — production and local were compared in Chrome with the
      repository's 12-workflow example catalog and representative completed
      and timeout jobs. Operator configuration, catalog reload and feedback,
      tool ids, output kinds/nodes/prefixes, descriptions, full parameter
      schemas and required markers, job states/timestamps/output/errors, and
      the not-configured/empty/error/loading states are restored. Reusable
      workflow/job components, all six catalogs, route integration and auth
      gates, domain tests, a 390 px browser journey, and a 1400×950 visual
      recheck pass.
- [x] `/admin/limits` — production and local were compared in Chrome with a
      populated mix of global, role, model, and API-token policies. The full
      policy-resolution and metering explanation, safe typed subject/model
      selection, token-owner identities, separate dimension/window/value
      columns, locale-aware values, save/delete feedback, and confirmation are
      restored through reusable form and table components. Pure domain tests,
      the existing route round trip, all six catalogs, a 390 px CRUD journey,
      and a 1400×950 visual recheck pass.
- [x] `/admin/settings` — production and local were compared with the complete
      declarative settings catalog. Bookmarkable category navigation and
      enabled counts, section titles/blurbs, effective values, disabled-feature
      disclosure, typed widths and controls, model availability, write-only
      secret clearing, per-section save/reload, no-backend guidance, and
      persistent restart warnings are restored through reusable rail, section,
      and field components. The enriched JSON contract, coercion round trip,
      all six catalogs, pure state tests, a 390 px save/restore journey, and a
      1400×950 visual recheck pass.

## Setup and exceptional flows

- [x] `/login` — production and local were compared in Chrome. The standalone
      card, explicit OIDC action, safe deep-link forwarding, language control,
      source offer, browser title, and 390 px containment are restored; a
      signed-out SPA route now stops here instead of immediately leaving for
      the provider.
- [x] `/setup` — production and local were compared in Chrome. The standalone
      two-step provider/admin wizard, redirect-URI guidance, typed fields,
      validation, protocol-claim filtering, lossless claim selection,
      completion, closed state, and token-gated recovery are covered by unit,
      route, and 390 px browser tests.
- [x] OAuth callback landing pages and provider error states. RAG and MCP
      connector success/error/replay paths remain server-owned; their localized
      standalone failure document escapes provider details and returns users to
      the SPA. Route integration coverage exercises both callback families.

## Completion gate

- [x] Every page above has recorded browser evidence for production and local.
- [x] `mise run test-web` passes.
- [x] Relevant crate and route integration tests pass.
- [x] `mise run e2e` passes against the debug UI server (62/62, no skips).
- [x] `mise run verify` passes after all known fixes are complete (2,066 Rust
      tests; Clippy, formatting, Svelte checks, and locale drift clean).

## Known failures found during the audit

- [x] Shared browser metadata: most migrated routes left `document.title`
      blank. A tested central route-title registry now preserves every
      production title, including branded, intentionally unbranded, dynamic
      conversation, and connector-audit variants. Final-turn metadata refresh
      also prevents generated chat titles from updating only the sidebar.
- [x] Frontend dependencies: the unused generated OpenAPI client had been
      removed from `package.json`, but `package-lock.json` still installed its
      generators and transitive packages. The pinned npm lock is synchronized
      with the intentionally small hand-written client.
- [x] `/chat/[id]`: the turn-stream E2E looked for the obsolete accessible
      name `Model`; it now uses the translated control name `Chat model`.
- [x] Shared sidebar: every section was expanded and the route menu introduced
      a nested scrollbar. The legacy disclosure cookie, icons, compact spacing,
      and conversation-only scrolling are restored.
- [x] `/` and `/chat`: the duplicate conversation directory was removed; both
      routes now resolve production's latest-or-first conversation directly.
- [x] `/memory`: restored the production grouping into preferences, project
      context, and facts, including its direct inline editing workflow.
- [x] `/scheduled`: restored existing-action rows, enablement, next/last-run
      details and chat links, tool policy, history reuse, compliance-aware model
      choices, the hourly schedule mode, timezone-aware previews, and the
      dedicated edit route.
- [x] `/webhooks`: restored hooks, one-time secret URLs, last-fire/chat state,
      full run history and payload reruns, synchronous mode, tool policy and its
      trust warning, conversation reuse, compliance labels, separate edit/rerun/
      runs routes, rotation, pause/resume, and deletion.
- [x] `/integrations`: restored provider identity and descriptions, global and
      per-user connection states, setup/reconnect/error guidance, all auth
      flows, tool discovery details, and per-tool/bulk Always/Ask/Off controls.
- [x] `/skills`: restored the production master-detail layout, explicit `.skill`
      archive upload, inline creation and editing, rendered detail, metadata,
      download, deletion, and every disabled/empty/loading/error state.
- [x] `/tools`: restored role-gated precise-location sharing and revocation,
      live accuracy state, underlying function names, and the production card
      hierarchy.
- [x] `/tokens`: restored model/tool scopes, quotas, notifications, usage,
      action guidance, timezone-aware dates, MCP policy, and account details.
- [x] `/usage`: limits, user/backend/source/model/token breakdowns, metrics and
      pricing state, admin scope, and the full reconstructable filter set are
      restored and browser-verified.
- [x] `/admin/users`: restored the production introduction, combined identity
      hierarchy, OIDC-to-gateway role distinction, current-user action state,
      feature-gated impersonation control, and recent activity trail.
- [x] `/admin/tokens`: restored lifecycle and accounting details, owner scopes
      and quotas, every credential state, and the independent operator model-
      scope editor with unrestricted and explicit-list states.
- [x] `/admin/groups`: restored the complete always-visible create/edit grant
      surface and the production inline information hierarchy.
- [x] `/admin/models`: restored capabilities, reasoning styles and efforts,
      parameter overrides, filters, aliases, feature defaults, search settings,
      fallback models, and clear-override actions. The browser audit also found
      and fixed a rama sibling-route collision that sent search settings to the
      feature-default handler.
- [x] `/rag`: the local page omitted the production introduction, always-visible
      full collection form, extraction-profile link and descriptions, aggregate
      mode, include/exclude globs, collection editing, index logs, ref editing,
      source counts, commit details, and sync-token clearing; these are restored.
- [x] `/rag/profiles`: the route and complete extraction-profile editor are
      restored, including profile versioning and collection re-index guidance.
      The shared SPA fallback also owns `/rag`, so direct loads and pasted RAG
      links resolve identically to client-side navigation.
- [x] `/admin/skills`: restored global skill upload, download, deletion,
      detail Markdown, bundle tree/source, effective access, and group editing.
- [x] `/admin/connectors`: restored the full definition editor, every auth and
      scope state, access grants, OAuth setup/discovery fields, lifecycle
      controls, readiness feedback, state badges, and audit log.
- [x] `/admin/comfyui`: restored operator configuration, loaded workflow
      schemas, reload feedback, and recent job diagnosis rather than only the
      reload affordance.
- [x] `/admin/limits`: restored the production page hierarchy and explanatory
      context around the rule editor.
- [x] `/admin/settings`: verified all collapsed production panels, validation,
      persistence, and secrets against the local editors.

The entries above are the first same-session desktop inventory. They are not
completion evidence: each remains open until its interactive, responsive, test,
and translation checks in the page checklist pass.
