// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::affinity::AffinityHint;
use super::{AcquireError, Acquired, PoolAccess, PoolKind, RouteError, UpstreamRegistry};
use crate::server::priority::Priority;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

#[derive(Default)]
pub(super) struct Scheduler {
    state: Mutex<Queue>,
}

#[derive(Default)]
struct Queue {
    next_id: u64,
    entries: BTreeMap<(Reverse<Priority>, u64), Entry>,
    next_dispatch: Option<Instant>,
    #[cfg(test)]
    dispatches: usize,
}

struct Entry {
    model: String,
    kind: PoolKind,
    access: PoolAccess,
    affinity: Option<AffinityHint>,
    deadline: Instant,
    attempted: bool,
    ready: Option<Result<Acquired, RouteError>>,
    last_error: Option<RouteError>,
}

pub(super) struct Ticket {
    scheduler: Arc<Scheduler>,
    key: (Reverse<Priority>, u64),
}

impl Scheduler {
    pub(super) fn register(
        self: &Arc<Self>,
        model: &str,
        kind: PoolKind,
        access: &PoolAccess,
        affinity: Option<&AffinityHint>,
        priority: Priority,
        budget: Duration,
    ) -> Ticket {
        let mut queue = self.state.lock().unwrap();
        if queue.entries.is_empty() {
            queue.next_dispatch = None;
        }
        let id = queue.next_id;
        queue.next_id += 1;
        let key = (Reverse(priority), id);
        queue.entries.insert(
            key,
            Entry {
                model: model.into(),
                kind,
                access: access.clone(),
                affinity: affinity.cloned(),
                deadline: Instant::now() + budget,
                attempted: false,
                ready: None,
                last_error: None,
            },
        );
        Ticket {
            scheduler: self.clone(),
            key,
        }
    }

    #[cfg(test)]
    fn pending(&self) -> usize {
        self.state.lock().unwrap().entries.len()
    }
}

impl Ticket {
    pub(super) fn poll(&self, registry: &UpstreamRegistry) -> Option<Result<Acquired, RouteError>> {
        let mut queue = self.scheduler.state.lock().unwrap();
        let now = Instant::now();
        let entry = queue.entries.get(&self.key)?;
        let first_attempt_due = !entry.attempted && now >= entry.deadline;
        if queue.next_dispatch.is_none_or(|next| now >= next) || first_attempt_due {
            queue.next_dispatch = Some(now + super::wait::POLL_INTERVAL);
            #[cfg(test)]
            {
                queue.dispatches += 1;
            }
            let order: Vec<_> = queue
                .entries
                .iter()
                .filter(|(_, entry)| entry.ready.is_none())
                .map(|(key, _)| *key)
                .collect();
            for key in order {
                let entry = queue.entries.get_mut(&key).unwrap();
                if now >= entry.deadline && entry.attempted {
                    entry.ready = Some(Err(entry.last_error.take().unwrap_or_else(|| {
                        RouteError::Acquire(AcquireError::Saturated {
                            pool: "request queue".into(),
                        })
                    })));
                    continue;
                }
                entry.attempted = true;
                match registry.route_access_affine(
                    &entry.model,
                    entry.kind,
                    &entry.access,
                    entry.affinity.as_ref(),
                ) {
                    Ok(acquired) => entry.ready = Some(Ok(acquired)),
                    Err(error) if matches!(error, RouteError::Acquire(_)) => {
                        entry.last_error = Some(error)
                    }
                    Err(error) => entry.ready = Some(Err(error)),
                }
            }
        }
        let entry = queue.entries.get_mut(&self.key)?;
        if let Some(ready) = entry.ready.take() {
            queue.entries.remove(&self.key);
            return Some(ready);
        }
        if now >= entry.deadline && entry.attempted {
            return Some(Err(entry.last_error.take().unwrap_or_else(|| {
                RouteError::Acquire(AcquireError::Saturated {
                    pool: "request queue".into(),
                })
            })));
        }
        None
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.scheduler
            .state
            .lock()
            .unwrap()
            .entries
            .remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::upstreams::config::UpstreamPoolConfig;

    #[test]
    fn waiting_requests_share_one_dispatch_pass_per_poll_interval() {
        let registry = registry();
        let held = registry.route("m", PoolKind::Chat).unwrap();
        let tickets: Vec<_> = (0..100)
            .map(|_| {
                registry.scheduler.register(
                    "m",
                    PoolKind::Chat,
                    &PoolAccess::all(),
                    None,
                    Priority::Medium,
                    Duration::from_secs(5),
                )
            })
            .collect();
        for ticket in &tickets {
            assert!(ticket.poll(&registry).is_none());
        }
        assert_eq!(registry.scheduler.state.lock().unwrap().dispatches, 1);
        let extra = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::High,
            Duration::from_secs(5),
        );
        assert!(extra.poll(&registry).is_none());
        assert_eq!(registry.scheduler.state.lock().unwrap().dispatches, 1);
        std::thread::sleep(super::super::wait::POLL_INTERVAL);
        for ticket in &tickets {
            assert!(ticket.poll(&registry).is_none());
        }
        assert_eq!(registry.scheduler.state.lock().unwrap().dispatches, 2);
        drop(held);
    }

    fn registry() -> std::sync::Arc<UpstreamRegistry> {
        let pool: UpstreamPoolConfig = toml::from_str(
            r#"
kind = "chat"
[[backend]]
name = "b"
base_url = "http://unused/v1"
max_inflight = 1
models = ["m"]
"#,
        )
        .unwrap();
        UpstreamRegistry::new(&std::collections::HashMap::from([("p".into(), pool)])).unwrap()
    }

    #[test]
    fn high_priority_gets_the_next_slot_and_equal_priority_is_fifo() {
        let registry = registry();
        let held = registry.route("m", PoolKind::Chat).unwrap();
        let low = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::Low,
            Duration::from_secs(5),
        );
        let high = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::High,
            Duration::from_secs(5),
        );
        let second = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::High,
            Duration::from_secs(5),
        );
        assert!(low.poll(&registry).is_none());
        drop(held);
        std::thread::sleep(super::super::wait::POLL_INTERVAL);
        assert!(low.poll(&registry).is_none());
        let assigned = high.poll(&registry).unwrap().unwrap();
        assert!(second.poll(&registry).is_none());
        drop(assigned);
        std::thread::sleep(super::super::wait::POLL_INTERVAL);
        let assigned = second.poll(&registry).unwrap().unwrap();
        assert!(low.poll(&registry).is_none());
        drop(assigned);
        std::thread::sleep(super::super::wait::POLL_INTERVAL);
        assert!(low.poll(&registry).unwrap().is_ok());
    }

    #[test]
    fn cancelled_assigned_request_releases_its_slot() {
        let registry = registry();
        let high = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::High,
            Duration::from_secs(5),
        );
        let low = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::Low,
            Duration::from_secs(5),
        );
        assert!(low.poll(&registry).is_none());
        drop(high);
        std::thread::sleep(super::super::wait::POLL_INTERVAL);
        assert!(low.poll(&registry).unwrap().is_ok());
        assert_eq!(registry.scheduler.pending(), 0);
        drop(low);
        assert_eq!(registry.scheduler.pending(), 0);
    }

    #[tokio::test]
    async fn deadline_and_cancellation_release_waiting_requests() {
        let registry = registry();
        let held = registry.route("m", PoolKind::Chat).unwrap();
        let high = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::High,
            Duration::ZERO,
        );
        assert!(matches!(
            high.poll(&registry),
            Some(Err(RouteError::Acquire(_)))
        ));
        drop(high);
        assert_eq!(registry.scheduler.pending(), 0);
        let waiting = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::High,
            Duration::from_secs(5),
        );
        assert!(waiting.poll(&registry).is_none());
        drop(waiting);
        drop(held);
        let low = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::Low,
            Duration::from_secs(5),
        );
        assert!(low.poll(&registry).unwrap().is_ok());
        assert_eq!(registry.scheduler.pending(), 0);
    }

    #[tokio::test]
    async fn an_expired_waiter_cannot_take_a_newly_freed_slot() {
        let registry = registry();
        let held = registry.route("m", PoolKind::Chat).unwrap();
        let ticket = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::High,
            Duration::from_millis(1),
        );
        assert!(ticket.poll(&registry).is_none());
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(held);
        assert!(matches!(
            ticket.poll(&registry),
            Some(Err(RouteError::Acquire(_)))
        ));
        assert!(registry.route("m", PoolKind::Chat).is_ok());
    }

    #[test]
    fn unavailable_high_model_does_not_block_other_models() {
        let registry = registry();
        let unknown = registry.scheduler.register(
            "unknown",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::High,
            Duration::from_secs(5),
        );
        let low = registry.scheduler.register(
            "m",
            PoolKind::Chat,
            &PoolAccess::all(),
            None,
            Priority::Low,
            Duration::from_secs(5),
        );
        assert!(low.poll(&registry).unwrap().is_ok());
        assert!(matches!(
            unknown.poll(&registry).unwrap(),
            Err(RouteError::UnknownModel(_))
        ));
    }
}
