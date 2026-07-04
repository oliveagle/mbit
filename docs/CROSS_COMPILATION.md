# 交叉编译指南

## 概述

mbit 支持交叉编译，可以在一个平台上构建出适用于其他平台的独立二进制文件。这通过 `cross` 工具实现，它使用 Docker 容器提供完整的交叉编译环境。

## 支持的目标平台

| 目标平台 | 操作系统 | 架构 | Triple |
|---------|---------|------|--------|
| Linux x86_64 | Linux | x86_64 | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | Linux | aarch64 | `aarch64-unknown-linux-gnu` |
| Windows x86_64 | Windows | x86_64 | `x86_64-pc-windows-gnu` |
| macOS x86_64 | macOS | x86_64 | `x86_64-apple-darwin` |
| macOS ARM64 | macOS | aarch64 | `aarch64-apple-darwin` |

## 前提条件

### 1. 安装 cross

```bash
cargo install cross
```

### 2. 安装 Docker

**macOS**:
```bash
brew install --cask docker
```

**Linux**:
```bash
# Ubuntu/Debian
sudo apt-get install docker.io

# CentOS/RHEL
sudo yum install docker
```

**Windows**:
- 下载 Docker Desktop：https://www.docker.com/products/docker-desktop

### 3. 启动 Docker

```bash
# macOS
open -a Docker

# Linux
sudo systemctl start docker

# 验证 Docker 运行
docker ps
```

## 基本用法

### 交叉编译命令

```bash
# Linux x86_64
mbit build --standalone --target x86_64-unknown-linux-gnu

# Linux ARM64
mbit build --standalone --target aarch64-unknown-linux-gnu

# Windows x86_64
mbit build --standalone --target x86_64-pc-windows-gnu

# macOS x86_64
mbit build --standalone --target x86_64-apple-darwin

# macOS ARM64 (Apple Silicon)
mbit build --standalone --target aarch64-apple-darwin
```

### 输出位置

交叉编译产物位于：

```
target/<target-triple>/release/app
```

例如：
- `target/x86_64-unknown-linux-gnu/release/app`
- `target/x86_64-pc-windows-gnu/release/app.exe`

## 工作原理

### cross 工具

`cross` 是 Rust 的交叉编译工具，它：
1. 使用 Docker 容器提供目标平台的工具链
2. 自动处理依赖和链接
3. 支持自定义 Docker 镜像

### 编译流程

```
mbit build --standalone --target <triple>
    ↓
生成临时 Rust 项目
    ↓
使用 cross 编译
    ↓
cross 启动 Docker 容器
    ↓
容器内使用目标平台工具链编译
    ↓
输出到 target/<triple>/release/app
```

## 实战示例

### 示例 1：在 macOS 上构建 Linux 版本

```bash
# 确保 Docker 运行
docker ps

# 构建 Linux 版本
cd /path/to/your-project
mbit build --standalone --target x86_64-unknown-linux-gnu

# 产物
ls -lh target/x86_64-unknown-linux-gnu/release/app

# 测试（需要 Linux 环境）
docker run --rm -v $(pwd)/target/x86_64-unknown-linux-gnu/release/app:/app ubuntu /app
```

### 示例 2：构建 Windows 版本

```bash
# 构建 Windows 版本
mbit build --standalone --target x86_64-pc-windows-gnu

# 产物
ls -lh target/x86_64-pc-windows-gnu/release/app.exe

# 测试（需要 Windows 环境或 Wine）
wine target/x86_64-pc-windows-gnu/release/app.exe
```

### 示例 3：批量构建多平台

创建 `build-all.sh` 脚本：

```bash
#!/bin/bash

set -e

TARGETS=(
    "x86_64-unknown-linux-gnu"
    "aarch64-unknown-linux-gnu"
    "x86_64-pc-windows-gnu"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
)

for target in "${TARGETS[@]}"; do
    echo "Building for $target..."
    mbit build --standalone --target "$target"
    echo "✓ Built for $target"
done

echo "All builds completed!"
```

运行：

```bash
chmod +x build-all.sh
./build-all.sh
```

## CI/CD 集成

### GitHub Actions

```yaml
name: Cross-platform Build

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    runs-on: ubuntu-latest
    
    strategy:
      matrix:
        target:
          - x86_64-unknown-linux-gnu
          - aarch64-unknown-linux-gnu
          - x86_64-pc-windows-gnu
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Install cross
        run: cargo install cross
      
      - name: Build
        run: |
          mbit build --standalone --target ${{ matrix.target }}
      
      - name: Upload artifact
        uses: actions/upload-artifact@v3
        with:
          name: app-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/app*
```

### macOS 构建（需要 macOS runner）

```yaml
  build-macos:
    runs-on: macos-latest
    
    strategy:
      matrix:
        target:
          - x86_64-apple-darwin
          - aarch64-apple-darwin
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Build
        run: |
          mbit build --standalone --target ${{ matrix.target }}
      
      - name: Upload artifact
        uses: actions/upload-artifact@v3
        with:
          name: app-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/app
```

## 自定义 cross 配置

### Cross.toml

在项目根目录创建 `Cross.toml`：

```toml
[target.x86_64-unknown-linux-gnu]
image = "ghcr.io/cross-rs/x86_64-unknown-linux-gnu:main"

[target.aarch64-unknown-linux-gnu]
image = "ghcr.io/cross-rs/aarch64-unknown-linux-gnu:main"

[target.x86_64-pc-windows-gnu]
image = "ghcr.io/cross-rs/x86_64-pc-windows-gnu:main"
```

### 自定义 Docker 镜像

如果需要额外的依赖，可以自定义镜像：

```dockerfile
# Dockerfile.x86_64-unknown-linux-gnu
FROM ghcr.io/cross-rs/x86_64-unknown-linux-gnu:main

# 安装额外依赖
RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config
```

构建并使用：

```bash
docker build -f Dockerfile.x86_64-unknown-linux-gnu -t my-cross-image .
```

在 `Cross.toml` 中指定：

```toml
[target.x86_64-unknown-linux-gnu]
image = "my-cross-image:latest"
```

## 常见问题

### Q: cross 构建失败，提示 Docker 未运行

**A**: 启动 Docker：
```bash
# macOS
open -a Docker

# Linux
sudo systemctl start docker
```

### Q: 交叉编译的 Windows 版本无法运行

**A**: 
- 确认使用了正确的目标 triple：`x86_64-pc-windows-gnu`
- 检查是否缺少 Windows 依赖
- 在 Windows 环境中测试

### Q: macOS 交叉编译失败

**A**: 
- macOS 交叉编译需要 macOS host
- 不能在 Linux/Windows 上交叉编译 macOS 版本
- 使用 GitHub Actions 的 macOS runner

### Q: 构建速度很慢

**A**: 
- cross 首次运行需要拉取 Docker 镜像
- 后续构建会使用缓存
- 可以使用 `cross build --release` 优化产物

### Q: 如何查看 cross 使用的 Docker 镜像

**A**: 
```bash
cross build --target x86_64-unknown-linux-gnu -v
```

## 高级用法

### 1. 并行构建

```bash
# 使用 GNU parallel 并行构建
parallel mbit build --standalone --target ::: \
    x86_64-unknown-linux-gnu \
    aarch64-unknown-linux-gnu \
    x86_64-pc-windows-gnu
```

### 2. 自定义构建脚本

```bash
#!/bin/bash
# build-release.sh

set -e

VERSION=${1:-"0.1.0"}
OUTPUT_DIR="release/$VERSION"

mkdir -p "$OUTPUT_DIR"

targets=(
    "x86_64-unknown-linux-gnu"
    "aarch64-unknown-linux-gnu"
    "x86_64-pc-windows-gnu"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
)

for target in "${targets[@]}"; do
    echo "Building for $target..."
    mbit build --standalone --target "$target"
    
    # 复制产物
    if [[ "$target" == *"windows"* ]]; then
        cp "target/$target/release/app.exe" "$OUTPUT_DIR/app-$target.exe"
    else
        cp "target/$target/release/app" "$OUTPUT_DIR/app-$target"
    fi
    
    echo "✓ Built for $target"
done

echo "All builds completed! Output: $OUTPUT_DIR"
```

### 3. 压缩产物

```bash
# 使用 upx 压缩二进制
upx --best target/x86_64-unknown-linux-gnu/release/app

# 使用 zip 打包
zip -j release/app-linux-x86_64.zip \
    target/x86_64-unknown-linux-gnu/release/app \
    README.md
```

## 性能对比

| 平台 | 构建时间 | 产物大小 |
|------|---------|---------|
| Linux x86_64 | ~2 分钟 | ~22 MB |
| Linux ARM64 | ~3 分钟 | ~22 MB |
| Windows x86_64 | ~2 分钟 | ~22 MB |
| macOS x86_64 | ~1 分钟 | ~21 MB |
| macOS ARM64 | ~1 分钟 | ~21 MB |

## 参考资源

- [cross 官方文档](https://github.com/cross-rs/cross)
- [Rust 交叉编译指南](https://rust-lang.github.io/rustup/concepts/archives.html)
- [Docker 文档](https://docs.docker.com/)
