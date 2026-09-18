-- Give backends and pools a surrogate key, so a name is a label rather than an
-- identity.
--
-- 0042 made the name the PRIMARY KEY, mirroring `config.toml` where a pool
-- listed its backends by name. The config file is gone (0064's release), and
-- the name-as-key shape had one lasting cost: eight foreign keys pointed at it
-- with `ON UPDATE NO ACTION`, so renaming meant moving every child table by
-- hand and trusting a list of them to be complete.
--
-- With an `id`, a rename is `UPDATE backends SET name = ?`. Children never
-- move, and nothing can be forgotten because nothing references the name.
--
-- `name` stays `NOT NULL UNIQUE`: it is what an operator types, what the admin
-- API addresses (`/api/v0/admin/backends/{name}`), and what `usage_daily`
-- records — that last one deliberately, as a denormalized historical label
-- with no foreign key, so accounting rows survive the row they name.
--
-- Rebuild rather than `ALTER`: SQLite cannot add a PRIMARY KEY or change a
-- foreign key in place. Children are pointed at the new parent before the old
-- one is dropped, so no statement here ever leaves a dangling reference.

-- ---------------------------------------------------------------------------
-- backends
-- ---------------------------------------------------------------------------
CREATE TABLE backends_new (
    id            INTEGER PRIMARY KEY,          -- rowid alias; stable across renames
    name          TEXT NOT NULL UNIQUE,         -- operator-facing label
    base_url      TEXT NOT NULL,
    api_key_env   TEXT,                         -- env-var NAME, never the key value
    api_key_ct    BLOB,
    api_key_nonce BLOB,
    weight        INTEGER NOT NULL DEFAULT 1,
    max_inflight  INTEGER NOT NULL DEFAULT 16,
    health_path   TEXT NOT NULL DEFAULT '/models',
    probe_models  INTEGER NOT NULL DEFAULT 1,   -- 0/1
    supports_edit INTEGER NOT NULL DEFAULT 0,   -- 0/1 (image pools only)
    enabled       INTEGER NOT NULL DEFAULT 1,   -- 0 = drained for maintenance
    created_at    TEXT NOT NULL,                -- RFC3339 UTC
    updated_at    TEXT NOT NULL
) STRICT;

INSERT INTO backends_new
    (name, base_url, api_key_env, api_key_ct, api_key_nonce, weight, max_inflight,
     health_path, probe_models, supports_edit, enabled, created_at, updated_at)
SELECT name, base_url, api_key_env, api_key_ct, api_key_nonce, weight, max_inflight,
       health_path, probe_models, supports_edit, enabled, created_at, updated_at
FROM backends;

CREATE TABLE backend_models_new (
    backend_id INTEGER NOT NULL REFERENCES backends_new(id) ON DELETE CASCADE,
    model_id   TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (backend_id, model_id)
) STRICT;
INSERT INTO backend_models_new (backend_id, model_id, sort_order)
SELECT b.id, m.model_id, m.sort_order
FROM backend_models m JOIN backends_new b ON b.name = m.backend_name;
DROP TABLE backend_models;
ALTER TABLE backend_models_new RENAME TO backend_models;

CREATE TABLE backend_aliases_new (
    backend_id INTEGER NOT NULL REFERENCES backends_new(id) ON DELETE CASCADE,
    alias      TEXT NOT NULL,
    target     TEXT,                            -- NULL = sole-model binding
    PRIMARY KEY (backend_id, alias)
) STRICT;
INSERT INTO backend_aliases_new (backend_id, alias, target)
SELECT b.id, a.alias, a.target
FROM backend_aliases a JOIN backends_new b ON b.name = a.backend_name;
DROP TABLE backend_aliases;
ALTER TABLE backend_aliases_new RENAME TO backend_aliases;

CREATE TABLE backend_probed_models_new (
    backend_id INTEGER NOT NULL REFERENCES backends_new(id) ON DELETE CASCADE,
    model_id   TEXT NOT NULL,
    -- When this set was last written, for operator debugging ("these models are
    -- from a probe three weeks ago"). Not read by the routing path.
    seen_at    TEXT NOT NULL,
    PRIMARY KEY (backend_id, model_id)
) STRICT;
INSERT INTO backend_probed_models_new (backend_id, model_id, seen_at)
SELECT b.id, p.model_id, p.seen_at
FROM backend_probed_models p JOIN backends_new b ON b.name = p.backend_name;
DROP TABLE backend_probed_models;
ALTER TABLE backend_probed_models_new RENAME TO backend_probed_models;

-- ---------------------------------------------------------------------------
-- pools
-- ---------------------------------------------------------------------------
CREATE TABLE pools_new (
    id               INTEGER PRIMARY KEY,
    name             TEXT NOT NULL UNIQUE,
    kind             TEXT NOT NULL,              -- chat|transcription|embedding|image|speech
    strategy         TEXT NOT NULL DEFAULT 'least_inflight', -- least_inflight|round_robin
    fallback_offline TEXT,                       -- model id for known-but-down spill
    compliance_gdpr  INTEGER NOT NULL DEFAULT 1, -- 0/1 (advisory UI warning)
    compliance_nda   INTEGER NOT NULL DEFAULT 1, -- 0/1
    enforce_limits   INTEGER NOT NULL DEFAULT 1, -- 0/1 (count toward quotas?)
    allowed_groups   TEXT NOT NULL DEFAULT '[]', -- JSON array of group names
    sort_order       INTEGER NOT NULL DEFAULT 0, -- deterministic iteration order
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
) STRICT;

INSERT INTO pools_new
    (name, kind, strategy, fallback_offline, compliance_gdpr, compliance_nda,
     enforce_limits, allowed_groups, sort_order, created_at, updated_at)
SELECT name, kind, strategy, fallback_offline, compliance_gdpr, compliance_nda,
       enforce_limits, allowed_groups, sort_order, created_at, updated_at
FROM pools;

-- The join table carries both surrogate keys.
CREATE TABLE pool_backends_new (
    pool_id    INTEGER NOT NULL REFERENCES pools_new(id) ON DELETE CASCADE,
    backend_id INTEGER NOT NULL REFERENCES backends_new(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (pool_id, backend_id)
) STRICT;
INSERT INTO pool_backends_new (pool_id, backend_id, sort_order)
SELECT p.id, b.id, pb.sort_order
FROM pool_backends pb
JOIN pools_new p ON p.name = pb.pool_name
JOIN backends_new b ON b.name = pb.backend_name;
DROP TABLE pool_backends;
ALTER TABLE pool_backends_new RENAME TO pool_backends;

CREATE TABLE pool_models_new (
    pool_id    INTEGER NOT NULL REFERENCES pools_new(id) ON DELETE CASCADE,
    model_id   TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (pool_id, model_id)
) STRICT;
INSERT INTO pool_models_new (pool_id, model_id, sort_order)
SELECT p.id, m.model_id, m.sort_order
FROM pool_models m JOIN pools_new p ON p.name = m.pool_name;
DROP TABLE pool_models;
ALTER TABLE pool_models_new RENAME TO pool_models;

CREATE TABLE pool_voices_new (
    pool_id   INTEGER NOT NULL REFERENCES pools_new(id) ON DELETE CASCADE,
    lang_code TEXT NOT NULL,                     -- lowercase ISO-639-1, '' = default
    voice_id  TEXT NOT NULL,
    PRIMARY KEY (pool_id, lang_code)
) STRICT;
INSERT INTO pool_voices_new (pool_id, lang_code, voice_id)
SELECT p.id, v.lang_code, v.voice_id
FROM pool_voices v JOIN pools_new p ON p.name = v.pool_name;
DROP TABLE pool_voices;
ALTER TABLE pool_voices_new RENAME TO pool_voices;

CREATE TABLE pool_offer_voices_new (
    pool_id    INTEGER NOT NULL REFERENCES pools_new(id) ON DELETE CASCADE,
    voice_id   TEXT NOT NULL,
    -- Menu order, so an operator can put the house voice first rather than
    -- having the UI sort alphabetically.
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (pool_id, voice_id)
) STRICT;
INSERT INTO pool_offer_voices_new (pool_id, voice_id, sort_order)
SELECT p.id, o.voice_id, o.sort_order
FROM pool_offer_voices o JOIN pools_new p ON p.name = o.pool_name;
DROP TABLE pool_offer_voices;
ALTER TABLE pool_offer_voices_new RENAME TO pool_offer_voices;

-- Parents last: by now nothing references the old tables.
DROP TABLE pools;
ALTER TABLE pools_new RENAME TO pools;
DROP TABLE backends;
ALTER TABLE backends_new RENAME TO backends;
