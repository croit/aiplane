use super::*;
use crate::db::{agents, system_principals as sp};
use std::path::Path;

const URL: &str = "https://hooks.slack.com/services/T000/B000/secret-part";

async fn pool_with_agent() -> (Pool, String) {
    let pool = aiplane_core::server::db::open(Path::new(":memory:"))
        .await
        .unwrap();
    let id = agents::create(
        &pool,
        &sp::NewPrincipal {
            name: "support",
            display: "Support",
            description: "",
        },
        "{}",
        "alice",
    )
    .await
    .unwrap()
    .unwrap()
    .principal
    .id;
    (pool, id)
}

fn new(name: &str) -> NewChannel<'_> {
    NewChannel {
        kind: ChannelKind::Slack,
        name,
        url: URL,
        url_host: "hooks.slack.com",
        details: false,
        lang: "en",
    }
}

#[tokio::test]
async fn the_url_is_sealed_at_rest_and_opens_for_sending_only() {
    let (pool, agent) = pool_with_agent().await;
    let crypto = Crypto::from_session(&[3u8; 32]);
    let channel = create(&pool, &crypto, &agent, &new("ops"), "alice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(channel.url_host, "hooks.slack.com");

    let stored: Vec<u8> = sqlx::query_scalar("SELECT url_ct FROM agent_notify_channels")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!stored.windows(11).any(|w| w == b"secret-part"));
    let audit = agent_audit::for_principal(&pool, &agent).await.unwrap();
    assert!(
        audit
            .iter()
            .all(|e| !e.detail.to_string().contains("secret-part"))
    );

    let targets = targets(&pool, &crypto, &agent).await.unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].url, URL);
}

#[tokio::test]
async fn a_name_is_unique_per_agent_and_delete_is_scoped_to_it() {
    let (pool, agent) = pool_with_agent().await;
    let crypto = Crypto::from_session(&[3u8; 32]);
    let first = create(&pool, &crypto, &agent, &new("ops"), "alice")
        .await
        .unwrap()
        .unwrap();
    assert!(
        create(&pool, &crypto, &agent, &new("ops"), "alice")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !delete(&pool, "another-agent", &first.id, "alice")
            .await
            .unwrap()
    );
    assert!(delete(&pool, &agent, &first.id, "alice").await.unwrap());
    assert!(list(&pool, &agent).await.unwrap().is_empty());
}
