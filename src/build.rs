// 构建模块 - MoonBit 项目编译为 WASM Component

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 构建配置
#[derive(Debug, Clone)]
pub struct BuildConfig {
    /// 是否生成独立可执行文件
    pub standalone: bool,
    /// 交叉编译目标平台 (e.g. "x86_64-unknown-linux-gnu")
    pub target: Option<String>,
    /// 是否使用 release 模式 (默认 true)
    pub release: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            standalone: false,
            target: None,
            release: true,
        }
    }
}

/// Builder 模式用于配置构建选项
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

    pub fn standalone(mut self, standalone: bool) -> Self {
        self.config.standalone = standalone;
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.config.target = Some(target.into());
        self
    }

    pub fn release(mut self, release: bool) -> Self {
        self.config.release = release;
        self
    }

    pub fn build_config(self) -> BuildConfig {
        self.config
    }
}

/// 构建 MoonBit 项目成 WebAssembly Component
pub fn build(project_dir: impl AsRef<Path>, config: &BuildConfig) -> Result<()> {
    let project_dir = project_dir.as_ref();
    let src_dir = project_dir.join("src");
    let wit_dir = project_dir.join("wit");
    let target_dir = project_dir.join("target");

    println!("=== mbit build ===");
    println!("项目目录: {}", project_dir.display());

    if !wit_dir.exists() {
        anyhow::bail!("找不到 wit/ 目录");
    }
    if !src_dir.exists() {
        anyhow::bail!("找不到 src/ 目录");
    }

    std::fs::create_dir_all(&target_dir)?;

    // 1. 检查 moon.mod.json
    let moon_mod = project_dir.join("moon.mod.json");
    if !moon_mod.exists() {
        println!("\n[1/6] 初始化 MoonBit 模块");
        run_cmd("moon", &["init"], project_dir)?;
    } else {
        println!("\n[1/6] MoonBit 模块已存在");
    }

    // 2. wit-bindgen 生成 MoonBit 绑定
    println!("\n[2/6] 生成 MoonBit 绑定 (wit-bindgen)");
    run_cmd(
        "wit-bindgen",
        &[
            "moonbit",
            "wit/world.wit",
            "--out-dir",
            ".",
            "--derive-eq",
            "--derive-show",
            "--derive-error",
        ],
        project_dir,
    )?;

    // 3. 复制 src/*.mbt 到 gen/ 目录
    println!("\n[3/6] 注入实现文件");
    let gen_dir = find_gen_dir(project_dir)?;
    copy_impl_files(&src_dir, &gen_dir)?;
    update_moon_pkg_files(&gen_dir)?;

    // 4. 添加依赖
    println!("\n[4/6] 添加依赖");
    run_cmd("moon", &["add", "moonbitlang/x@0.4.46"], project_dir).ok();

    // 5. 编译 MoonBit → WASM
    println!(
        "\n[5/6] 编译 MoonBit → WASM (wasm-gc){}",
        if config.release { " [release]" } else { "" }
    );
    if config.release {
        run_cmd(
            "moon",
            &["build", "--target", "wasm", "--release"],
            project_dir,
        )?;
    } else {
        run_cmd("moon", &["build", "--target", "wasm"], project_dir)?;
    }

    // 6. 打包成 Component
    println!("\n[6/6] 打包成 WebAssembly Component");
    let wasm_file = find_built_wasm(project_dir)?;
    let output_wasm = target_dir.join("component.wasm");

    run_cmd(
        "wasm-tools",
        &[
            "component",
            "embed",
            "wit",
            &wasm_file.to_string_lossy(),
            "--encoding",
            "utf16",
            "--output",
            &target_dir.join("temp.wasm").to_string_lossy(),
        ],
        project_dir,
    )?;

    run_cmd(
        "wasm-tools",
        &[
            "component",
            "new",
            &target_dir.join("temp.wasm").to_string_lossy(),
            "--output",
            &output_wasm.to_string_lossy(),
        ],
        project_dir,
    )?;

    let _ = std::fs::remove_file(target_dir.join("temp.wasm"));

    let size = std::fs::metadata(&output_wasm)?.len();
    println!(
        "\n✓ 构建完成\n  输出: {} ({:.1} KB)",
        output_wasm.display(),
        size as f64 / 1024.0
    );

    if config.standalone {
        println!("\n=== 生成独立可执行文件 ===");
        standalone_build(project_dir, &wit_dir, &output_wasm, config)?;
    } else {
        println!("\n运行: mbit run");
    }

    Ok(())
}

/// 生成独立可执行文件
fn standalone_build(
    project_dir: &Path,
    wit_dir: &Path,
    wasm_path: &Path,
    config: &BuildConfig,
) -> Result<()> {
    let host_dir = project_dir.join("target/host");
    std::fs::create_dir_all(&host_dir)?;
    std::fs::create_dir_all(host_dir.join("src"))?;
    std::fs::create_dir_all(host_dir.join("wit"))?;

    // 1. 复制 WIT 文件
    println!("[1/3] 复制 WIT 定义");
    copy_dir_recursive(wit_dir, &host_dir.join("wit"))?;

    // 2. 生成 Cargo.toml
    println!("[2/3] 生成 Rust host 项目");
    let cargo_toml = r#"[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
wasmtime = { version = "46", features = ["component-model"] }
wasmtime-wasi = "46"
anyhow = "1"
"#;
    std::fs::write(host_dir.join("Cargo.toml"), cargo_toml)?;

    // 3. 生成 main.rs（嵌入 WASM）
    let main_rs = r#"// 自动生成的独立可执行文件
use anyhow::Result;
use wasmtime::{Engine, Store};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView, WasiCtxView};
use wasmtime_wasi::p2::add_to_linker_sync;

const WASM_BYTES: &[u8] = include_bytes!("../component.wasm");

struct HostCtx {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for HostCtx {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

fn main() -> Result<()> {
    let engine = Engine::new(wasmtime::Config::new().wasm_component_model(true))?;
    let component = Component::new(&engine, WASM_BYTES)?;

    let mut store = Store::new(
        &engine,
        HostCtx {
            wasi: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::new(),
        },
    );

    let mut linker = Linker::new(&engine);
    add_to_linker_sync(&mut linker)?;

    let instance = linker.instantiate(&mut store, &component)?;

    if let Some(run_func) = instance.get_func(&mut store, "run") {
        run_func.call(&mut store, &[], &mut [])?;
    } else {
        println!("组件已加载（库组件，无 run 函数）");
    }

    Ok(())
}
"#;
    std::fs::write(host_dir.join("src/main.rs"), main_rs)?;
    std::fs::copy(wasm_path, host_dir.join("component.wasm"))?;

    // 4. 编译
    println!(
        "[3/3] 编译独立二进制{}",
        if config.release { " [release]" } else { "" }
    );

    let (binary_name, output_name) = if let Some(ref target_triple) = config.target {
        println!("  目标平台: {}", target_triple);

        if Command::new("cross").arg("--version").output().is_err() {
            anyhow::bail!(
                "交叉编译需要安装 cross\n运行: cargo install cross\n并确保 Docker 或 Podman 已安装"
            );
        }

        if config.release {
            run_cmd(
                "cross",
                &["build", "--release", "--target", target_triple],
                &host_dir,
            )?;
        } else {
            run_cmd("cross", &["build", "--target", target_triple], &host_dir)?;
        }

        let ext = if target_triple.contains("windows") {
            ".exe"
        } else {
            ""
        };
        (
            format!("app{}", ext),
            format!("app-{}{}", target_triple, ext),
        )
    } else {
        if config.release {
            run_cmd("cargo", &["build", "--release"], &host_dir)?;
        } else {
            run_cmd("cargo", &["build"], &host_dir)?;
        }

        let ext = if cfg!(windows) { ".exe" } else { "" };
        (format!("app{}", ext), format!("app{}", ext))
    };

    // 复制到 target/
    let mode = if config.release { "release" } else { "debug" };
    let src_binary = if let Some(ref target_triple) = config.target {
        host_dir
            .join("target")
            .join(target_triple)
            .join(mode)
            .join(&binary_name)
    } else {
        host_dir.join("target").join(mode).join(&binary_name)
    };

    let dst_binary = project_dir.join(format!("target/{}", output_name));
    std::fs::copy(&src_binary, &dst_binary)?;

    let size = std::fs::metadata(&dst_binary)?.len();
    println!(
        "\n✓ 独立二进制已生成\n  输出: {} ({:.1} MB)\n\n运行: ./target/{}",
        dst_binary.display(),
        size as f64 / 1024.0 / 1024.0,
        output_name
    );

    Ok(())
}

// ========== 辅助函数 ==========

fn run_cmd(cmd: &str, args: &[&str], cwd: &Path) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("执行 {} 失败，请确保已安装", cmd))?;

    if !status.success() {
        anyhow::bail!("{} 退出码: {}", cmd, status.code().unwrap_or(-1));
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn find_gen_dir(project_dir: &Path) -> Result<PathBuf> {
    let gen_dir = project_dir.join("gen/interface");
    if !gen_dir.exists() {
        anyhow::bail!("找不到 gen/interface/ 目录");
    }

    for entry in std::fs::read_dir(&gen_dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            for sub_entry in std::fs::read_dir(entry.path())? {
                let sub_entry = sub_entry?;
                if sub_entry.path().is_dir() {
                    return Ok(sub_entry.path());
                }
            }
        }
    }

    anyhow::bail!("找不到 gen 目录下的接口目录")
}

fn copy_impl_files(src_dir: &Path, gen_dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("mbt") {
            let filename = path.file_stem().unwrap().to_str().unwrap();

            if let Some(subdir) = filename_to_subdir(filename) {
                let target = gen_dir.join(subdir).join("impl.mbt");
                if target.parent().map(|p| p.exists()).unwrap_or(false) {
                    std::fs::copy(&path, &target).with_context(|| {
                        format!("复制 {} → {} 失败", path.display(), target.display())
                    })?;
                    println!(
                        "  {} → {}/impl.mbt",
                        path.file_name().unwrap().to_str().unwrap(),
                        subdir
                    );
                }
            }
        }
    }
    Ok(())
}

fn filename_to_subdir(filename: &str) -> Option<&str> {
    let lower = filename.to_lowercase();
    if lower.contains("string") {
        Some("stringUtils")
    } else if lower.contains("hash") {
        Some("hashUtils")
    } else if lower.contains("json") {
        Some("jsonUtils")
    } else if lower.contains("uuid") {
        Some("uuidUtils")
    } else {
        None
    }
}

fn update_moon_pkg_files(gen_dir: &Path) -> Result<()> {
    let configs = [
        (
            "hashUtils",
            r#"{ "warn-list": "-44", "import": ["moonbitlang/x/crypto"] }"#,
        ),
        (
            "jsonUtils",
            r#"{ "warn-list": "-44", "import": ["moonbitlang/core/json"] }"#,
        ),
        (
            "uuidUtils",
            r#"{ "warn-list": "-44", "import": ["moonbitlang/x/crypto"] }"#,
        ),
    ];

    for (subdir, config) in configs {
        let pkg_file = gen_dir.join(subdir).join("moon.pkg.json");
        if pkg_file.parent().map(|p| p.exists()).unwrap_or(false) {
            std::fs::write(&pkg_file, config)?;
        }
    }
    Ok(())
}

fn find_built_wasm(project_dir: &Path) -> Result<PathBuf> {
    let build_dir = project_dir.join("_build/wasm/debug/build");
    if !build_dir.exists() {
        anyhow::bail!("找不到 _build/ 目录");
    }

    for entry in std::fs::read_dir(&build_dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            let gen_wasm = entry.path().join("gen.wasm");
            if gen_wasm.exists() {
                return Ok(gen_wasm);
            }
        }
    }

    anyhow::bail!("找不到编译产物 gen.wasm")
}
