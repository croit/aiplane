CREATE TABLE automatic_routes (
    alias               TEXT PRIMARY KEY,
    selector_model      TEXT NOT NULL,
    objective           TEXT NOT NULL CHECK (objective IN ('quality', 'balanced', 'cost')),
    instructions        TEXT NOT NULL DEFAULT '',
    minimum_confidence  REAL NOT NULL DEFAULT 0.0 CHECK (minimum_confidence >= 0.0 AND minimum_confidence <= 1.0),
    selector_timeout_ms INTEGER NOT NULL DEFAULT 1500 CHECK (selector_timeout_ms BETWEEN 100 AND 30000),
    fallback_target     TEXT NOT NULL,
    session_affinity    INTEGER NOT NULL DEFAULT 0 CHECK (session_affinity IN (0, 1)),
    session_ttl_seconds INTEGER NOT NULL DEFAULT 3600 CHECK (session_ttl_seconds BETWEEN 60 AND 604800),
    rollout             TEXT NOT NULL DEFAULT 'shadow' CHECK (rollout IN ('active', 'shadow')),
    version             INTEGER NOT NULL DEFAULT 1,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE TABLE automatic_route_candidates (
    route_alias  TEXT NOT NULL REFERENCES automatic_routes(alias) ON DELETE CASCADE,
    key          TEXT NOT NULL,
    target       TEXT NOT NULL,
    description  TEXT NOT NULL,
    sort_order   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (route_alias, key),
    UNIQUE (route_alias, target)
);

CREATE TRIGGER automatic_routes_reject_reverse_nesting
BEFORE INSERT ON automatic_routes
WHEN EXISTS (
    SELECT 1 FROM automatic_route_candidates WHERE target = NEW.alias
)
BEGIN
    SELECT RAISE(ABORT, 'nested automatic routes are not supported');
END;

CREATE TRIGGER automatic_route_candidates_reject_nesting
BEFORE INSERT ON automatic_route_candidates
WHEN EXISTS (
    SELECT 1 FROM automatic_routes WHERE alias = NEW.target
)
BEGIN
    SELECT RAISE(ABORT, 'nested automatic routes are not supported');
END;

CREATE TABLE automatic_route_decisions (
    id                INTEGER PRIMARY KEY,
    created_at        TEXT NOT NULL,
    route_alias       TEXT NOT NULL,
    route_version     INTEGER NOT NULL,
    selector_model    TEXT NOT NULL,
    selected_key      TEXT,
    selected_target   TEXT,
    effective_target  TEXT NOT NULL,
    confidence        REAL,
    reason            TEXT NOT NULL,
    eligible_targets  TEXT NOT NULL,
    probabilities     TEXT,
    duration_ms       INTEGER NOT NULL
);

CREATE INDEX automatic_route_decisions_alias_created
    ON automatic_route_decisions(route_alias, created_at DESC);
