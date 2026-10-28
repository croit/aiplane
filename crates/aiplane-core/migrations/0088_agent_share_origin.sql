-- Who gave each agent share, and when: the agent list names the person who
-- shared an agent with you. Existing shares take both from the activity log,
-- the latest `agent_share_set` for the same subject, else the agent's
-- creation for the creator's own share; a share neither explains stays NULL.
ALTER TABLE agent_shares ADD COLUMN granted_by TEXT;
ALTER TABLE agent_shares ADD COLUMN granted_at TEXT;

UPDATE agent_shares SET
    granted_by = (
        SELECT a.actor_id FROM agent_audit a
         WHERE a.principal_id = agent_shares.principal_id
           AND a.kind = 'agent_share_set'
           AND json_extract(a.detail, '$.subject_kind') = agent_shares.subject_kind
           AND json_extract(a.detail, '$.subject_id') = agent_shares.subject_id
         ORDER BY a.created_at DESC LIMIT 1),
    granted_at = (
        SELECT a.created_at FROM agent_audit a
         WHERE a.principal_id = agent_shares.principal_id
           AND a.kind = 'agent_share_set'
           AND json_extract(a.detail, '$.subject_kind') = agent_shares.subject_kind
           AND json_extract(a.detail, '$.subject_id') = agent_shares.subject_id
         ORDER BY a.created_at DESC LIMIT 1);

UPDATE agent_shares SET
    granted_by = (
        SELECT a.actor_id FROM agent_audit a
         WHERE a.principal_id = agent_shares.principal_id AND a.kind = 'agent_created'
         ORDER BY a.created_at LIMIT 1),
    granted_at = (
        SELECT a.created_at FROM agent_audit a
         WHERE a.principal_id = agent_shares.principal_id AND a.kind = 'agent_created'
         ORDER BY a.created_at LIMIT 1)
 WHERE granted_by IS NULL
   AND subject_kind = 'user'
   AND subject_id = (
        SELECT a.actor_id FROM agent_audit a
         WHERE a.principal_id = agent_shares.principal_id AND a.kind = 'agent_created'
         ORDER BY a.created_at LIMIT 1);
