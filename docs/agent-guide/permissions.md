# Grant resources and share agent access

An agent's **resource grants** control what its principal may use. Its **shares** control which people may manage it or respond to waiting work. Configuring one does not configure the other.

You need agent-management permission and write access to change grants or shares. The server verifies resource grant authority when a grant is made; a manager may grant only resources they hold. A new agent starts with no grants.

## Grant the resources an agent uses

1. Open `/agents/<id>` → **Setup** → advanced → **Grants**.
2. Choose a resource kind and select/enter its reference from the available resource catalog.
3. Add the grant. Inspect the resulting kind/reference/granted-by row.
4. In Setup, select that resource in the spec where the task needs it.
5. Save and test. Publish validation checks required grants for the configured resources.

The grant kinds are `model`, `tool`, `connector`, `skill` and `rag_collection`, as offered by the agent resource catalog. Use real catalog references. Model choice reuses the same models, aliases and automatic routes as the rest of AIplane; it is not a separate pool/tier model selection.

Granting a tool permits its use by the principal; selecting it in `main.tools` makes it part of that agent's intended behavior. A knowledge collection grant concerns the corpus; the agent also needs a suitable retrieval tool. A skill grant concerns the installed bundle; the agent selects it in `main.skills`.

Revoke removes the selected grant. Grants are not versioned with the spec: changing a live version or restoring an older one still uses today's grants. Missing grants can therefore affect a previously working published version.

## Configure tool approval policy

Open advanced **Edit** and expand the selected tool's settings. Set `permission` to `always_allow` or `always_ask`, or leave it unset for default policy. Configure `approval_timeout` through the supported spec editor when a task needs an explicit deadline.

`always_ask` creates a persisted turn suspension before the permitted tool call executes. The waiting card includes the tool and its arguments/preview where available. Approving one call is separate from changing the agent's permanent grant. [Inbox handling](run-observe.md#answer-waiting-work) explains who can respond and what expiry means.

Bound tool arguments come from configured constants/state rather than letting the model freely replace those inputs. Use a trusted state slot for an input that depends on verified identity. Test both accepted and refused paths before publishing.

## Share with managers or responders

1. Open **Settings → Sharing** (`/agents/<id>?tab=settings&sub=sharing`).
2. Choose user or group and the access level.
3. Search for the actual subject and select a returned result. Search begins from two characters; the form does not expose the whole user roster.
4. Add. Inspect the table, then change access or revoke a share when needed.

| Access | Intended use |
|---|---|
| `read` | Inspect agent configuration and its conversations |
| `write` | Manage agent configuration and access |
| `respond` | Answer authorized waiting inbox items |

Read and write recipients must be agent managers because those rights expose the spec and visitor conversations. Respond access can be granted to users/groups without builder permission. The server rejects a change that would remove the final write share. An error states the missing requirement; changing the browser form cannot bypass it.

Share access is sensitive because it can expose visitor conversation content. Choose the narrow access needed for a responder, and review the existing table after each change.

## Organization Dot configuration

An organization Dot uses these same grants, shares and spec controls. Its
persisted organization owner supplies an additional membership boundary:
current membership and an appropriate share are both required. Read and write
still require agent-management permission. Respond access supplies authorized
Inbox standing without exposing the configuration. Administration retains
configuration recovery rights.

Organization share searches suggest eligible current members and the
organization's bound groups. Results are capped at eight users and eight
groups. A broad search can reach its bounded candidate budget before finding
a member; when `truncated` is true, narrow the query. Absence from that partial
result does not establish absence of an eligible account.

The resource inventory describes configuration grants. Organization connector
tool inventory and direct connector grants remain unavailable until the
principal runtime bridge is complete. Current member writers can manage the
Dot’s own OAuth credential through the connection panel in **Setup**.
Authorization and explicit selection are separate actions. The server repeats
current membership, Write access and catalogue checks before OAuth exchange
and credential save. Read/respond shares and nonmember administrator recovery
provide no credential access. An authorized writer can erase retained local
credentials even when the connector is unavailable. Organization execution remains
unavailable; saving, publishing or activating its configuration does not start
a model or give it an employee's account. See [organization
Dots](../dots-organizations.md) for the implementation boundary.

## Troubleshooting

| Symptom | Check |
|---|---|
| Grant refused | Manager's own rights to that resource and agent write access |
| Spec lists a tool but run cannot use it | Real tool ID, principal grant and tool availability |
| Older live version fails after rollback | Current grants; rollback does not restore them |
| Share subject not offered | Search length, actual user/group and server eligibility |
| Read/write share refused | Recipient's `can_manage_agents` permission |
| Respond-only person cannot open builder | Expected separation of responder and manager access |
| Cannot revoke writer | At least one write share must remain |
