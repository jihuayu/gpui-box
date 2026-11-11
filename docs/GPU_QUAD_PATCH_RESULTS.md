# 单矩形颜色局部 GPU 上传原型与实测

2026-11-11，在 `codex/rendering-performance-research` 分支完成。
原型通过 `gpu-prototype` 和 `gpui_wgpu/quad-patch-prototype` 启用，复用 GPUI 的 Quad、shader、pipeline 和 headless 读回接口。

## 结果

**30,000 个固定矩形中只改一个颜色时，局部路径将单次实例上传从 4,800,000 bytes 降到 160 bytes，CPU 更新 p50 从 1.586 ms 降到 0.235 ms，提交至 GPU 完成的整个更新耗时从 2.281 ms 降到 0.763 ms。**

这组数字来自三轮各 201 次更新，每轮先分别取 p50，再取三轮中位数。
硬件为 NVIDIA GTX 1650 Ti，Vulkan，NVIDIA driver 571.96。目标为 1200×400 RGBA8 offscreen texture。
CPU 和 OS 与前一轮相同，Ryzen 7 4800H、Windows 11 LTSC build 26100。
完整工具链、power plan、源码 SHA-256 与原始样本见[硬件证据目录](performance/2026-11-11-gpu/README.md)。

| 矩形数 | 路径 | 实例上传 bytes | CPU 上传 μs | CPU 更新 μs | 更新完成 μs | GPU 绘制 μs |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 300 | `full` | 48,000 | 31.50 | 201.50 | 433.20 | 7.84 |
| 300 | `retained_full` | 48,000 | 26.80 | 166.60 | 391.20 | 7.84 |
| 300 | `patch` | 160 | 18.20 | 152.80 | 364.70 | 8.00 |
| 3,000 | `full` | 480,000 | 201.10 | 447.40 | 928.00 | 43.14 |
| 3,000 | `retained_full` | 480,000 | 199.70 | 395.10 | 861.70 | 43.01 |
| 3,000 | `patch` | 160 | 27.00 | 210.70 | 669.10 | 59.36 |
| 30,000 | `full` | 4,800,000 | 905.20 | 1,585.50 | 2,280.90 | 213.02 |
| 30,000 | `retained_full` | 4,800,000 | 898.30 | 1,130.90 | 1,776.20 | 213.31 |
| 30,000 | `patch` | 160 | 30.00 | 235.40 | 762.70 | 213.02 |

`full` 调用原 `render_frame`，包含 globals 更新、完整实例上传、binding 创建与 Scene batch 遍历。
`retained_full` 保留 buffer 和 binding，使用固定绘制编码，每次上传整个数组。
`patch` 使用同样的保留资源和绘制编码，只写目标 slot。
三条路径都清空整个目标并绘制全部矩形。

在 30,000 个矩形规模，`patch` 相对原完整路径的 CPU 更新中位数降低约 85.2%，更新完成耗时降低约 66.6%。
相对共享相同保留资源的 `retained_full` 对照，CPU 更新降低约 79.2%，完成耗时降低约 57.1%。
后一个比较更接近上传范围缩小本身的收益，前一个还包括复用 binding、固定 globals、避免完整 Scene 遍历等收益。

GPU 绘制中位数在 30,000 项规模约为 213 μs，三条路径接近。
30,000 项每轮 CPU 更新 p95 的范围为完整路径 1.831–2.449 ms、局部路径 0.321–0.329 ms。
更新完成 p95 的范围分别为 2.836–4.056 ms 和 1.217–1.239 ms。
3,000 项时局部路径为 59.36 μs，完整路径为 43.14 μs，局部路径的 draw 时间更长。
这一现象在之前的正式采样中也出现，可能涉及 buffer 内容更新范围带来的 cache/driver 行为，当前计时无法单独归因。
它表明局部上传对 CPU 和完成时间的收益不要求 GPU render pass 本身更快。

## 原型实现

`QuadPatchPrototype` 拥有一个固定 Scene、独立 storage buffer、稳定 binding 和 offscreen target。
`QuadHandle` 带有 session 身份和 slot，typed setter 只允许有限且归一化的不透明 HSLA。
布局、几何、裁剪、绘制顺序和对象数量在 session 内固定，结构变化通过重建 session 处理。
同值写入立即返回，实例写入、绘制与提交计数均为零。

GPU 更新使用 `queue.write_buffer(buffer, slot * 160, quad_bytes)`。
同一 queue 保证先前提交的读取与后续写入、绘制顺序。buffer 由 session 独占，样本逐次等待 GPU 完成。
写入失败会使 session 进入错误状态，后续更新要求重建，避免部分更新被当成有效基线。
句柄校验覆盖跨 session 和重建后的旧句柄。

原型模块中的 raw-byte 转换复用现有后端的 GPU ABI helper，边界遵循现有 Quad 的无 padding 布局契约。
引擎修改记录在 [DOWNSTREAM_CHANGES.md](../vendor/gpui/DOWNSTREAM_CHANGES.md)。

## 正确性验证

每种规模执行 6 次不同 slot 的颜色更新，覆盖首个、中间、末尾与重复修改，共 18 次。
每次将局部路径整幅像素读回，再使用当前 CPU Scene 经原完整路径重画并读回，逐字节比较 1200×400×4 bytes。
18 次比较全部一致，每次恰好只有目标矩形的 9 个像素变化。
测量结束时，三条路径的最终整幅像素也一致。

硬件程序同时验证以下边界条件。

- 超出范围的 slot 被拒绝。
- 跨 session 句柄和重建前的旧句柄被拒绝。
- NaN 颜色、半透明颜色被拒绝。
- 同值写入返回零上传。
- 5,427 个测量样本的实际 instance write 均为一次，局部 payload 均为 160 bytes，全量 payload 均等于 N × 160。

证据保留实际读取的 18 张 PNG、validation.json、adapter.json、metadata.json、逐样本 CSV 和按轮分位数。

## 计时范围

每次更新的 CPU 计时包含 setter 内的 CPU 属性赋值、上传、编码与提交。
`upload_ns` 是 CPU 调用成本，原完整路径包含创建 instance bindings。
`completion_ns` 从同一更新开始计至 device poll 确认提交完成，包含 host/driver 等待成本。
GPU timestamp 设在 render pass 首尾，覆盖 clear 和 drawing，upload copies 位于 pass 之前。
timestamp resolve/copy 与 draw 在同一提交，timestamp map/read 在 completion 计时之后。
截图、PNG 编码和像素比较均在计时样本之外。

三轮各 201 样本、三个规模、三条路径，合计 5,427 样本。每条路径预热 40 次。
同一逻辑更新内轮换三条路径的执行位置。数据保留每轮单次更新的 p50/p95/p99。
本机使用平衡电源计划，未锁频或控制后台负载，单次微秒数字存在系统噪声。

本轮测量从已建立的固定 Scene 开始，Signal 传播、组件构建、Taffy 布局、Scene 重录和窗口 Present 需要另行接入。
每次等待完成限制为一个在途提交，连续多帧 pipeline 的吞吐与调度表现仍待验证。
这次运行选择 Windows 上的 WGPU Vulkan，原生 Windows D3D11 与 Android 实机需要各自测量。

## 复现与检查

```powershell
./tooling/rendering-perf/run-gpu.ps1 -Output target/quad-patch-gpu -Samples 201 -Rounds 3
```

参数、三条路径、时序及结果字段详见[运行说明](../tooling/rendering-perf/GPU_PROTOTYPE.md)。
原型测试配置的 5 项现有测试、Clippy `-D warnings`、Rust 格式检查通过。
`gpu-backend` 配置成功编译 WGPU 的原型关闭路径。
曾直接从根 workspace 以 `-p gpui_wgpu` 检查这个 excluded vendored crate，本机 Cargo fork 在 feature resolver 中 panic。
随后通过消费该依赖的根 workspace 包 `gpui-box-rendering-perf --features gpu-backend` 完成同配置编译验证。
