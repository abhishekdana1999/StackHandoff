//! Live probe: is the *running* app actually serving its transfer listener?
//!
//! Why this exists. The unit tests build their own listener, so every one of
//! them passes whether or not the real app binds, serves, and routes a
//! connection. That distinction is not theoretical: the receive module's own
//! header records a build where the listener was bound and its port advertised,
//! and nothing ever called `accept_once`. A peer connected, the connection sat
//! in the kernel's accept queue, and the app was silently incapable of
//! receiving. A port being bound proves nothing about it being served.
//!
//! The probe distinguishes the two without needing a key. It connects, sends
//! bytes that cannot be a valid Noise handshake, and waits for the app to close
//! the connection:
//!
//! - Served: the accept loop reads the garbage, fails the handshake, and drops
//!   the connection. The probe sees EOF well inside `HANDSHAKE_TIMEOUT`.
//! - Bound but unserved: nothing ever reads the socket. The bytes sit in the
//!   kernel buffer and the connection stays open until the probe gives up.
//!
//! So EOF is the positive signal, and a timeout is the failure. This cannot
//! prove a transfer succeeds end to end -- that needs a paired peer and a
//! workspace -- but it does prove the part that has silently failed before.
//!
//! Exits 0 on success. Reads the port from `WC_PROBE_PORT`.

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Longer than the app's own handshake timeout, so a served listener has had
/// time to give up before the probe does.
const PROBE_TIMEOUT_SECS: u64 = 25;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("WC_PROBE_PORT")
        .expect("set WC_PROBE_PORT to the app's bound transfer port")
        .parse()
        .expect("WC_PROBE_PORT must be a number");
    let addr = format!("127.0.0.1:{port}");

    let mut stream = tokio::net::TcpStream::connect(&addr)
        .await
        .unwrap_or_else(|e| panic!("could not connect to the app's listener on {addr}: {e}"));
    println!("connected to {addr}");

    // Not a valid Noise handshake message. A serving loop reads it, fails, and
    // closes; a loop that never accepts leaves it unread.
    stream.write_all(b"not-a-noise-handshake").await.expect("write");
    stream.flush().await.ok();

    let mut buf = [0u8; 64];
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(PROBE_TIMEOUT_SECS),
        stream.read(&mut buf),
    )
    .await;

    match outcome {
        Ok(Ok(0)) => {
            println!("the app closed the connection: the accept loop is serving");
            println!("PASS: the running app serves its transfer listener");
        }
        Ok(Ok(n)) => {
            // Unexpected, but not a failure: the app may have sent something
            // before closing. Either way it spoke, so it is serving.
            println!("the app sent {n} bytes before closing: the accept loop is serving");
            println!("PASS: the running app serves its transfer listener");
        }
        Ok(Err(e)) => panic!("read failed against the app's listener: {e}"),
        Err(_) => {
            panic!(
                "the connection to {addr} stayed open for {PROBE_TIMEOUT_SECS}s and was never \
                 read. The port is bound but nothing is accepting on it, which is the failure \
                 this probe exists to catch."
            )
        }
    }
}
