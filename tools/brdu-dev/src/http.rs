//! Cliente HTTP mínimo (sem dependências): `Connection: close`, JSON.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::Value;

/// Faz uma requisição e devolve `(código, corpo JSON)`.
pub fn request(addr: &str, method: &str, path: &str, body: &str) -> Result<(u16, Value), String> {
    let target = addr
        .to_socket_addrs()
        .map_err(|e| format!("{addr}: {e}"))?
        .next()
        .ok_or("sem endereço")?;
    let mut s = TcpStream::connect_timeout(&target, Duration::from_secs(3))
        .map_err(|e| format!("{addr}: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| e.to_string())?;
    write!(s, "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("resposta inválida")?;
    let body = out.split("\r\n\r\n").nth(1).unwrap_or("");
    Ok((code, serde_json::from_str(body).unwrap_or(Value::Null)))
}

/// GET que exige 200.
pub fn get(addr: &str, path: &str) -> Result<Value, String> {
    match request(addr, "GET", path, "")? {
        (200, v) => Ok(v),
        (c, v) => Err(format!("{path}: HTTP {c} {v}")),
    }
}

/// Conexão HTTP persistente (keep-alive): o teste de carga não pode esgotar portas efêmeras.
pub struct Conn {
    addr: String,
    stream: Option<std::net::TcpStream>,
}

impl Conn {
    /// Nova (conecta na primeira requisição).
    #[must_use]
    pub fn new(addr: &str) -> Self {
        Self {
            addr: addr.to_string(),
            stream: None,
        }
    }

    fn connect(&mut self) -> Result<(), String> {
        let target = self
            .addr
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("sem endereço")?;
        let s = TcpStream::connect_timeout(&target, Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        s.set_nodelay(true).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(20)))
            .map_err(|e| e.to_string())?;
        self.stream = Some(s);
        Ok(())
    }

    /// Uma requisição; reconecta uma vez se a conexão caiu.
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        body: &str,
    ) -> Result<(u16, String), String> {
        for attempt in 0..2 {
            if self.stream.is_none() {
                self.connect()?;
            }
            match self.try_once(method, path, body) {
                Ok(r) => return Ok(r),
                Err(e) => {
                    self.stream = None;
                    if attempt == 1 {
                        return Err(e);
                    }
                }
            }
        }
        Err("falhou".into())
    }

    fn try_once(&mut self, method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
        let s = self.stream.as_mut().ok_or("sem conexão")?;
        write!(
            s,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\n\r\n{body}",
            self.addr,
            body.len()
        )
        .map_err(|e| e.to_string())?;
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > 16 * 1024 || s.read(&mut byte).map_err(|e| e.to_string())? == 0 {
                return Err("resposta truncada".into());
            }
            head.push(byte[0]);
        }
        let text = String::from_utf8_lossy(&head).to_string();
        let code = text
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .ok_or("resposta inválida")?;
        let lower = text.to_ascii_lowercase();
        let body = if lower.contains("transfer-encoding: chunked") {
            // o tiny_http responde em pedaços quando o corpo passa de ~32 KB
            let mut out = Vec::new();
            loop {
                let mut line = Vec::new();
                while !line.ends_with(b"\r\n") {
                    if s.read(&mut byte).map_err(|e| e.to_string())? == 0 {
                        return Err("chunk truncado".into());
                    }
                    line.push(byte[0]);
                }
                let size = usize::from_str_radix(
                    String::from_utf8_lossy(&line)
                        .trim()
                        .split(';')
                        .next()
                        .unwrap_or("0"),
                    16,
                )
                .map_err(|_| "chunk inválido")?;
                let mut chunk = vec![0u8; size + 2];
                s.read_exact(&mut chunk).map_err(|e| e.to_string())?;
                if size == 0 {
                    break;
                }
                out.extend_from_slice(&chunk[..size]);
            }
            out
        } else {
            let len = lower
                .lines()
                .find_map(|l| {
                    l.strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let mut body = vec![0u8; len];
            s.read_exact(&mut body).map_err(|e| e.to_string())?;
            body
        };
        Ok((code, String::from_utf8_lossy(&body).to_string()))
    }
}
