# Z-09 系统组合键让位协议 — 行为表

数据源：src/lib/keymap/system-combos.ts（与 M-36 系统保留键库同源共用 SYSTEM_RESERVED）。
规则：passthrough 永不拦截；yield 全屏自动全让位（Z-18 检测接口未落地前以 setFullscreenActive() 标志位对接）；translate 翻译为本引擎动作并登记进 Z-08 注册表。

## passthrough（永不拦截）

- super+e / super+d / super+m / super+n / super+i / super+v / super+s / super+l — Win11 系统保留
- super+1 … super+9 — 任务栏槽位
- alt+tab / alt+f4 / alt+escape — 窗口切换/关闭/循环
- ctrl+alt+delete — 安全屏（SAS）
- ctrl+shift+escape — 任务管理器
- printscreen / f6 / f10 — 系统/焦点/菜单栏
- shift / ctrl+space / ctrl+alt+shift — IME 保留

## yield（全屏让位，桌面层可消费）

- super+left/right/up/down — 贴靠；全屏让位
- super+tab → translate — 任务视图；winTabSwitcher 开启时翻译为 wintab（既有契约，登记注册表）
- alt+space — 系统菜单；桌面层可消费
- ctrl+alt+arrowleft/arrowright — 虚拟桌面切换
- ctrl+shift+arrowleft/arrowright — IME 选择文字
- f11 / f12 / f5 / f1 — 全屏/DevTools/刷新/帮助

## 边界

- 全屏应用内：所有 yield/passthrough 组合一律让位，本引擎零拦截。
- 四大独立软件（write/mindmap/project/fate）内部键位不在本协议范围（隔离红线）。
- 双击 Esc 切环境由 kbdhook.rs 独立判定（30ms 轮询），与本协议的 escape 单击条目互不影响（M-34 红线，改动须专项回归）。
