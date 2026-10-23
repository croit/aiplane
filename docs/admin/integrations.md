# Manage connectors, skills and image workflows

These administration screens require the `admin` role. Configuration makes resources available; group permissions and agent grants determine who may use them.

## Add an MCP connector

Open `/admin/connectors`, then add a connector at `/admin/connectors/new`.

1. Choose a stable connector key and a display name. The key is read-only when editing an existing connector.
2. Set its MCP base URL, optional icon/category/description and allowed groups.
3. Choose the connection scope and authentication type.
4. Configure authentication using the instructions and redirect URI shown in the editor.
5. Save. Enable the connector after its required setup is complete.
6. For a per-user connector, have an allowed user connect their account under **Tools → Integrations**. Check discovered tools before granting them.

### Scope and authentication

| Choice | Meaning |
|---|---|
| `per_user` scope | Users connect their own account |
| `global` scope | Shared connection for the installation |
| `agent` scope | Connection intended for an agent context |
| `oauth2` authentication | OAuth connection with configured or discovered provider endpoints |
| `static_bearer` authentication | Bearer token supplied according to the selected scope |
| `none` authentication | No token authentication for this connector |

OAuth fields include client JSON, client ID, write-only client secret (the form shows a head/tail fingerprint of the stored secret), dynamic client registration, scopes and advanced authorization/token/registration URL overrides. Register the exact displayed redirect URI with the provider. For static bearer authentication, the global scope exposes the shared-token field; other scopes direct users to supply their token through their connection flow.

Allowed groups restrict connector availability. Audit logging is a separate checkbox. The enable action is disabled while a connector needs setup. Edit changes its configuration; Disable stops offering it; Delete requires confirmation. **Restore defaults** has a confirmation and restores the built-in connector definitions—review your configuration before using it.

Changing the server URL, authentication type, scope, effective shared secret
or applicable OAuth configuration invalidates earlier cached calls and pending
approvals. Owned user and Dot credentials require a new connection to the
changed resource. Changing the URL back does not reactivate them. Display
fields, audit settings and allowed groups preserve resource identity; current
enablement and group permissions are checked before execution. A request
already admitted to an external server may finish after a configuration change.

## Inspect connector audit records

Enable audit logging on the connector, then open its Audit screen at `/admin/connectors/<key>/audit`. It shows timestamp, user, tool, outcome and detail. Detail can contain tool arguments or an error; treat the records as potentially sensitive operational data.

This audit concerns connector tool calls. Agent activity, impersonation audit and usage totals are separate views for separate events. An empty audit does not prove that a connector was never used with logging disabled.

## Install and grant skills

Enable Skills and configure an accessible directory in [system settings](settings.md). Open `/admin/skills`.

1. Upload a `.skill` bundle using the skill rail.
2. Select the installed skill. Inspect its declared content in the detail pane.
3. Set the groups allowed to use it and save the grants.
4. Verify that an allowed user sees it in **Tools → Skills** and can make it available to chat.

The model reads permitted skill instructions through `read_skill`. The administration detail and the group's skill matrix edit the same grants. Installing a skill does not grant every user access. An agent also needs the corresponding skill grant and spec selection. Delete requires confirmation and removes the installed skill.

A `.skill` bundle is a ZIP containing `SKILL.md` and optional references/assets, directly or in a wrapping directory. Its YAML frontmatter must include `name` and `description`; these provide the lookup key and model-facing description. The Markdown body is instruction context, not an executable plugin. The configured directory can also contain unpacked bundles. Keep its actual content accurate: granting a skill does not install an external service mentioned in its instructions.

If the screen reports directory access failure, fix the configured path and process permissions. A configured source with no loaded bundles differs from Skills being disabled.

## Operate ComfyUI workflows

Configure ComfyUI in [system settings](settings.md), with a reachable private worker and workflow content directory. Open `/admin/comfyui`.

1. Read the worker status and health result.
2. Select a workflow from the searchable rail.
3. Inspect its declared tool contract, input parameters and output information.
4. After changing workflow files, use **Reload** to rescan the directory. Read any skipped-workflow report.
5. Grant the resulting `comfyui_*` tools to the intended groups and agents.

The catalog reads workflow definitions from the configured files. It does not create a working model/graph on the ComfyUI worker. A reachable worker can still lack the nodes or model assets required by a particular workflow.

Each workflow is a subdirectory with `manifest.toml` and `workflow.json`. The manifest defines `id`, `title`, `description`, `output_kind`, `output_node_id`, `output_filename_prefix` and parameters. Output kinds are image, video, audio and JSON. A parameter has `key`, target `node_id`/`input_key`, description, optional default, required flag and schema. Supported types include string, integer, number, boolean and image/video/audio attachments. Schemas can constrain bounds, enum values and length. Integer parameters explicitly marked `randomize_on_sentinel` replace `-1` with a random value.

AIplane validates tool arguments and injects declared values into the operator's graph. Parameters not declared in the manifest are not model-controlled settings. Attachment parameters require S3-backed attachment resolution; the worker's upload filename is supplied to the corresponding graph input. Invalid bundles are skipped with a report instead of appearing as a usable workflow.

Open `/admin/comfyui/jobs` for recent jobs. Filter all, completed, pending or failed. The workflow summary shows run count, failures and median duration for the displayed job window. The job table gives run details. Use **Refresh** to reload it; do not interpret that recent window as lifetime statistics.

Changing the worker URL or workflow directory requires a restart. Reload is for rescanning the current workflow directory. Queue polling, run deadline and concurrency are system settings.

## Troubleshooting

| Symptom | Check |
|---|---|
| Connector cannot be enabled | Required OAuth/client setup and the needs-setup indicator |
| OAuth callback fails | Exact redirect URI, client credentials, scopes and endpoint overrides |
| User cannot see connector tools | Allowed groups, connected account, discovered tools and tool grants |
| Audit is empty | Audit enabled, completed tool calls and correct connector |
| Skill upload or scan fails | Directory accessibility and valid `.skill` bundle |
| Workflow missing after reload | Skipped-workflow report and content files |
| ComfyUI health fails | Worker reachability and configured base URL |
| Workflow job fails with healthy worker | Job detail, graph nodes and worker model assets |
