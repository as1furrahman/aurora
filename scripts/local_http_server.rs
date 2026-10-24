use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

const MAX_REQUEST_BYTES: usize = 16 * 1024;

fn response(stream: &mut TcpStream, status: &str, body: &[u8], include_body: bool) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if include_body {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn requested_file(root: &Path, target: &str) -> Option<PathBuf> {
    let path = target.strip_prefix('/')?;
    if path.is_empty() || path.contains(['?', '#', '%']) {
        return None;
    }
    let relative = Path::new(path);
    if relative.components().any(|component| !matches!(component, Component::Normal(_))) {
        return None;
    }
    let file = root.join(relative).canonicalize().ok()?;
    (file.starts_with(root) && file.is_file()).then_some(file)
}

fn serve(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0; 1024];
    while request.len() < MAX_REQUEST_BYTES {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(());
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            break;
        }
    }

    let request_line = request.split(|byte| *byte == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(request_line).trim_end_matches('\r').to_string();
    let mut words = line.split_whitespace();
    let method = words.next().unwrap_or_default();
    let target = words.next().unwrap_or_default();
    if !matches!(method, "GET" | "HEAD") {
        return response(&mut stream, "405 Method Not Allowed", b"method not allowed\n", true);
    }

    let Some(file) = requested_file(root, target) else {
        return response(&mut stream, "404 Not Found", b"not found\n", method == "GET");
    };
    let body = fs::read(file)?;
    response(&mut stream, "200 OK", &body, method == "GET")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("usage: local-http-server ROOT ADDRESS")?).canonicalize()?;
    let address = args.next().ok_or("usage: local-http-server ROOT ADDRESS")?;
    let address = address.to_string_lossy().parse::<std::net::SocketAddr>()?;
    let listener = TcpListener::bind(address)?;
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                if let Err(error) = serve(stream, &root) {
                    eprintln!("request failed: {error}");
                }
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }
    Ok(())
}
