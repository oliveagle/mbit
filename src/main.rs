// mbit - MoonBit WASM 构建 + 运行工具（公共 runtime）
//
// 编译：moon CLI（不可避免） + wasmtime crate 验证
// 运行：wasmtime crate + 自定义 host imports
//
// 全部用 crate API；不 spawn moon 之外的编译工具链 CLI。

use anyhow::Result;
use mbit::{Builder, McpConfig, Transport};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let quiet = args.iter().any(|a| a == "--quiet" || a == "-q");

    match args.get(1).map(|s| s.as_str()) {
        Some("build") => {
            let debug = args.iter().any(|a| a == "--debug" || a == "-d");
            let target = args
                .iter()
                .position(|a| a == "--target" || a == "-t")
                .and_then(|i| args.get(i + 1))
                .cloned();

            let mut builder = Builder::new().release(!debug).quiet(quiet);
            if let Some(t) = target {
                builder = builder.target(t);
            }
            let project_dir = std::env::current_dir()?;
            mbit::build(&project_dir, &builder.build_config())
        }
        Some("run") => {
            let project_dir = std::env::current_dir()?;
            let wasm_path = args.get(2).map(PathBuf::from);
            mbit::run(&project_dir, wasm_path.as_deref())
        }
        Some("mcp") => {
            let transport = if args.iter().any(|a| a == "--stdio") {
                Transport::Stdio
            } else if args.iter().any(|a| a == "--sse") {
                Transport::Sse
            } else {
                anyhow::bail!("必须指定传输模式: --stdio 或 --sse");
            };

            let port: u16 = args
                .iter()
                .position(|a| a == "--port" || a == "-p")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080);

            let host = args
                .iter()
                .position(|a| a == "--host" || a == "-H")
                .and_then(|i| args.get(i + 1).cloned())
                .unwrap_or_else(|| "127.0.0.1".to_string());

            let bridge_command = args
                .iter()
                .position(|a| a == "--bridge-cmd" || a == "-b")
                .and_then(|i| args.get(i + 1).cloned())
                .unwrap_or_else(|| "br".to_string());

            // wasm 路径 = 最后一个非 flag 参数（允许 flags 出现在路径之前或之后）
            let wasm_path = args
                .iter()
                .skip(2)
                .filter(|a| !a.starts_with("--") && !a.starts_with("-"))
                .next_back()
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("必须提供 wasm 文件路径"))?;

            let mut config = McpConfig::stdio(wasm_path);
            config.bridge_command = bridge_command;
            config.quiet = quiet;
            if transport == Transport::Sse {
                config = config.sse(host, port);
            }
            mbit::run_mcp(config)
        }
        Some("test") => {
            let project_dir = std::env::current_dir()?;
            mbit::test(&project_dir, &args[2..])
        }
        Some("bench") => {
            let project_dir = std::env::current_dir()?;
            mbit::bench(&project_dir, &args[2..])
        }
        Some("--help") | Some("-h") | Some("help") => {
            print_help();
            Ok(())
        }
        Some(other) => {
            anyhow::bail!("未知命令: {}\n运行 'mbit help' 查看用法", other);
        }
        None => {
            println!("mbit: 缺少命令");
            println!("运行 'mbit help' 查看用法");
            Ok(())
        }
    }
}

fn print_help() {
    println!("mbit - MoonBit WASM 构建 + 运行工具");
    println!();
    println!("编译: moon CLI (不可避免) + wasmtime crate 验证");
    println!("运行: wasmtime crate + 自定义 host imports (externref handle 桥接)");
    println!();
    println!("全局选项:");
    println!("  -q, --quiet                抑制 stderr 进度日志");
    println!();
    println!("用法:");
    println!("  mbit build [options]                       编译当前 MoonBit 项目");
    println!("  mbit build --debug                         Debug 模式编译");
    println!("  mbit build --target <triple>               交叉编译到指定平台");
    println!("  mbit run [wasm]                            加载 wasm 并调用 _start/run()");
    println!("  mbit mcp --stdio [options] <wasm>          以 stdio MCP 服务器运行");
    println!("  mbit mcp --sse   [options] <wasm>          以 HTTP+SSE MCP 服务器运行（简化）");
    println!("  mbit test [args...]                        运行测试 (透传给 moon test)");
    println!("  mbit bench [args...]                       运行基准测试 (透传给 moon bench)");
    println!();
    println!("mcp 选项:");
    println!("  --stdio                  stdio JSON-RPC 传输");
    println!("  --sse                    HTTP + Server-Sent Events 传输");
    println!("  -p, --port <port>        SSE 监听端口 (默认 8080)");
    println!("  -H, --host <host>        SSE 监听地址 (默认 127.0.0.1)");
    println!("  -b, --bridge-cmd <cmd>   br 子进程命令 (默认 br)");
    println!();
    println!("典型工作流:");
    println!("  1. cd your-moonbit-project");
    println!("  2. mbit build                # → target/<pkg>.wasm");
    println!("  3. mbit mcp --stdio target/<pkg>.wasm");
}
