ALTER TABLE gateway_groups ADD COLUMN can_manage_dots INTEGER NOT NULL DEFAULT 0 CHECK (can_manage_dots IN (0, 1));

CREATE TABLE organizations (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    revision TEXT NOT NULL CHECK (length(revision) > 0),
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE organization_groups (
    organization_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    gateway_group TEXT NOT NULL REFERENCES gateway_groups(name) ON DELETE CASCADE,
    PRIMARY KEY (organization_id, gateway_group)
) STRICT;

CREATE INDEX idx_organization_groups_group ON organization_groups (gateway_group);

CREATE TRIGGER organization_groups_rotate_revision_after_insert
AFTER INSERT ON organization_groups
BEGIN
    UPDATE organizations SET revision = lower(hex(randomblob(16))),
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = NEW.organization_id;
END;

CREATE TRIGGER organization_groups_rotate_revision_after_delete
AFTER DELETE ON organization_groups
BEGIN
    UPDATE organizations SET revision = lower(hex(randomblob(16))),
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = OLD.organization_id;
END;

CREATE TRIGGER organization_groups_reject_update
BEFORE UPDATE ON organization_groups
BEGIN
    SELECT RAISE(ABORT, 'Organization membership bindings must be replaced');
END;

CREATE TABLE dots (
    principal_id TEXT PRIMARY KEY NOT NULL REFERENCES agents(principal_id) ON DELETE CASCADE,
    owner_user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    organization_id TEXT REFERENCES organizations(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK ((owner_user_id IS NOT NULL) <> (organization_id IS NOT NULL))
) STRICT;

CREATE UNIQUE INDEX idx_dots_user_name ON dots (owner_user_id, name) WHERE owner_user_id IS NOT NULL;
CREATE UNIQUE INDEX idx_dots_organization_name ON dots (organization_id, name) WHERE organization_id IS NOT NULL;
CREATE INDEX idx_dots_user_active ON dots (owner_user_id, active) WHERE owner_user_id IS NOT NULL;
CREATE INDEX idx_dots_organization_active ON dots (organization_id, active) WHERE organization_id IS NOT NULL;

CREATE TABLE dot_workspace_cleanup (
    dot_id TEXT PRIMARY KEY NOT NULL,
    owner_user_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    next_attempt_at TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    claim_token TEXT,
    claim_until TEXT,
    last_error_code TEXT CHECK (last_error_code IN ('unavailable', 'conflict', 'failed')),
    CHECK ((claim_token IS NULL) = (claim_until IS NULL))
) STRICT;

CREATE INDEX idx_dot_workspace_cleanup_due ON dot_workspace_cleanup (next_attempt_at);

CREATE TRIGGER personal_dot_workspace_cleanup_after_delete
AFTER DELETE ON dots
WHEN OLD.owner_user_id IS NOT NULL
BEGIN
    INSERT INTO dot_workspace_cleanup (dot_id, owner_user_id, created_at, next_attempt_at)
    VALUES (OLD.principal_id, OLD.owner_user_id,
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
END;

CREATE TRIGGER dots_reject_owner_update
BEFORE UPDATE OF principal_id, owner_user_id, organization_id ON dots
WHEN NEW.principal_id IS NOT OLD.principal_id OR NEW.owner_user_id IS NOT OLD.owner_user_id OR NEW.organization_id IS NOT OLD.organization_id
BEGIN
    SELECT RAISE(ABORT, 'Dot ownership is immutable');
END;

CREATE TABLE dot_delegation_edges (
    caller_dot_id TEXT NOT NULL REFERENCES dots(principal_id) ON DELETE CASCADE,
    receiver_dot_id TEXT NOT NULL REFERENCES dots(principal_id) ON DELETE CASCADE,
    outbound_enabled INTEGER NOT NULL DEFAULT 0 CHECK (outbound_enabled IN (0, 1)),
    inbound_enabled INTEGER NOT NULL DEFAULT 0 CHECK (inbound_enabled IN (0, 1)),
    generation TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (caller_dot_id, receiver_dot_id),
    CHECK (caller_dot_id <> receiver_dot_id)
) STRICT;

CREATE INDEX idx_dot_delegation_edges_receiver ON dot_delegation_edges (receiver_dot_id);

CREATE TABLE dot_memories (
    id TEXT PRIMARY KEY NOT NULL,
    dot_id TEXT REFERENCES dots(principal_id) ON DELETE CASCADE,
    organization_id TEXT REFERENCES organizations(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('preference', 'project', 'fact')),
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK ((dot_id IS NOT NULL) <> (organization_id IS NOT NULL))
) STRICT;

CREATE UNIQUE INDEX idx_dot_memories_dot_content ON dot_memories (dot_id, content) WHERE dot_id IS NOT NULL;
CREATE UNIQUE INDEX idx_dot_memories_organization_content ON dot_memories (organization_id, content) WHERE organization_id IS NOT NULL;
CREATE INDEX idx_dot_memories_dot_created ON dot_memories (dot_id, created_at DESC) WHERE dot_id IS NOT NULL;
CREATE INDEX idx_dot_memories_organization_created ON dot_memories (organization_id, created_at DESC) WHERE organization_id IS NOT NULL;

CREATE TRIGGER personal_dots_delete_principals_before_owner_delete
BEFORE DELETE ON users
BEGIN
    DELETE FROM system_principals
    WHERE id IN (
        SELECT principal_id FROM dots WHERE owner_user_id = OLD.id
    );
END;
