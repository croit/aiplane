// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The source-kind half of the `/rag` create and edit forms.
//!
//! Everything here is driven by [`ProviderFactory::config_fields`]: the
//! picker is built from the registered providers, and each provider's inputs
//! are rendered from the fields it declares. **No provider is named in this
//! file.** That is the point — a page that matched on `"webdav"` to decide
//! which inputs to draw would put the extensibility back where it started,
//! and adding Dropbox would mean editing the admin UI.
//!
//! `git` is the one special case, and deliberately so: it is not a
//! [`FileProvider`] (a clone materialises the tree on disk, which the worker
//! reads directly) so it has no factory to enumerate. It is offered as the
//! first option and maps to [`SourceSpec::default`].
//!
//! Secrets never round-trip to the browser. On the edit form a stored secret
//! renders as an empty input labelled "stored"; leaving it empty keeps what
//! is stored, and a **Clear** checkbox is the only way to remove one.
//!
//! [`FileProvider`]: gateway_features::server::rag::source::FileProvider
