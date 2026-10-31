-- Configurable data classification: an ordered scheme of levels, data classes
-- that map onto levels, and one assignment table giving every destination a
-- maximum level. Replaces the GDPR/NDA pool flags, the sticky confidential bit
-- of a conversation and the content guard's two fixed questions.
--
-- A fresh installation has exactly one level, so nothing is visible and
-- nothing is enforced. An installation that used the old mechanism is carried
-- into the ISO-typical four-level scheme at the end of this file.

CREATE TABLE classification_levels (
    id            TEXT PRIMARY KEY NOT NULL,
    rank          INTEGER NOT NULL UNIQUE CHECK (rank >= 0),
    name          TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    color         TEXT NOT NULL DEFAULT 'neutral'
                  CHECK (color IN ('neutral', 'info', 'success', 'warning', 'error')),
    marking_text  TEXT NOT NULL DEFAULT '',
    mip_label_id  TEXT NOT NULL DEFAULT '',
    filename_tag  TEXT NOT NULL DEFAULT ''
) STRICT;

INSERT INTO classification_levels (id, rank, name, color)
VALUES ('public', 0, 'Public', 'success');

CREATE TABLE classification_data_classes (
    id                TEXT PRIMARY KEY NOT NULL,
    name              TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    level_id          TEXT NOT NULL REFERENCES classification_levels(id),
    patterns          TEXT NOT NULL DEFAULT '[]',
    rule_question     TEXT NOT NULL DEFAULT '',
    rule_positive     TEXT NOT NULL DEFAULT '',
    rule_negative     TEXT NOT NULL DEFAULT '',
    rule_threshold    REAL NOT NULL DEFAULT 0.5 CHECK (rule_threshold >= 0 AND rule_threshold <= 1),
    action            TEXT NOT NULL DEFAULT 'raise' CHECK (action IN ('raise', 'confirm', 'block')),
    terms_uploaded_by TEXT,
    terms_uploaded_at TEXT
) STRICT;

CREATE TABLE classification_terms (
    class_id TEXT NOT NULL REFERENCES classification_data_classes(id) ON DELETE CASCADE,
    term     TEXT NOT NULL,
    PRIMARY KEY (class_id, term)
) STRICT;

CREATE TABLE classification_markings (
    kind     TEXT NOT NULL CHECK (kind IN ('mip', 'tscp', 'sensitivity', 'stamp')),
    value    TEXT NOT NULL,
    level_id TEXT NOT NULL REFERENCES classification_levels(id),
    PRIMARY KEY (kind, value)
) STRICT;

CREATE TABLE classification_assignments (
    kind     TEXT NOT NULL CHECK (kind IN ('pool', 'tool', 'mcp', 'connector', 'collection')),
    id       TEXT NOT NULL,
    level_id TEXT NOT NULL REFERENCES classification_levels(id),
    over     TEXT NOT NULL DEFAULT 'hide' CHECK (over IN ('hide', 'gate')),
    PRIMARY KEY (kind, id)
) STRICT;

-- A pool is named in the assignment by its name, which an operator can change
-- and which disappears with the pool.
CREATE TRIGGER classification_assignments_follow_pool_rename
AFTER UPDATE OF name ON pools
WHEN OLD.name <> NEW.name
BEGIN
    UPDATE classification_assignments SET id = NEW.name WHERE kind = 'pool' AND id = OLD.name;
END;

CREATE TRIGGER classification_assignments_follow_pool_delete
AFTER DELETE ON pools
BEGIN
    DELETE FROM classification_assignments WHERE kind = 'pool' AND id = OLD.name;
END;

ALTER TABLE chat_session_settings ADD COLUMN level_id TEXT REFERENCES classification_levels(id);

CREATE TABLE chat_classification_events (
    id         TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    at         TEXT NOT NULL,
    from_level TEXT,
    to_level   TEXT NOT NULL,
    cause      TEXT NOT NULL,
    actor      TEXT NOT NULL,
    reason     TEXT NOT NULL DEFAULT '',
    monitored  INTEGER NOT NULL DEFAULT 0 CHECK (monitored IN (0, 1))
) STRICT;

CREATE INDEX idx_chat_classification_events_session ON chat_classification_events (session_id, at);

CREATE TABLE chat_session_shares (
    session_id   TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('user', 'group')),
    subject_id   TEXT NOT NULL,
    PRIMARY KEY (session_id, subject_kind, subject_id)
) STRICT;

CREATE INDEX idx_chat_session_shares_subject ON chat_session_shares (subject_kind, subject_id);

-- ---------------------------------------------------------------------------
-- Carry an installation that used the old mechanism into the ISO-typical scheme.
-- ---------------------------------------------------------------------------

CREATE TEMP TABLE classification_legacy AS
SELECT (
    EXISTS (SELECT 1 FROM pools WHERE compliance_gdpr = 0 OR compliance_nda = 0)
    OR EXISTS (SELECT 1 FROM app_settings
                WHERE key = 'settings.content_guard.enabled'
                  AND trim(value) IN ('true', '1', 'yes', 'on'))
    OR EXISTS (SELECT 1 FROM chat_session_settings WHERE confidential = 1)
) AS used;

INSERT INTO classification_levels (id, rank, name, color)
SELECT 'internal', 1, 'Internal', 'info' FROM classification_legacy WHERE used = 1
UNION ALL
SELECT 'confidential', 2, 'Confidential', 'warning' FROM classification_legacy WHERE used = 1
UNION ALL
SELECT 'strictly_confidential', 3, 'Strictly confidential', 'error' FROM classification_legacy WHERE used = 1;

-- A guard action of `allow` never changed anything, so its class keeps no
-- detector; `deny` and `confirm` become `block` and `confirm`. A guard that was
-- off leaves both classes without a detector as well.
INSERT INTO classification_data_classes
    (id, name, description, level_id, rule_question, action)
SELECT 'personal_data', 'Personal data', 'Migrated from the GDPR pool flag and content guard.', 'internal',
       CASE WHEN guard_on AND act <> 'allow'
            THEN 'Does this content contain personal data?' ELSE '' END,
       CASE act WHEN 'allow' THEN 'raise' WHEN 'confirm' THEN 'confirm' ELSE 'block' END
FROM (
    SELECT
        EXISTS (SELECT 1 FROM app_settings
                 WHERE key = 'settings.content_guard.enabled'
                   AND trim(value) IN ('true', '1', 'yes', 'on')) AS guard_on,
        COALESCE((SELECT trim(value) FROM app_settings
                   WHERE key = 'settings.content_guard.gdpr_action'), 'deny') AS act
), classification_legacy
WHERE used = 1;

INSERT INTO classification_data_classes
    (id, name, description, level_id, rule_question, action)
SELECT 'nda', 'Confidential material under NDA', 'Migrated from the NDA pool flag and content guard.', 'confidential',
       CASE WHEN guard_on AND act <> 'allow'
            THEN 'Does this content contain confidential or NDA-protected information?' ELSE '' END,
       CASE act WHEN 'allow' THEN 'raise' WHEN 'confirm' THEN 'confirm' ELSE 'block' END
FROM (
    SELECT
        EXISTS (SELECT 1 FROM app_settings
                 WHERE key = 'settings.content_guard.enabled'
                   AND trim(value) IN ('true', '1', 'yes', 'on')) AS guard_on,
        COALESCE((SELECT trim(value) FROM app_settings
                   WHERE key = 'settings.content_guard.nda_action'), 'deny') AS act
), classification_legacy
WHERE used = 1;

-- A pool that met `settings.chat.confidential.requires` (default `both`) served
-- confidential conversations: it is cleared for `confidential`. Both flags
-- clear it for everything; GDPR alone keeps `internal`.
INSERT INTO classification_assignments (kind, id, level_id)
SELECT 'pool', name,
       CASE WHEN compliance_gdpr = 1 AND compliance_nda = 1 THEN 'strictly_confidential'
            WHEN (requires = 'gdpr' AND compliance_gdpr = 1)
              OR (requires = 'nda' AND compliance_nda = 1) THEN 'confidential'
            WHEN compliance_gdpr = 1 THEN 'internal'
            ELSE 'public' END
FROM pools, classification_legacy,
     (SELECT COALESCE((SELECT trim(value) FROM app_settings
                        WHERE key = 'settings.chat.confidential.requires'), 'both') AS requires)
WHERE used = 1;

-- Nothing was above confidential before, so a link that worked keeps working.
INSERT INTO app_settings (key, value, updated_at)
SELECT 'settings.classification.link_share_max_level', 'confidential', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM classification_legacy
WHERE used = 1;

UPDATE chat_session_settings SET level_id = 'confidential'
WHERE confidential = 1 AND (SELECT used FROM classification_legacy) = 1;

INSERT INTO chat_classification_events (id, session_id, at, from_level, to_level, cause, actor)
SELECT lower(hex(randomblob(16))), session_id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
       'public', 'confidential', 'migration', 'system'
FROM chat_session_settings
WHERE confidential = 1 AND (SELECT used FROM classification_legacy) = 1;

INSERT INTO app_settings (key, value, updated_at)
SELECT 'settings.classification.default_level', 'confidential', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM app_settings, classification_legacy
WHERE used = 1 AND key = 'settings.chat.confidential.default'
  AND trim(value) IN ('true', '1', 'yes', 'on');

INSERT INTO app_settings (key, value, updated_at)
SELECT 'settings.classification.detector_model', value, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM app_settings
WHERE key = 'settings.content_guard.model' AND trim(value) <> '';

INSERT INTO app_settings (key, value, updated_at)
SELECT 'settings.classification.mode', trim(value), strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM app_settings
WHERE key = 'settings.content_guard.mode' AND trim(value) IN ('monitor', 'enforce');

-- The guard's own default was monitor; the classification default is enforce.
INSERT INTO app_settings (key, value, updated_at)
SELECT 'settings.classification.mode', 'monitor', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
WHERE EXISTS (SELECT 1 FROM app_settings
               WHERE key = 'settings.content_guard.enabled'
                 AND trim(value) IN ('true', '1', 'yes', 'on'))
  AND NOT EXISTS (SELECT 1 FROM app_settings WHERE key = 'settings.classification.mode');

DELETE FROM app_settings
WHERE key LIKE 'settings.content_guard.%' OR key LIKE 'settings.chat.confidential.%';

DROP TABLE classification_legacy;

ALTER TABLE pools DROP COLUMN compliance_gdpr;
ALTER TABLE pools DROP COLUMN compliance_nda;
ALTER TABLE chat_session_settings DROP COLUMN confidential;
