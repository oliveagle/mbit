// MCP (Model Context Protocol) 主机模块
//
// 在 wasmtime 上加载 MoonBit WASM (MVP/wasm-gc) 作为 MCP 服务器运行。
// 支持 stdio 和 SSE 两种传输模式（当前 SSE 简化实现 = 阻塞 stdin）。
//
// host imports 严格按 br-mcp-server-mbt 实际导出的符号注册（extern ABI）：
//   env.host_begin_create_string     ()          -> externref
//   env.host_string_append_char      (ext, i32)  -> ()
//   env.host_finish_create_string    (ext)       -> externref
//   env.host_begin_read_string       (ext)       -> externref
//   env.host_string_read_char        (ext)       -> i32
//   env.host_finish_read_string      (ext)       -> ()
//   env.host_br_version              ()          -> i32
//   env.host_br_path                 ()          -> externref
//   env.host_br_run                  (ext)       -> externref
//   env.host_read_stdin_chunk        (i32)       -> externref
//   __moonbit_time_unstable.now      ()          -> i64
//
// wasi_snapshot_preview1.fd_write 由 wasmtime-wasi p1 提供。
// 入口 = wasm 导出 _start (()) -> ()。
//
// 句柄传递：MoonBit 编译产物中 opaque extern 类型降级为 externref。
// wasmtime 46 的 Rooted<ExternRef> 没有 WasmRet 实现，因此用
// Func::new + Val 数组形式手动注册 host imports，用 ExternRef::to_raw
// 拿 u32 句柄值，绑定到 HostCtx.handles<u32, Arc<Mutex<Handle>>>。

use anyhow::Result;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wasmtime::{Caller, Engine, Extern, ExternRef, Func, FuncType, Linker, Module, Store, Val, ValType};
use wasmtime_wasi::p1::WasiP1Ctx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stdio,
    Sse,
}

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub wasm_path: PathBuf,
    pub transport: Transport,
    pub port: u16,
    pub host: String,
    pub bridge_command: String,
    /// 安静模式：抑制 stderr 进度日志
    pub quiet: bool,
}

impl McpConfig {
    pub fn stdio(wasm_path: impl Into<PathBuf>) -> Self {
        Self {
            wasm_path: wasm_path.into(),
            transport: Transport::Stdio,
            port: 8080,
            host: "127.0.0.1".to_string(),
            bridge_command: "br".to_string(),
            quiet: false,
        }
    }
    pub fn sse(mut self, host: String, port: u16) -> Self {
        self.transport = Transport::Sse;
        self.host = host;
        self.port = port;
        self
    }
}

// ---------------------------------------------------------------------------
// Handle types
// ---------------------------------------------------------------------------

pub struct StringCreateBody(pub Mutex<Vec<u8>>);
pub struct StringReadBody(pub Mutex<StringReadBodyState>);
pub struct StringReadBodyState { pub s: String, pub pos: usize }

impl StringReadBodyState {
    pub fn read_char(&mut self) -> i32 {
        if self.pos >= self.s.len() {
            return -1;
        }
        let rest = &self.s.as_bytes()[self.pos..];
        match std::str::from_utf8(rest) {
            Ok(s) => {
                let c = s.chars().next().unwrap();
                self.pos += c.len_utf8();
                c as i32
            }
            Err(_) => { let b = rest[0]; self.pos += 1; b as i32 }
        }
    }
}

const HOST_ABI_VERSION: i32 = 1;

pub struct HostCtx {
    pub wasi: WasiP1Ctx,
    pub bridge_command: String,
    pub handles: Mutex<HandleMap>,
    /// 共享 stdin buffer：stdio 模式由 background thread 从 stdin() 喂入；
    /// SSE/HTTP 模式由 HTTP handler 把 request body 一次性塞入。
    pub stdin_buf: Arc<Mutex<Vec<u8>>>,
}

pub struct HandleMap {
    map: HashMap<u32, Arc<dyn std::any::Any + Send + Sync>>,
}

impl HandleMap {
    pub fn new() -> Self { Self { map: HashMap::new() } }
    pub fn insert_at(&mut self, raw: u32, v: Arc<dyn std::any::Any + Send + Sync>) {
        self.map.insert(raw, v);
    }
    pub fn get<T: 'static + Send + Sync>(&self, raw: u32) -> Option<Arc<T>> {
        self.map.get(&raw).and_then(|a| a.clone().downcast::<T>().ok())
    }
    pub fn remove(&mut self, raw: u32) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.map.remove(&raw)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_nanos() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0)
}

fn raw_of(caller: &mut Caller<'_, HostCtx>, v: &Val) -> u32 {
    match v { Val::ExternRef(Some(r)) => r.to_raw(caller).unwrap_or(0), _ => 0 }
}

fn make_handle(caller: &mut Caller<'_, HostCtx>, value: Arc<dyn std::any::Any + Send + Sync>) -> Result<Val> {
    let rooted = ExternRef::new(&mut *caller, value.clone())?;
    let raw = rooted.to_raw(&mut *caller)?;
    caller.data_mut().handles.lock().insert_at(raw, value);
    Ok(Val::ExternRef(Some(rooted)))
}

fn read_handle(host: &HostCtx, raw: u32) -> Option<String> {
    host.handles.lock()
        .get::<StringReadBody>(raw)
        .map(|b| b.0.lock().s.clone())
}

fn make_read_body(s: String) -> Arc<dyn std::any::Any + Send + Sync> {
    Arc::new(StringReadBody(Mutex::new(StringReadBodyState { s, pos: 0 })))
}

// ---------------------------------------------------------------------------
// Host function implementations — 简化为单行
// ---------------------------------------------------------------------------

fn host_time_now(_c: &mut Caller<'_, HostCtx>, _a: &[Val], r: &mut [Val]) -> Result<()> {
    r[0] = Val::I64(now_nanos()); Ok(())
}
fn host_begin_create_string(c: &mut Caller<'_, HostCtx>, _a: &[Val], r: &mut [Val]) -> Result<()> {
    r[0] = make_handle(c, Arc::new(StringCreateBody(Mutex::new(Vec::new()))))?; Ok(())
}
fn host_string_append_char(c: &mut Caller<'_, HostCtx>, a: &[Val], _r: &mut [Val]) -> Result<()> {
    let raw = raw_of(c, &a[0]);
    let cp = match a[1] { Val::I32(n) => n as u32, _ => return Ok(()) };
    if let Some(b) = c.data().handles.lock().get::<StringCreateBody>(raw) {
        let mut buf = b.0.lock();
        if let Some(ch) = char::from_u32(cp) {
            let mut tmp = [0u8; 4];
            buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
        }
    }
    Ok(())
}
fn host_finish_create_string(c: &mut Caller<'_, HostCtx>, a: &[Val], r: &mut [Val]) -> Result<()> {
    let raw = raw_of(c, &a[0]);
    let s = c.data().handles.lock()
        .get::<StringCreateBody>(raw)
        .map(|b| b.0.lock().drain(..).collect::<Vec<u8>>())
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .unwrap_or_default();
    r[0] = make_handle(c, make_read_body(s))?; Ok(())
}
fn host_begin_read_string(c: &mut Caller<'_, HostCtx>, a: &[Val], r: &mut [Val]) -> Result<()> {
    let raw = raw_of(c, &a[0]);
    let s = read_handle(c.data(), raw).unwrap_or_default();
    r[0] = make_handle(c, make_read_body(s))?; Ok(())
}
fn host_string_read_char(c: &mut Caller<'_, HostCtx>, a: &[Val], r: &mut [Val]) -> Result<()> {
    let raw = raw_of(c, &a[0]);
    r[0] = Val::I32(c.data().handles.lock()
        .get::<StringReadBody>(raw)
        .map(|b| b.0.lock().read_char())
        .unwrap_or(-1));
    Ok(())
}
fn host_finish_read_string(c: &mut Caller<'_, HostCtx>, a: &[Val], _r: &mut [Val]) -> Result<()> {
    let raw = raw_of(c, &a[0]);
    c.data().handles.lock().remove(raw); Ok(())
}
fn host_br_version(_c: &mut Caller<'_, HostCtx>, _a: &[Val], r: &mut [Val]) -> Result<()> {
    r[0] = Val::I32(HOST_ABI_VERSION); Ok(())
}
fn host_br_path(c: &mut Caller<'_, HostCtx>, _a: &[Val], r: &mut [Val]) -> Result<()> {
    let p = std::env::var("BR_MCP_BR_PATH").ok()
        .or_else(|| Command::new("which").arg("br").output().ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()))
        .unwrap_or_else(|| "br".to_string());
    r[0] = make_handle(c, make_read_body(p))?; Ok(())
}
fn host_br_run(c: &mut Caller<'_, HostCtx>, a: &[Val], r: &mut [Val]) -> Result<()> {
    let args_str = {
        let raw = raw_of(c, &a[0]);
        read_handle(c.data(), raw).unwrap_or_default()
    };
    let bridge = c.data().bridge_command.clone();
    let arg_list: Vec<String> = args_str.split('\n').filter(|s| !s.is_empty()).map(String::from).collect();
    r[0] = make_handle(c, make_read_body(run_bridge(&bridge, &arg_list).0))?; Ok(())
}
fn host_read_stdin_chunk(c: &mut Caller<'_, HostCtx>, a: &[Val], r: &mut [Val]) -> Result<()> {
    let max = match a[0] { Val::I32(n) if n > 0 => n as usize, _ => 1024 };
    let mut buf = c.data().stdin_buf.lock();
    let n = max.min(buf.len());
    let chunk: Vec<u8> = buf.drain(..n).collect();
    drop(buf);
    r[0] = make_handle(c, make_read_body(String::from_utf8_lossy(&chunk).into_owned()))?;
    Ok(())
}

fn run_bridge(cmd: &str, arg_list: &[String]) -> (String, bool) {
    match Command::new(cmd).args(arg_list).output() {
        Ok(out) => {
            let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
            if !out.stderr.is_empty() { if !s.is_empty() { s.push('\n'); } s.push_str(&String::from_utf8_lossy(&out.stderr)); }
            (s.trim().to_string(), out.status.success())
        }
        Err(e) => (format!("bridge command not found: {} ({})", cmd, e), false),
    }
}

// ---------------------------------------------------------------------------
// Linker 注册
// ---------------------------------------------------------------------------

type HostFn = Box<dyn Fn(&mut Caller<'_, HostCtx>, &[Val], &mut [Val]) -> Result<()> + Send + Sync>;

fn register_func(
    linker: &mut Linker<HostCtx>, store: &mut Store<HostCtx>,
    module: &str, name: &str, params: &[ValType], results: &[ValType], f: HostFn,
) -> Result<()> {
    let ty = FuncType::new(store.engine(), params.to_vec(), results.to_vec());
    let func = Func::new(&mut *store, ty, move |mut caller, args, results| {
        f(&mut caller, args, results).map_err(|e| wasmtime::Error::msg(e.to_string()))
    });
    linker.define(&mut *store, module, name, Extern::Func(func))?;
    Ok(())
}

pub(crate) fn register_host_imports(linker: &mut Linker<HostCtx>, store: &mut Store<HostCtx>) -> Result<()> {
    let e = &[ValType::EXTERNREF];
    let i = &[ValType::I32];
    let l = &[ValType::I64];
    let v: &[ValType] = &[];
    let ei = &[ValType::EXTERNREF, ValType::I32];

    register_func(linker, store, "env", "host_begin_create_string", v, e, Box::new(host_begin_create_string))?;
    register_func(linker, store, "env", "host_string_append_char", ei, v, Box::new(host_string_append_char))?;
    register_func(linker, store, "env", "host_finish_create_string", e, e, Box::new(host_finish_create_string))?;
    register_func(linker, store, "env", "host_begin_read_string", e, e, Box::new(host_begin_read_string))?;
    register_func(linker, store, "env", "host_string_read_char", e, i, Box::new(host_string_read_char))?;
    register_func(linker, store, "env", "host_finish_read_string", e, v, Box::new(host_finish_read_string))?;
    register_func(linker, store, "env", "host_br_version", v, i, Box::new(host_br_version))?;
    register_func(linker, store, "env", "host_br_path", v, e, Box::new(host_br_path))?;
    register_func(linker, store, "env", "host_br_run", e, e, Box::new(host_br_run))?;
    register_func(linker, store, "env", "host_read_stdin_chunk", i, e, Box::new(host_read_stdin_chunk))?;
    register_func(linker, store, "__moonbit_time_unstable", "now", v, l, Box::new(host_time_now))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 公共入口
// ---------------------------------------------------------------------------

pub fn run_mcp(config: McpConfig) -> Result<()> {
    let engine = Arc::new(Engine::new(
        &wasmtime::Config::new().wasm_gc(true).wasm_component_model(false))?);
    let module = Arc::new(Module::from_file(&engine, &config.wasm_path)?);
    match config.transport {
        Transport::Stdio => run_mcp_stdio(engine, module, config),
        Transport::Sse => crate::sse::run_mcp_sse(engine, module, config),
    }
}

fn run_mcp_stdio(engine: Arc<Engine>, module: Arc<Module>, config: McpConfig) -> Result<()> {
    let wasi = wasmtime_wasi::WasiCtxBuilder::new()
        .inherit_stdio().inherit_env()
        .args(&["mbit-mcp-server"]).build_p1();
    // stdio 模式：MoonBit wasm 通过 wasi.fd_read 读 stdin (WasiCtxBuilder::inherit_stdin)，
    // host_read_stdin_chunk 不被调用，stdin_buf 保持空。
    let stdin_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let host = HostCtx {
        wasi, bridge_command: config.bridge_command.clone(),
        handles: Mutex::new(HandleMap::new()),
        stdin_buf,
    };
    let mut linker: Linker<HostCtx> = Linker::new(&engine);
    let mut store = Store::new(&engine, host);

    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |h| &mut h.wasi)?;
    register_host_imports(&mut linker, &mut store)?;

    if !config.quiet { eprintln!("[mbit mcp] stdio 模式启动"); }

    let instance = linker.instantiate(&mut store, &module)?;
    let func_name = ["_start", "mcp", "run"].iter()
        .find(|n| instance.get_func(&mut store, n).is_some()).copied()
        .ok_or_else(|| anyhow::anyhow!("wasm 既无 _start 也无 mcp()/run()"))?;
    if !config.quiet { eprintln!("[mbit mcp] 调用 wasm.{}", func_name); }
    let func = instance.get_typed_func::<(), ()>(&mut store, func_name)?;
    func.call(&mut store, ())?;
    Ok(())
}
