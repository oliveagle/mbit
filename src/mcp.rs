// MCP (Model Context Protocol) 主机模块
//
// 在 wasmtime 上加载 MoonBit WASM (MVP/wasm-gc) 作为 MCP 服务器运行。
// 支持 stdio 和 SSE 两种传输模式。
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
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use wasmtime::{
    Caller, Engine, Extern, ExternRef, Func, FuncType, Linker, Module, Store, Val, ValType,
};
use wasmtime_wasi::p1::WasiP1Ctx;

/// MCP 传输模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stdio,
    Sse,
}

/// MCP 主机配置
#[derive(Debug, Clone)]
pub struct McpConfig {
    pub wasm_path: PathBuf,
    pub transport: Transport,
    pub port: u16,
    pub host: String,
    pub bridge_command: String,
}

impl McpConfig {
    pub fn stdio(wasm_path: impl Into<PathBuf>) -> Self {
        Self {
            wasm_path: wasm_path.into(),
            transport: Transport::Stdio,
            port: 8080,
            host: "127.0.0.1".to_string(),
            bridge_command: "br".to_string(),
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

pub struct StringReadBodyState {
    pub s: String,
    pub pos: usize,
}

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
            Err(_) => {
                let b = rest[0];
                self.pos += 1;
                b as i32
            }
        }
    }
}

const HOST_ABI_VERSION: i32 = 1;

pub struct HostCtx {
    pub wasi: WasiP1Ctx,
    pub bridge_command: String,
    pub handles: Mutex<HandleMap>,
}

pub struct HandleMap {
    map: HashMap<u32, Arc<dyn std::any::Any + Send + Sync>>,
}

impl HandleMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert_at(&mut self, raw: u32, v: Arc<dyn std::any::Any + Send + Sync>) {
        self.map.insert(raw, v);
    }

    pub fn get<T: 'static + Send + Sync>(&self, raw: u32) -> Option<Arc<T>> {
        self.map
            .get(&raw)
            .and_then(|a| a.clone().downcast::<T>().ok())
    }

    pub fn remove(&mut self, raw: u32) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.map.remove(&raw)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

fn externref_to_raw(caller: &mut Caller<'_, HostCtx>, v: &Val) -> u32 {
    match v {
        Val::ExternRef(Some(r)) => r.to_raw(&mut *caller).unwrap_or(0),
        _ => 0,
    }
}

fn make_externref(
    caller: &mut Caller<'_, HostCtx>,
    value: Arc<dyn std::any::Any + Send + Sync>,
) -> Result<Val> {
    let rooted = ExternRef::new(&mut *caller, value.clone())?;
    let raw = rooted.to_raw(&mut *caller)?;
    caller
        .data_mut()
        .handles
        .lock()
        .unwrap()
        .insert_at(raw, value);
    Ok(Val::ExternRef(Some(rooted)))
}

fn new_read_body(s: String) -> Arc<dyn std::any::Any + Send + Sync> {
    Arc::new(StringReadBody(Mutex::new(StringReadBodyState {
        s,
        pos: 0,
    })))
}

fn new_create_body() -> Arc<dyn std::any::Any + Send + Sync> {
    Arc::new(StringCreateBody(Mutex::new(Vec::new())))
}

// ---------------------------------------------------------------------------
// Host function implementations
// ---------------------------------------------------------------------------

fn host_time_now(
    _caller: &mut Caller<'_, HostCtx>,
    _args: &[Val],
    results: &mut [Val],
) -> Result<()> {
    results[0] = Val::I64(now_nanos());
    Ok(())
}

fn host_begin_create_string(
    caller: &mut Caller<'_, HostCtx>,
    _args: &[Val],
    results: &mut [Val],
) -> Result<()> {
    results[0] = make_externref(caller, new_create_body())?;
    Ok(())
}

fn host_string_append_char(
    caller: &mut Caller<'_, HostCtx>,
    args: &[Val],
    _results: &mut [Val],
) -> Result<()> {
    let raw = externref_to_raw(caller, &args[0]);
    let cp = match &args[1] {
        Val::I32(n) => *n as u32,
        _ => return Ok(()),
    };
    if let Some(body) = caller
        .data()
        .handles
        .lock()
        .unwrap()
        .get::<StringCreateBody>(raw)
    {
        let mut buf = body.0.lock().unwrap();
        if let Some(c) = char::from_u32(cp) {
            let mut tmp = [0u8; 4];
            let s = c.encode_utf8(&mut tmp);
            buf.extend_from_slice(s.as_bytes());
        }
    }
    Ok(())
}

fn host_finish_create_string(
    caller: &mut Caller<'_, HostCtx>,
    args: &[Val],
    results: &mut [Val],
) -> Result<()> {
    let raw = externref_to_raw(caller, &args[0]);
    let s = caller
        .data()
        .handles
        .lock()
        .unwrap()
        .get::<StringCreateBody>(raw)
        .map(|b| b.0.lock().unwrap().drain(..).collect::<Vec<u8>>())
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .unwrap_or_default();
    results[0] = make_externref(caller, new_read_body(s))?;
    Ok(())
}

fn host_begin_read_string(
    caller: &mut Caller<'_, HostCtx>,
    args: &[Val],
    results: &mut [Val],
) -> Result<()> {
    let raw = externref_to_raw(caller, &args[0]);
    let s = caller
        .data()
        .handles
        .lock()
        .unwrap()
        .get::<StringReadBody>(raw)
        .map(|b| b.0.lock().unwrap().s.clone())
        .unwrap_or_default();
    results[0] = make_externref(caller, new_read_body(s))?;
    Ok(())
}

fn host_string_read_char(
    caller: &mut Caller<'_, HostCtx>,
    args: &[Val],
    results: &mut [Val],
) -> Result<()> {
    let raw = externref_to_raw(caller, &args[0]);
    if let Some(body) = caller
        .data()
        .handles
        .lock()
        .unwrap()
        .get::<StringReadBody>(raw)
    {
        let mut state = body.0.lock().unwrap();
        results[0] = Val::I32(state.read_char());
    } else {
        results[0] = Val::I32(-1);
    }
    Ok(())
}

fn host_finish_read_string(
    caller: &mut Caller<'_, HostCtx>,
    args: &[Val],
    _results: &mut [Val],
) -> Result<()> {
    let raw = externref_to_raw(caller, &args[0]);
    caller.data().handles.lock().unwrap().remove(raw);
    Ok(())
}

fn host_br_version(
    _caller: &mut Caller<'_, HostCtx>,
    _args: &[Val],
    results: &mut [Val],
) -> Result<()> {
    results[0] = Val::I32(HOST_ABI_VERSION);
    Ok(())
}

fn host_br_path(
    caller: &mut Caller<'_, HostCtx>,
    _args: &[Val],
    results: &mut [Val],
) -> Result<()> {
    let path = std::env::var("BR_MCP_BR_PATH")
        .ok()
        .or_else(|| {
            Command::new("which")
                .arg("br")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_else(|| "br".to_string());
    results[0] = make_externref(caller, new_read_body(path))?;
    Ok(())
}

fn host_br_run(caller: &mut Caller<'_, HostCtx>, args: &[Val], results: &mut [Val]) -> Result<()> {
    let raw = externref_to_raw(caller, &args[0]);
    let args_str = caller
        .data()
        .handles
        .lock()
        .unwrap()
        .get::<StringReadBody>(raw)
        .map(|b| b.0.lock().unwrap().s.clone())
        .unwrap_or_default();
    let bridge_cmd = caller.data().bridge_command.clone();
    let arg_list: Vec<String> = args_str
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let (output, _success) = run_bridge(&bridge_cmd, &arg_list);
    results[0] = make_externref(caller, new_read_body(output))?;
    Ok(())
}

fn host_read_stdin_chunk(
    caller: &mut Caller<'_, HostCtx>,
    args: &[Val],
    results: &mut [Val],
) -> Result<()> {
    let max = match &args[0] {
        Val::I32(n) => {
            if *n <= 0 {
                1024
            } else {
                *n as usize
            }
        }
        _ => 1024,
    };
    let mut buf = vec![0u8; max];
    let n = std::io::stdin().read(&mut buf).unwrap_or_default();
    buf.truncate(n);
    let s = String::from_utf8_lossy(&buf).into_owned();
    results[0] = make_externref(caller, new_read_body(s))?;
    Ok(())
}

fn run_bridge(cmd: &str, arg_list: &[String]) -> (String, bool) {
    match Command::new(cmd).args(arg_list).output() {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
            if !out.stderr.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            (combined.trim().to_string(), out.status.success())
        }
        Err(e) => (format!("bridge command not found: {} ({})", cmd, e), false),
    }
}

// ---------------------------------------------------------------------------
// Linker 注册
// ---------------------------------------------------------------------------

type HostFn = Box<dyn Fn(&mut Caller<'_, HostCtx>, &[Val], &mut [Val]) -> Result<()> + Send + Sync>;

fn register_func(
    linker: &mut Linker<HostCtx>,
    store: &mut Store<HostCtx>,
    module: &str,
    name: &str,
    params: &[ValType],
    results: &[ValType],
    f: HostFn,
) -> Result<()> {
    let ty = FuncType::new(store.engine(), params.to_vec(), results.to_vec());
    let func = Func::new(&mut *store, ty, move |mut caller, args, results| {
        f(&mut caller, args, results).map_err(|e| wasmtime::Error::msg(e.to_string()))
    });
    linker.define(&mut *store, module, name, Extern::Func(func))?;
    Ok(())
}

fn register_host_imports(linker: &mut Linker<HostCtx>, store: &mut Store<HostCtx>) -> Result<()> {
    let e = &[ValType::EXTERNREF];
    let i = &[ValType::I32];
    let l = &[ValType::I64];
    let v: &[ValType] = &[];

    register_func(
        linker,
        store,
        "env",
        "host_begin_create_string",
        v,
        e,
        Box::new(host_begin_create_string),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_string_append_char",
        e,
        v,
        Box::new(host_string_append_char),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_finish_create_string",
        e,
        e,
        Box::new(host_finish_create_string),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_begin_read_string",
        e,
        e,
        Box::new(host_begin_read_string),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_string_read_char",
        e,
        i,
        Box::new(host_string_read_char),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_finish_read_string",
        e,
        v,
        Box::new(host_finish_read_string),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_br_version",
        v,
        i,
        Box::new(host_br_version),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_br_path",
        v,
        e,
        Box::new(host_br_path),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_br_run",
        e,
        e,
        Box::new(host_br_run),
    )?;
    register_func(
        linker,
        store,
        "env",
        "host_read_stdin_chunk",
        i,
        e,
        Box::new(host_read_stdin_chunk),
    )?;
    register_func(
        linker,
        store,
        "__moonbit_time_unstable",
        "now",
        v,
        l,
        Box::new(host_time_now),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 公共入口
// ---------------------------------------------------------------------------

pub fn run_mcp(config: McpConfig) -> Result<()> {
    let engine = Engine::new(
        wasmtime::Config::new()
            .wasm_gc(true)
            .wasm_component_model(false),
    )?;
    let module = Module::from_file(&engine, &config.wasm_path)?;

    let wasi = wasmtime_wasi::WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_env()
        .args(&["mbit-mcp-server"])
        .build_p1();
    let host = HostCtx {
        wasi,
        bridge_command: config.bridge_command.clone(),
        handles: Mutex::new(HandleMap::new()),
    };
    let mut linker: Linker<HostCtx> = Linker::new(&engine);
    let mut store = Store::new(&engine, host);

    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |h| &mut h.wasi)?;
    register_host_imports(&mut linker, &mut store)?;

    match config.transport {
        Transport::Stdio => eprintln!("[mbit mcp] stdio 模式启动"),
        Transport::Sse => eprintln!("[mbit mcp] SSE 模式（简化：阻塞 stdin）"),
    }

    let instance = linker.instantiate(&mut store, &module)?;
    let func_name = ["_start", "mcp", "run"]
        .iter()
        .find(|name| instance.get_func(&mut store, name).is_some())
        .copied()
        .ok_or_else(|| anyhow::anyhow!("wasm 既无 _start 也无 mcp()/run()"))?;

    eprintln!("[mbit mcp] 调用 wasm.{}", func_name);
    let func = instance.get_typed_func::<(), ()>(&mut store, func_name)?;
    func.call(&mut store, ())?;
    Ok(())
}
