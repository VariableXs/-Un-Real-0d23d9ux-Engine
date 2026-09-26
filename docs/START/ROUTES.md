# ROUTES — variable:// 深链路由注册公开表

> 自动生成：`node tools/gen-routes.cjs`（源：`docs/routes.src.json`）。手工修改会被下次生成覆盖。
> 深链解析协议见 U-37 协议中枢；协议注册仅便携部署态且退出退订。

共 7 条路由。

| Verb | 参数 | 必填 | 参数说明 | 所有者 | Since | 说明 | 示例链接 |
|---|---|---|---|---|---|---|---|
| `open` | path | ✓ | path: 要打开的本地路径（URL 编码） | AI-14 开放接口组 | 1.5xw | 打开本地路径（文件管理器语义） | `variable://open?path=C%3A%2Ftemp` |
| `lock` | — | — | — | AI-14 开放接口组 | 1.5xw | 锁定桌面环境（进入锁定态） | `variable://lock` |
| `scene/apply` | name | ✓ | name: 场景名（focus / relax / …） | AI-14 开放接口组 | 1.5xw | 应用命名场景（主题+壁纸+氛围组合） | `variable://scene/apply?name=focus` |
| `theme/set` | id | ✓ | id: 主题 id | AI-15 开放工具组 | 1.5xw | 切换主题（与计划任务工坊 theme.set 动作同语义） | `variable://theme/set?id=deep-space` |
| `wallpaper/set` | path | ✓ | path: 壁纸文件本地路径（URL 编码） | AI-15 开放工具组 | 1.5xw | 切换桌面壁纸（仅本地文件，零网络） | `variable://wallpaper/set?path=D%3A%2Fpics%2Fstar.png` |
| `perf/set` | mode | ✓ | mode: 性能模式（eco / balanced / performance） | AI-15 开放工具组 | 1.5xw | 切换性能模式（与计划任务工坊 perf.set 动作同语义） | `variable://perf/set?mode=balanced` |
| `notify/remind` | text | ✓ | text: 提醒文本 | AI-15 开放工具组 | 1.5xw | 发送一条本地提醒（仅 UI 通知，零网络） | `variable://notify/remind?text=%E5%96%9D%E6%B0%B4` |

## 可复制示例链接

- 打开本地路径（文件管理器语义）：`variable://open?path=C%3A%2Ftemp`
- 锁定桌面环境（进入锁定态）：`variable://lock`
- 应用命名场景（主题+壁纸+氛围组合）：`variable://scene/apply?name=focus`
- 切换主题（与计划任务工坊 theme.set 动作同语义）：`variable://theme/set?id=deep-space`
- 切换桌面壁纸（仅本地文件，零网络）：`variable://wallpaper/set?path=D%3A%2Fpics%2Fstar.png`
- 切换性能模式（与计划任务工坊 perf.set 动作同语义）：`variable://perf/set?mode=balanced`
- 发送一条本地提醒（仅 UI 通知，零网络）：`variable://notify/remind?text=%E5%96%9D%E6%B0%B4`
