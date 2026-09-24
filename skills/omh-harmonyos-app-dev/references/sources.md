# 来源与版本边界

维护核对日期：2026-09-18。命令组合来自源项目已验证的 AGENTS 工作流，并去掉了模型、包名和设备 ID；不是承诺全部厂商版本都具有相同输出。

- [OpenHarmony HDC 指南](https://github.com/openharmony/docs/blob/master/en/application-dev/dfx/hdc.md)：传输/设备命令。
- [官方 native 应用构建部署示例](https://github.com/openharmony/docs/blob/master/en/application-dev/ai/mindspore/mindspore-guidelines-based-native.md)：HAP→file send→bm install→aa start。
- [OpenHarmony arkXtest 使用说明](https://github.com/openharmony/testfwk_arkxtest/blob/master/README_zh.md)：uitest help、screenCap、dumpLayout及过滤选项。引用时按目标设备版本复核；master会变化。
- [华为 UIViewer 截图与元素树排障](https://developer.huawei.com/consumer/cn/doc/doccenter-tools-faq/faqs-utilities-uiviewer-2)：dumpLayout / snapshot_display。网站可能动态加载；设备帮助比未能读取的页面更可靠。
- 实际设备只读帮助已核对：`uitest help` 支持 screenCap、dumpLayout -p/-b/-w/-i/-a/-m/-d/-e；`hilog --help` 支持 -x/-v/-P/-T；`aa help`列出start/test/dump/force-stop。没有将“读取帮助”当作完成真实构建/部署/测试。

遇版本差异先查目标SDK/设备帮助和对应版本官方文档；不要猜参数，也不要以测试不足为理由替换系统工具或关闭设备保护。
