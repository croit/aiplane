// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use shared::api::ToolDef;

use super::{Tool, ToolContext, ToolFuture, ToolSource};

pub const SEARCH_TOOL_ID: &str = "search_gateway_tools";
const MAX_MATCHES: usize = 5;

struct SearchGatewayTools {
    candidates: Vec<ToolDef>,
    active: Arc<Mutex<HashSet<String>>>,
}

impl Tool for SearchGatewayTools {
    fn id(&self) -> &str {
        SEARCH_TOOL_ID
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            SEARCH_TOOL_ID,
            "Find gateway tools relevant to a task. Matching tools become available in the next round. Search by capability, service, or action; refine the query if needed.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["query"],
                "properties": {"query": {"type": "string", "description": "Capability, service, or action to find"}}
            }),
        )
    }

    fn run<'a>(&'a self, _ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if query.is_empty() {
                return Ok(json!({"tools": [], "message": "Provide a search query."}));
            }
            let terms = query
                .split_whitespace()
                .map(str::to_lowercase)
                .collect::<Vec<_>>();
            let matches = self
                .candidates
                .iter()
                .filter(|def| {
                    let haystack = format!("{} {}", def.function.name, def.function.description)
                        .to_lowercase();
                    terms.iter().all(|term| haystack.contains(term))
                })
                .take(MAX_MATCHES)
                .collect::<Vec<_>>();
            if let Ok(mut active) = self.active.lock() {
                active.extend(matches.iter().map(|def| def.function.name.clone()));
            }
            Ok(json!({"tools": matches.iter().map(|def| json!({
                "name": def.function.name,
                "description": def.function.description,
            })).collect::<Vec<_>>()}))
        })
    }
}

/// Per-request source: Always On definitions plus a small search bootstrap.
/// Auto tools join the source only after a matching search call.
pub struct DiscoverableToolSource<'a> {
    source: &'a dyn ToolSource,
    always: HashSet<String>,
    auto: HashSet<String>,
    active: Arc<Mutex<HashSet<String>>>,
    search: Option<Arc<SearchGatewayTools>>,
}

impl<'a> DiscoverableToolSource<'a> {
    pub fn new(source: &'a dyn ToolSource, always: &[String], auto: &[String]) -> Self {
        let active = Arc::new(Mutex::new(HashSet::new()));
        let search = (!auto.is_empty()).then(|| {
            let mut candidates = source.defs_for(auto);
            candidates.sort_by(|left, right| left.function.name.cmp(&right.function.name));
            Arc::new(SearchGatewayTools {
                candidates,
                active: active.clone(),
            })
        });
        Self {
            source,
            always: always.iter().cloned().collect(),
            auto: auto.iter().cloned().collect(),
            active,
            search,
        }
    }

    pub fn offered_ids(&self) -> Vec<String> {
        let mut ids = self.always.iter().cloned().collect::<Vec<_>>();
        if let Ok(active) = self.active.lock() {
            ids.extend(active.iter().filter(|id| self.auto.contains(*id)).cloned());
        }
        ids.sort();
        if self.search.is_some() {
            ids.insert(0, SEARCH_TOOL_ID.to_string());
        }
        ids
    }
}

impl ToolSource for DiscoverableToolSource<'_> {
    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        if id == SEARCH_TOOL_ID {
            return self
                .search
                .as_ref()
                .map(|tool| tool.clone() as Arc<dyn Tool>);
        }
        if self.always.contains(id)
            || (self.auto.contains(id)
                && self.active.lock().is_ok_and(|active| active.contains(id)))
        {
            self.source.get(id)
        } else {
            None
        }
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        let mut defs = Vec::new();
        for id in allowed {
            if id == SEARCH_TOOL_ID {
                if let Some(search) = &self.search {
                    defs.push(search.schema());
                }
            } else if self.get(id).is_some() {
                defs.extend(self.source.defs_for(std::slice::from_ref(id)));
            }
        }
        defs
    }

    fn ids(&self) -> Vec<String> {
        self.offered_ids()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tools::ToolRegistry;
    use crate::server::tools::echo::Echo;
    use crate::server::tools::time::CurrentTimestamp;

    #[tokio::test]
    async fn search_activates_only_matching_auto_tools() {
        let registry = ToolRegistry::new().with(Echo).with(CurrentTimestamp);
        let source = DiscoverableToolSource::new(
            &registry,
            &["company_echo".into()],
            &["get_current_timestamp".into()],
        );
        assert_eq!(source.offered_ids(), vec![SEARCH_TOOL_ID, "company_echo"]);
        assert!(!source.contains("get_current_timestamp"));
        let pool = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        source
            .get(SEARCH_TOOL_ID)
            .unwrap()
            .run(ToolContext::for_test(pool), json!({"query": "timestamp"}))
            .await
            .unwrap();
        assert!(source.contains("get_current_timestamp"));
        assert_eq!(source.offered_ids().len(), 3);
    }
}
