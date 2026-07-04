# 调试指南

## 概述

WASM 组件可以通过多种方式调试，包括使用原生调试器（lldb/gdb）、WASM 专用工具以及浏览器开发者工具。

## 准备

### Debug 模式构建

```bash
# 构建时保留调试信息
mbit build --debug
```

这会：
- 保留 DWARF 调试信息
- 不优化代码（便于调试）
- 保留变量名和源码映射

## 使用 lldb 调试

### 基本流程

```bash
# 1. 启动 lldb
lldb -- mbit run

# 2. 设置断点
(lldb) breakpoint set --name main
(lldb) breakpoint set --file main.rs --line 42

# 3. 运行程序
(lldb) run

# 4. 调试命令
(lldb) next          # 单步执行（不进入函数）
(lldb) step          # 单步进入函数
(lldb) finish        # 执行到函数返回
(lldb) continue      # 继续执行
(lldb) print x       # 打印变量
(lldb) bt            # 查看调用栈
(lldb) frame var     # 查看当前帧变量
```

### 调试 WASM 组件

```bash
# 启动调试
lldb -- mbit run target/component.wasm

# 在 WASM 函数上设置断点
(lldb) breakpoint set --name to_snake_case

# 运行
(lldb) run

# 查看 WASM 调用栈
(lldb) bt all

# 查看 WASM 变量
(lldb) frame var
```

### 脚本化调试

创建 `debug.lldb` 脚本：

```
# 设置断点
breakpoint set --name main
breakpoint set --name to_snake_case

# 运行
run

# 命中断点后的操作
bt
frame var
continue

# 退出
quit
```

运行：

```bash
lldb -s debug.lldb -- mbit run
```

## 使用 gdb 调试

### 基本流程

```bash
# 启动 gdb
gdb --args mbit run

# 设置断点
(gdb) break main
(gdb) break to_snake_case

# 运行
(gdb) run

# 调试命令
(gdb) next           # 单步执行
(gdb) step           # 单步进入
(gdb) finish         # 执行到返回
(gdb) continue       # 继续执行
(gdb) print x        # 打印变量
(gdb) backtrace      # 查看调用栈
(gdb) info locals    # 查看局部变量
```

## WASM 专用工具

### wasm2wat

将 WASM 转换为可读的 WAT 文本格式：

```bash
# 转换整个组件
wasm2wat target/component.wasm > component.wat

# 查看前 50 行
wasm2wat target/component.wasm | head -50
```

### wasm-objdump

查看 WASM 模块信息：

```bash
# 查看所有段
wasm-objdump -h target/component.wasm

# 查看导出函数
wasm-objdump -x target/component.wasm | grep Export

# 查看导入函数
wasm-objdump -x target/component.wasm | grep Import

# 反汇编
wasm-objdump -d target/component.wasm
```

### wasm-tools

查看 Component 接口：

```bash
# 查看 WIT 接口
wasm-tools component wit target/component.wasm

# 验证组件
wasm-tools validate target/component.wasm
```

## 浏览器调试

### Chrome DevTools

1. **准备 WASM 文件**
   ```bash
   mbit build --debug
   ```

2. **创建 HTML 页面**
   ```html
   <!DOCTYPE html>
   <html>
   <head>
       <title>WASM Debug</title>
   </head>
   <body>
       <script>
           async function loadWasm() {
               const response = await fetch('target/component.wasm');
               const bytes = await response.arrayBuffer();
               const { instance } = await WebAssembly.instantiate(bytes);
               console.log('WASM loaded:', instance);
               
               // 调用导出函数
               const result = instance.exports.to_snake_case("HelloWorld");
               console.log('Result:', result);
           }
           
           loadWasm();
       </script>
   </body>
   </html>
   ```

3. **启动本地服务器**
   ```bash
   python3 -m http.server 8000
   ```

4. **在 Chrome 中打开**
   - 访问 `http://localhost:8000`
   - 打开 DevTools（F12）
   - 切换到 Sources 标签
   - 可以找到 WASM 文件并设置断点

### Firefox Developer Tools

类似 Chrome，Firefox 也支持 WASM 调试：
- 打开 DevTools
- 切换到 Debugger 标签
- 找到 WASM 文件
- 设置断点并调试

## VS Code 调试

### 安装扩展

1. **WebAssembly** (by ms-vscode)
2. **C/C++** (by ms-vscode)
3. **CodeLLDB** (by vadimcn)

### 配置 launch.json

```json
{
    "version": "0.2.0",
    "configurations": [
        {
            "type": "lldb",
            "request": "launch",
            "name": "Debug mbit",
            "program": "${workspaceFolder}/target/release/mbit",
            "args": ["run"],
            "cwd": "${workspaceFolder}"
        }
    ]
}
```

### 调试步骤

1. 打开项目
2. 按 F5 启动调试
3. 在代码中设置断点
4. 使用调试工具栏控制执行

## 调试技巧

### 1. 查看 WASM 大小

```bash
ls -lh target/component.wasm
```

### 2. 分析 WASM 结构

```bash
# 查看段信息
wasm-objdump -h target/component.wasm

# 查看函数数量
wasm-objdump -x target/component.wasm | grep "func" | wc -l
```

### 3. 性能分析

```bash
# 使用 wasmtime 的性能分析功能
wasmtime run --profile target/component.wasm

# 生成火焰图
wasmtime run --trace-reservation target/component.wasm
```

### 4. 内存调试

```bash
# 启用 WASM 内存检查
WASMTIME_LOG=wasmtime_runtime=debug mbit run
```

### 5. 日志调试

```bash
# 启用详细日志
RUST_LOG=debug mbit run

# 启用 wasmtime 日志
WASMTIME_LOG=debug mbit run
```

## 常见问题

### Q: 断点无法命中

**A**: 确保使用 `mbit build --debug` 构建，保留调试信息。

### Q: 看不到变量名

**A**: 
- 确认使用 debug 模式构建
- 检查 WASM 是否包含 DWARF 信息：`wasm-objdump --dwarf target/component.wasm`

### Q: WASM 函数名被混淆

**A**: 
- 使用 debug 模式构建
- 或在 WIT 中明确命名函数

### Q: 调试器无法识别 WASM

**A**: 
- 更新 lldb/gdb 到最新版本
- 使用 wasm-objdump 确认 WASM 文件有效

## 高级调试

### 1. 条件断点

```bash
(lldb) breakpoint set --name to_snake_case --condition 'input == "HelloWorld"'
```

### 2. _watchpoint（内存断点）

```bash
(lldb) watchpoint set variable my_var
```

### 3. 反向调试

使用 `rr` 工具（仅 Linux）：

```bash
# 记录执行
rr record mbit run

# 反向调试
rr replay
(rr) reverse-continue
(rr) reverse-step
```

### 4. 核心转储分析

```bash
# 生成核心转储
ulimit -c unlimited
mbit run

# 分析转储
lldb -c core mbit
```

## 相关工具

| 工具 | 用途 | 安装 |
|------|------|------|
| lldb | 原生调试器 | Xcode (macOS) |
| gdb | 原生调试器 | `brew install gdb` |
| wasm2wat | WASM 转文本 | `brew install wabt` |
| wasm-objdump | WASM 分析 | `brew install wabt` |
| wasm-tools | Component 工具 | `cargo install wasm-tools` |
| rr | 反向调试 | Linux only |

## 参考资源

- [WebAssembly Debugging](https://webassembly.org/docs/debugging/)
- [wasmtime 调试指南](https://docs.wasmtime.dev/examples-debugging.html)
- [WASM Debugging in Chrome](https://developer.chrome.com/blog/webassembly-debugging/)
