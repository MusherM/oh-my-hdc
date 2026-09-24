# aa start 与 HiLog 采集

使用 build-deploy 中已确认的 `LEASE/BUNDLE/ABILITY/MODULE/RUN_DIR/RUN_ID`。先读同项目 omh skill 的租约和长任务规则。

## 先采集再启动

冷启动需要 force-stop 时先执行并验收，部署阶段已完成则不重复。用 omh 管理日志流：

```bash
omh logs --lease "$LEASE" --output "$RUN_DIR/hilog.log"
omh exec --lease "$LEASE" -- shell aa start -a "$ABILITY" -b "$BUNDLE" -m "$MODULE" 2>&1 | tee "$RUN_DIR/start.log"
```

logs 输出必须是新文件；返回采集信息不等于已取得业务日志，仍需检查新内容、错误和本轮标记。日志属于租约，释放时停止，不手动管理原生 HDC 进程，不将日志输出当作续租。当前 logs 不接受自定义 hilog 参数；不能通过 exec 或脚本绕过此限制。需要额外能力时说明缺口。

## 启动不是测试通过

已实现自定义测试协议时的最短链：确认入口契约 → 一次冷启动 force-stop → 先采集 → 唯一一次带本次 run ID 的 start → 核对启动/PID/目标前台 → 等待本次业务结束标记 → 保存证据 → 释放时由 omh 停止采集。若项目采用 BEGIN/END_OK/END_ERROR，必须按本次ID关联；其他应用使用其真实结束协议，不臆造这些日志。

- 必须检查 `start ability successfully.`。
- `omh exec --lease "$LEASE" -- shell pidof "$BUNDLE"` 取得PID；多进程应用要按真实进程名与日志确认，不能把包名匹配失败直接说成崩溃。
- `omh exec --lease "$LEASE" -- shell aa dump -a` 保存到 `ability.log`，核对目标 bundle/ability 对应的 `FOREGROUND`。不要只在整份 dump 中搜到任意前台词就通过。
- 普通 UI 应用确认目标页面/交互；有自动化业务协议的应用必须等**本次**结束标记/结果断言。

应用只有实现了 Want 参数消费才会执行自定义测试。例如项目支持时：

```bash
omh exec --lease "$LEASE" -- shell aa start -a "$ABILITY" -b "$BUNDLE" -m "$MODULE"   --ps agentAction runBundledImage --ps agentRunId "$RUN_ID"   2>&1 | tee "$RUN_DIR/start.log"
```

这是对普通 start 的**替代命令**，不要两条都执行。`agentAction`/`agentRunId`/`runBundledImage` 是应用约定，不是 aa 内置测试参数。只在 onCreate 读取参数的入口需要冷启动；已运行应用的重复 start 不保证重做测试。优先复用项目现有测试脚本，并显式指定唯一 `/tmp` 输出目录，覆盖其可能写 dev 的旧默认值。

真正的测试 runner 使用 `aa test`，不是 `aa start`：只在已有 ohosTest 配置和项目测试命令时使用。不要给生产应用凭空添加测试 runner 或隐式改业务逻辑。

## 回收与诊断

日志已落在主机文件，不必 recv。按本次时间和唯一 run ID 圈定窗口，保留原始日志，再离线用 `rg` 过滤。不能假设日志时间格式，也不能把历史缓冲中的成功当作本轮成功。

设备已有可读日志文件时用 `omh exec --lease "$LEASE" -- file recv <确切设备路径> <主机RUN_DIR路径>`，检查传输成功及本地非空；不要猜测所有应用都有权限访问 `/data/log`。

区分：启动失败、进程退出、业务 END_ERROR、无结束标记超时、日志限流/丢弃。失败时在有效租约内完成回收后释放；若已超时释放，保留已有日志，不自动重启应用；日志声称丢失时不能保证完整。先查询已保存日志，只有修复代码/输入/环境后才重跑。结束时按 omh 契约释放并核对清理结果；长任务必须在 job 退出前保存业务证据。
