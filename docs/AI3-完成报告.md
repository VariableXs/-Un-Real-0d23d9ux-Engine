# AI03 完成报告 —— 任务栏与托盘组

> 版本：ENGINE-Version-01XHI9DN.1.5xw
> 交付范围：U-15、M-10…M-18、V-15…V-20（共 15 项）
> 原则：纯本地实现（零网络）、如实反馈、不越界（未改动其他 AI 任务文件的功能逻辑）

## 一、交付清单

### U-15 任务栏进化（总纲）
- `src/system/taskbar/Taskbar.tsx`：居中毛玻璃浮条扩展 —— 溢出折叠区、跳转列表、等待态、IM 未读角标、便签速贴入口、音量徽章、时钟详情卡、空区右键菜单全量接入；`data-ind` 运行指示样式属性下发。

### M 组（里程碑功能）
| 任务 | 内容 | 关键文件 |
| --- | --- | --- |
| M-10 | 图标右键 Jump List：最近记录（按 appKey 聚合去重）+ 活动实例窗口（z 降序）+ 全部关闭 | `taskbar/jumplist.ts` |
| M-11 | 启动等待态：登记 + 800ms 防抖（重复点击绝不重复 ShellExecute），>3s 转"仍在启动" | `taskbar/pending.ts` |
| M-12 | 时钟悬停详情卡：多时区（≤3，IANA 校验）+ ISO 周数 + 今日未读（只读消费） | `taskbar/clockcard.ts` |
| M-13 | 等待态超时与失败如实呈现（pendingPhase: launch/slow） | `taskbar/pending.ts` |
| M-14 | 托盘收纳抽屉（TrayDrawer）+ 图标搜索 | `taskbar/TrayDrawer.tsx` |
| M-15 | 任务栏空区右键菜单：注册表内安全项显隐 + 顺序调整 + 用户覆盖持久化 | `desktop/taskbarMenu.ts` + SettingsModal |
| M-16 | 媒体呼吸：播放态时钟旁 2% 幅度 / 4s 周期，默认关，reduce-motion 自动停用 | `desktop.css` + SettingsModal |
| M-17 | 便签速贴：四角堆叠、可拖动、10 分钟淡隐（500ms 动画）、钉住恢复、≤20 上限 | `taskbar/stickies.ts` + `StickyNotes.tsx` |
| M-18 | 音量滚轮/多媒体键：只监听显示浮标（1s 淡出），不改系统行为，DND 照常显示 | `taskbar/VolumeBadge.tsx` |

### V 组（体验打磨）
| 任务 | 内容 | 位置 |
| --- | --- | --- |
| V-15 | 中键：官方=关窗 / 第三方=新进程 | `Taskbar.tsx`（taskbarClickVwm / launchThirdApp 接线） |
| V-16 | 拖文件到任务栏图标打开：悬停放大 1.1 + 高亮；多文件弹确认；不支持的类型如实提示 | `Taskbar.tsx` + `desktop.css` |
| V-17 | 溢出折叠：纯几何计算（pinned 永远在外，从右往左，保持原顺序），溢出菜单按 id 解析动作 | `taskbar/overflow.ts` |
| V-18 | 运行指示样式三选：dot（Win11 圆点，默认）/ underline（Win10 下划线）/ capsule（胶囊），设置即时生效 | `desktop.css` + SettingsModal |
| V-19 | 关机会话清单：电源菜单展示"环境内尚在进行"的会话（vwm 窗口/任务），防误关 | `Taskbar.tsx`（电源菜单区） |
| V-20 | 电源菜单增强：睡眠项 + 本次开机时长（uptime） | `Taskbar.tsx` + i18n（powerSleep/powerUptime/powerSessions） |

## 二、i18n
- `src/i18n/dictionaries.ts`：新增 32 个 AI03 键（zh/en 各一份，104+ 键位校验通过）。
- 修复与其他 AI 的键名冲突：快捷键块 `scApply` → `scApplyBinds`（zh/en 同步 + SettingsModal 引用更新），场景块保留 `scApply`（AI02）/`scApplyScene`（en）。

## 三、样式
- `src/styles/desktop.css`：新增运行指示三态、溢出折叠、时钟卡、托盘抽屉、音量徽章、便签、媒体呼吸、拖拽打开高亮等样式（reduce-motion 全部适配）。

## 四、自检结果
| 项 | 结果 |
| --- | --- |
| vitest（taskbar 全部 7 个测试文件：overflow / clockcard / taskbarMenu / pending / stickies / jumplist / imbadge） | **28/28 通过** |
| tsc --noEmit（AI03 全部文件） | **0 错误** |
| i18n zh/en 键位对齐 | 通过 |
| IPC 全量注册审计 | 无新增后端命令（本组纯前端/localStorage 实现） |

> 注：全仓 tsc 目前仍存在其他 AI 范围内的错误（MiniNotes/miniframe、motion/orchestrate、datavault/FirewallPanel、explorer、dictionaries 中 AI09 Explorer 2.0 en 块重复键等），不属于 AI03 任务范围，未越界处理。

## 五、已知边界（如实披露）
- M-10 最近记录为 localStorage 纯本地聚合（不读系统 shell 最近文档）。
- M-12 多时区 ≤3 个为规格上限，输入非法 IANA 名时如实过滤。
- M-17 便签为纯文本、无提醒时间、不与写作空间打通（规格明确不做）。
- M-18 多媒体键监听依赖 webview keydown 捕获；系统级拦截的按键收不到（如实降级为不显示徽标）。
- V-16 拖拽打开支持官方四软件可接受类型 + 文件管理器，其余应用显示"不支持"提示。
- V-19 会话清单依赖环境内窗口镜像，系统原生进程不在清单内。

## 六、变更文件列表
- 新增：`src/system/taskbar/{jumplist,overflow,pending,clockcard,imbadge,stickies}.ts`、`src/system/taskbar/{StickyNotes,TrayDrawer,VolumeBadge}.tsx`、`src/system/desktop/taskbarMenu.ts`、`src/system/taskbar/__tests__/{jumplist,overflow,pending,clockcard,imbadge,stickies,taskbarMenu}.test.ts`
- 修改：`src/system/taskbar/Taskbar.tsx`、`src/features/settings/SettingsModal.tsx`、`src/i18n/dictionaries.ts`、`src/styles/desktop.css`、`src/system/startmenu/recent.ts`（补 clearRecent）
