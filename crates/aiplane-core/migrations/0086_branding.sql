-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright (C) 2026 croit GmbH

-- Operator-uploaded company logo, served to the SPA at /brand/logo and
-- /brand/logo-dark. One row per theme slot; the bytes are the stored,
-- normalised image (raster uploads are resized server-side), so serving
-- reads a single column.

CREATE TABLE branding (
    slot         TEXT PRIMARY KEY CHECK (slot IN ('light', 'dark')),
    bytes        BLOB NOT NULL,
    content_type TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
