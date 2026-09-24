---
name: omh-harmonyos-app-optimization
description: >-
  Measure and compare HarmonyOS/OpenHarmony app power, charge consumption and memory
  on omh-managed real devices. Requires an omh-enabled project; use standalone harmonyos-app-optimization elsewhere. Use for 鸿蒙应用功耗测试、
  耗电归因、插线/USB供电干扰、mAh、峰值/平均电流、峰值/平均内存、NPU/DMA内存、
  benchmark整批测试、分支资源占用对比 and validating before/after resource optimizations.
  Covers HDC, battery gauges, HiDumper, SmartPerf and XPower with workload completion
  gates, baseline subtraction and sampling coverage. Not for generic ArkTS coding,
  UI-only automation, model conversion, or static leak review without measurement.
---

# 鸿蒙应用功耗与内存测量

以真实业务完成为验收依据，交付可复查的整批耗电量、峰值/平均电流、峰值/平均内存。把“整机电池变化”“扣基线的任务增量”“系统应用归因模型”分开；不要把任意一个称为独立测得的 NPU 功耗。

## 项目设备契约

首次测量先读同项目 [omh](../omh/SKILL.md) 和 [托管测量](references/omh-measurement.md)。所有设备发现、探测、采样命令与文件回收都经 omh；既有 runner 也必须满足这一点。接入异常不回退到原生 HDC。开发部署结合项目内 omh-harmonyos-app-dev，不能加载普通版的直接 HDC 操作示例。

## 按需读取

- 制定任务、重复试验、完成验收、报告：[实验协议](references/protocol.md)。首次测量先读。
- 电量计、电流积分、XPower、基线和单位：[功耗方法](references/power.md)。
- HiDumper、SmartPerf、PSS/RSS、NPU/DMA：[内存方法](references/memory.md)。
- 插线 HIZ 探测、采集生命周期、工具兼容陷阱：[设备适配](references/device-adapter.md)。变更设备/系统/工具版本时先读。
- 区分外部技能的真实能力与目录宣传：[外部技能对照](references/ecosystem.md)。

若需构建、部署、启动或 HiLog，结合已安装的 `omh-harmonyos-app-dev`，复用项目正常构建和业务 runner。技术接口不确定时查当前官方文档和目标设备 `--help`，不把参考中的设备实例当跨版本契约。

## 执行顺序

1. **定义测量任务。** 发现目标 checkout、提交、包/Ability、已构建 HAP、模型与输入清单；记录哈希、完整输入数量和顺序。用户指定整套测试集时跑完整套，不拿单图乘张数代替。明确冷启动一次连续处理，还是每张冷启动；前者通常更贴合批量使用。
2. **建立可比条件。** 明确包含初始化、预处理、推理、后处理与切换的窗口；记录亮度、温度、电量、网络、性能模式与前后台。保留各分支原有模型/输入尺寸时，明确它们是同一业务任务比较，未必计算量或输出质量相同。
3. **先验收安装。** HAP 与源资产一致 → 构建证据有效 → 传输/安装成功 → 启动/PID/前台。安装完成后再开始测量。跳过安装参数只表示复用，不能证明设备正在运行哪份 HAP；另存对应部署证据。
4. **排除 USB 干扰。** 验证实际输入路径与电池放电；HIZ 只有在托管测量约束内验证了正常、取消、超时及断连恢复后才可用；不能仅凭 trap/finally 推定安全。不能排除外部供电时，禁止把电池净变化当应用总耗电；仅采用事先确认可保持调度身份与连接的通信方式或外部仪器，并说明新边界；不得在租约中直接拔线或私自切换连接。
5. **拆分采集轮次。** 功耗轮保持低干扰，交叉采电量计、sysfs 电流、可用的 XPower；内存轮独立采 HiDumper、/proc、SmartPerf。不要把密集内存工具运行时的电量混入正式功耗均值。
6. **覆盖整批和前后基线。** 先启动采集，再由唯一 run ID 触发一次业务。使用设备时间戳对齐，覆盖首个业务 BEGIN 到本批终止标记，另留前后空闲区间；从实际最大采样间隔评估覆盖，不只看配置频率。
7. **严格验收。** 同时检查 runner 退出码、批次完成、每项索引/哈希/结果、PID、前台、采样与 trace 完整性。区分“全部尝试完成”和“全部成功”；业务失败的成本保留，基础设施失败不混入正式均值。失败先查已有证据。
8. **重复和交叉核查。** 为比较预先安排重复整批，至少两轮作为初步波动检查；需要更强结论再增加次数。交替/平衡分支顺序并控制温度。观察工具干扰；不同读数接近只表示一致性，不证明准确，尤其注意共用电量计的数据源。
9. **恢复并报告。** 停止并回收本次采集进程，恢复原供电和屏幕设置并回读，保存错误。按协议交付五项指标、每轮值/范围、时长、成功率、覆盖率、单位、采样局限与方法选择依据。

## 必须守住的边界

- mAh 是电荷量，不是 W/mW；能量用 mWh/J 时需包含电压。电流 mA 的正负和节点缩放必须实测核对。
- 扣空闲基线的电池增量包含任务引起的 NPU、CPU、内存和系统服务变化，也可能残留后台干扰；不能称为纯应用或纯 NPU 硬件计量。
- XPower 应用分项若没有可验证的 NPU/DDR 覆盖，不把它当完整 NPU 应用耗电真值；不由“无独立分项”推断“一定完全漏计”。
- 内存优先用已核验含图形/设备缓冲的 Total PSS；单独 /proc PSS 可能漏 NPU DMA。Graph/Dma 重叠时禁止再次相加。VmHWM 是历史 RSS 峰值，不能冒充总 PSS 峰值。
- 缺失窗口不填零、不跨长缺口插值；采样峰值只是观测下界。只报告实际覆盖区间的时间加权平均。
- 不在 skill 中保存单次测量数值、固定设备 ID、应用包名或项目绝对路径；这些属于项目报告和本轮 manifest。

## 产物

保留项目内的真实 runner 和设备适配脚本，不把带硬编码路径的实验脚本原样搬成全局工具。本 skill 提供协议与适配要点，不声称附带跨设备通用采集器。临时 trace/日志/JSON 存本轮唯一临时目录；最终报告按项目约定保存，并注明时间、提交、范围和状态。
