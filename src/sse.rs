// MCP HTTP + Server-Sent Events 传输层
//
// 模型：per-request 启动 wasmtime instance。
//   - 共享 Engine + Module（Arc）
//   - 每次 HTTP request = 新 Store + 新 wasi pipe (p2 MemoryInputPipe/MemoryOutputPipe)
//   - 一次性 instantiate + _start，单 stdin 消息处理 → 退出
//   - MemoryOutputPipe.contents() 拿 stdout
//
// 端点：
//   POST /messages | POST /  (Content-Type: application/json)
//       body 注入 wasm stdin → 收 stdout → 200 OK
//   GET  /sse
//       返回 server info JSON（不是真正的 SSE 事件流；MCP 调试用）
//   GET  /health
//       200 OK text/plain "ok"

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;
use wasmtime::{Engine, Module};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};

use crate::mcp::{register_host_imports, HostCtx, McpConfig};

const READ_TIMEOUT: Duration = Duration::from_secs(30);

pub fn run_mcp_sse(engine: Arc<Engine>, module: Arc<Module>, config: McpConfig) -> Result<()> {
    let listener = TcpListener::bind((config.host.as_str(), config.port))?;
    if !config.quiet {
        eprintln!("[mbit mcp] SSE 监听 http://{}:{}/  (POST /messages, GET /sse, GET /health)",
            config.host, config.port);
    }
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let e = Arc::clone(&engine);
                let m = Arc::clone(&module);
                let c = config.clone();
                std::thread::spawn(move || {
                    if let Err(err) = handle_conn(s, e, m, &c) {
                        if !c.quiet { eprintln!("[mbit mcp] connection error: {err}"); }
                    }
                });
            }
            Err(e) => {
                if !config.quiet { eprintln!("[mbit mcp] accept failed: {e}"); }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP/1.1 极简解析
// ---------------------------------------------------------------------------

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn parse_request(stream: &mut TcpStream) -> Result<Option<Request>> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 1024];
    let mut header_end = None;
    while header_end.is_none() {
        let n = stream.read(&mut tmp)?;
        if n == 0 { return Ok(None); }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_double_crlf(&buf) { header_end = Some(pos); }
        if buf.len() > 1_048_576 { return Err(anyhow!("request header too large")); }
    }
    let pos = header_end.unwrap();
    let header_str = std::str::from_utf8(&buf[..pos]).map_err(|_| anyhow!("invalid header encoding"))?;
    let mut lines = header_str.split("\r\n");
    let request_line = lines.next().ok_or_else(|| anyhow!("missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or_else(|| anyhow!("missing method"))?.to_string();
    let path = parts.next().ok_or_else(|| anyhow!("missing path"))?.to_string();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() { break; }
        if let Some(v) = line.strip_prefix("Content-Length:") { content_length = v.trim().parse().unwrap_or(0); }
    }
    let body_start = pos + 4;
    while buf.len() < body_start + content_length {
        let n = stream.read(&mut tmp)?;
        if n == 0 { break; }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > body_start + 8 * 1024 * 1024 {
            return Err(anyhow!("request body too large"));
        }
    }
    let end = body_start + content_length;
    let body = if buf.len() >= end { buf[body_start..end].to_vec() } else { buf[body_start..].to_vec() };
    Ok(Some(Request { method, path, body }))
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn write_response(stream: &mut TcpStream, status: u16, status_text: &str, content_type: &str, body: &[u8]) -> Result<()> {
    let header = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.set_write_timeout(Some(READ_TIMEOUT))?;
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 每条连接处理
// ---------------------------------------------------------------------------

fn handle_conn(mut stream: TcpStream, engine: Arc<Engine>, module: Arc<Module>, config: &McpConfig) -> Result<()> {
    let req = match parse_request(&mut stream)? {
        Some(r) => r,
        None => return Ok(()),
    };
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/health") => {
            write_response(&mut stream, 200, "OK", "text/plain", b"ok")?;
        }
        ("GET", "/sse") => {
            let info = r#"{"name":"mbit","version":"0.1.0","transport":"sse","hint":"POST JSON-RPC to /messages; GET /sse here is informational"}"#;
            write_response(&mut stream, 200, "OK", "application/json", info.as_bytes())?;
        }
        ("POST", "/messages") | ("POST", "/") => {
            let mut body = req.body;
            if body.is_empty() {
                let err = br#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"empty body"}}"#;
                write_response(&mut stream, 400, "Bad Request", "application/json", err)?;
                return Ok(());
            }
            // JSON-RPC 是 newline-delimited：末尾没换行就补一个（wasm 内部按行解析）
            if !body.ends_with(b"\n") { body.push(b'\n'); }
            match run_one_request(&engine, &module, &body, &config.bridge_command, config.quiet) {
                Ok(stdout_bytes) => {
                    write_response(&mut stream, 200, "OK", "application/json", &stdout_bytes)?;
                }
                Err(e) => {
                    let msg = format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"wasm error: {}"}}}}"#, e);
                    write_response(&mut stream, 500, "Internal Server Error", "application/json", msg.as_bytes())?;
                }
            }
        }
        _ => {
            let msg = br#"{"error":"not found; try POST /messages or GET /health"}"#;
            write_response(&mut stream, 404, "Not Found", "application/json", msg)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 跑一次 wasm：单 stdin 消息 → 完整 stdout 响应
// ---------------------------------------------------------------------------

fn run_one_request(
    engine: &Engine,
    module: &Module,
    body: &[u8],
    bridge_command: &str,
    quiet: bool,
) -> Result<Vec<u8>> {
    let stdin = MemoryInputPipe::new(body.to_vec());
    let stdout = MemoryOutputPipe::new(4 * 1024 * 1024);
    let stderr = MemoryOutputPipe::new(64 * 1024);

    let wasi = wasmtime_wasi::WasiCtxBuilder::new()
        .stdin(stdin)
        .stdout(stdout.clone())
        .stderr(stderr)
        .args(&["mbit-mcp-server"]).build_p1();
    // 把 request body 一次性塞进 host.stdin_buf，wasm 通过 host_read_stdin_chunk 读取
    let stdin_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(body.to_vec()));
    let host = HostCtx {
        wasi,
        bridge_command: bridge_command.to_string(),
        handles: Mutex::new(crate::mcp::HandleMap::new()),
        stdin_buf,
    };
    let mut linker: wasmtime::Linker<HostCtx> = wasmtime::Linker::new(engine);
    let mut store = wasmtime::Store::new(engine, host);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |h| &mut h.wasi)?;
    register_host_imports(&mut linker, &mut store)?;

    let instance = linker.instantiate(&mut store, module)?;
    let func_name = ["_start", "mcp", "run"].iter()
        .find(|n| instance.get_func(&mut store, n).is_some()).copied()
        .ok_or_else(|| anyhow!("wasm 既无 _start 也无 mcp()/run()"))?;
    if !quiet { eprintln!("[mbit mcp] POST /messages → wasm.{} ({} bytes body)", func_name, body.len()); }
    let func = instance.get_typed_func::<(), ()>(&mut store, func_name)?;
    func.call(&mut store, ())?;
    Ok(stdout.contents().to_vec())
}
