//! Minimal bounded HTTP/1.1 client for the RustDesk API server (accounts, address book).
//! HTTPS uses rustls with the bundled webpki roots; certificates are always verified.

use hbb_common::{
    anyhow::{anyhow, bail, Context},
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        time::{timeout, Duration},
    },
    ResultType,
};
use std::sync::Arc;
use tokio_rustls::{
    rustls::{self, pki_types::ServerName},
    TlsConnector,
};

const MAX_BODY: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);

pub struct Response {
    pub status: u16,
    pub body: String,
}

trait Io: hbb_common::tokio::io::AsyncRead + hbb_common::tokio::io::AsyncWrite + Unpin + Send {}
impl<T: hbb_common::tokio::io::AsyncRead + hbb_common::tokio::io::AsyncWrite + Unpin + Send> Io for T {}

fn tls_connector() -> TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("tls versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(cfg))
}

pub async fn request(
    method: &str,
    url_s: &str,
    bearer: Option<&str>,
    json_body: Option<&str>,
) -> ResultType<Response> {
    timeout(TIMEOUT, request_inner(method, url_s, bearer, json_body))
        .await
        .map_err(|_| anyhow!("request timed out"))?
}

async fn request_inner(
    method: &str,
    url_s: &str,
    bearer: Option<&str>,
    json_body: Option<&str>,
) -> ResultType<Response> {
    let u = url::Url::parse(url_s).context("invalid API server URL")?;
    let host = u.host_str().context("API URL has no host")?.to_owned();
    let https = match u.scheme() {
        "https" => true,
        "http" => false,
        s => bail!("unsupported scheme {s}"),
    };
    let port = u.port_or_known_default().unwrap_or(if https { 443 } else { 80 });
    let tcp = TcpStream::connect((host.as_str(), port)).await?;
    let mut io: Box<dyn Io> = if https {
        let name = ServerName::try_from(host.clone()).map_err(|_| anyhow!("invalid TLS server name"))?;
        Box::new(tls_connector().connect(name, tcp).await?)
    } else {
        Box::new(tcp)
    };
    let mut path = u.path().to_owned();
    if let Some(q) = u.query() {
        path.push('?');
        path.push_str(q);
    }
    let body = json_body.unwrap_or("");
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: RemoteDeskTV\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if let Some(t) = bearer {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    if json_body.is_some() || method == "POST" {
        req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    io.write_all(req.as_bytes()).await?;
    io.write_all(body.as_bytes()).await?;
    io.flush().await?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        let n = io.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > MAX_BODY {
            bail!("response too large");
        }
    }
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> ResultType<Response> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("malformed HTTP response")?;
    let head = std::str::from_utf8(&raw[..split])?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .context("bad status line")?;
    let mut chunked = false;
    for l in lines {
        let lower = l.to_ascii_lowercase();
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }
    let mut body = raw[split + 4..].to_vec();
    if chunked {
        body = dechunk(&body)?;
    }
    Ok(Response {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn dechunk(mut b: &[u8]) -> ResultType<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n").context("bad chunk")?;
        let size_s = std::str::from_utf8(&b[..eol])?.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_s, 16).context("bad chunk size")?;
        b = &b[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size {
            bail!("truncated chunk");
        }
        out.extend_from_slice(&b[..size]);
        b = &b[size..];
        if b.starts_with(b"\r\n") {
            b = &b[2..];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_chunked() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!((r.status, r.body.as_str()), (200, "{}"));
        let r = parse_response(b"HTTP/1.1 401 X\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!((r.status, r.body.as_str()), (401, "abcde"));
        assert!(parse_response(b"garbage").is_err());
    }
}
