# 设备适配与可恢复采集

## 先发现能力

读取项目配置与已有测量脚本；通过 `omh devices --json` 发现设备并申请租约；按任务显式选择设备。设备探测使用 `omh exec --lease "$LEASE" -- shell ...`，不读取 SDK 路径直接调用。读取设备/系统版本、工具帮助、可读节点、既有 profiler 与前台状态。不要写死历史设备 ID，也不要结束不属于本轮的会话。

发现设备权限不足时报告可用方法与缺口，不自动 root/remount、修改系统库或替换 SDK。HIZ 不可用时，按托管测量参考评估事先建立且保持租约身份的通信路径或外部仪器；不能通过拔线、本地脱管采集绕过 omh。

## 插线状态的供电验证

“关闭充电”不等于“切断 USB 输入”；USB 可能直接供系统，电池电流只反映充放电净额。`Charging` 状态文本也可能滞后。

在部分华为设备上已见以下节点，仅作为探测候选：

```text
/sys/class/hw_power/charger/charge_data/enable_hiz
/sys/class/hw_power/charger/charge_data/enable_charger
/sys/class/hw_power/charger/charge_data/Ibus
/sys/class/power_supply/Battery/current_now
/sys/class/power_supply/Battery/voltage_now
/sys/class/power_supply/Battery/charge_counter
/sys/class/power_supply/Battery/temp
```

同类设备曾出现：`enable_charger=0` 写入后读回仍为 1；`enable_hiz=1` 则使 Ibus=0，电池放电且 HDC 在线。**这不是所有鸿蒙设备的保证，也不是直接执行的固定命令。**

对当前设备确认节点存在、权限、原值和语义后，再使用该方案。必须同时验证：开关读回、输入电流为零、电池放电方向、通信仍在线；排除其他供电路径。写入失败不能当成功。

## 恢复协议

- 先保存供电开关原值和实际屏幕设置，不能一律恢复为 0 或某固定亮度。
- 修改前按 [托管测量](omh-measurement.md) 验证恢复能力。设备端 trap/有限截止时间及主机 finally 不能证明 omh 强制取消后的恢复；禁止脱管 watchdog。缺乏异常恢复证据时不修改供电状态。
- trap 覆盖正常退出和可捕获信号，不声称能覆盖 SIGKILL/掉电；操作完成后必须回读。
- 记录并恢复亮度、自适应亮度、保活/超时等实际改动。目标版本若支持 PowerManagerService 临时保活/恢复命令，先从帮助和当前状态核对；不要借测试改变全局性能模式。
- 停止自己的子进程，有限等待，必要时只结束已记录的本轮 PID；不得 `pkill hdc`。
- 每条设备调用经 omh 且有界（单条最多 10 分钟），复杂命令优先写入唯一远端脚本后执行；避免多层引号导致挂起或意外扩展。
- 回收原始 trace、采样、日志和恢复状态，再删除本轮确切远端文件。恢复失败单独告知，不能由后续成功日志覆盖。

## 工具陷阱

- `SP_daemon` 的参数组合和文件名解析因版本而异；曾见 `-PKG`/`-PID` 互斥及带连字符的输出名被误解析。唯一十六进制文件名可作为已验证版本的规避；新版本先核对帮助。
- `hiprofiler_cmd` 的 session 正常结束与 trace 可解析都要验证；收到文件不等于时间窗完整。
- 跨 checkout 用 Python `-m` 调度时，当前工作目录可能优先于 PYTHONPATH，误导入目标分支旧 runner。显式确定 runner 代码来源与业务 checkout，先用 help/离线检查验证，不要等真机轮才发现参数不兼容。
- 新目录、防旧日志、唯一 run ID、设备时间戳是防止“历史成功”污染的关键；capture 返回码必须传播真实 runner 失败。
