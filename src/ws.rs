use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
pub const MAX_FRAME: u64 = 1024 * 1024;

pub fn accept(key: &str) -> String {
    let mut data = format!("{key}{GUID}").into_bytes();
    let bit_len = (data.len() as u64) * 8;
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut digest = [0u8; 20];
    for (i, &h) in [h0, h1, h2, h3, h4].iter().enumerate() {
        digest[i * 4..i * 4 + 4].copy_from_slice(&h.to_be_bytes());
    }
    base64(&digest)
}

fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | (chunk.get(2).copied().unwrap_or(0) as u32);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn handshake_response(key: &str) -> Vec<u8> {
    let accept = accept(key);
    format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    )
    .into_bytes()
}

pub fn extract_key(head: &str) -> Option<String> {
    for line in head.lines() {
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case("Sec-WebSocket-Key")
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

pub fn text_frame(payload: &str) -> Vec<u8> {
    frame(0x1, payload.as_bytes())
}

pub fn pong_frame(payload: &[u8]) -> Vec<u8> {
    frame(0xA, payload)
}

pub fn close_frame(code: u16) -> Vec<u8> {
    frame(0x8, &code.to_be_bytes())
}

pub fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u64;
    let mut out = vec![0x80 | opcode];
    if len < 126 {
        out.push(len as u8);
    } else if len <= 0xFFFF {
        out.push(126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&len.to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

pub enum Frame {
    Text(Vec<u8>),
    Binary(Vec<u8>),
    Continuation,
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(u16),
}

pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut header = [0u8; 2];
    if !read_eof(r, &mut header)? {
        return Ok(None);
    }
    let opcode = header[0] & 0x0F;
    let masked = header[1] & 0x80 != 0;
    let mut len = (header[1] & 0x7F) as u64;
    if len == 126 {
        let mut b = [0u8; 2];
        r.read_exact(&mut b)?;
        len = u16::from_be_bytes(b) as u64;
    } else if len == 127 {
        let mut b = [0u8; 8];
        r.read_exact(&mut b)?;
        len = u64::from_be_bytes(b);
    }
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "websocket frame too large",
        ));
    }
    let mut mask = [0u8; 4];
    if masked {
        r.read_exact(&mut mask)?;
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    let frame = match opcode {
        0x8 => {
            let code = if payload.len() >= 2 {
                u16::from_be_bytes([payload[0], payload[1]])
            } else {
                1000
            };
            Frame::Close(code)
        }
        0x9 => Frame::Ping(payload),
        0xA => Frame::Pong(payload),
        0x1 => Frame::Text(payload),
        0x2 => Frame::Binary(payload),
        0x0 => Frame::Continuation,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported opcode {opcode}"),
            ));
        }
    };
    Ok(Some(frame))
}

fn read_eof(r: &mut impl Read, buf: &mut [u8]) -> io::Result<bool> {
    let mut read = 0;
    while read < buf.len() {
        match r.read(&mut buf[read..]) {
            Ok(0) => return Ok(false),
            Ok(n) => read += n,
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

pub struct Hub {
    clients: Mutex<Vec<Arc<Mutex<TcpStream>>>>,
    version: AtomicU64,
}

impl Hub {
    pub fn new() -> Self {
        Hub {
            clients: Mutex::new(Vec::new()),
            version: AtomicU64::new(1),
        }
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    pub fn connect(&self, stream: TcpStream) -> io::Result<Arc<Mutex<TcpStream>>> {
        let handle = Arc::new(Mutex::new(stream));
        self.clients.lock().unwrap().push(handle.clone());
        let hello = format!(r#"{{"type":"hello","version":{}}}"#, self.version());
        self.send(&handle, &text_frame(&hello))?;
        Ok(handle)
    }

    pub fn send(&self, handle: &Arc<Mutex<TcpStream>>, bytes: &[u8]) -> io::Result<()> {
        handle.lock().unwrap().write_all(bytes)
    }

    pub fn remove(&self, handle: &Arc<Mutex<TcpStream>>) {
        let mut clients = self.clients.lock().unwrap();
        if let Some(pos) = clients.iter().position(|h| Arc::ptr_eq(h, handle)) {
            clients.remove(pos);
        }
    }

    pub fn reload(&self) {
        let version = self.version.fetch_add(1, Ordering::Relaxed) + 1;
        let msg = format!(r#"{{"type":"reload","version":{version}}}"#);
        self.broadcast(&text_frame(&msg));
    }

    fn broadcast(&self, bytes: &[u8]) {
        let mut clients = self.clients.lock().unwrap();
        let mut dead = Vec::new();
        for handle in clients.iter() {
            let mut stream = match handle.lock() {
                Ok(s) => s,
                Err(_) => {
                    dead.push(handle.clone());
                    continue;
                }
            };
            if stream.write_all(bytes).is_err() {
                drop(stream);
                dead.push(handle.clone());
            }
        }
        if !dead.is_empty() {
            clients.retain(|h| !dead.iter().any(|d| Arc::ptr_eq(d, h)));
        }
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_accept_matches_rfc_example() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        assert_eq!(accept(key), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn frame_building_small_and_extended() {
        assert_eq!(text_frame("hi"), vec![0x81, 2, b'h', b'i']);
        let big = [0u8; 200];
        let f = frame(0x1, &big);
        assert_eq!(f[0], 0x81);
        assert_eq!(f[1], 126);
        assert_eq!(u16::from_be_bytes([f[2], f[3]]), 200);
        assert_eq!(f.len(), 204);
    }
}