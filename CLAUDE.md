# 项目工作约定

- 每次更新本项目代码后，直接执行 `scripts/init-project.sh`，依照 README 的流程构建、安装、查询设备并更新本项目 Codex 接入，方便用户立即测试。
- 运行前确保 `OMH_HDC` 指向 SDK 可执行文件，或已将其加入 `PATH`。耗时命令先将日志落在 `/tmp`；检查退出码、版本、设备查询和 setup 结果，失败时如实报告。
- 真实设备操作必须经 `omh` 调度。设备清单中的新增信息字段须先征得用户选择，不自行扩充输出。

<!-- omh:skills:start -->
## omh 项目技能路由

- 本项目鸿蒙应用开发、构建、部署及 UI 验证使用 [omh-harmonyos-app-dev](.agents/skills/omh-harmonyos-app-dev/SKILL.md)。
- 本项目真实功耗、耗电及内存测量使用 [omh-harmonyos-app-optimization](.agents/skills/omh-harmonyos-app-optimization/SKILL.md)。
- 所有设备操作遵循 [omh](.agents/skills/omh/SKILL.md) 的租约、托管任务和清理契约；不得直接执行其他 skill 或脚本中的原生 HDC 操作。接入异常先修复，不能绕过调度。
- 按任务读取上述项目版本，不依赖同名 skill 的覆盖优先级；普通 harmonyos-app-dev / harmonyos-app-optimization 不作为本项目的设备执行入口。纯本机构建不占用设备。
<!-- omh:skills:end -->
