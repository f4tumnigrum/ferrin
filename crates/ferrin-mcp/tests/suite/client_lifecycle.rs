//! Client ownership and cancellation of background tasks.

use std::sync::Arc;
use std::time::Duration;

use ferrin_mcp::McpClient;
use ferrin_mcp::protocol::JsonRpcMessage;
use tokio::sync::Notify;

use super::common::MockTransport;
use super::common::config;
use super::common::connect;
use super::common::discover_result;
use super::common::discovery_capabilities;
use super::common::reply;
use super::common::reply_error;

struct DropNotice(Arc<Notify>);

impl Drop for DropNotice {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

fn observed_transport(notify: &Arc<Notify>, fail: bool) -> Arc<MockTransport> {
    let notice = DropNotice(Arc::clone(notify));
    MockTransport::new(discovery_capabilities(), move |message| {
        let _keep_alive = &notice;
        if matches!(message, JsonRpcMessage::Request(_)) {
            if fail {
                Ok(vec![reply_error(message, -32022, "unsupported version")])
            } else {
                Ok(vec![reply(message, discover_result())])
            }
        } else {
            Ok(Vec::new())
        }
    })
}

#[tokio::test]
async fn final_client_drop_releases_transport_and_dispatcher() {
    let dropped = Arc::new(Notify::new());
    let transport = observed_transport(&dropped, false);
    let weak = Arc::downgrade(&transport);
    let client = connect(Arc::clone(&transport)).await;
    let clone = client.clone();
    drop(transport);
    drop(client);
    assert!(weak.upgrade().is_some());
    drop(clone);
    tokio::time::timeout(Duration::from_secs(5), dropped.notified())
        .await
        .unwrap();
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn explicit_close_releases_dispatcher_when_handles_drop() {
    let dropped = Arc::new(Notify::new());
    let transport = observed_transport(&dropped, false);
    let client = connect(Arc::clone(&transport)).await;
    client.close().await.unwrap();
    assert!(transport.is_closed());
    drop(client);
    drop(transport);
    tokio::time::timeout(Duration::from_secs(5), dropped.notified())
        .await
        .unwrap();
}

#[tokio::test]
async fn failed_connection_closes_and_releases_transport() {
    let dropped = Arc::new(Notify::new());
    let transport = observed_transport(&dropped, true);
    assert!(
        McpClient::connect(config(Arc::clone(&transport)))
            .await
            .is_err()
    );
    assert!(transport.is_closed());
    drop(transport);
    tokio::time::timeout(Duration::from_secs(5), dropped.notified())
        .await
        .unwrap();
}
