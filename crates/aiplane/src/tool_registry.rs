// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::sync::Arc;

use aiplane_core::server::rbac::Resolver;
use aiplane_runtime::server as rt;

/// `allow_private_networks` is the operator's
/// `$AIPLANE_ALLOW_PRIVATE_NETWORKS`, for the tools that reach a host the
/// model names.
pub fn base_registry(
    rbac: Arc<Resolver>,
    sandbox_client: Option<Arc<rt::tools::sandbox::SandboxClient>>,
    allow_private_networks: bool,
) -> rt::tools::ToolRegistry {
    rt::tools::ToolRegistry::new()
        .with(rt::tools::echo::Echo)
        .with(rt::tools::time::CurrentTimestamp)
        .with(aiplane_runtime::agents::router::ForwardRequest::personal())
        .with(aiplane_tools::fetch_url::FetchUrl::new(
            allow_private_networks,
        ))
        // Fetch an image from a URL and keep it as a reusable attachment
        // (so it can be embedded in a later typst render). Always on — the
        // runtime guard errors cleanly off the chat path / without [chat.s3].
        .with(aiplane_tools::load_image_url::LoadImageUrl::new(
            allow_private_networks,
        ))
        .with(aiplane_tools::fetch_attachment::FetchAttachment::new(
            sandbox_client.clone(),
        ))
        .with(aiplane_tools::upload_attachment::UploadAttachment)
        // Hand an existing conversation object to the user as a download —
        // a file from an earlier turn, or a data object that never got a
        // chip (a render's `.json` base). Reference-only: the bytes are
        // copied inside storage instead of being re-emitted by the model.
        .with(aiplane_tools::offer_download::OfferDownload)
        // Same, for a *set* of files: one zip chip instead of one chip per
        // file, which past about four stops being a delivery and starts being
        // a scavenger hunt (and on a phone buries the reply).
        .with(aiplane_tools::zip_attachments::ZipAttachments)
        // The other direction: an uploaded/produced text file becomes an
        // editable, versioned canvas document, so it can be changed a
        // passage at a time (and hand-edited by the user) instead of
        // rewritten wholesale through the model.
        .with(aiplane_tools::import_file::ImportFile)
        // Inventory of the conversation's files (uploads + tool outputs), so
        // the model reuses existing assets instead of regenerating them.
        // Reads only the session's turn markers — no storage config needed.
        .with(aiplane_tools::list_attachments::ListAttachments)
        .with(aiplane_tools::search_web::SearchWeb)
        .with(aiplane_tools::location::GetUserLocation)
        // Mid-turn question prompt. Configuration-free; the runtime gate is
        // `chat_feedback` being present, so it errors cleanly off the chat path
        // (and `requires_chat_session` keeps it out of the /v1 tool list).
        .with(aiplane_tools::ask_user::AskUser)
        // Act in the user's own logged-in browser, through the extension paired
        // with the open conversation. Configuration-free like `ask_user`: the
        // runtime gate is `chat_feedback` plus a relay that answers, so a
        // deployment where nobody installed the extension gets a clean "no
        // extension" rather than a hang.
        .with(aiplane_tools::browser_control::BrowserControl)
        // The same browser, but the capture goes into the reply: the user
        // cannot see a `browser_control` screenshot. Needs [chat.s3] at
        // runtime and says so before touching the browser.
        .with(aiplane_tools::show_screenshot::ShowScreenshot)
        // Reach the user when they aren't watching: a finished long job, or a
        // scheduled action that found something. Runtime-gated on `[push]`
        // being configured plus a subscribed device, so it stays registered
        // (and RBAC-grantable) on deployments without push.
        .with(aiplane_tools::notify_user::NotifyUser)
        // Reach the existing cron stack from a conversation. Create + delete
        // require an `ask_user` confirmation (and so are chat-only); listing
        // works anywhere. Actions created here always run without tools.
        .with(aiplane_tools::schedule::ScheduleAction)
        .with(aiplane_tools::schedule::ListScheduledActions)
        .with(aiplane_tools::schedule::DeleteScheduledAction)
        .with(aiplane_tools::memory::Remember)
        .with(aiplane_tools::memory::Recall)
        // Correcting a memory needs no config either, and without these the
        // store is append-only: a changed fact could only be answered by a
        // second, contradicting memory.
        .with(aiplane_tools::memory::UpdateMemory)
        .with(aiplane_tools::memory::Forget)
        // Read-only public-data lookups — no secrets, no writes, safe to
        // leave always-on.
        .with(aiplane_tools::netcheck::DnsLookup)
        .with(aiplane_tools::netcheck::WhoisLookup)
        .with(aiplane_tools::netcheck::TlsCert::new(
            allow_private_networks,
        ))
        .with(aiplane_tools::wikipedia::Wikipedia)
        .with(aiplane_tools::currency::ConvertCurrency)
        // RAG. These tools are no-ops without the indexer wired into
        // AppState; registering them unconditionally keeps RBAC config
        // stable across deployments where `[rag]` is only sometimes set.
        .with(aiplane_tools::rag::RagListCollections::new(rbac.clone()))
        .with(aiplane_tools::rag::RagSearch::new(rbac.clone()))
        // Regex over the same indexed corpus, for patterns BM25 can't express
        // (`TODO\(.*\)`, `impl .* for Tool`). Same per-collection group ACL.
        .with(aiplane_tools::rag::RagGrep::new(rbac.clone()))
        // Document-level retrieval over the fields the extraction profile
        // pulled out. This is what answers questions about *sets* of
        // documents — "the latest invoice from X", "everything about project
        // Y" — which passage retrieval structurally cannot.
        .with(aiplane_tools::rag_documents::RagQueryDocuments::new(
            rbac.clone(),
        ))
        .with(aiplane_tools::rag_documents::RagListDocuments::new(
            rbac.clone(),
        ))
        .with(aiplane_tools::rag_documents::RagFetchDocument::new(
            rbac.clone(),
        ))
        // Document canvas — build up and incrementally edit long documents
        // across turns. Content lives in the `documents` store, not S3, so
        // these need no extra config; off the chat path they error cleanly.
        .with(aiplane_tools::document::CreateDocument)
        .with(aiplane_tools::document::EditDocument)
        .with(aiplane_tools::document::ReadDocument)
        .with(aiplane_tools::document::ListDocuments)
        .with(aiplane_tools::document::EditDocumentSection)
        .with(aiplane_tools::document::ListDocumentVersions)
        .with(aiplane_tools::document::RestoreDocumentVersion)
        // Soft delete, so the canvas can be cleaned up without breaking its
        // "nothing is ever lost" promise — the tombstone keeps the version
        // history and `undelete_document` reverses it.
        .with(aiplane_tools::document::DeleteDocument)
        .with(aiplane_tools::document::UndeleteDocument)
        // QR codes render natively in-process (no sandbox, no upstream), so
        // the tool is always on; it needs [chat.s3] at runtime to deliver
        // the file and errors cleanly without it.
        .with(aiplane_tools::qr::GenerateQrCode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_registry_includes_the_user_facing_tool_families() {
        let rbac = std::sync::Arc::new(Resolver::empty());
        let tools = base_registry(rbac, None, false);
        for id in [
            "browser_control",
            "forward_request",
            "show_screenshot",
            "dns_lookup",
            "create_document",
            "fetch_attachment",
            "generate_qr_code",
            "rag_search",
            "remember",
            "schedule_action",
            "upload_attachment",
            "wikipedia",
        ] {
            assert!(tools.contains(id), "missing built-in tool {id}");
        }
    }

    /// One toggle key per capability, everywhere. `/tools`, the chat composer
    /// and the token panel all render `catalog::entries`; discovery
    /// (`enable_tools`), the per-chat overlay, token prefs and RBAC all resolve
    /// through `entry_key_for`. A row whose key the enforcement side never
    /// looks up is a switch that does nothing — `offer_download` and
    /// `zip_attachments` were exactly that, listed on their own while
    /// `upload_attachment` governed them — and a key missing from either side
    /// is a tool the user cannot find or cannot turn off.
    #[test]
    fn every_listed_toggle_is_the_key_discovery_and_enforcement_use() {
        use rt::tools::catalog::{self, BOOTSTRAP_TOOL_ID, entry_key_for, is_hidden};
        use std::collections::BTreeSet;

        let registry = base_registry(std::sync::Arc::new(Resolver::empty()), None, false);
        let ids: Vec<String> = registry.ids().map(str::to_string).collect();
        let enforced: BTreeSet<String> = ids
            .iter()
            .filter(|id| !is_hidden(id) && id.as_str() != BOOTSTRAP_TOOL_ID)
            .map(|id| entry_key_for(id).to_string())
            .collect();

        let rows = catalog::entries(&registry, &ids, &[], &[]);
        let listed: Vec<String> = rows.iter().map(|row| row.key.clone()).collect();
        let listed_set: BTreeSet<String> = listed.iter().cloned().collect();
        assert_eq!(
            listed.len(),
            listed_set.len(),
            "a key is listed twice: {listed:?}"
        );
        assert_eq!(
            listed_set, enforced,
            "the tool list and the enforcement keys disagree — a key only on the left is a \
             dead switch, one only on the right is a tool nobody can see or turn off"
        );

        let discoverable: BTreeSet<String> =
            aiplane_tools::enable_tools::EnableTools::from_registry(&registry)
                .keys()
                .into_iter()
                .collect();
        assert_eq!(
            discoverable, enforced,
            "enable_tools advertises different keys than the tool list shows"
        );

        for row in &rows {
            assert!(
                row.title != row.tech,
                "`{}` renders with its raw id as the title — give it display copy",
                row.key
            );
        }
    }
}
