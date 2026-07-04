# mbit - MoonBit Component Model 构建工具

## 项目概述

mbit 是 MoonBit Component Model 的通用构建和运行工具，简化 WASM 组件的开发、构建和部署。

## 技术栈

- **语言**: Rust (edition 2021)
- **WASM 运行时**: wasmtime v46.0.1 (Component Model)
- **WASI**: wasmtime-wasi v46.0.1 (Preview 2)
- **绑定生成**: wit-bindgen v0.58.0
- **WASM 工具**: wasm-tools v1.252.0
- **MoonBit 编译器**: moon v0.1.20260618

## 项目结构

```
mbit/
├── src/
│   ├── main.rs        # CLI 入口（薄层）
│   ├── lib.rs         # 库公共 API
│   ├── build.rs       # 构建逻辑
│   ├── runner.rs      # WASM 运行时
│   └── test_bench.rs  # 测试/基准测试
├── docs/              # 文档
├── scripts/           # 版本管理脚本
├── .github/workflows/ # CI/CD
├── VERSION            # 版本号 (single source of truth)
└── Cargo.toml         # 同时包含 [[bin]] 和 [lib]
```

## 构建

```bash
cargo build          # 开发构建
cargo build --release  # 发布构建
cargo test           # 运行测试
cargo clippy         # 代码检查
cargo fmt            # 格式化
```

## 发布流程

1. 更新 `VERSION` 文件
2. 运行 `scripts/bump_version.sh patch`
3. 创建 GitHub Release（CI 自动构建多平台产物）
4. GitHub Actions 自动编译并上传各平台二进制

## 注意事项

- 所有代码文件不超过 500 行
- 默认 release 模式构建
- 支持交叉编译（需要 cross + Docker）
- wasmtime v46 API 使用 `WasiCtxView` 结构体
