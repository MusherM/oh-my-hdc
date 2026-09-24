# 编译、签名与部署

## 发现配置

从项目 AGENTS、README、`AppScope/app.json5`、模块 `src/main/module.json5`、`build-profile.json5`、`hvigorfile.ts` 确定应用根目录、bundleName、module/ability、product、设备类型和签名。原始 JSON5 允许注释/尾逗号，不能直接 `json.load`。

本机构建工具优先级：项目现有脚本 > 项目 wrapper > 已安装 DevEco 工具。复用脚本前核对其设备操作全部经过 omh。macOS 常见根目录 `/Applications/DevEco-Studio.app/Contents`，它是候选位置，不是跨机器保证。Windows 从实际 DevEco 安装目录找 node.exe、JBR、SDK 和 hdc.exe；PowerShell 设置 `$env:JAVA_HOME`、`$env:DEVECO_SDK_HOME`、`$env:Path`，用 `&` 调用带空格路径。Linux 使用实际可用的 SDK/wrapper，不假设存在 macOS App bundle。

## 可改参数的 macOS/Bash 模板

在一个 Bash 会话设置变量；用已核对的实际值替换占位符，未替换前不要执行后续代码。

```bash
set -euo pipefail
APP_ROOT='<实际鸿蒙应用根目录>'
DEVECO_ROOT='/Applications/DevEco-Studio.app/Contents'
MODULE='entry'       # 从项目配置核对
PRODUCT='default'
DEVICE_TYPE='phone'
BUNDLE='<实际 bundleName>'
ABILITY='<实际 abilityName>'
RUN_ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
RUN_DIR="/tmp/harmony-$RUN_ID"
mkdir "$RUN_DIR"
export JAVA_HOME="$DEVECO_ROOT/jbr/Contents/Home"
export DEVECO_SDK_HOME="$DEVECO_ROOT/sdk"
export PATH="$DEVECO_ROOT/tools/node/bin:$JAVA_HOME/bin:$PATH"
NODE="$DEVECO_ROOT/tools/node/bin/node"
HVIGOR="$DEVECO_ROOT/tools/hvigor/bin/hvigorw.js"
# 修改上述变量后逐个检查工具存在；Windows/Linux 使用各自实际路径。
```

确认签名配置引用的证书/profile/keystore可读，且目标设备在授权范围内；不得把含密码的配置整段输出。日常不 clean，只有依赖缺失才执行项目要求的 ohpm 安装。

```bash
cd "$APP_ROOT"
"$NODE" "$HVIGOR" --mode module -p "module=$MODULE@default"   -p "product=$PRODUCT" -p "requiredDeviceType=$DEVICE_TYPE" assembleHap   --analyze=normal --parallel --incremental --daemon   2>&1 | tee "$RUN_DIR/build.log"
```

必须退出码0且日志含 `BUILD SUCCESSFUL`；在真实 outputs 目录找到本次配置的 signed HAP，验证非空、时间/缓存一致性及 SHA-256。常见路径是 `<module>/build/<product>/outputs/<target>/<module>-<target>-signed.hap`，不要盲猜复杂项目输出名。`assembleHap` 配置签名后会包含 SignHap；`UP-TO-DATE` 只是缓存复用。构建失败不得安装旧 HAP。

## 选择设备与安装

完整验收链：build退出码0 + BUILD SUCCESSFUL → 新产物/缓存一致性及非空signed HAP → FileTransfer finish → install bundle successfully. → start ability successfully. → 目标PID/前台 → 本次业务结果；任一前层失败即停止。启动/日志的具体步骤见下一参考，不能跳过后半段。

```bash
omh doctor --project "$APP_ROOT"
omh devices --json
# 按用户目标选择；明确任意空闲设备均可时才省略 --device。
omh acquire --package "$BUNDLE" --device '<已选择设备 ID>' --wait 30m --json
# 从申请结果取得租约；不把真实凭证写入报告或共享脚本。
LEASE='<本会话返回的租约>'
HAP='<已验证的 signed.hap 绝对路径>'
test -s "$HAP"
REMOTE_DIR="/data/local/tmp/harmony-$RUN_ID"
```

确认租约有效且声明了所有被测包；接入与 hook 验证遵循 omh skill。下列命令**逐条执行并检查后再继续**，不是忽略错误的一整段脚本：

```bash
omh exec --lease "$LEASE" -- shell aa force-stop "$BUNDLE" 2>&1 | tee "$RUN_DIR/force-stop.log"
omh exec --lease "$LEASE" -- shell mkdir "$REMOTE_DIR" 2>&1 | tee "$RUN_DIR/mkdir.log"
omh exec --lease "$LEASE" -- file send "$HAP" "$REMOTE_DIR/app.hap" 2>&1 | tee "$RUN_DIR/send.log"
omh exec --lease "$LEASE" -- shell bm install -p "$REMOTE_DIR" 2>&1 | tee "$RUN_DIR/install.log"
```

- force-stop失败需辨明是否仅是首次安装/应用未运行，其他异常停止。
- mkdir失败停止，不复用同名旧目录；send必须包含 `FileTransfer finish`。
- 安装必须包含 `install bundle successfully.`，失败先诊断签名、profile、版本兼容和设备空间，不自动卸载数据。
- 多模块/共享 HSP 按项目配置一起打包推送，不能只装一个 entry HAP 就宣称完整部署。
- 安装验收后只删除 `$REMOTE_DIR` 的本次文件；严格确认变量来自本次 UUID、非空且位于 `/data/local/tmp/harmony-`，禁止拼出宽泛删除路径。失败时保留现场待定位。
- 然后转 [launch-logs.md](launch-logs.md)，必须先采集再启动。

PowerShell 的外部命令管道没有 Bash pipefail：`& $NODE ... 2>&1 | Tee-Object -FilePath $BuildLog` 后立即检查 `$LASTEXITCODE`，且仍核对成功文本；不要把 `$?` 当作全部业务验收。

普通测试完成或失败后，由租约所有者在有效租约内完成必要恢复与证据回收，再执行 `omh release --lease "$LEASE"` 并检查清理结果；不要因中途命令失败而遗留租约。已过期的凭证不能用于恢复操作，遵循 omh 故障流程。托管 job 自动释放，必须在 job 退出前完成恢复与回收；不要在 job 内提前 release。
