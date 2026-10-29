-- One name for one concept: a personal or team agent's ownership row and
-- everything hanging off it are agent tables.

ALTER TABLE dots RENAME TO agent_owners;
ALTER TABLE dot_memories RENAME TO agent_memories;
ALTER TABLE dot_delegation_edges RENAME TO agent_delegation_edges;
ALTER TABLE dot_connector_bindings RENAME TO agent_connector_bindings;
ALTER TABLE dot_workspace_cleanup RENAME TO agent_workspace_cleanup;
ALTER TABLE personal_dot_sessions RENAME TO personal_agent_sessions;

ALTER TABLE gateway_groups RENAME COLUMN can_manage_dots TO can_manage_personal_agents;
ALTER TABLE agent_architect_sessions RENAME COLUMN dot_owner TO agent_owner;
ALTER TABLE agent_architect_sessions RENAME COLUMN dot_plan TO agent_plan;
ALTER TABLE agent_workspace_cleanup RENAME COLUMN dot_id TO agent_id;
ALTER TABLE agent_delegation_edges RENAME COLUMN caller_dot_id TO caller_agent_id;
ALTER TABLE agent_delegation_edges RENAME COLUMN receiver_dot_id TO receiver_agent_id;
ALTER TABLE agent_memories RENAME COLUMN dot_id TO agent_id;
ALTER TABLE personal_agent_sessions RENAME COLUMN dot_id TO agent_id;
ALTER TABLE personal_agent_sessions RENAME COLUMN caller_dot_id TO caller_agent_id;
ALTER TABLE scheduled_actions RENAME COLUMN dot_id TO personal_agent_id;
ALTER TABLE webhooks RENAME COLUMN dot_id TO personal_agent_id;
ALTER TABLE agent_connector_bindings RENAME COLUMN dot_id TO agent_id;
ALTER TABLE cross_boundary_delegations RENAME COLUMN caller_dot_id TO caller_agent_id;
ALTER TABLE cross_boundary_delegations RENAME COLUMN receiver_dot_id TO receiver_agent_id;

DROP INDEX idx_dots_user_name;
DROP INDEX idx_dots_organization_name;
DROP INDEX idx_dots_user_active;
DROP INDEX idx_dots_organization_active;
DROP INDEX idx_dot_workspace_cleanup_due;
DROP INDEX idx_dot_delegation_edges_receiver;
DROP INDEX idx_dot_memories_dot_content;
DROP INDEX idx_dot_memories_organization_content;
DROP INDEX idx_dot_memories_dot_created;
DROP INDEX idx_dot_memories_organization_created;
DROP INDEX idx_personal_dot_sessions_owner_dot;

DROP TRIGGER personal_dot_workspace_cleanup_after_delete;
DROP TRIGGER dots_reject_owner_update;
DROP TRIGGER personal_dots_delete_principals_before_owner_delete;

CREATE UNIQUE INDEX idx_agent_owners_user_name ON agent_owners (owner_user_id, name) WHERE owner_user_id IS NOT NULL;
CREATE UNIQUE INDEX idx_agent_owners_organization_name ON agent_owners (organization_id, name) WHERE organization_id IS NOT NULL;
CREATE INDEX idx_agent_owners_user_active ON agent_owners (owner_user_id, active) WHERE owner_user_id IS NOT NULL;
CREATE INDEX idx_agent_owners_organization_active ON agent_owners (organization_id, active) WHERE organization_id IS NOT NULL;
CREATE INDEX idx_agent_workspace_cleanup_due ON agent_workspace_cleanup (next_attempt_at);
CREATE INDEX idx_agent_delegation_edges_receiver ON agent_delegation_edges (receiver_agent_id);
CREATE UNIQUE INDEX idx_agent_memories_agent_content ON agent_memories (agent_id, content) WHERE agent_id IS NOT NULL;
CREATE UNIQUE INDEX idx_agent_memories_organization_content ON agent_memories (organization_id, content) WHERE organization_id IS NOT NULL;
CREATE INDEX idx_agent_memories_agent_created ON agent_memories (agent_id, created_at DESC) WHERE agent_id IS NOT NULL;
CREATE INDEX idx_agent_memories_organization_created ON agent_memories (organization_id, created_at DESC) WHERE organization_id IS NOT NULL;
CREATE INDEX idx_personal_agent_sessions_owner_agent
    ON personal_agent_sessions (owner_user_id, agent_id, created_at DESC);

CREATE TRIGGER personal_agent_workspace_cleanup_after_delete
AFTER DELETE ON agent_owners
WHEN OLD.owner_user_id IS NOT NULL
BEGIN
    INSERT INTO agent_workspace_cleanup (agent_id, owner_user_id, created_at, next_attempt_at)
    VALUES (OLD.principal_id, OLD.owner_user_id,
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
END;

CREATE TRIGGER agent_owners_reject_owner_update
BEFORE UPDATE OF principal_id, owner_user_id, organization_id ON agent_owners
WHEN NEW.principal_id IS NOT OLD.principal_id OR NEW.owner_user_id IS NOT OLD.owner_user_id OR NEW.organization_id IS NOT OLD.organization_id
BEGIN
    SELECT RAISE(ABORT, 'Agent ownership is immutable');
END;

CREATE TRIGGER personal_agents_delete_principals_before_owner_delete
BEFORE DELETE ON users
BEGIN
    DELETE FROM system_principals
    WHERE id IN (
        SELECT principal_id FROM agent_owners WHERE owner_user_id = OLD.id
    );
END;

UPDATE app_settings SET key = 'settings.agents.max_active_per_user'
WHERE key = 'settings.dots.max_active_per_user';
UPDATE app_settings SET key = 'settings.agents.max_active_per_organization'
WHERE key = 'settings.dots.max_active_per_organization';
