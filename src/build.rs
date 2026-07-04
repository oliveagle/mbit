// Build module: MoonBit → core wasm 编译流水线
//
// 设计原则：
//   - moon CLI 是 MoonBit 项目自己的编译器，没有可替代的 crate；
//     spawn moon 是不可避免的（这是 moon 工具链自己的 binary）。
//   - 其他所有步骤（产物定位、验证）全部用 crate API（wasmtime）。
//   - 不 spawn wasm-tools / wit-bindgen / cargo。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

macro_rules! bprintln {
    ($cfg:expr, $($arg:tt)*) => {
        if !$cfg.quiet { println!($($arg)*); }
    };
}


/// 构建配置
#[derive(Debug, Clone)]
pub struct BuildConfig {
    /// 是否使用 release 模式 (默认 true)
    pub release: bool,
    /// 交叉编译目标平台 (e.g. "x86_64-unknown-linux-gnu")
    pub target: Option<String>,
    /// 安静模式：抑制 stdout 进度日志
    pub quiet: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            release: true,
            target: None,
            quiet: false,
        }
    }
}

pub struct Builder {
    config: BuildConfig,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        Self {
            config: BuildConfig::default(),
        }
    }

    pub fn release(mut self, release: bool) -> Self {
        self.config.release = release;
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.config.target = Some(target.into());
        self
    }
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.config.quiet = quiet;
        self
    }

    pub fn build_config(self) -> BuildConfig {
        self.config
    }
}

/// MoonBit 项目编译入口
///
/// 流水线：
///   1. moon build [--target wasm] [--release]  →  core wasm
///   2. 定位编译产物
///   3. 复制到 target/<basename>.wasm
///   4. 用 wasmtime 验证 module 可加载
pub fn build(project_dir: impl AsRef<Path>, config: &BuildConfig) -> Result<()> {
    let project_dir = project_dir.as_ref();
    let target_dir = project_dir.join("target");
    let src_dir = project_dir.join("src");

    bprintln!(config, "=== mbit build ===");
    bprintln!(config, "项目目录: {}", project_dir.display());

    if !src_dir.exists() {
        anyhow::bail!("找不到 src/ 目录");
    }
    std::fs::create_dir_all(&target_dir)?;

    // 1. 调用 moon 编译 MoonBit → core wasm
    bprintln!(config, "\n[1/4] moon build --target wasm");
    let mut cmd = Command::new("moon");
    cmd.arg("build").arg("--target").arg("wasm");
    if config.release {
        cmd.arg("--release");
    } else {
        cmd.arg("--debug");
    }
    if let Some(t) = &config.target {
        cmd.arg("--target").arg(t);
    }
    let status = cmd
        .current_dir(project_dir)
        .status()
        .context("moon 编译失败（确认 moon 在 PATH 中）")?;
    if !status.success() {
        anyhow::bail!("moon build 退出码非零");
    }

    // 2. 找到编译产物
    bprintln!(config, "[2/4] 定位编译产物");
    let mode = if config.release { "release" } else { "debug" };
    let core_wasm = find_built_wasm(project_dir, mode)?;
    bprintln!(config, "  core wasm: {}", core_wasm.display());

    // 3. 复制到 target/ 目录
    bprintln!(config, "[3/4] 复制到 target/");
    let file_name = core_wasm
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("无法取产物文件名"))?;
    let target_wasm = target_dir.join(file_name);
    std::fs::copy(&core_wasm, &target_wasm)?;
    bprintln!(config, "  output: {}", target_wasm.display());

    // 4. 用 wasmtime 验证 module 可加载
    bprintln!(config, "[4/4] wasmtime 验证 module 可加载");
    let engine = wasmtime::Engine::new(wasmtime::Config::new().wasm_gc(true))?;
    let _module = wasmtime::Module::from_file(&engine, &target_wasm)
        .map_err(|e| anyhow::anyhow!("wasmtime 无法加载 module: {}", e))?;
    bprintln!(config, "  验证通过");

    bprintln!(config, "\n✓ 构建完成");
    bprintln!(config, "  运行 MCP: mbit mcp --stdio {}", target_wasm.display());

    Ok(())
}

fn find_built_wasm(project_dir: &Path, mode: &str) -> Result<PathBuf> {
    // 优先尝试 _build/wasm/<mode>/build（moon build --target wasm 默认）
    let primary = project_dir.join(format!("_build/wasm/{}/build", mode));
    if primary.exists() {
        return find_wasm_in(&primary);
    }
    // 兼容 _build/wasm-gc/<mode>/build（moon build --target wasm-gc）
    let alt = project_dir.join(format!("_build/wasm-gc/{}/build", mode));
    if alt.exists() {
        return find_wasm_in(&alt);
    }
    anyhow::bail!(
        "找不到 _build/wasm/{}/build 或 _build/wasm-gc/{}/build，\
         确认 moon build --target wasm 成功",
        mode,
        mode
    )
}

fn find_wasm_in(dir: &Path) -> Result<PathBuf> {
    // MoonBit 编译产物命名约定：<pkg>.wasm，可能在多级子目录
    for entry in walkdir_iter(dir)? {
        if entry.extension().and_then(|s| s.to_str()) == Some("wasm")
            && !entry
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.contains("test"))
                .unwrap_or(true)
        {
            return Ok(entry);
        }
    }
    anyhow::bail!("{} 中找不到非测试 .wasm 产物", dir.display())
}

fn walkdir_iter(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}
