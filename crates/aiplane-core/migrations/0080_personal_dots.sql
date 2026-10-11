ALTER TABLE gateway_groups ADD COLUMN can_manage_dots INTEGER NOT NULL DEFAULT 0 CHECK (can_manage_dots IN (0, 1));

CREATE TABLE personal_dots (
    principal_id TEXT PRIMARY KEY NOT NULL REFERENCES agents(principal_id) ON DELETE CASCADE,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX idx_personal_dots_owner_name ON personal_dots (owner_user_id, name);
CREATE INDEX idx_personal_dots_owner_active ON personal_dots (owner_user_id, active);

CREATE TABLE personal_dot_memories (
    id TEXT PRIMARY KEY NOT NULL,
    dot_id TEXT NOT NULL REFERENCES personal_dots(principal_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('preference', 'project', 'fact')),
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX idx_personal_dot_memories_dot_content ON personal_dot_memories (dot_id, content);
CREATE INDEX idx_personal_dot_memories_dot_created ON personal_dot_memories (dot_id, created_at DESC);

CREATE TRIGGER personal_dots_delete_principals_before_owner_delete
BEFORE DELETE ON users
BEGIN
    DELETE FROM system_principals
    WHERE id IN (
        SELECT principal_id FROM personal_dots WHERE owner_user_id = OLD.id
    );
END;
