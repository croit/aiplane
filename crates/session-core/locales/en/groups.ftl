groups-heading = Gateway groups
groups-intro = Map OIDC claim values onto clean, IdP-independent group names, and choose what each group grants. Pools, RAG collections, and MCP connectors reference these groups by name to restrict who may use them.
groups-new-heading = New group
groups-existing-heading = Groups
groups-empty = No groups yet. Create one above.
groups-field-name = Name
groups-field-description = Description
groups-field-admin = Admin (grants /admin access)
groups-field-default = Default (applies to every signed-in user)
groups-field-oidc = OIDC group values
groups-field-oidc-help = Comma-separated raw claim values from your IdP (e.g. Entra object-id GUIDs, LDAP DNs, or plain group names). Any of these maps a user into this group.
groups-field-tools = Tools
groups-field-skills = Skills
groups-save = Save
groups-delete = Delete
groups-error-name-required = A group name is required.

# SPA group editor: the edit heading, the observed-claim hint, the per-row
# grant summary, and the delete confirmation.
groups-edit-heading = Edit { $name }
groups-observed-values = Observed: { $values }
groups-summary-counts = { $oidc } OIDC · { $tools } tools · { $skills } skills
groups-delete-confirm = Delete group { $name }? Its mappings and grants go with it.
