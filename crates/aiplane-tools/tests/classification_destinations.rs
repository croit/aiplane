// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Tools that carry content out of a conversation are classification
//! destinations: the real tools, judged by the real level filter.

use aiplane_core::server::classification::{Level, Scheme};
use aiplane_runtime::classification_gate::retain_visible;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_tools::memory::{Forget, Recall, Remember, UpdateMemory};
use aiplane_tools::schedule::{DeleteScheduledAction, ListScheduledActions, ScheduleAction};

fn level(id: &str, rank: u32) -> Level {
    Level {
        id: id.into(),
        rank,
        name: id.into(),
        description: String::new(),
        color: "neutral".into(),
        marking_text: String::new(),
        mip_label_id: String::new(),
        filename_tag: String::new(),
    }
}

fn offered_at(rank: u32) -> Vec<String> {
    let registry = ToolRegistry::new()
        .with(Remember)
        .with(UpdateMemory)
        .with(Recall)
        .with(Forget)
        .with(ScheduleAction)
        .with(ListScheduledActions)
        .with(DeleteScheduledAction);
    let scheme = Scheme::new(vec![level("public", 0), level("internal", 1)], Vec::new());
    let mut offered = [
        "remember",
        "update_memory",
        "recall",
        "forget",
        "schedule_action",
        "list_scheduled_actions",
        "delete_scheduled_action",
    ]
    .map(str::to_owned)
    .to_vec();
    retain_visible(&scheme, rank, &mut offered, &registry);
    offered
}

#[test]
fn unrated_memory_writes_and_scheduled_actions_are_hidden_above_the_lowest_level() {
    assert_eq!(
        offered_at(1),
        [
            "recall",
            "forget",
            "list_scheduled_actions",
            "delete_scheduled_action"
        ]
    );
}

#[test]
fn at_the_lowest_level_every_memory_and_schedule_tool_is_offered() {
    assert_eq!(offered_at(0).len(), 7);
}
