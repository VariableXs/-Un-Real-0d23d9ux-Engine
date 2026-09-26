# Code Analysis 实操 QA 报告（2026-09-18）

## 执行方式
真 Rust 壳（CodeAnalysis.exe :8977）+ 真实浏览器驱动，页内异常监听（error/unhandledrejection）全程开启。
操作覆盖：全部契约 ID/按钮逐个点击、7 级下钻 × 5 视图交叉、3 界面模式、8 风格、主题切换、
画布缩放/平移/键盘/搜索/书签/详情四动作、词典面板/导入/识别/语法/去重、视口五档、导出三键真实下载、
打印 null 守卫、真项目（4.6 万节点）与演示项目双载入。

## 检查量（全新断言，非文档内容）
B1 结构 105 / B2 IR 完整性 34 / B3 级别×视图 123 / B4 交互+工具栏 56 / B5a 节点字段契约 1478 /
B5b 详情动作 119 / B6 风格+界面 59 / B7 词典 79 / B8 视口+视觉+无障碍 114 / 修复复测 ~15

**合计 ≈ 2182 项，全部通过（含修复后复测）**

## 发现并修复的 bug（5 个真问题）
1. core/ir.rs 不跳过隐藏目录 → 打开项目扫入 .mimosa 等工具噪声（9 万节点）。修复：跳过一切 `.` 开头目录（保留原显式清单并补 vendor）。
2. 真实项目节点 domain 全空 → 语义色全灰地图。修复：irFromJSON 按根下首段路径推断域 + semanticColor 确定性哈希调色（演示项目四域色不受影响，smoke 断言保持）。
3. 大项目力导向布局 O(n²) 冻结风险。修复：>1500 节点直接确定性环形排布，>150 缩减迭代至 60。
4. 空层级（如 auth-service 无 L3 子系统）切到全空画布无反馈。修复：滑块拒绝切换 + toast 如实说明并停留原层级。
5. 视口五档在部分宿主（自动化环境/壳B WebView）原生 resize 事件不派发导致适配滞留（QA 实测 1200px 停留 wide）。修复：shell.js 600ms innerWidth 轮询兜底。复测 1200/900/700/420/1680 五档全过。

## 甄别为非 bug 的告警（3 项）
- 启动期导出下载 ×3：旧标签页残留，干净重载不复现（已验证）。
- 词典导入 skip/count 与壁纸像素差两项：QA 断言口径问题（引擎行为正确，修正口径复测通过：skipped=1、先到先得成立、壁纸 background 确有差异）。
- nav/stage 重叠 1px：grid 过渡未结束的时序误报，settled 后 308==308 精确贴合。

## 产物清单
- b00-boot.png / b02-fixed-L4.png / b05-detail.png / b06-styles.png / b07-dicts.png / b08-tier-fix.png（过程截图）
- flowchart.png / flowchart.svg / code-analysis-wallpaper.png（导出功能实测产物）
