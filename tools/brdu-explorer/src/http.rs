//! Minimal HTTP client (no dependencies) for talking to a node's API.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::Value;

/// GET returning JSON (requires 200). Understands `Content-Length` and `chunked`.
pub fn get_json(addr: &str, path: &str) -> Result<Value, String> {
    let target = addr
        .to_socket_addrs()
        .map_err(|e| format!("{addr}: {e}"))?
        .next()
        .ok_or("no address")?;
    let mut s = TcpStream::connect_timeout(&target, Duration::from_secs(3))
        .map_err(|e| format!("{addr}: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| e.to_string())?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
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
    let body = &raw[split + 4..];
    let body = if head.contains("transfer-encoding: chunked") {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    if code != 200 {
        return Err(format!("{path}: HTTP {code}"));
    }
    serde_json::from_slice(&body).map_err(|e| format!("{path}: invalid JSON ({e})"))
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = b
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("truncated chunk")?;
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&b[..eol])
                .trim()
                .split(';')
                .next()
                .unwrap_or("0"),
            16,
        )
        .map_err(|_| "invalid chunk")?;
        b = &b[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(b.get(..size).ok_or("truncated chunk")?);
        b = b.get(size + 2..).ok_or("truncated chunk")?;
    }
}
