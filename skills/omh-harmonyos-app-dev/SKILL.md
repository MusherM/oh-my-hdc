---
name: omh-harmonyos-app-dev
description: >-
  Develop, debug and test HarmonyOS / OpenHarmony applications in omh-enabled projects.
  Use for app coding, pages/components, Hvigor/DevEco builds, HAP signing/install,
  omh-managed deployment, launching, logs, screenshots and UI component trees. Requires omh; use standalone harmonyos-app-dev elsewhere.
  Also use for implicit follow-ups such as “编译一下”“装到手机”“抓日志”“看组件树”
  when the active task or target module is already a HarmonyOS app.
  Require an explicit HarmonyOS app-development intent or confirmed HarmonyOS project context
  (ArkTS .ets plus hvigorfile/build-profile and module.json5); generic hdc/aa/component/screenshot
  words, a mixed repository alone, Harmony phone shopping/news, Android/ADB, iOS/Swift,
  web/React, pure ONNX conversion and unrelated scripts do not trigger it.
---

# 鸿蒙应用开发与真机验证

## 项目设备契约

先读取同项目 [omh](../omh/SKILL.md)，所有设备操作经 omh；接入缺失或异常时先修复，不回退到 SDK hdc、其他连接器或内部绕过调度的脚本。通常先构建再申请部署租约；缺少签名时，按 build-deploy 参考先登录并申请短签名租约，通过 omh 自动签名，释放后构建。按目标设备选择，不覆盖租约绑定。普通会话在释放前完成验收、恢复和证据回收；长流程使用托管 job，并在退出前完成这些步骤。租约、超时与清理规则以 omh skill 为准。

## 路由与范围

1. 明确要开发/修改/调试鸿蒙应用，即使还没有项目，也使用本 skill。当前任务已是鸿蒙开发的简短后续请求也继续使用。
2. 请求未说明平台时，检查目标模块的 `hvigorfile.ts`、`build-profile.json5`、`src/main/module.json5` 与 `.ets`。不能因为仓库里有鸿蒙子目录就接管整个仓库的其他任务。
3. 排除 Android/ADB、iOS、Web、鸿蒙手机资讯/选购、仅模型导出或普通脚本任务。跨平台项目只对鸿蒙子任务应用本 skill。
4. 使用 skill 不代表每次都构建部署：语法解释只查相关文档；代码变更做适当编译验证；安装、启动、采集以任务已授权范围为准，不重复询问已明确授权。
5. `arkts-syntax-assistant` 可补充语言/迁移规则；实际编译入口以当前项目配置为准，不机械调用另一 skill 的重新装依赖脚本。

## 按需读取

- 编译、签名、设备选择、推送安装：[build-deploy.md](references/build-deploy.md)。
- `aa start`、冷启动参数、HiLog、PID/前台与业务验收：[launch-logs.md](references/launch-logs.md)。
- 截屏、回收文件、UI 组件树、层级与属性、交互定位：[ui-inspection.md](references/ui-inspection.md)。
- 命令来源及版本边界：[sources.md](references/sources.md)。
- 已回收 UI JSON 用 [ui_tree_inspect.py](scripts/ui_tree_inspect.py) 展示父子层级、类型、ID、文本与边界；节点数只是附加摘要。

## 执行契约

- 先读目标项目 AGENTS；发现真实 app 根目录、module/product、bundleName、abilityName、SDK、签名路径，不复制示例包名/设备 ID。JSON5 不用标准 JSON 解析器强行读取。
- Python 使用 `uv`；复用现有 DevEco Node/JBR/SDK、依赖、签名与增量缓存。没有依据不 clean、不重装、不打印签名密码。
- 每个耗时/有副作用步骤只执行一次并落盘；日志/JSON/截图临时放唯一 `/tmp/harmony-<UUID>/`，不要污染项目目录。最终有价值报告才放 `dev/`。
- 同一设备上的部署、测试、采集串行；不要中断其他运行中的测试或 profiler。需要特定设备时通过 `omh acquire --device` 选择；设备操作只使用返回的租约，不再传 HDC 目标参数。
- 构建→签名产物→传输→安装→启动→PID/前台→业务结束→可视验收，逐层检查；前层成功不代表后层成功。
- HDC 退出码不充分，核对远端输出和新产物；第一次失败先分析已有日志，不自动反复安装/启动。
- 只删除本次创建的准确设备路径；不清空系统 HiLog，不默认关闭隐私/限流，不 root/remount、不改系统工具。
- 报告注明具体通过层级、失败根因、证据路径与未验证项；UI 树统计不宣称是源码组件数、渲染节点总数或整个应用全部页面。
