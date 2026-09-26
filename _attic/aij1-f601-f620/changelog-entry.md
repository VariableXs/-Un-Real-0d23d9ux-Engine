## [Unreleased] — Varix STAR I · AI-J1 J 鼠标域·一分队（F601-F620 深化批次三 · 跨窗口运行时接线）

**src/features/mouse/ + MouseJ1Tab|MouseJ1Panels + 八窗口接线**（AI-J1 泳道三一分队，
2026-09-26 v3 批次；对齐 H4 批次二先例形态，v1/v2 判据逻辑层之上补齐跨窗口运行时）：

- **窗口无关运行时内核**（windowRuntime.ts，611 行）：整条指针/滚轮/侧键/手势/
  自动滚管线从桌面窗 React 组件解耦——desktop 全量层（副本/锚标/墨迹渲染）+
  app-write/mind/code/fate 锚标墨迹层 + explorer/taskbar/datavault headless，
  八窗口共用同一内核；documentElement 声明 data-app-id/data-app-class（F605 应用
  覆盖/F616 应用档案/F615 侧键作用域的真实挂点——此前全系统无人声明，解析链
  永远落默认档）。
- **动作路由中心**（actions.ts，215 行）：vx-j1-action 从「派发后零消费者」变为
  17 内置动作登记表（12 手势+5 侧键）+ 应用>全局两级路由 + 开放扩展点
  （registerActionHandler 退订即撤销、handler 抛错隔离）+ 未处理显性化（遥测
  no-feedback + 桌面窗每动作一次性提示）；Tauri 窗口管理（最小化/最大化切换）、
  编辑三兄弟、声明式 nav.up 内建接线；shortcut:/launch: 别名事件化（F244 快捷键
  路由与启动器的消费面）。
- **判据内真实补全**：F616 生效参数进管线（应用档案从此真实改变指针手感）、
  F614 首交互建档+气泡、F606 真实 deltaX 倾斜路径、F604 滚轮打断退出、
  F607/F613 Tauri 多屏缓存刷新（物理像素虚拟桌面坐标）、F610 tooltip 独立四档
  修正（基线 500ms 在档，v2 遗留缺陷）+ --hover-delay 偏离接管通道、F602 blur
  修饰键复位、deltaMode 归一化、F609 声明容器两处（设置弹窗滚动体/explorer
  文件列表）、F617 画中实时墨迹、v2 监听器逐事件重挂副作用修复。
- **验证**：本域单测 119/119 全绿（新增 j1v3.spec 33 项）；「运行时接线审计」
  面板（设置中心 → 鼠标）如实呈现八窗挂载身份/动作路由表/F609 容器/F610 变量
  通道；行数对账 4,844 / 15,660（30.9%，同一过滤口径，计数脚本归档
  _attic/aij1-f601-f620/）。
