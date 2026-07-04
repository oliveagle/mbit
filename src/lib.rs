// mbit library - MoonBit Component Model 构建和运行库
//
// 提供构建、运行、测试 MoonBit WASM 组件的功能。
// 可作为库在其他 Rust 项目中使用。

mod build;
mod runner;
mod test_bench;

pub use build::{build, BuildConfig, Builder};
pub use runner::{run, Runner};
pub use test_bench::{bench, test};

/// mbit 库版本
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
