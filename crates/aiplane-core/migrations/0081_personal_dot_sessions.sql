CREATE TABLE personal_dot_sessions (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    dot_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    parent_turn_id TEXT,
    parent_tool_call_id TEXT,
    caller_dot_id TEXT,
    edge_generation TEXT,
    CHECK (
        (parent_turn_id IS NULL AND parent_tool_call_id IS NULL AND caller_dot_id IS NULL AND edge_generation IS NULL)
        OR
        (parent_turn_id IS NOT NULL AND parent_tool_call_id IS NOT NULL AND caller_dot_id IS NOT NULL AND edge_generation IS NOT NULL)
    ),
    UNIQUE (parent_turn_id, parent_tool_call_id)
) STRICT;

CREATE INDEX idx_personal_dot_sessions_owner_dot
    ON personal_dot_sessions (owner_user_id, dot_id, created_at DESC);

CREATE TEMP TABLE dot_scheduled_runs_preserved AS SELECT * FROM scheduled_runs;
CREATE TABLE scheduled_actions_new (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    principal_id TEXT REFERENCES system_principals(id) ON DELETE RESTRICT,
    created_by TEXT,
    dot_id TEXT,
    agent_version INTEGER,
    configuration_generation TEXT NOT NULL DEFAULT (lower(hex(randomblob(16))))
        CHECK (length(configuration_generation) > 0),
    name TEXT NOT NULL,
    prompt TEXT NOT NULL,
    model TEXT,
    cron TEXT NOT NULL,
    timezone TEXT NOT NULL,
    tools_enabled INTEGER NOT NULL DEFAULT 1,
    enabled INTEGER NOT NULL DEFAULT 1,
    next_run_at TEXT,
    last_run_at TEXT,
    last_status TEXT,
    last_session_id TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    reuse_conversation INTEGER NOT NULL DEFAULT 0,
    reuse_rounds INTEGER NOT NULL DEFAULT 5,
    CHECK (
        (user_id IS NOT NULL AND principal_id IS NULL AND model IS NOT NULL AND agent_version IS NULL)
        OR
        (user_id IS NULL AND principal_id IS NOT NULL AND model IS NULL AND dot_id IS NULL
         AND agent_version IS NOT NULL AND agent_version > 0 AND reuse_conversation = 0)
    )
);
INSERT INTO scheduled_actions_new
    (id, user_id, name, prompt, model, cron, timezone, tools_enabled, enabled,
     next_run_at, last_run_at, last_status, last_session_id, last_error,
     created_at, updated_at, reuse_conversation, reuse_rounds)
SELECT id, user_id, name, prompt, model, cron, timezone, tools_enabled, enabled,
       next_run_at, last_run_at, last_status, last_session_id, last_error,
       created_at, updated_at, reuse_conversation, reuse_rounds
FROM scheduled_actions;
DROP TABLE scheduled_actions;
ALTER TABLE scheduled_actions_new RENAME TO scheduled_actions;
DELETE FROM scheduled_runs;
INSERT INTO scheduled_runs (id, action_id, fired_at, status, session_id, error, created_at)
SELECT id, action_id, fired_at, status, session_id, error, created_at
FROM dot_scheduled_runs_preserved;
DROP TABLE dot_scheduled_runs_preserved;
CREATE INDEX scheduled_actions_due ON scheduled_actions(enabled, next_run_at);
CREATE INDEX scheduled_actions_user ON scheduled_actions(user_id, created_at DESC);
CREATE INDEX scheduled_actions_principal ON scheduled_actions(principal_id, created_at DESC);
CREATE TRIGGER scheduled_actions_owner_immutable
BEFORE UPDATE OF user_id, principal_id ON scheduled_actions
WHEN NEW.user_id IS NOT OLD.user_id OR NEW.principal_id IS NOT OLD.principal_id
BEGIN
    SELECT RAISE(ABORT, 'The scheduled action owner is immutable');
END;
CREATE TEMP TABLE dot_webhook_runs_preserved AS SELECT * FROM webhook_runs;
CREATE TABLE webhooks_new (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    principal_id TEXT REFERENCES system_principals(id) ON DELETE RESTRICT,
    created_by TEXT,
    dot_id TEXT,
    agent_version INTEGER,
    configuration_generation TEXT NOT NULL DEFAULT (lower(hex(randomblob(16))))
        CHECK (length(configuration_generation) > 0),
    name TEXT NOT NULL,
    prompt TEXT NOT NULL,
    model TEXT,
    tools_enabled INTEGER NOT NULL DEFAULT 0,
    synchronous INTEGER NOT NULL DEFAULT 0,
    secret_hash TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_fired_at TEXT,
    last_status TEXT,
    last_session_id TEXT,
    last_error TEXT,
    last_payload TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    reuse_conversation INTEGER NOT NULL DEFAULT 0,
    reuse_rounds INTEGER NOT NULL DEFAULT 5,
    CHECK (
        (user_id IS NOT NULL AND principal_id IS NULL AND model IS NOT NULL AND agent_version IS NULL)
        OR
        (user_id IS NULL AND principal_id IS NOT NULL AND model IS NULL AND dot_id IS NULL
         AND agent_version IS NOT NULL AND agent_version > 0
         AND reuse_conversation = 0 AND synchronous = 0)
    )
);
INSERT INTO webhooks_new
    (id, user_id, name, prompt, model, tools_enabled, synchronous, secret_hash, enabled,
     last_fired_at, last_status, last_session_id, last_error, last_payload,
     created_at, updated_at, reuse_conversation, reuse_rounds)
SELECT id, user_id, name, prompt, model, tools_enabled, synchronous, secret_hash, enabled,
       last_fired_at, last_status, last_session_id, last_error, last_payload,
       created_at, updated_at, reuse_conversation, reuse_rounds
FROM webhooks;
DROP TABLE webhooks;
ALTER TABLE webhooks_new RENAME TO webhooks;
DELETE FROM webhook_runs;
INSERT INTO webhook_runs
    (id, webhook_id, fired_at, status, session_id, error, prompt, payload, source, created_at)
SELECT id, webhook_id, fired_at, status, session_id, error, prompt, payload, source, created_at
FROM dot_webhook_runs_preserved;
DROP TABLE dot_webhook_runs_preserved;
CREATE UNIQUE INDEX webhooks_secret ON webhooks(secret_hash);
CREATE INDEX webhooks_user ON webhooks(user_id, created_at DESC);
CREATE INDEX webhooks_principal ON webhooks(principal_id, created_at DESC);
CREATE TRIGGER webhooks_owner_immutable
BEFORE UPDATE OF user_id, principal_id ON webhooks
WHEN NEW.user_id IS NOT OLD.user_id OR NEW.principal_id IS NOT OLD.principal_id
BEGIN
    SELECT RAISE(ABORT, 'The webhook owner is immutable');
END;

ALTER TABLE scheduled_runs ADD COLUMN assistant_turn_id TEXT;
ALTER TABLE scheduled_runs ADD COLUMN tools_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (tools_enabled IN (0, 1));
ALTER TABLE scheduled_runs ADD COLUMN history_limit INTEGER
    CHECK (history_limit IS NULL OR history_limit >= 0);
ALTER TABLE scheduled_runs ADD COLUMN configuration_generation TEXT
    CHECK (configuration_generation IS NULL OR length(configuration_generation) > 0);
ALTER TABLE scheduled_runs ADD COLUMN agent_version INTEGER
    CHECK (agent_version IS NULL OR (agent_version > 0 AND configuration_generation IS NOT NULL));
CREATE UNIQUE INDEX scheduled_runs_assistant_turn ON scheduled_runs (assistant_turn_id)
    WHERE assistant_turn_id IS NOT NULL;

ALTER TABLE webhook_runs ADD COLUMN assistant_turn_id TEXT;
ALTER TABLE webhook_runs ADD COLUMN tools_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (tools_enabled IN (0, 1));
ALTER TABLE webhook_runs ADD COLUMN history_limit INTEGER
    CHECK (history_limit IS NULL OR history_limit >= 0);
ALTER TABLE webhook_runs ADD COLUMN configuration_generation TEXT
    CHECK (configuration_generation IS NULL OR length(configuration_generation) > 0);
ALTER TABLE webhook_runs ADD COLUMN agent_version INTEGER
    CHECK (agent_version IS NULL OR (agent_version > 0 AND configuration_generation IS NOT NULL));
CREATE UNIQUE INDEX webhook_runs_assistant_turn ON webhook_runs (assistant_turn_id)
    WHERE assistant_turn_id IS NOT NULL;

CREATE TEMP TABLE dot_mcp_catalog_tools_preserved AS SELECT * FROM mcp_connector_tools;
CREATE TABLE mcp_catalog_connectors_new (
    key                  TEXT PRIMARY KEY NOT NULL,
    name                 TEXT NOT NULL,
    description          TEXT,
    icon                 TEXT,
    category             TEXT,
    url                  TEXT NOT NULL,
    auth                 TEXT NOT NULL DEFAULT 'oauth2',
    use_dcr              INTEGER NOT NULL DEFAULT 1,
    client_id            TEXT,
    client_secret_ct     BLOB,
    client_secret_nonce  BLOB,
    authorize_url        TEXT,
    token_url            TEXT,
    registration_url     TEXT,
    scopes_json          TEXT NOT NULL DEFAULT '[]',
    enabled              INTEGER NOT NULL DEFAULT 0,
    seeded               INTEGER NOT NULL DEFAULT 0,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
, scope TEXT NOT NULL DEFAULT 'per_user', audit INTEGER NOT NULL DEFAULT 0, allowed_groups TEXT NOT NULL DEFAULT '[]', dispatch_generation TEXT NOT NULL DEFAULT (lower(hex(randomblob(4)))||'-'||lower(hex(randomblob(2)))||'-4'||substr(lower(hex(randomblob(2))),2)||'-'||substr('89ab',1+abs(random()%4),1)||substr(lower(hex(randomblob(2))),2)||'-'||lower(hex(randomblob(6)))) CHECK (length(dispatch_generation)>0)) STRICT;
INSERT INTO mcp_catalog_connectors_new (key, name, description, icon, category, url, auth, use_dcr, client_id, client_secret_ct, client_secret_nonce, authorize_url, token_url, registration_url, scopes_json, enabled, seeded, created_at, updated_at, scope, audit, allowed_groups) SELECT key, name, description, icon, category, url, auth, use_dcr, client_id, client_secret_ct, client_secret_nonce, authorize_url, token_url, registration_url, scopes_json, enabled, seeded, created_at, updated_at, scope, audit, allowed_groups FROM mcp_catalog_connectors;
DROP TABLE mcp_catalog_connectors;
ALTER TABLE mcp_catalog_connectors_new RENAME TO mcp_catalog_connectors;
DELETE FROM mcp_connector_tools;
INSERT INTO mcp_connector_tools(connector_key,tool_id,description,seen_at)
SELECT connector_key,tool_id,description,seen_at FROM dot_mcp_catalog_tools_preserved;
DROP TABLE dot_mcp_catalog_tools_preserved;

CREATE TABLE mcp_connections (
 id TEXT PRIMARY KEY NOT NULL,
 user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
 principal_id TEXT REFERENCES system_principals(id) ON DELETE CASCADE,
 connector_key TEXT NOT NULL,
 catalog_generation TEXT,
 generation TEXT NOT NULL,
 authorization_generation TEXT,
 authorization_expires_at TEXT,
 status TEXT NOT NULL,
 access_token_ct BLOB, access_token_nonce BLOB,
 refresh_token_ct BLOB, refresh_token_nonce BLOB,
 token_expires_at TEXT, token_url TEXT,
 scopes_json TEXT NOT NULL DEFAULT '[]',
 dcr_client_id TEXT, dcr_client_secret_ct BLOB, dcr_client_secret_nonce BLOB,
 last_error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 CHECK ((user_id IS NOT NULL AND principal_id IS NULL) OR (user_id IS NULL AND principal_id IS NOT NULL)),
 CHECK ((authorization_generation IS NULL AND authorization_expires_at IS NULL) OR
        (authorization_generation IS NOT NULL AND authorization_expires_at IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX mcp_connections_user_connector ON mcp_connections(user_id,connector_key) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX mcp_connections_principal_connector ON mcp_connections(principal_id,connector_key) WHERE principal_id IS NOT NULL;
INSERT INTO mcp_connections
 (id,user_id,connector_key,generation,status,access_token_ct,access_token_nonce,refresh_token_ct,refresh_token_nonce,
 token_expires_at,token_url,scopes_json,dcr_client_id,dcr_client_secret_ct,dcr_client_secret_nonce,last_error,created_at,updated_at)
SELECT id,user_id,connector_key,lower(hex(randomblob(4)))||'-'||lower(hex(randomblob(2)))||'-4'||substr(lower(hex(randomblob(2))),2)||'-'||substr('89ab',1+abs(random()%4),1)||substr(lower(hex(randomblob(2))),2)||'-'||lower(hex(randomblob(6))),status,access_token_ct,access_token_nonce,refresh_token_ct,refresh_token_nonce,
 token_expires_at,token_url,scopes_json,dcr_client_id,dcr_client_secret_ct,dcr_client_secret_nonce,last_error,created_at,updated_at
FROM user_mcp_connections;

CREATE TEMP TABLE dot_mcp_pending_preserved AS
SELECT p.*,lower(hex(randomblob(4)))||'-'||lower(hex(randomblob(2)))||'-4'||substr(lower(hex(randomblob(2))),2)||'-'||substr('89ab',1+abs(random()%4),1)||substr(lower(hex(randomblob(2))),2)||'-'||lower(hex(randomblob(6))) AS authorization_generation
FROM pending_mcp_oauth p JOIN users u ON u.id=p.user_id
WHERE NOT EXISTS (
 SELECT 1 FROM pending_mcp_oauth newer
 WHERE newer.user_id=p.user_id AND newer.connector_key=p.connector_key
 AND (rtrim(newer.created_at,'Z')>rtrim(p.created_at,'Z') OR (rtrim(newer.created_at,'Z')=rtrim(p.created_at,'Z') AND newer.state>p.state))
);
INSERT INTO mcp_connections(id,user_id,connector_key,generation,status,created_at,updated_at)
SELECT lower(hex(randomblob(4)))||'-'||lower(hex(randomblob(2)))||'-4'||substr(lower(hex(randomblob(2))),2)||'-'||substr('89ab',1+abs(random()%4),1)||substr(lower(hex(randomblob(2))),2)||'-'||lower(hex(randomblob(6))),p.user_id,p.connector_key,lower(hex(randomblob(4)))||'-'||lower(hex(randomblob(2)))||'-4'||substr(lower(hex(randomblob(2))),2)||'-'||substr('89ab',1+abs(random()%4),1)||substr(lower(hex(randomblob(2))),2)||'-'||lower(hex(randomblob(6))),'pending',p.created_at,p.created_at
FROM dot_mcp_pending_preserved p
WHERE NOT EXISTS(SELECT 1 FROM mcp_connections c WHERE c.user_id=p.user_id AND c.connector_key=p.connector_key);
UPDATE mcp_connections SET
 authorization_generation=(SELECT p.authorization_generation FROM dot_mcp_pending_preserved p WHERE p.user_id=mcp_connections.user_id AND p.connector_key=mcp_connections.connector_key),
 authorization_expires_at=(SELECT p.expires_at FROM dot_mcp_pending_preserved p WHERE p.user_id=mcp_connections.user_id AND p.connector_key=mcp_connections.connector_key)
WHERE EXISTS(SELECT 1 FROM dot_mcp_pending_preserved p WHERE p.user_id=mcp_connections.user_id AND p.connector_key=mcp_connections.connector_key);

UPDATE mcp_connections SET catalog_generation=(SELECT dispatch_generation FROM mcp_catalog_connectors WHERE key=mcp_connections.connector_key);

CREATE TABLE pending_mcp_oauth_new (
 state TEXT PRIMARY KEY NOT NULL,
 initiator_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
 principal_id TEXT REFERENCES system_principals(id) ON DELETE CASCADE,
 subject_principal_id TEXT REFERENCES system_principals(id) ON DELETE CASCADE,
 connector_key TEXT NOT NULL,
 catalog_generation TEXT,
 connection_id TEXT NOT NULL REFERENCES mcp_connections(id) ON DELETE CASCADE,
 authorization_generation TEXT NOT NULL, expected_connection_generation TEXT NOT NULL,
 pkce_verifier TEXT NOT NULL, redirect_uri TEXT NOT NULL, token_url TEXT NOT NULL,
 resource TEXT,dcr_client_id TEXT,dcr_client_secret_ct BLOB,dcr_client_secret_nonce BLOB,
 return_to TEXT,created_at TEXT NOT NULL,expires_at TEXT NOT NULL,
 CHECK ((user_id IS NOT NULL AND principal_id IS NULL) OR (user_id IS NULL AND principal_id IS NOT NULL))
) STRICT;
INSERT INTO pending_mcp_oauth_new
 (state,initiator_user_id,user_id,connector_key,catalog_generation,connection_id,authorization_generation,expected_connection_generation,
 pkce_verifier,redirect_uri,token_url,resource,dcr_client_id,dcr_client_secret_ct,dcr_client_secret_nonce,return_to,created_at,expires_at)
SELECT p.state,p.user_id,p.user_id,p.connector_key,c.catalog_generation,c.id,p.authorization_generation,c.generation,
 p.pkce_verifier,p.redirect_uri,p.token_url,p.resource,p.dcr_client_id,p.dcr_client_secret_ct,p.dcr_client_secret_nonce,p.return_to,p.created_at,p.expires_at
FROM dot_mcp_pending_preserved p JOIN mcp_connections c ON c.user_id=p.user_id AND c.connector_key=p.connector_key;
DROP TABLE pending_mcp_oauth;
ALTER TABLE pending_mcp_oauth_new RENAME TO pending_mcp_oauth;
CREATE INDEX idx_pending_mcp_oauth_expires ON pending_mcp_oauth(expires_at);
CREATE TRIGGER mcp_oauth_cancel_before_initiator_delete
BEFORE DELETE ON users
BEGIN
    DELETE FROM mcp_connections
    WHERE status = 'pending' AND EXISTS (
        SELECT 1 FROM pending_mcp_oauth p
        WHERE p.initiator_user_id = OLD.id
          AND p.connection_id = mcp_connections.id
          AND p.expected_connection_generation = mcp_connections.generation
          AND p.authorization_generation = mcp_connections.authorization_generation
    );
    UPDATE mcp_connections
    SET authorization_generation = NULL, authorization_expires_at = NULL
    WHERE EXISTS (
        SELECT 1 FROM pending_mcp_oauth p
        WHERE p.initiator_user_id = OLD.id
          AND p.connection_id = mcp_connections.id
          AND p.expected_connection_generation = mcp_connections.generation
          AND p.authorization_generation = mcp_connections.authorization_generation
    );
END;
DROP TABLE dot_mcp_pending_preserved;
DROP TABLE user_mcp_connections;

CREATE TABLE dot_connector_bindings (
    dot_id TEXT NOT NULL REFERENCES dots(principal_id) ON DELETE CASCADE,
    connector_key TEXT NOT NULL,
    identity_mode TEXT NOT NULL CHECK (identity_mode IN ('user', 'principal')),
    connection_generation TEXT NOT NULL,
    binding_generation TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (dot_id, connector_key)
) STRICT;
