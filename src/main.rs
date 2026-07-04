// mbit - MoonBit Component Model 通用构建和运行工具
//
// CLI 入口，核心逻辑在 mbit library 中。

use anyhow::Result;
use mbit::Builder;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("build") => {
            let standalone = args.iter().any(|a| a == "--standalone" || a == "-s");
            let debug = args.iter().any(|a| a == "--debug" || a == "-d");
            let target = args
                .iter()
                .position(|a| a == "--target" || a == "-t")
                .and_then(|i| args.get(i + 1))
                .cloned();

            let mut builder = Builder::new().standalone(standalone).release(!debug);
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
    println!("mbit - MoonBit Component Model 通用构建和运行工具");
    println!();
    println!("用法:");
    println!("  mbit build                        编译 wit/ + src/ → target/component.wasm (默认 release)");
    println!("  mbit build --debug                Debug 模式编译");
    println!("  mbit build --standalone           生成独立可执行文件 (默认 release 模式)");
    println!("  mbit build --standalone --debug   Debug 模式生成独立二进制");
    println!("  mbit build --standalone --target <triple>  交叉编译到指定平台");
    println!("  mbit run [wasm]                   运行 component (默认 target/component.wasm)");
    println!("  mbit test [args...]               运行测试 (透传给 moon test)");
    println!("  mbit bench [args...]              运行基准测试 (透传给 moon bench)");
    println!();
    println!("测试选项 (moon test):");
    println!("  -f, --filter <pattern>            按名称过滤测试");
    println!("  -p, --package <package>           指定包");
    println!("  --release                         Release 模式");
    println!("  --enable-coverage                 启用覆盖率");
    println!("  -u, --update                      更新测试快照");
    println!();
    println!("交叉编译目标示例:");
    println!("  x86_64-unknown-linux-gnu          Linux x86_64");
    println!("  aarch64-unknown-linux-gnu         Linux ARM64");
    println!("  x86_64-pc-windows-gnu             Windows x86_64");
    println!("  x86_64-apple-darwin               macOS x86_64");
    println!("  aarch64-apple-darwin              macOS ARM64 (Apple Silicon)");
    println!();
    println!("交叉编译前提:");
    println!("  安装 cross: cargo install cross");
    println!("  安装 Docker 或 Podman");
}
