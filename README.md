# mbit

MoonBit Component Model 通用构建和运行工具。

## 用法

```bash
# 构建当前目录的 MoonBit 项目
mbit build

# Release 模式构建
mbit build --release

# 构建并生成独立可执行文件
mbit build --standalone

# Release 模式生成独立二进制
mbit build --standalone --release

# 构建并运行测试
mbit test

# 运行基准测试
mbit bench

# 运行构建产物
mbit run

# 运行指定的 wasm component
mbit run path/to/component.wasm
```

## 项目结构

mbit 工具期望当前目录包含：

```
your-project/
├── wit/
│   └── world.wit          # WIT 接口定义
├── src/
│   ├── impl-string.mbt    # 实现文件（按命名约定）
│   ├── impl-hash.mbt
│   └── ...
└── target/                # 构建输出（自动生成）
    └── component.wasm
```

### 实现文件命名约定

mbit 根据文件名自动将 `src/*.mbt` 复制到对应的接口目录：

| 文件名包含 | 目标目录 |
|-----------|---------|
| `string` | `stringUtils/impl.mbt` |
| `hash` | `hashUtils/impl.mbt` |
| `json` | `jsonUtils/impl.mbt` |
| `uuid` | `uuidUtils/impl.mbt` |

## 构建流程

`mbit build` 执行以下步骤：

1. 检查/初始化 MoonBit 模块 (`moon.mod.json`)
2. 运行 `wit-bindgen` 生成 MoonBit 绑定
3. 复制 `src/*.mbt` 到 `gen/` 目录
4. 编译 MoonBit → WASM (`moon build --target wasm`)
5. 打包成 WebAssembly Component (`wasm-tools component embed` + `new`)

输出：`target/component.wasm`

### 独立可执行文件

使用 `--standalone` 选项生成包含 wasmtime 运行时的独立二进制：

```bash
mbit build --standalone
mbit build --standalone --release  # Release 模式
```

输出：`target/app`（约 17 MB）

这个二进制文件可以直接运行，不需要 mbit 工具或外部 WASM 文件。

### 测试

`mbit test` 透传参数给 `moon test`：

```bash
# 运行所有测试
mbit test

# 运行特定包的测试
mbit test -p <package>

# 按名称过滤测试
mbit test -f "test_*"

# Release 模式运行测试
mbit test --release

# 生成覆盖率报告
mbit test --enable-coverage

# 更新测试快照
mbit test -u
```

### 基准测试

`mbit bench` 透传参数给 `moon bench`：

```bash
# 运行所有基准测试
mbit bench

# Release 模式运行基准测试
mbit bench --release

# 运行特定包的基准测试
mbit bench -p <package>
```

### 交叉编译

使用 `--target` 选项为不同平台构建独立二进制：

```bash
# 为 Linux x86_64 构建
mbit build --standalone --target x86_64-unknown-linux-gnu

# 为 Linux ARM64 构建
mbit build --standalone --target aarch64-unknown-linux-gnu

# 为 Windows x86_64 构建
mbit build --standalone --target x86_64-pc-windows-gnu

# 为 macOS ARM64 (Apple Silicon) 构建
mbit build --standalone --target aarch64-apple-darwin
```

输出：`target/app-<target>`

**交叉编译前提：**

1. 安装 cross：
   ```bash
   cargo install cross
   ```

2. 安装 Docker 或 Podman

3. 确保 Docker daemon 正在运行

**常用目标平台：**

| 目标 | 平台 |
|------|------|
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |
| `x86_64-pc-windows-gnu` | Windows x86_64 |
| `x86_64-apple-darwin` | macOS x86_64 |
| `aarch64-apple-darwin` | macOS ARM64 |

## 运行

`mbit run` 使用 wasmtime 加载并运行 component：

- 列出所有导出的接口
- 如果组件导出 `run` 函数，自动调用
- 否则显示导出列表，提示需要通过 host 代码调用

## 前提

需要安装：

- `moon` - MoonBit 编译器
- `wit-bindgen` - WIT 绑定生成器
- `wasm-tools` - WASM 工具链

## 构建 mbit

```bash
cargo build --release
```

产物：`target/release/mbit` (约 17 MB)

## 示例

### 基础构建

```bash
# 在 MoonBit Component Model 项目中
cd /path/to/your-project
mbit build
mbit run
```

输出：

```
=== mbit build ===
项目目录: /path/to/your-project

[1/6] MoonBit 模块已存在

[2/6] 生成 MoonBit 绑定 (wit-bindgen)
Generating "./gen/ffi.mbt"
...

[3/6] 注入实现文件
  impl-string.mbt → stringUtils/impl.mbt
  impl-hash.mbt → hashUtils/impl.mbt

[4/6] 添加依赖

[5/6] 编译 MoonBit → WASM (wasm-gc)
Finished. moon: ran 6 tasks, now up to date

[6/6] 打包成 WebAssembly Component

✓ 构建完成
  输出: target/component.wasm (255.2 KB)

运行: mbit run
```

### 生成独立二进制

```bash
mbit build --standalone
```

输出：

```
=== 生成独立可执行文件 ===
[1/3] 复制 WIT 定义
[2/3] 生成 Rust host 项目
[3/3] 编译独立二进制
   Compiling app v0.1.0
    Finished release [optimized]

✓ 独立二进制已生成
  输出: target/app (17.1 MB)

运行: ./target/app
```

### 交叉编译到多个平台

```bash
# 在 macOS 上为所有平台构建
mbit build --standalone --target x86_64-unknown-linux-gnu
mbit build --standalone --target aarch64-unknown-linux-gnu
mbit build --standalone --target x86_64-pc-windows-gnu

# 查看生成的二进制
ls -lh target/app-*
# target/app-x86_64-unknown-linux-gnu (17 MB)
# target/app-aarch64-unknown-linux-gnu (17 MB)
# target/app-x86_64-pc-windows-gnu.exe (17 MB)
```
