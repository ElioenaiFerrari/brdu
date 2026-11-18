//! Minimal HTTP client for the node and explorer APIs (no dependencies).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::Value;

/// A response: status code and JSON body (`Null` if the body is not JSON).
pub struct Reply {
    pub code: u16,
    pub body: Value,
}

fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&b[..eol])
                .trim()
                .split(';')
                .next()?,
            16,
        )
        .ok()?;
        b = &b[eol + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(b.get(..size)?);
        b = b.get(size + 2..)?;
    }
}

/// One request (`Connection: close`).
pub fn request(addr: &str, method: &str, path: &str, body: &str) -> Result<Reply, String> {
    let target = addr
        .to_socket_addrs()
        .map_err(|e| format!("{addr}: {e}"))?
        .next()
        .ok_or("no address")?;
    let mut s = TcpStream::connect_timeout(&target, Duration::from_secs(3))
        .map_err(|e| format!("{addr}: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| e.to_string())?;
    write!(s, "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("truncated response")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let code: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("invalid response")?;
    let payload = &raw[split + 4..];
    let payload = if head.contains("transfer-encoding: chunked") {
        dechunk(payload).ok_or("truncated chunk")?
    } else {
        payload.to_vec()
    };
    Ok(Reply {
        code,
        body: serde_json::from_slice(&payload).unwrap_or(Value::Null),
    })
}
