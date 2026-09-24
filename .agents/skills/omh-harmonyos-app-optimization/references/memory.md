# 内存占用与 NPU/DMA

## 首选口径

- 用已在目标系统核验的 `hidumper --mem <PID>` Total PSS 作为应用内存主指标，并记录其是否包含 Graph/GL/设备缓冲。
- 同步记录 `/proc/<PID>/smaps_rollup` 的 Pss/Rss/SwapPss，以及 `/proc/<PID>/status` 的 VmRSS/VmHWM，作为审计而非同义替代。
- 单列 SwapPss；Total 是否已经包含 swap 需核实，避免重复加。未采集到 swap 时写 unavailable，不能默认零。
- 单位：内核 kB 通常按 KiB 解释但仍需核对。KiB × 1024 / 10⁶ = MB；KiB / 1024 = MiB。不要把 MiB 数字直接标为 MB。

## NPU、图形和外部服务

1. 比较 HiDumper 与 /proc。明显差额优先检查 Graph/GL/Dma，而不是断言某一工具错误。
2. 使用目标系统支持的 dmabuf 明细命令（先查 `hidumper --help`，如有 `--show-dmabuf`）查看缓冲区 exporter、大小和归属，关联 NPU 权重/特征图分配。
3. 某些设备上 Graph 与 Dma 是同一批缓冲区的两个展示位置，且 Total 已包含 Graph。先核对重叠，禁止 `Total + Graph + Dma` 或 `PSS + Graph + Dma` 的重复计算。
4. 可用 `/proc PSS + 不重叠且可归属的 Graph/GL` 重建值交叉检查，但采样时刻差会影响结果；不要把不同瞬间的分项峰值相加冒充同时发生的总峰值。
5. 外部服务、驱动共享缓冲、多进程归属可能不完整。多进程应用按同一时间窗明确进程集合，避免共享内存和设备缓冲的跨进程重复计入；不能直接照搬单 PID 结论。

## 采样与统计

- 冷启动后重新获得 PID，校验每条样本所属 PID；进程消失时不沿用旧值。
- 在查询前后记录设备时间戳，可用中点代表观测时刻，同时保存查询耗时。命令末尾 sleep 0.05 秒不代表实际 20 Hz，查询本身可能耗时数百毫秒。
- 用真实采样间隔进行时间加权平均。仅覆盖区间积分；缺失启动区间写覆盖率。长缺口拆为不连续有效区间，不能只用首末时间宣称完全覆盖。
- 内存峰值为任务内最大有效观测，可能漏短尖峰。VmHWM 是进程历史 RSS 高水位，可能属于窗口之前，且不含与 Total 相同的 NPU/DMA 口径。
- 功耗轮不要同步高密度 HiDumper/SmartPerf；内存轮保持业务与资产相同，但电量另列为 profiler 干扰观察值。

## SmartPerf 交叉验证

检查实际 `SP_daemon --help` 与输出：某些版本 `-PKG` 和 `-PID` 不能组合；长会话按包名跟踪可能在冷启动后仍缓存旧 PID。可在每次单次查询前刷新 PID，但返回时间戳和内存实际观测仍可能滞后。

比较共同覆盖窗口上的 HiDumper Total、SmartPerf PSS 和 /proc 重建值。前两者可能共享底层来源，一致性不等于独立校准。不要把 SmartPerf 启动滞后产生的低值当峰值或缺失区间补零。

## 占用与泄漏区分

一次任务的峰值/平均值只能说明占用，不足以证明泄漏。泄漏需在相同场景循环与稳定回收点比较，结合 ArkTS heap 的保留引用链、Native 分配栈、smaps 分类或设备缓冲生命周期。静态 NAPI/RAII 审查有助定位原因，但不能替代真实占用数据。
