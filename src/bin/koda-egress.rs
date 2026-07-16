//! Small HTTPS CONNECT allowlist proxy for the isolated harness network.
//! It intentionally supports only CONNECT; ordinary HTTP requests are denied.
use std::{env, sync::Arc};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpListener, TcpStream}};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = env::var("KODA_EGRESS_BIND").unwrap_or_else(|_| "0.0.0.0:3128".into());
    let allowed: Arc<Vec<String>> = Arc::new(env::var("KODA_EGRESS_ALLOWLIST").unwrap_or_default().split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_ascii_lowercase).collect());
    let listener = TcpListener::bind(&bind).await?;
    loop { let (stream, _) = listener.accept().await?; let allowed = allowed.clone(); tokio::spawn(async move { let _ = handle(stream, allowed).await; }); }
}

async fn handle(mut client: TcpStream, allowed: Arc<Vec<String>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut line = String::new(); { let mut reader = BufReader::new(&mut client); reader.read_line(&mut line).await?; }
    let mut parts = line.split_whitespace();
    if parts.next() != Some("CONNECT") { client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?; return Ok(()); }
    let target = parts.next().unwrap_or(""); let host = target.split(':').next().unwrap_or("").to_ascii_lowercase();
    if !allowed.iter().any(|rule| host == *rule || host.ends_with(&format!(".{rule}"))) { client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?; return Ok(()); }
    let upstream = TcpStream::connect(target).await?;
    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
    let (mut cr, mut cw) = client.into_split(); let (mut ur, mut uw) = upstream.into_split();
    tokio::try_join!(tokio::io::copy(&mut cr, &mut uw), tokio::io::copy(&mut ur, &mut cw))?;
    Ok(())
}
