# 架构设计

## 系统概述

mbit 是一个 MoonBit Component Model 的通用构建和运行工具，简化 WASM 组件的开发、构建和部署流程。

## 核心组件

### 1. 构建系统

```
mbit build
    ↓
读取 wit/world.wit (WIT 接口定义)
    ↓
读取 src/*.mbt (MoonBit 实现)
    ↓
wit-bindgen 生成绑定
    ↓
moon build --target wasm (编译为 WASM)
    ↓
wasm-tools component (打包为 Component)
    ↓
输出 target/component.wasm
```

### 2. 独立二进制生成

```
mbit build --standalone
    ↓
生成 Rust host 项目 (target/host/)
    ↓
嵌入 WASM (include_bytes!)
    ↓
cargo build --release
    ↓
输出 target/app (独立可执行文件)
```

### 3. 运行时

```
mbit run
    ↓
加载 component.wasm
    ↓
wasmtime v46 实例化
    ↓
调用导出函数
```

## 技术栈

| 组件 | 技术 | 版本 |
|------|------|------|
| WASM 运行时 | wasmtime | 46.0.1 |
| WASI 支持 | wasmtime-wasi | 46.0.1 |
| 组件模型 | Component Model | Preview 2 |
| 绑定生成 | wit-bindgen | 0.58.0 |
| WASM 工具 | wasm-tools | 1.252.0 |
| MoonBit 编译器 | moon | 0.1.20260618 |

## 设计原则

### 1. 通用性

- 支持任意 WIT 接口定义
- 自动识别实现文件（按命名约定）
- 不绑定特定业务逻辑

### 2. 简化流程

- 一键构建：`mbit build`
- 一键运行：`mbit run`
- 一键打包：`mbit build --standalone`

### 3. 灵活性

- 支持 debug/release 模式
- 支持交叉编译
- 支持测试和基准测试

## 数据流

### 构建流程

```
wit/world.wit ──┐
                ├─→ wit-bindgen ─→ gen/ ─┐
src/*.mbt ──────┘                        ├─→ moon build ─→ _build/
                                         │                    ↓
                                         └─→ wasm-tools ─→ target/component.wasm
```

### 运行流程

```
target/component.wasm
        ↓
   wasmtime 加载
        ↓
   实例化组件
        ↓
   调用导出函数
        ↓
   返回结果
```

## 模块结构

### mbit 工具

```rust
src/main.rs
├── cmd_build()        # 构建命令
├── cmd_run()          # 运行命令
├── cmd_test()         # 测试命令
├── cmd_bench()        # 基准测试命令
└── generate_standalone_host()  # 生成独立二进制
```

### 关键特性

1. **动态 WIT 解析**：读取项目目录的 wit/world.wit
2. **智能文件映射**：根据文件名自动映射到接口目录
3. **嵌入式编译**：使用 `include_bytes!` 嵌入 WASM
4. **跨平台支持**：通过 `cross` 工具实现交叉编译

## 性能优化

### Release 模式

- 默认使用 release 模式构建
- 优化 WASM 代码生成
- 减小编译产物大小（21.9 MB vs 87.9 MB）

### WASM 优化

- wasm-gc 垃圾回收
- 组件模型零开销抽象
- 高效的字符串处理

## 扩展性

### 添加新接口

1. 在 `wit/world.wit` 定义接口
2. 在 `src/` 创建对应实现文件（遵循命名约定）
3. 运行 `mbit build`

### 自定义构建

- 修改 `generate_standalone_host()` 自定义 host
- 调整 WASI 配置
- 添加额外的依赖

## 安全考虑

- WASM 沙箱隔离
- WASI 权限控制
- 无外部文件系统访问（默认）
