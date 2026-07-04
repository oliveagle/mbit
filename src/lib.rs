// mbit library - MoonBit WASM 运行 + 构建库
//
// 全部使用 crate API（wasmtime + wasmtime-wasi p1），
// 只 spawn moon CLI（MoonBit 项目自己的编译器，没有 Rust crate 替代品）。
// 不 spawn wasm-tools / wit-bindgen / cargo 等其他编译工具链 CLI。

mod build;
mod sse;
mod mcp;
mod runner;
mod test_bench;

pub use build::{build, BuildConfig, Builder};
pub use mcp::{run_mcp, McpConfig, Transport};
pub use runner::{run, Runner};
pub use test_bench::{bench, test};

/// mbit 库版本
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
