# mbit 文档索引

## 核心文档

- [架构设计](ARCHITECTURE.md) - 系统架构和设计原理
- [用户指南](USER_GUIDE.md) - mbit 工具使用指南
- [开发文档](DEVELOPMENT.md) - 内部实现细节

## 专项指南

- [调试指南](DEBUGGING.md) - WASM 组件调试方法
- [交叉编译](CROSS_COMPILATION.md) - 多平台构建指南

## 快速开始

```bash
# 构建项目
mbit build

# 运行组件
mbit run

# 生成独立二进制
mbit build --standalone

# 运行测试
mbit test
```

## 项目结构

```
mbit/
├── src/              # Rust 源码
├── docs/             # 文档（本目录）
├── Cargo.toml        # Rust 配置
└── README.md         # 项目说明
```
