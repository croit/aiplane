ALTER TABLE gateway_groups ADD COLUMN priority TEXT CHECK (priority IN ('low', 'medium', 'high'));
ALTER TABLE tokens ADD COLUMN priority TEXT CHECK (priority IN ('low', 'medium', 'high'));
ALTER TABLE backends ADD COLUMN priority TEXT NOT NULL DEFAULT '{"mode":"auto"}' CHECK (json_valid(priority));
