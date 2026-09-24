# 截屏与 UI 组件树

最短验收链：保存设备帮助和 `aa dump -a` → 核对目标前台 → 导出/回收原始树并验证 → 导出/回收截图并解码检查 → 对照层级与属性 → 仅删除本次设备临时文件。树与截图分开验收；页面可能变化，记录采集时间，不能假定二者是原子快照。

先核对目标应用处于前台，避免把桌面、锁屏或系统弹窗误当成目标页面。设备已选择，使用 `LEASE/BUNDLE/RUN_DIR/RUN_ID`；临时输出全部放 `/tmp`。先保存 `omh exec --lease "$LEASE" -- shell uitest help`，参数以设备能力为准。

## 截屏与回收

```bash
REMOTE_SCREEN="/data/local/tmp/harmony-$RUN_ID.png"
omh exec --lease "$LEASE" -- shell uitest screenCap -p "$REMOTE_SCREEN"   2>&1 | tee "$RUN_DIR/screen-capture.log"
omh exec --lease "$LEASE" -- file recv "$REMOTE_SCREEN" "$RUN_DIR/screen.png"   2>&1 | tee "$RUN_DIR/screen-recv.log"
```

逐步确认截图命令成功、FileTransfer finish、本地文件非空且可解码，再用可用图片查看工具检查页面。截图黑屏/空白可能是锁屏、安全窗口或渲染时机问题，不宣称 UI 验收成功。回收后仅删除本次 REMOTE_SCREEN。

若设备没有 uitest screenCap，而提供 snapshot_display，可使用经设备验证的：

```bash
omh exec --lease "$LEASE" -- shell snapshot_display -f "$REMOTE_SCREEN"   2>&1 | tee "$RUN_DIR/snapshot.log"
```

然后同样 file recv。注意 `snapshot_display -h` 在实测设备上是**高度参数**，不是帮助；不要自动尝试它来查询帮助。已有能力不失败就不再重复截图。

## 获取目标窗口的 UI 树

```bash
REMOTE_LAYOUT="/data/local/tmp/harmony-$RUN_ID.json"
omh exec --lease "$LEASE" -- shell uitest dumpLayout -b "$BUNDLE" -p "$REMOTE_LAYOUT"   2>&1 | tee "$RUN_DIR/layout-dump.log"
omh exec --lease "$LEASE" -- file recv "$REMOTE_LAYOUT" "$RUN_DIR/layout.json"   2>&1 | tee "$RUN_DIR/layout-recv.log"
```

`-b` 是版本相关选项：不支持时按帮助用 `-w <真实windowId>`；两者都不支持则只能导出前台树，并明确统计未按包名隔离，不能静默宣称是目标应用。不要把系统窗口数量算入目标应用的组件数。

- 默认树通常做可见性过滤和窗口合并；设备支持 `-i` 时可导出未过滤/不合并树，统计时注明模式，不能和默认模式直接比较。
- `-a` 请求额外字体属性；官方当前文档指出 `-a` 与 `-i` 不同时使用。其他可选扩展必须先检查版本帮助。
- `-d` 指定显示屏、`-w` 指定窗口，均从实际可见窗口/显示信息选择，不能写死历史 ID。
- file recv 成功后检验 JSON 有有效 UI 节点再查看层级；获取错误、未知 schema 和无节点返回错误，不把失败计为0。

`SKILL_DIR` 是当前加载的本 skill 绝对目录。

脚本默认打印可读的父子树：类型、id、文本、bounds，以及导出中真实存在的 clickable/enabled/visible。文本通过 JSON 引号转义，避免多行内容破坏缩进；不把缺失字段补成false。

```bash
uv run python "$SKILL_DIR/scripts/ui_tree_inspect.py" "$RUN_DIR/layout.json" \
  > "$RUN_DIR/component-tree.txt"
# 可选：保留规范化完整属性与子树，附加节点数/类型/深度摘要。
uv run python "$SKILL_DIR/scripts/ui_tree_inspect.py" "$RUN_DIR/layout.json" --format json \
  > "$RUN_DIR/component-tree.json"
```

支持 `attributes` + `children`、直接节点属性 + `children`，以及列表或 `root`/`roots`/`windows`/`hierarchy`/`nodes`/`tree`/`windowTree` 包装。仅类型节点进入规范化树；无类型合成包装被折叠，原始 layout.json 必须保留用于核对真实窗口结构。未知 schema、空树或失败返回错误，不把metadata误当组件。

树中的重复id可以是不同节点，不能去重。先按路径/父节点/类型/文本定位，再核对bounds与截图；不能因树中有目标文本就推断它可点击。要精确定位用本次属性与窗口上下文，别复用上次的坐标。

## 组件树口径与交互

这是**该时刻、该窗口、该导出模式暴露的自动化 UI 组件树**。不是源码中 @Component 的定义树，不是整个应用所有页面的组件树，也不是 GPU/RenderService 渲染树。懒加载列表、WebView/Canvas、安全窗口、自定义绘制可能不暴露内部节点；必要时使用 DevEco 对应 inspector/profiler，不能用其节点数量代替性能指标。

点击/滑动需要测试授权，优先用项目 UiTest selector；只有坐标方式可用时，先从本次截图/树定位，再用 `uitest uiInput click x y`，动作后重新获取状态，不重复使用旧坐标。组件属性可辅助统计 Text/Button/Image、clickable、enabled、bounds；导出未包含的字段保持unknown。
