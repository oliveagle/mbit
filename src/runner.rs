// 运行模块 - WebAssembly Component 运行时

use anyhow::Result;
use std::path::Path;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::p2::add_to_linker_sync;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

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

/// WASM Component 运行器
pub struct Runner {
    engine: Engine,
}

impl Runner {
    pub fn new() -> Result<Self> {
        let engine = Engine::new(wasmtime::Config::new().wasm_component_model(true))?;
        Ok(Self { engine })
    }

    /// 运行指定的 WASM 组件文件
    pub fn run(&self, component_path: impl AsRef<Path>) -> Result<()> {
        let component_path = component_path.as_ref();

        if !component_path.exists() {
            anyhow::bail!("组件不存在: {}", component_path.display());
        }

        println!("加载组件: {}", component_path.display());

        let component = Component::from_file(&self.engine, component_path)?;

        let mut store = Store::new(
            &self.engine,
            HostCtx {
                wasi: WasiCtxBuilder::new().inherit_stdio().build(),
                table: ResourceTable::new(),
            },
        );

        let mut linker = Linker::new(&self.engine);
        add_to_linker_sync(&mut linker)?;

        let instance = linker.instantiate(&mut store, &component)?;

        if let Some(run_func) = instance.get_func(&mut store, "run") {
            println!("\n调用 run 函数...");
            run_func.call(&mut store, &[], &mut [])?;
            println!("运行完成");
        } else {
            println!("\n组件已加载");
            println!("注意: 这是一个库组件，需要通过 host 代码调用其导出的接口");
        }

        Ok(())
    }

    /// 运行默认路径的组件 (target/component.wasm)
    pub fn run_default(&self, project_dir: impl AsRef<Path>) -> Result<()> {
        let component_path = project_dir.as_ref().join("target/component.wasm");
        if !component_path.exists() {
            anyhow::bail!(
                "组件不存在: {}\n请先运行: mbit build",
                component_path.display()
            );
        }
        self.run(&component_path)
    }
}

/// 便捷函数：运行指定路径的组件，None 时使用默认路径
pub fn run(project_dir: impl AsRef<Path>, wasm_path: Option<&Path>) -> Result<()> {
    let runner = Runner::new()?;
    match wasm_path {
        Some(p) => runner.run(p),
        None => runner.run_default(project_dir),
    }
}
