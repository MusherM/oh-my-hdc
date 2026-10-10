# oh-my-hdc（`omh`）

English: [README.md](README.md)

让多个编码 agent 共享有限的鸿蒙手机，避免测试操作交错。
核心规则：**申请 → 独占测试 → 清理 → 释放**。

仓库包含 Rust CLI、三个项目级 Codex skill 与命令 hook，以及四目标构建打包流程。
不捆绑 hdc 或鸿蒙 SDK。

## 安装与接入

本仓库每次更新代码后运行 `scripts/init-project.sh`。先将 `OMH_HDC` 指向 SDK
可执行文件（或加入 `PATH`）；如需接入其他项目，将其目录作为首个参数。脚本依次执行
下方步骤。
脚本仅在设备空闲时停止现有 omh 后台进程，使后续查询启动新安装的版本；设备被占用时
会安全退出。

使用当前稳定版 Rust 工具链构建：

```sh
cargo build --locked --release
cargo install --path . --locked
omh --version
omh --hdc /absolute/path/to/hdc devices --json
omh setup codex --project /absolute/path/to/project
```

Windows 使用 `omh.exe` 和 SDK 的 `hdc.exe`，路径与引号遵循 PowerShell 规则。
macOS/Linux 使用相同子命令。基础调度不需要 Python、Node 或管理员权限；
可选 DevEco 自动签名接入需要 Node.js 与官方 DevEco CLI。
请将二进制安装到稳定位置：项目 hook 保存它的绝对路径，移动后需重新运行 setup。

setup 从二进制内嵌资源安装三个完整的 skill 目录：

- `.agents/skills/omh`：共享设备调度与生命周期。
- `.agents/skills/omh-harmonyos-app-dev`：通过 omh 完成鸿蒙开发、部署与 UI 验证。
- `.agents/skills/omh-harmonyos-app-optimization`：测量协议与 omh 托管的功耗、内存实验。

无需预装用户级 skill 或联网下载，用户目录中的普通版保持不变。setup 在 `AGENTS.md`
追加带标记的路由段落，并将 hook 合并到 `.codex/config.toml`；通过明确的项目规则选择
专用版，不依赖同名 skill 的优先级。仅有 `CLAUDE.md` 时在其中追加，并硬链接为
`AGENTS.md`；两者均无时创建该硬链接文件对。保留既有硬链接，不合并两份独立指引。

`.codex/omh-skills.json` 记录安装内容。重复 setup 保持幂等；新版本可更新未被修改的
受管理文件，并补齐缺失资源。用户修改冲突、路由标记异常或目标路径为符号链接时，
在写入前拒绝操作。请保留安装记录供后续升级，明确处理报告的改动后再重试，setup
不会静默覆盖。既有 Codex 配置保持不变的部分会被保留，修改前备份。
`omh doctor --project .` 的 `skill_bundle_current` 检查完整资源和路由段落是否为当前版本；
这是文件检查，不代表运行时保护或实际 skill 选择已经验证。

长测 job 必须在退出前完成基线、业务验收、设置恢复与证据回收。omh 清理不恢复供电或
屏幕设置。测量 skill 不提供通用恢复执行器；未验证取消、超时及断连恢复时，不允许
进行 HIZ 等状态修改。

**重新启动 Codex，信任项目，并通过 `/hooks` 审阅、信任 hook。** 在实际 Codex
任务中要求执行 `hdc --version`：工具调用必须在执行前被拒绝；`omh --version`
应正常运行。安装 skill 或手动运行 hook 都不能证明 Codex 执行路径受到保护。
`omh doctor --project .` 不会仅凭配置文件推断保护生效。
参见 [Codex hook 官方文档](https://learn.chatgpt.com/docs/hooks)。

## 使用设备测试

`omh devices --json` 在每台设备的 `info` 中提供 `name`（设备名称）、`model`
（型号代码）、`chip`（芯片型号）、`brand`（品牌）、`os_version`（系统版本）和
`api_version`（API 版本）。后台仅在设备空闲时读取一次；读不到的字段为 `null`。

编译完成后再申请。多个被测应用重复传入 `--package`；省略 `--device` 时分配任意
空闲手机。`--wait` 支持 `s`、`m`、`h`。

```sh
omh acquire --package com.example.app --device DEVICE_ID --wait 30m --json
omh exec --lease TOKEN -- install /absolute/path/app.hap
omh exec --lease TOKEN -- shell aa start -b com.example.app -a EntryAbility
omh logs --lease TOKEN --output /absolute/path/new-test.log
omh exec --lease TOKEN -- file recv /data/local/tmp/result.json /absolute/path/result.json
omh release --lease TOKEN
```

可用 `OMH_LEASE` 代替 `--lease`，但不要在独立测试之间共享凭证。
命令保留参数边界、工作目录、二进制标准输入输出、标准错误和子进程退出码。
这是管道接口，不是终端模拟器；不支持不带命令的交互式 `hdc shell`。
设备或服务全局选项不能覆盖已分配目标。`exec` 支持安装、卸载、shell、文件发送与
接收、bugreport、jpid，以及本会话拥有的 fport/rport 创建。
hilog 使用 `logs`，全局服务操作使用维护入口。

## DevEco 自动签名

当前适配官方 `@deveco/deveco-cli` **1.2.1**；其他版本在执行前拒绝，需验证兼容性后
再开放。复用本机 CLI、SDK 和登录状态，不修改第三方文件，也不自动更新 CLI。
`omh deveco --cli /path/to/dist/cli.js --node /path/to/node ...` 可指定非 PATH 安装。

项目开发 skill 会在当前 product 未配置签名或材料文件缺失时尝试以下流程；已有
材料则复用。登录无需设备租约：

```sh
omh deveco auth status
omh deveco auth login
```

`auth status` 退出 0 不代表已登录，须检查 `Current user:` / `Not logged in`。
Codex 环境中的登录输出 `omh.deveco.login` 事件，由 agent 立即用 `open_in_codex`
在聊天侧栏打开其中的 URL，保持登录进程等待用户完成认证。非 Codex 环境直接打开
系统默认浏览器。可用 `--browser codex|default` 明确选择；普通终端没有内置侧栏工具
时不要选择 `codex`。页面打开不等于登录成功，须等待官方回调。

在应用根目录、登录成功后申请目标设备并签名：

```sh
omh acquire --package com.example.app --device DEVICE_ID --wait 30m --json
omh deveco signature generate --lease TOKEN --product default --timeout 10m
```

签名需要在首次 signed 构建前短暂读取设备 UDID，是“先构建再申请部署租约”的例外。
此入口创建托管任务并等待结束和清理，完成、失败、超时后自动释放租约；不嵌套在
`omh job` 中。启动时打印任务 ID，可用 `omh job status/cancel` 查询或取消。终端退出
不自动取消托管任务。之后检查材料、构建 signed HAP，并重新申请设备安装。

适配只让官方 CLI 看见租约设备，UDID/设备类型查询经 omh；跨设备、服务维护及
未知调用会失败，设备查询失败不能被 CLI 吞掉后当作成功。默认不传 `--force`，
可按需传 `--team-id`。云端证书/Profile 仍由官方管理，Profile 可能包含账号已注册
的其他设备。签名成功、材料有效、安装成功分别验收。

这不是通用 DevEco 设备代理：`run/ui/log/emulator/serve` 不在适配范围内。
Codex hook 拒绝常见的直接 DevEco 设备操作；它仍是合作性防护，不是系统沙箱。

## 长测试与回收

- 每台手机同时只有一个占用者。按可满足请求的先到先得分配；等待指定忙碌手机
  不会阻塞另一台空闲手机。
- 普通会话连续 **10 分钟**没有设备操作就过期，单条普通命令也最多运行 10 分钟。
  状态查询、标准输入数据和日志输出不刷新占用时间。
- 日志采集属于会话，释放时停止。输出文件必须是新文件，防止覆盖已有证据。
- 长测试使用托管任务，必须指定正数超时，最多 7 天；没有统一的一小时限制，
  也不要求 agent 定期发送心跳。

```sh
omh job start --lease TOKEN --timeout 90m -- sh /absolute/path/test.sh
omh job status RUN_ID --json
omh job cancel RUN_ID
```

Windows 使用已安装的可执行程序，例如 `powershell.exe -File test.ps1`。
任务按参数数组执行，不隐式解释 shell 字符串。调度器提供 `OMH_LEASE`、`OMH_HOME`、
`OMH_DEVICE`、`OMH_BIN`，并将 omh 目录加入 PATH；其他环境变量来自调度器启动环境。
脚本必须通过 omh 操作设备，等待真实业务完成信号，检查结果，并在退出前保存证据。
持续日志输出或单纯 sleep 不能作为测试。不要让子进程后台脱管或留下脱管的设备任务。

提交 CLI 退出后任务继续运行。任务完成、失败、超时或显式取消，都会立即触发清理和
释放。离线分析保存的结果，需要继续操作手机时重新申请。同时检查 `result` 和
`lease_state`：脚本结束不代表清理成功。CLI/基础设施错误使用退出码 125，超时 124，
取消 130；子进程也可能使用这些数字，因此需结合结构化 `reason` 判断。

## 清理与恢复

清理会停止本会话的电脑端进程树与日志采集，移除所属端口转发，然后停止声明的应用。
保留安装与应用数据，不恢复系统设置。只有确认清理完成才能交给下一个会话。
不接管已有的转发规则；清理检查 hdc 成功标记，不只看退出码。
不认识的 SDK 返回格式会保守地判为清理失败。
明确返回 `10104002` 且说明声明的应用未安装时，也确认无需停止该应用；其他错误仍隔离设备。

```sh
omh status --json
omh recover --device DEVICE_ID
omh maintenance restart
omh doctor --project .
```

拔线会使旧凭证失效；重连后不恢复旧测试，也不偷偷替换手机。重连后执行 `recover`
重试清理，查询状态直到 `free`；失败则保持 `blocked` 并说明原因。
恢复不会强删状态记录，也不会按任意 PID 杀进程。

维护会暂停新分配和新长测试，等待已有会话与清理结束，重启共享 hdc 服务一次，再
重新发现设备。请求是异步的，`draining` 不代表成功；重复请求合并。
失败后继续暂停分配，诊断后显式重试。现有交互会话需要完成并释放，维护才能继续。

## 架构与边界

同一操作系统用户的所有项目共用一个自动启动的调度器。本地 TCP 使用随机回环端口
及实例凭证，凭证保存在用户私有数据目录；文件锁保证该目录仅运行一个调度器。
hdc 路径保存在其中并在重启时复用。切换 SDK 前需排空测试并停止调度器。
不要用不同 `--home`/`OMH_HOME` 对同一批手机建立互相竞争的设备池。

数据目录保存崩溃记录、任务参数、标准输出错误和调度器诊断，位置见 `doctor`。
Unix 下目录设为私有；Windows 应使用默认用户目录或具有等效私有 ACL 的目录。
其中含占用凭证，请保持私有。应用证据与任务历史保留，不自动删除历史资料。

每个命令有监督进程，发现调度器管道断开后会取消进程树。调度器重启后，先前占用的
设备会被隔离，需恢复后再分配。如果监督进程自身被强杀，或缺少可信的完成记录，
恢复不会猜测后代进程已经退出：需检查提示的任务目录、处理遗留进程后再使用。
不要删除状态来强制复用。主动脱离进程组的脚本不在支持契约内。系统重启虽然会
停止进程，但不会抹掉这份保守隔离记录。

Codex hook 防止常见直接调用、SDK 绝对路径、命令串与包装调用，**不是沙箱**。
它不会解释任意脚本、Python 子进程调用或已打开交互 shell 中注入的文本。
IDE、人工终端、未接入项目和刻意绕过不受其保护，应避免这些来源同时操作设备。

## 构建、验证与分发

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked -- --test-threads=1
cargo build --locked --release
```

集成测试使用 `rustc` 编译仅依赖标准库的模拟 hdc，不需要 SDK 或手机。每项测试
使用独立临时目录和调度器。只有 debug 构建接受 `OMH_TEST_IDLE_MS` 来缩短回收测试；
release 始终使用 10 分钟。压缩时间测试证明调度规则，不代表已完成真机一小时耐久测试。

setup 测试另覆盖完整内嵌资源、旧版迁移、重复安装、受管理升级、冲突预检、路由链接
与指引文件保留；Unix 下额外检查硬链接身份和符号链接拒绝。UI 树辅助脚本可在本机执行
`uv run --no-project python -B skills/omh-harmonyos-app-dev/scripts/test_ui_tree_inspect.py` 验证。

CI 在 Windows x64、Linux x64、macOS ARM64 和 macOS x64 上分别原生构建、执行测试，
上传带版本号的 ZIP/tar.gz 和 SHA-256 文件。发布包包含二进制、三个 skill 及完整资源、许可证及双语
README，未签名或公证。配置了工作流不等于已经完成远端运行。
带时间的验证范围与未验证项见 `dev/implementation/`。
