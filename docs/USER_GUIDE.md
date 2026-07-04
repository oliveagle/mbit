# 用户指南

## 安装

### 编译 mbit

```bash
cd /path/to/mbit
cargo build --release
```

产物：`target/release/mbit`

### 安装到系统（可选）

```bash
cp target/release/mbit /usr/local/bin/
```

## 基本用法

### 1. 构建项目

```bash
cd your-moonbit-project
mbit build
```

**前提条件**：
- 项目根目录包含 `wit/world.wit`
- 项目根目录包含 `src/` 目录及实现文件

**输出**：`target/component.wasm`

### 2. 运行组件

```bash
mbit run
```

或指定 WASM 文件：

```bash
mbit run path/to/component.wasm
```

### 3. 生成独立二进制

```bash
mbit build --standalone
```

**输出**：`target/app`（约 21.9 MB）

独立二进制可以直接运行，无需 mbit 工具：

```bash
./target/app
```

## 构建选项

### Debug 模式

默认构建使用 release 模式。如需 debug 模式：

```bash
mbit build --debug
```

适用于调试场景，保留调试信息。

### 交叉编译

为其他平台构建：

```bash
# Linux x86_64
mbit build --standalone --target x86_64-unknown-linux-gnu

# Linux ARM64
mbit build --standalone --target aarch64-unknown-linux-gnu

# Windows x86_64
mbit build --standalone --target x86_64-pc-windows-gnu

# macOS ARM64 (Apple Silicon)
mbit build --standalone --target aarch64-apple-darwin
```

**前提**：安装 `cross` 工具和 Docker/Podman。

详见 [交叉编译指南](CROSS_COMPILATION.md)。

## 测试

### 运行测试

```bash
# 所有测试
mbit test

# 指定包
mbit test -p package_name

# 过滤测试
mbit test -f "test_*"

# Release 模式
mbit test --release

# 生成覆盖率
mbit test --enable-coverage
```

### 运行基准测试

```bash
# 所有基准测试
mbit bench

# Release 模式
mbit bench --release
```

## 项目结构要求

### 标准结构

```
your-project/
├── wit/
│   └── world.wit          # WIT 接口定义
├── src/
│   ├── impl-string.mbt    # 实现文件
│   ├── impl-hash.mbt
│   └── impl-json.mbt
└── moon.mod.json          # MoonBit 模块配置（自动生成）
```

### 实现文件命名约定

mbit 根据文件名自动映射到接口目录：

| 文件名包含 | 目标目录 |
|-----------|---------|
| `string` | `stringUtils/impl.mbt` |
| `hash` | `hashUtils/impl.mbt` |
| `json` | `jsonUtils/impl.mbt` |
| `uuid` | `uuidUtils/impl.mbt` |

**示例**：
- `src/impl-string.mbt` → `gen/interface/.../stringUtils/impl.mbt`
- `src/hash-utils.mbt` → `gen/interface/.../hashUtils/impl.mbt`

## 命令参考

### mbit build

```bash
mbit build [选项]
```

**选项**：
- `--debug` - Debug 模式构建（默认 release）
- `--standalone` - 生成独立可执行文件
- `--target <triple>` - 交叉编译目标平台

### mbit run

```bash
mbit run [wasm文件路径]
```

**参数**：
- `wasm文件路径` - 可选，默认为 `target/component.wasm`

### mbit test

```bash
mbit test [moon test 参数...]
```

透传所有参数给 `moon test`。

### mbit bench

```bash
mbit bench [moon bench 参数...]
```

透传所有参数给 `moon bench`。

### mbit help

```bash
mbit help
```

显示帮助信息。

## 常见问题

### Q: 构建失败，提示找不到 wit/ 目录

**A**: 确保在项目根目录下运行 `mbit build`，且存在 `wit/world.wit` 文件。

### Q: 独立二进制太大

**A**: 默认使用 release 模式，产物约 21.9 MB。如需更小，可以考虑：
- 使用 `wasm-opt` 优化 WASM
- 启用 LTO（Link Time Optimization）

### Q: 交叉编译失败

**A**: 确保已安装 `cross` 和 Docker：
```bash
cargo install cross
docker ps  # 确保 Docker 运行
```

### Q: 如何调试 WASM 组件

**A**: 使用 debug 模式构建，然后用 lldb/gdb 调试：
```bash
mbit build --debug
lldb -- mbit run
```

详见 [调试指南](DEBUGGING.md)。

### Q: 如何查看 WASM 导出函数

**A**: 使用 wasm-tools：
```bash
wasm-tools component wit target/component.wasm
```

或使用 wasm-objdump：
```bash
wasm-objdump -x target/component.wasm
```

## 最佳实践

### 1. 版本控制

```bash
# 提交源码
git add wit/ src/

# 忽略构建产物
# .gitignore 已配置忽略 target/, _build/, gen/ 等
```

### 2. CI/CD 集成

```yaml
# GitHub Actions 示例
- name: Build
  run: |
    cargo build --release
    ./target/release/mbit build
    
- name: Test
  run: ./target/release/mbit test
```

### 3. 开发流程

```bash
# 1. 编写接口
vim wit/world.wit

# 2. 实现功能
vim src/impl-*.mbt

# 3. 构建测试
mbit build
mbit test

# 4. 调试（如需要）
mbit build --debug
lldb -- mbit run

# 5. 发布
mbit build --standalone
```

## 相关文档

- [架构设计](ARCHITECTURE.md)
- [开发文档](DEVELOPMENT.md)
- [调试指南](DEBUGGING.md)
- [交叉编译](CROSS_COMPILATION.md)
