# 开发文档

## 代码结构

```
mbit/
├── src/
│   └── main.rs              # 主程序入口
├── Cargo.toml                # Rust 依赖配置
├── .gitignore               # Git 忽略规则
└── README.md                # 项目说明
```

## 核心实现

### main.rs 结构

```rust
fn main() -> Result<()> {
    match args.get(1) {
        Some("build") => cmd_build(standalone, release, target),
        Some("run") => cmd_run(wasm_path),
        Some("test") => cmd_test(args),
        Some("bench") => cmd_bench(args),
        Some("help") => show_help(),
        _ => show_usage(),
    }
}
```

### 关键函数

#### cmd_build()

构建 MoonBit 项目为 WASM 组件。

**流程**：
1. 读取 `wit/world.wit`
2. 复制 `src/*.mbt` 到 `gen/` 目录
3. 运行 `moon build --target wasm`
4. 打包为 Component（使用 `wasm-tools`）
5. 输出到 `target/component.wasm`

**关键代码**：
```rust
fn cmd_build(standalone: bool, release: bool, target: Option<&str>) -> Result<()> {
    // 1. 解析 WIT
    let wit_content = fs::read_to_string("wit/world.wit")?;
    
    // 2. 复制实现文件
    copy_impl_files("src", "gen")?;
    
    // 3. 编译 WASM
    run_command("moon", &["build", "--target", "wasm"])?;
    
    // 4. 打包组件
    run_command("wasm-tools", &["component", "new", ...])?;
    
    // 5. 可选：生成独立二进制
    if standalone {
        generate_standalone_host(wit_content, release, target)?;
    }
}
```

#### cmd_run()

运行 WASM 组件。

**关键代码**：
```rust
fn cmd_run(wasm_path: Option<&str>) -> Result<()> {
    let wasm_bytes = fs::read(wasm_path.unwrap_or("target/component.wasm"))?;
    
    // 创建 wasmtime engine
    let engine = Engine::default();
    
    // 加载组件
    let component = Component::new(&engine, &wasm_bytes)?;
    
    // 创建 linker 并添加 WASI
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker_sync(&mut linker)?;
    
    // 实例化并运行
    let instance = linker.instantiate(&mut store, &component)?;
    // ... 调用导出函数
}
```

#### generate_standalone_host()

生成独立可执行文件。

**原理**：
1. 创建临时 Rust 项目
2. 使用 `include_bytes!` 嵌入 WASM
3. 编译为独立二进制

**关键代码**：
```rust
fn generate_standalone_host(wit_content: String, release: bool, target: Option<&str>) -> Result<()> {
    // 1. 创建临时项目
    let temp_dir = tempdir()?;
    create_cargo_project(&temp_dir)?;
    
    // 2. 生成 main.rs
    let main_rs = generate_main_rs(&wit_content)?;
    fs::write(temp_dir.join("src/main.rs"), main_rs)?;
    
    // 3. 嵌入 WASM
    let wasm_bytes = fs::read("target/component.wasm")?;
    // 使用 include_bytes! 宏
    
    // 4. 编译
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if release {
        cmd.arg("--release");
    }
    if let Some(target) = target {
        // 使用 cross 进行交叉编译
        cmd = Command::new("cross");
        cmd.args(&["build", "--target", target]);
    }
    
    // 5. 复制产物
    fs::copy(binary_path, "target/app")?;
}
```

## 依赖说明

### wasmtime (46.0.1)

WebAssembly 运行时，支持 Component Model。

**关键特性**：
- Component Model 支持
- WASI Preview 2
- 高性能 JIT 编译

**API 使用**：
```rust
use wasmtime::{Engine, Store, Component, Linker};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView, IoView};

struct HostContext {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl IoView for HostContext {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiView for HostContext {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
```

### wit-bindgen (0.58.0)

从 WIT 文件生成绑定代码。

**使用方式**：
```rust
// 在 build.rs 或运行时调用
wit_bindgen::generate!({
    path: "wit",
    world: "my-world",
});
```

### wasm-tools (1.252.0)

WASM 工具集，用于打包 Component。

**命令**：
```bash
# 打包组件
wasm-tools component new input.wasm -o output.wasm

# 查看组件接口
wasm-tools component wit output.wasm
```

## 构建流程详解

### 1. WIT 解析

```rust
// 读取 wit/world.wit
let wit_content = fs::read_to_string("wit/world.wit")?;

// 解析接口定义
// WIT 格式示例：
// package my:project;
// 
// world my-world {
//     import my-interface;
//     export run: func() -> result<string, string>;
// }
// 
// interface my-interface {
//     my-function: func(input: string) -> string;
// }
```

### 2. 文件映射

```rust
fn copy_impl_files(src_dir: &str, gen_dir: &str) -> Result<()> {
    // 遍历 src/ 目录
    for entry in fs::read_dir(src_dir)? {
        let path = entry?.path();
        let filename = path.file_name().to_str().unwrap();
        
        // 根据文件名映射到目标目录
        let target_dir = match filename {
            f if f.contains("string") => "stringUtils",
            f if f.contains("hash") => "hashUtils",
            f if f.contains("json") => "jsonUtils",
            f if f.contains("uuid") => "uuidUtils",
            _ => continue,
        };
        
        // 复制文件
        let target_path = format!("{}/{}/impl.mbt", gen_dir, target_dir);
        fs::copy(&path, target_path)?;
    }
}
```

### 3. MoonBit 编译

```rust
fn compile_moonbit(release: bool) -> Result<()> {
    let mut args = vec!["build", "--target", "wasm"];
    if release {
        args.push("--release");
    }
    
    run_command("moon", &args)?;
    
    // 产物位于 _build/wasm/debug/build/gen/gen.wasm
    // 或 _build/wasm/release/build/gen/gen.wasm
}
```

### 4. Component 打包

```rust
fn package_component() -> Result<()> {
    // 使用 wasm-tools 打包
    run_command("wasm-tools", &[
        "component", "new",
        "_build/wasm/debug/build/gen/gen.wasm",
        "-o", "target/component.wasm"
    ])?;
}
```

## 测试策略

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_file_mapping() {
        assert_eq!(
            get_target_dir("impl-string.mbt"),
            "stringUtils"
        );
    }
    
    #[test]
    fn test_build_command() {
        // 测试构建命令生成
    }
}
```

### 集成测试

```bash
# 在 mbit-cm 项目中测试
cd /path/to/mbit-cm
mbit build
mbit run
mbit test
```

## 性能优化

### 1. Release 模式

```toml
# Cargo.toml
[profile.release]
lto = true           # Link Time Optimization
codegen-units = 1    # 更好的优化
panic = 'abort'      # 更小的二进制
strip = true         # 移除调试信息
```

### 2. WASM 优化

```bash
# 使用 wasm-opt 进一步优化
wasm-opt -O3 target/component.wasm -o target/component.wasm
```

### 3. 编译缓存

```rust
// 利用 cargo 的增量编译
// MoonBit 也有自己的缓存机制
```

## 错误处理

```rust
use anyhow::{Result, Context};

fn build_project() -> Result<()> {
    // 使用 ? 运算符传播错误
    let wit = fs::read_to_string("wit/world.wit")
        .context("Failed to read WIT file")?;
    
    // 自定义错误
    if !Path::new("src").exists() {
        anyhow::bail!("src directory not found");
    }
    
    Ok(())
}
```

## 扩展开发

### 添加新的构建目标

```rust
fn cmd_build_new_target() -> Result<()> {
    // 1. 解析新的配置
    // 2. 调用相应的编译器
    // 3. 打包产物
}
```

### 自定义 WASI 实现

```rust
// 实现自定义的 WASI 接口
struct CustomWasi {
    // 自定义状态
}

impl WasiView for CustomWasi {
    // 实现所需的方法
}
```

## 调试技巧

### 1. 启用详细日志

```bash
RUST_LOG=debug mbit build
```

### 2. 查看中间产物

```bash
# 查看生成的 gen/ 目录
ls -la gen/

# 查看编译的 WASM
wasm2wat _build/wasm/debug/build/gen/gen.wasm | head -50
```

### 3. 使用 lldb 调试

```bash
lldb -- mbit build
(lldb) breakpoint set --name cmd_build
(lldb) run
```

## 贡献指南

### 代码风格

- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 遵循 Rust 命名约定

### 提交规范

```
feat: 新功能
fix: 修复 bug
docs: 文档更新
style: 代码格式
refactor: 重构
test: 测试相关
chore: 构建/工具
```

### 发布流程

详见 [发布指南](RELEASE.md)。
