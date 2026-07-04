// 测试和基准测试模块

use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// 运行 MoonBit 测试 (透传参数给 moon test)
pub fn test(project_dir: impl AsRef<Path>, args: &[String]) -> Result<()> {
    let project_dir = project_dir.as_ref();
    println!("=== mbit test ===");

    let moon_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let status = Command::new("moon")
        .arg("test")
        .args(&moon_args)
        .current_dir(project_dir)
        .status()
        .map_err(|e| anyhow::anyhow!("执行 moon test 失败，请确保 moon 已安装: {}", e))?;

    if !status.success() {
        anyhow::bail!("moon test 退出码: {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

/// 运行 MoonBit 基准测试 (透传参数给 moon bench)
pub fn bench(project_dir: impl AsRef<Path>, args: &[String]) -> Result<()> {
    let project_dir = project_dir.as_ref();
    println!("=== mbit bench ===");

    let moon_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let status = Command::new("moon")
        .arg("bench")
        .args(&moon_args)
        .current_dir(project_dir)
        .status()
        .map_err(|e| anyhow::anyhow!("执行 moon bench 失败，请确保 moon 已安装: {}", e))?;

    if !status.success() {
        anyhow::bail!("moon bench 退出码: {}", status.code().unwrap_or(-1));
    }
    Ok(())
}
