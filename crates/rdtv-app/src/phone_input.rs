//! "Type on your phone": while the TV's typing screen is open, a one-time web page on the
//! home network lets a phone type (or paste, e.g. from a password manager) the text.
//!
//! The only listener in the app, and deliberately narrow: it starts when the person asks
//! for it, serves one page at an unguessable random path (128 bits) on a random port,
//! accepts one submission of at most 2 KB, and stops after that, when the typing screen
//! closes, or after five minutes. Plain HTTP on the local network (a TV app has no
//! certificate a phone would trust), so it is meant for a home network you trust.

use rdtv_core::hbb_common::{
    rand,
    tokio::{
        self,
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    },
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

const LIFETIME: Duration = Duration::from_secs(300);
const MAX_REQUEST: usize = 8 * 1024;
const MAX_VALUE: usize = 2 * 1024;

pub struct PhoneInput {
    pub url: String,
    slot: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    /// The page was opened on a phone.
    pub opened: Arc<AtomicBool>,
}

/// The TV's address on the home network (the interface its default route uses; no packet
/// is sent).
fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).ok()?;
    s.connect(("192.0.2.1", 9)).ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

impl PhoneInput {
    pub fn start(rt: &tokio::runtime::Runtime, label: &str, secret: bool) -> Result<PhoneInput, String> {
        let ip = lan_ip().ok_or("the TV has no home network address")?;
        let std_listener = std::net::TcpListener::bind(SocketAddr::from((ip, 0)))
            .map_err(|e| format!("cannot open a port on the TV ({e})"))?;
        std_listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = std_listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = format!("{:032x}", rand::random::<u128>());
        let url = format!("http://{ip}:{port}/{token}");
        let slot = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let opened = Arc::new(AtomicBool::new(false));
        let page = page(label, secret);
        let (s2, st2, o2) = (slot.clone(), stop.clone(), opened.clone());
        rt.spawn(async move {
            let Ok(listener) = TcpListener::from_std(std_listener) else {
                return;
            };
            log::info!("phone input: listening on port {port}");
            let deadline = tokio::time::Instant::now() + LIFETIME;
            while !st2.load(Ordering::SeqCst) && tokio::time::Instant::now() < deadline {
                let Ok(Ok((mut sock, from))) =
                    tokio::time::timeout(Duration::from_millis(500), listener.accept()).await
                else {
                    continue;
                };
                let req = match tokio::time::timeout(Duration::from_secs(5), read_request(&mut sock))
                    .await
                {
                    Ok(Some(r)) => r,
                    _ => continue,
                };
                let (status, body) = match handle(&req, &token) {
                    Reply::Page => {
                        o2.store(true, Ordering::SeqCst);
                        ("200 OK", page.clone())
                    }
                    Reply::Value(v) => {
                        log::info!("phone input: text received from {}", from.ip());
                        if let Ok(mut g) = s2.lock() {
                            *g = Some(v);
                        }
                        st2.store(true, Ordering::SeqCst);
                        ("200 OK", done_page())
                    }
                    Reply::NotFound => ("404 Not Found", "Not found".to_owned()),
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
            log::info!("phone input: stopped");
        });
        Ok(PhoneInput {
            url,
            slot,
            stop,
            opened,
        })
    }

    /// Text sent from the phone, once.
    pub fn take(&self) -> Option<String> {
        self.slot.lock().ok().and_then(|mut g| g.take())
    }
}

impl Drop for PhoneInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

async fn read_request(sock: &mut tokio::net::TcpStream) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_REQUEST {
            return None;
        }
        if let Some(h) = find(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..h]).to_ascii_lowercase();
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if buf.len() >= h + 4 + len {
                break;
            }
        }
    }
    Some(buf)
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

#[derive(Debug, PartialEq)]
enum Reply {
    Page,
    Value(String),
    NotFound,
}

fn handle(req: &[u8], token: &str) -> Reply {
    let Some(h) = find(req, b"\r\n\r\n") else {
        return Reply::NotFound;
    };
    let head = String::from_utf8_lossy(&req[..h]);
    let mut parts = head.lines().next().unwrap_or("").split(' ');
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if path != format!("/{token}") {
        return Reply::NotFound;
    }
    match method {
        "GET" => Reply::Page,
        "POST" => {
            let body = String::from_utf8_lossy(&req[h + 4..]);
            let v = body
                .split('&')
                .find_map(|kv| kv.strip_prefix("v="))
                .map(form_decode)
                .unwrap_or_default();
            if v.is_empty() || v.len() > MAX_VALUE {
                Reply::Page
            } else {
                Reply::Value(v)
            }
        }
        _ => Reply::NotFound,
    }
}

/// `application/x-www-form-urlencoded` value: `+` is a space, `%XX` a byte, UTF-8.
fn form_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out)
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = "body{font-family:system-ui,sans-serif;background:#0a1222;color:#ecf1fa;margin:0;padding:24px}h1{font-size:22px}input{width:100%;box-sizing:border-box;font-size:20px;padding:14px;border-radius:10px;border:1px solid #33496f;background:#1b2842;color:#fff}button{margin-top:16px;width:100%;font-size:20px;padding:14px;border:0;border-radius:10px;background:#006fff;color:#fff}label{display:block;margin-top:12px;color:#a0afc8}";

fn page(label: &str, secret: bool) -> String {
    let ty = if secret { "password" } else { "text" };
    let show = if secret {
        r#"<label><input type="checkbox" style="width:auto" onchange="document.getElementById('v').type=this.checked?'text':'password'"> Show</label>"#
    } else {
        ""
    };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>RustCouch</title><style>{STYLE}</style></head><body><h1>RustCouch: {label}</h1><form method="post"><input id="v" name="v" type="{ty}" autofocus autocomplete="off" autocapitalize="off" spellcheck="false" maxlength="1024">{show}<button type="submit">Send to TV</button></form></body></html>"#,
        label = html_escape(label),
    )
}

fn done_page() -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>RustCouch</title><style>{STYLE}</style></head><body><h1>Sent to the TV.</h1><p>You can close this page. The link no longer works.</p></body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, path: &str, body: &str) -> Vec<u8> {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: tv\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn only_the_token_path_is_served() {
        let t = "abc123";
        assert_eq!(handle(&req("GET", "/abc123", ""), t), Reply::Page);
        assert_eq!(handle(&req("GET", "/", ""), t), Reply::NotFound);
        assert_eq!(handle(&req("GET", "/abc1234", ""), t), Reply::NotFound);
        assert_eq!(
            handle(&req("POST", "/abc123", "v=p%C3%A1ss+w%26rd%21"), t),
            Reply::Value("páss w&rd!".into())
        );
        assert_eq!(handle(&req("POST", "/abc123", "v="), t), Reply::Page);
    }

    #[test]
    fn form_values_are_decoded() {
        assert_eq!(form_decode("a%2Bb+c%25"), "a+b c%");
        assert_eq!(form_decode("bad%zz%4"), "bad%zz%4");
        assert_eq!(form_decode("line%0Abreak"), "linebreak");
    }

    #[test]
    fn label_is_escaped() {
        assert!(page("<x>", true).contains("&lt;x&gt;"));
        assert!(page("Password", true).contains(r#"type="password""#));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use std::io::{Read, Write};

    fn http(addr: &str, req: &str) -> String {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn page_then_one_submission() {
        if lan_ip().is_none() {
            return; // no network in this environment
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let p = PhoneInput::start(&rt, "Password", true).unwrap();
        let rest = p.url.strip_prefix("http://").unwrap();
        let (addr, path) = rest.split_once('/').unwrap();
        let get = http(addr, &format!("GET /{path} HTTP/1.1\r\nHost: tv\r\n\r\n"));
        assert!(get.starts_with("HTTP/1.1 200") && get.contains("Send to TV"));
        let wrong = http(addr, "GET /nope HTTP/1.1\r\nHost: tv\r\n\r\n");
        assert!(wrong.starts_with("HTTP/1.1 404"));
        let body = "v=a+long%21pass";
        let post = http(
            addr,
            &format!(
                "POST /{path} HTTP/1.1\r\nHost: tv\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(post.contains("Sent to the TV"));
        assert_eq!(p.take().as_deref(), Some("a long!pass"));
        // One submission only: the listener stops.
        std::thread::sleep(Duration::from_millis(800));
        assert!(std::net::TcpStream::connect(addr).is_err());
    }
}
