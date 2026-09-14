use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::Args;

use crate::build::{self, BuildArgs};
use crate::term;
use crate::ws::{self, Frame, Hub};

#[derive(Args)]
pub struct ServeArgs {
    /// Source directory containing templates
    #[arg(long, default_value = ".")]
    pub input: String,

    /// Directory to write the generated site into
    #[arg(long, default_value = ".dreamfish/build")]
    pub output: String,

    /// Port to listen on
    #[arg(long, default_value_t = 8022)]
    pub port: u16,
}

pub fn run(args: ServeArgs) -> Result<(), String> {
    let build_args = BuildArgs {
        input: args.input.clone(),
        output: args.output.clone(),
    };
    build::run(build_args.clone())?;

    let hub = Arc::new(Hub::new());
    let watch_hub = Arc::clone(&hub);
    let output = args.output.clone();
    thread::spawn(move || watch_loop(args.input, args.output, build_args, watch_hub));

    let listener = TcpListener::bind(("127.0.0.1", args.port))
        .map_err(|e| format!("cannot listen on port {}: {e}", args.port))?;

    println!("\n===== live site =====");
    println!("{}", term::blue(&format!("http://localhost:{}", args.port)));
    println!("{}", term::gray("Press Ctrl+C to stop"));
    println!("=====================\n");

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let hub = Arc::clone(&hub);
        let output = PathBuf::from(&output);
        thread::spawn(move || handle(stream, hub, output));
    }
    Ok(())
}

fn watch_loop(input: String, output: String, build_args: BuildArgs, hub: Arc<Hub>) {
    let input = PathBuf::from(&input);
    let output = PathBuf::from(&output);
    let mut last = snapshot(&input, &output);
    loop {
        thread::sleep(Duration::from_millis(300));
        let now = snapshot(&input, &output);
        if now == last {
            continue;
        }
        last = now;
        println!("\n{}", term::gray("Change detected, rebuilding..."));
        match build::run(build_args.clone()) {
            Ok(()) => hub.reload(),
            Err(e) => eprintln!("{}: {e}", term::red("rebuild failed")),
        }
    }
}

fn snapshot(input: &Path, output: &Path) -> BTreeMap<PathBuf, (i64, u32, u64)> {
    let mut map = BTreeMap::new();
    let out = build::abs_norm(output);
    collect_snapshot(input, input, &out, &mut map);
    map
}

fn collect_snapshot(
    root: &Path,
    dir: &Path,
    out: &Path,
    map: &mut BTreeMap<PathBuf, (i64, u32, u64)>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if build::abs_norm(&path).starts_with(out) {
                continue;
            }
            collect_snapshot(root, &path, out, map);
        } else {
            let key = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let Ok(meta) = fs::metadata(&path) else { continue };
            let (secs, nanos) = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| (d.as_secs() as i64, d.subsec_nanos()))
                .unwrap_or((0, 0));
            map.insert(key, (secs, nanos, meta.len()));
        }
    }
}

fn handle(mut stream: TcpStream, hub: Arc<Hub>, output: PathBuf) {
    let mut buf = [0u8; 8192];
    let mut used = 0usize;
    loop {
        match stream.read(&mut buf[used..]) {
            Ok(0) => return,
            Ok(n) => {
                used += n;
                if used == buf.len() || buf[..used].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return,
        }
    }
    let head = String::from_utf8_lossy(&buf[..used]);
    let request_line = head.lines().next().unwrap_or("").trim();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let head_only = method == "HEAD";
    if method != "GET" && method != "HEAD" {
        write_response(
            &mut stream,
            "HTTP/1.1 405 Method Not Allowed\r\n",
            "text/plain; charset=utf-8",
            b"405 Method Not Allowed",
            head_only,
        );
        return;
    }
    let path = target.split(['?', '#']).next().unwrap_or("/");
    if path == "/__dreamfish__ws" {
        handle_ws(stream, &hub, &head);
        return;
    }

    let rel = percent_decode(path.trim_start_matches('/'));
    if rel.split('/').any(|c| c == "..") || rel.contains('\0') {
        write_response(
            &mut stream,
            "HTTP/1.1 403 Forbidden\r\n",
            "text/plain; charset=utf-8",
            b"403 Forbidden",
            head_only,
        );
        return;
    }
    let out = build::abs_norm(&output);
    let candidate = out.join(&rel);
    if !candidate.starts_with(&out) {
        write_response(
            &mut stream,
            "HTTP/1.1 403 Forbidden\r\n",
            "text/plain; charset=utf-8",
            b"403 Forbidden",
            head_only,
        );
        return;
    }
    let file = if candidate.is_dir() {
        candidate.join("index.html")
    } else {
        candidate
    };
    match fs::read(&file) {
        Ok(mut data) => {
            let ctype = content_type(&file);
            if ctype.starts_with("text/html") {
                let html = String::from_utf8_lossy(&data);
                data = inject_reload(&html);
            }
            write_response(&mut stream, "HTTP/1.1 200 OK\r\n", ctype, &data, head_only);
        }
        Err(_) => write_response(
            &mut stream,
            "HTTP/1.1 404 Not Found\r\n",
            "text/plain; charset=utf-8",
            b"404 Not Found",
            head_only,
        ),
    }
}

fn handle_ws(mut stream: TcpStream, hub: &Arc<Hub>, head: &str) {
    if !head.to_ascii_lowercase().contains("upgrade: websocket") {
        write_response(
            &mut stream,
            "HTTP/1.1 400 Bad Request\r\n",
            "text/plain; charset=utf-8",
            b"400 Bad Request",
            false,
        );
        return;
    }
    let Some(key) = ws::extract_key(head) else {
        write_response(
            &mut stream,
            "HTTP/1.1 400 Bad Request\r\n",
            "text/plain; charset=utf-8",
            b"400 Bad Request",
            false,
        );
        return;
    };
    let response = ws::handshake_response(&key);
    if stream.write_all(&response).is_err() {
        return;
    }
    let reader = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let handle = match hub.connect(reader) {
        Ok(h) => h,
        Err(_) => return,
    };
    loop {
        match ws::read_frame(&mut stream) {
            Ok(Some(Frame::Ping(payload))) => {
                if hub.send(&handle, &ws::pong_frame(&payload)).is_err() {
                    break;
                }
            }
            Ok(Some(Frame::Close(_))) => {
                let _ = hub.send(&handle, &ws::close_frame(1000));
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    hub.remove(&handle);
}

fn write_response(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8], head_only: bool) {
    let mut out = Vec::new();
    out.extend_from_slice(status.as_bytes());
    out.extend_from_slice(format!("Content-Type: {ctype}\r\n").as_bytes());
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    out.extend_from_slice(b"Cache-Control: no-store\r\nConnection: close\r\n\r\n");
    if !head_only {
        out.extend_from_slice(body);
    }
    let _ = stream.write_all(&out);
}

const RELOAD_SCRIPT: &str = r#"<script>
(function(){
  var last = null;
  var ws = null;
  function connect(){
    ws = new WebSocket('ws://'+location.host+'/__dreamfish__ws');
    ws.onmessage = function(e){
      var j; try { j = JSON.parse(e.data); } catch (_) { return; }
      if (j.type === 'hello') { if (last === null) last = j.version; return; }
      if (j.type === 'reload' && j.version !== last) { last = j.version; location.reload(); }
    };
    ws.onclose = function(){ ws = null; setTimeout(connect, 1000); };
  }
  connect();
})();
</script>"#;

fn inject_reload(html: &str) -> Vec<u8> {
    let lower = html.to_ascii_lowercase();
    match lower.rfind("</body>") {
        Some(i) => {
            let mut out = String::with_capacity(html.len() + RELOAD_SCRIPT.len() + 16);
            out.push_str(&html[..i]);
            out.push_str(RELOAD_SCRIPT);
            out.push_str("</body>");
            out.push_str(&html[i + "</body>".len()..]);
            out.into_bytes()
        }
        None => format!("{html}{RELOAD_SCRIPT}").into_bytes(),
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" | "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() && hex(bytes[i + 1]).is_some() && hex(bytes[i + 2]).is_some() {
            out.push(hex(bytes[i + 1]).unwrap() * 16 + hex(bytes[i + 2]).unwrap());
            i += 3;
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}