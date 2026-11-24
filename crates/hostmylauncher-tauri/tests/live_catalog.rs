//! Live catalog over a local server that speaks both REST and WebSocket.
use futures_util::{SinkExt, StreamExt};
use hostmylauncher::Client;
use hostmylauncher_tauri::{contract::*, Bridge};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

mod common;

fn instance(id: &str, title: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "launcher_id": "catalog", "title": title})
}

/// Answers the list and single-instance reads; hands each WebSocket to the test.
async fn serve(listener: TcpListener, sockets: mpsc::UnboundedSender<WebSocketStream<TcpStream>>) {
    loop {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = [0; 2048];
        let read = stream.peek(&mut head).await.unwrap();
        let request = String::from_utf8_lossy(&head[..read]).to_ascii_lowercase();
        if request.contains("upgrade: websocket") {
            let socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _ = sockets.send(socket);
            continue;
        }
        let _ = stream.read(&mut head).await.unwrap();
        let body = if request.starts_with("get /launchers/catalog/instances/b ") {
            serde_json::json!({"data": instance("b", "B")})
        } else {
            serde_json::json!({"results": [instance("a", "A")]})
        }
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    }
}

fn ids(instances: &[InstanceDto]) -> Vec<&str> {
    instances.iter().map(|i| i.id.as_str()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watched_catalog_follows_events_and_stopping_closes_the_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (socket_tx, mut sockets) = mpsc::unbounded_channel();
    tokio::spawn(serve(listener, socket_tx));

    let bridge = Bridge::new(
        Client::with_base_url(&base).unwrap(),
        vec!["catalog".into()],
        common::located(vec![]),
        Arc::new(|_| {}),
    )
    .unwrap();
    let (snapshot_tx, mut snapshots) = mpsc::unbounded_channel();
    bridge.add_catalog_sink(Arc::new(move |snapshot| {
        let _ = snapshot_tx.send(snapshot);
    }));

    let initial = bridge
        .watch_catalog(WatchCatalogRequest {
            launcher_id: Some("catalog".into()),
        })
        .await
        .unwrap();
    assert_eq!(ids(&initial), ["a"]);

    let wait = Duration::from_secs(5);
    let mut socket = tokio::time::timeout(wait, sockets.recv())
        .await
        .unwrap()
        .unwrap();
    socket
        .send(Message::Text(
            r#"{"type":"instance.created","revision":1,"payload":{"id":"b"}}"#.into(),
        ))
        .await
        .unwrap();
    let snapshot = tokio::time::timeout(wait, snapshots.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.launcher_id, "catalog");
    assert_eq!(ids(&snapshot.instances), ["a", "b"]);

    let stopped = bridge
        .watch_catalog(WatchCatalogRequest { launcher_id: None })
        .await
        .unwrap();
    assert!(stopped.is_empty());
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match socket.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the previous provider's socket stayed open");
}
