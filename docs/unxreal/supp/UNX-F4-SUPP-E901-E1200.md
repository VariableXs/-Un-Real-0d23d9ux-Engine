
---

## 增补卷四 · AI-29 · UNX-F4-E901–E1200（300 项新功能 · E46–E60 批 · 15 批 × 20 条）

> 收录纪律：独立增补编号，不占域账 F22401–F23200；状态列统一「增补」；
> 全部围绕 Varix 内核锚定（checks.rs 自检面 / lxprocfs.rs 自省面 / 内核事件·会话·IPC 通道 / vxwm surface·popup·特效通道）；
> 与卷一至卷三判据颗粒零重复；保管母本 docs/unxreal/supp/UNX-F4-SUPP-E901-E1200.md（R-PROC-002 生成器重生成后须回播）。

### 批 E46 · 护眼模式与色温调度（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E901 | [E46] 护眼模式与色温调度 · 基础功能面（域内核锚定主路径） | `night_light_temp/night_light_temp_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E901-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E902 | [E46] 护眼模式与色温调度 · 参数化配置面（开放格式持久化） | `night_light_temp/night_light_temp_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E902-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E903 | [E46] 护眼模式与色温调度 · 用户设置面板（即时生效预览） | `night_light_temp/night_light_temp_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E903-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E904 | [E46] 护眼模式与色温调度 · 状态查询与自省接口（lxprocfs 投影） | `night_light_temp/night_light_temp_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E904-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E905 | [E46] 护眼模式与色温调度 · 异常三要素呈现（错误码→人话映射） | `night_light_temp/night_light_temp_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E905-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E906 | [E46] 护眼模式与色温调度 · 隐蔽异常捕获（异步回调/静默失败探针） | `night_light_temp/night_light_temp_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E906-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E907 | [E46] 护眼模式与色温调度 · 体验日志埋点（交互细节层） | `night_light_temp/night_light_temp_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E907-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E908 | [E46] 护眼模式与色温调度 · 挫败信号自动标记（rage/dead click） | `night_light_temp/night_light_temp_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E908-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E909 | [E46] 护眼模式与色温调度 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `night_light_temp/night_light_temp_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E909-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E910 | [E46] 护眼模式与色温调度 · 屏幕阅读器投影（UIA 节点语义） | `night_light_temp/night_light_temp_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E910-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E911 | [E46] 护眼模式与色温调度 · 高对比度与色弱兼容（非仅色相区分） | `night_light_temp/night_light_temp_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E911-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E912 | [E46] 护眼模式与色温调度 · DPI 与多屏适配（Per-Monitor v2 感知） | `night_light_temp/night_light_temp_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E912-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E913 | [E46] 护眼模式与色温调度 · 性能账（P95/内存/帧率埋点入账） | `night_light_temp/night_light_temp_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E913-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E914 | [E46] 护眼模式与色温调度 · 与内核健康账联动（心跳/看门狗） | `night_light_temp/night_light_temp_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E914-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E915 | [E46] 护眼模式与色温调度 · 开放接口与版本化（十年不变承诺） | `night_light_temp/night_light_temp_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E915-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E916 | [E46] 护眼模式与色温调度 · 端到端跨域联测（三类窗口矩阵） | `night_light_temp/night_light_temp_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E916-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E917 | [E46] 护眼模式与色温调度 · 边界与 fuzz（乱操作不崩） | `night_light_temp/night_light_temp_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E917-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E918 | [E46] 护眼模式与色温调度 · 恢复路径（断电/休眠唤醒状态保全） | `night_light_temp/night_light_temp_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E918-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E919 | [E46] 护眼模式与色温调度 · 文档与交付三件套（使用/接口/CHANGELOG） | `night_light_temp/night_light_temp_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E919-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |
| UNX-F4-E920 | [E46] 护眼模式与色温调度 · 域内总日志接入与收官自检 | `night_light_temp/night_light_temp_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E920-J1：构造内核事件通道与 vxwm surface stub，调 night_light_temp_probe() 断言） | 增补 |

### 批 E47 · 壁纸引擎与主题联动（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E921 | [E47] 壁纸引擎与主题联动 · 基础功能面（域内核锚定主路径） | `wallpaper_theme_link/wallpaper_theme_link_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E921-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E922 | [E47] 壁纸引擎与主题联动 · 参数化配置面（开放格式持久化） | `wallpaper_theme_link/wallpaper_theme_link_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E922-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E923 | [E47] 壁纸引擎与主题联动 · 用户设置面板（即时生效预览） | `wallpaper_theme_link/wallpaper_theme_link_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E923-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E924 | [E47] 壁纸引擎与主题联动 · 状态查询与自省接口（lxprocfs 投影） | `wallpaper_theme_link/wallpaper_theme_link_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E924-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E925 | [E47] 壁纸引擎与主题联动 · 异常三要素呈现（错误码→人话映射） | `wallpaper_theme_link/wallpaper_theme_link_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E925-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E926 | [E47] 壁纸引擎与主题联动 · 隐蔽异常捕获（异步回调/静默失败探针） | `wallpaper_theme_link/wallpaper_theme_link_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E926-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E927 | [E47] 壁纸引擎与主题联动 · 体验日志埋点（交互细节层） | `wallpaper_theme_link/wallpaper_theme_link_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E927-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E928 | [E47] 壁纸引擎与主题联动 · 挫败信号自动标记（rage/dead click） | `wallpaper_theme_link/wallpaper_theme_link_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E928-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E929 | [E47] 壁纸引擎与主题联动 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `wallpaper_theme_link/wallpaper_theme_link_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E929-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E930 | [E47] 壁纸引擎与主题联动 · 屏幕阅读器投影（UIA 节点语义） | `wallpaper_theme_link/wallpaper_theme_link_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E930-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E931 | [E47] 壁纸引擎与主题联动 · 高对比度与色弱兼容（非仅色相区分） | `wallpaper_theme_link/wallpaper_theme_link_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E931-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E932 | [E47] 壁纸引擎与主题联动 · DPI 与多屏适配（Per-Monitor v2 感知） | `wallpaper_theme_link/wallpaper_theme_link_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E932-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E933 | [E47] 壁纸引擎与主题联动 · 性能账（P95/内存/帧率埋点入账） | `wallpaper_theme_link/wallpaper_theme_link_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E933-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E934 | [E47] 壁纸引擎与主题联动 · 与内核健康账联动（心跳/看门狗） | `wallpaper_theme_link/wallpaper_theme_link_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E934-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E935 | [E47] 壁纸引擎与主题联动 · 开放接口与版本化（十年不变承诺） | `wallpaper_theme_link/wallpaper_theme_link_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E935-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E936 | [E47] 壁纸引擎与主题联动 · 端到端跨域联测（三类窗口矩阵） | `wallpaper_theme_link/wallpaper_theme_link_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E936-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E937 | [E47] 壁纸引擎与主题联动 · 边界与 fuzz（乱操作不崩） | `wallpaper_theme_link/wallpaper_theme_link_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E937-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E938 | [E47] 壁纸引擎与主题联动 · 恢复路径（断电/休眠唤醒状态保全） | `wallpaper_theme_link/wallpaper_theme_link_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E938-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E939 | [E47] 壁纸引擎与主题联动 · 文档与交付三件套（使用/接口/CHANGELOG） | `wallpaper_theme_link/wallpaper_theme_link_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E939-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |
| UNX-F4-E940 | [E47] 壁纸引擎与主题联动 · 域内总日志接入与收官自检 | `wallpaper_theme_link/wallpaper_theme_link_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E940-J1：构造内核事件通道与 vxwm surface stub，调 wallpaper_theme_link_probe() 断言） | 增补 |

### 批 E48 · 字体渲染与 ClearType 调优 UI（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E941 | [E48] 字体渲染与 ClearType 调优 UI · 基础功能面（域内核锚定主路径） | `font_render_tuning/font_render_tuning_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E941-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E942 | [E48] 字体渲染与 ClearType 调优 UI · 参数化配置面（开放格式持久化） | `font_render_tuning/font_render_tuning_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E942-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E943 | [E48] 字体渲染与 ClearType 调优 UI · 用户设置面板（即时生效预览） | `font_render_tuning/font_render_tuning_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E943-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E944 | [E48] 字体渲染与 ClearType 调优 UI · 状态查询与自省接口（lxprocfs 投影） | `font_render_tuning/font_render_tuning_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E944-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E945 | [E48] 字体渲染与 ClearType 调优 UI · 异常三要素呈现（错误码→人话映射） | `font_render_tuning/font_render_tuning_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E945-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E946 | [E48] 字体渲染与 ClearType 调优 UI · 隐蔽异常捕获（异步回调/静默失败探针） | `font_render_tuning/font_render_tuning_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E946-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E947 | [E48] 字体渲染与 ClearType 调优 UI · 体验日志埋点（交互细节层） | `font_render_tuning/font_render_tuning_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E947-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E948 | [E48] 字体渲染与 ClearType 调优 UI · 挫败信号自动标记（rage/dead click） | `font_render_tuning/font_render_tuning_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E948-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E949 | [E48] 字体渲染与 ClearType 调优 UI · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `font_render_tuning/font_render_tuning_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E949-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E950 | [E48] 字体渲染与 ClearType 调优 UI · 屏幕阅读器投影（UIA 节点语义） | `font_render_tuning/font_render_tuning_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E950-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E951 | [E48] 字体渲染与 ClearType 调优 UI · 高对比度与色弱兼容（非仅色相区分） | `font_render_tuning/font_render_tuning_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E951-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E952 | [E48] 字体渲染与 ClearType 调优 UI · DPI 与多屏适配（Per-Monitor v2 感知） | `font_render_tuning/font_render_tuning_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E952-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E953 | [E48] 字体渲染与 ClearType 调优 UI · 性能账（P95/内存/帧率埋点入账） | `font_render_tuning/font_render_tuning_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E953-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E954 | [E48] 字体渲染与 ClearType 调优 UI · 与内核健康账联动（心跳/看门狗） | `font_render_tuning/font_render_tuning_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E954-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E955 | [E48] 字体渲染与 ClearType 调优 UI · 开放接口与版本化（十年不变承诺） | `font_render_tuning/font_render_tuning_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E955-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E956 | [E48] 字体渲染与 ClearType 调优 UI · 端到端跨域联测（三类窗口矩阵） | `font_render_tuning/font_render_tuning_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E956-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E957 | [E48] 字体渲染与 ClearType 调优 UI · 边界与 fuzz（乱操作不崩） | `font_render_tuning/font_render_tuning_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E957-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E958 | [E48] 字体渲染与 ClearType 调优 UI · 恢复路径（断电/休眠唤醒状态保全） | `font_render_tuning/font_render_tuning_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E958-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E959 | [E48] 字体渲染与 ClearType 调优 UI · 文档与交付三件套（使用/接口/CHANGELOG） | `font_render_tuning/font_render_tuning_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E959-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |
| UNX-F4-E960 | [E48] 字体渲染与 ClearType 调优 UI · 域内总日志接入与收官自检 | `font_render_tuning/font_render_tuning_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E960-J1：构造内核事件通道与 vxwm surface stub，调 font_render_tuning_probe() 断言） | 增补 |

### 批 E49 · 鼠标触控板辅助（大指针/轨迹）（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E961 | [E49] 鼠标触控板辅助（大指针/轨迹） · 基础功能面（域内核锚定主路径） | `pointer_assist/pointer_assist_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E961-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E962 | [E49] 鼠标触控板辅助（大指针/轨迹） · 参数化配置面（开放格式持久化） | `pointer_assist/pointer_assist_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E962-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E963 | [E49] 鼠标触控板辅助（大指针/轨迹） · 用户设置面板（即时生效预览） | `pointer_assist/pointer_assist_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E963-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E964 | [E49] 鼠标触控板辅助（大指针/轨迹） · 状态查询与自省接口（lxprocfs 投影） | `pointer_assist/pointer_assist_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E964-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E965 | [E49] 鼠标触控板辅助（大指针/轨迹） · 异常三要素呈现（错误码→人话映射） | `pointer_assist/pointer_assist_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E965-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E966 | [E49] 鼠标触控板辅助（大指针/轨迹） · 隐蔽异常捕获（异步回调/静默失败探针） | `pointer_assist/pointer_assist_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E966-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E967 | [E49] 鼠标触控板辅助（大指针/轨迹） · 体验日志埋点（交互细节层） | `pointer_assist/pointer_assist_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E967-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E968 | [E49] 鼠标触控板辅助（大指针/轨迹） · 挫败信号自动标记（rage/dead click） | `pointer_assist/pointer_assist_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E968-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E969 | [E49] 鼠标触控板辅助（大指针/轨迹） · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `pointer_assist/pointer_assist_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E969-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E970 | [E49] 鼠标触控板辅助（大指针/轨迹） · 屏幕阅读器投影（UIA 节点语义） | `pointer_assist/pointer_assist_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E970-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E971 | [E49] 鼠标触控板辅助（大指针/轨迹） · 高对比度与色弱兼容（非仅色相区分） | `pointer_assist/pointer_assist_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E971-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E972 | [E49] 鼠标触控板辅助（大指针/轨迹） · DPI 与多屏适配（Per-Monitor v2 感知） | `pointer_assist/pointer_assist_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E972-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E973 | [E49] 鼠标触控板辅助（大指针/轨迹） · 性能账（P95/内存/帧率埋点入账） | `pointer_assist/pointer_assist_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E973-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E974 | [E49] 鼠标触控板辅助（大指针/轨迹） · 与内核健康账联动（心跳/看门狗） | `pointer_assist/pointer_assist_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E974-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E975 | [E49] 鼠标触控板辅助（大指针/轨迹） · 开放接口与版本化（十年不变承诺） | `pointer_assist/pointer_assist_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E975-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E976 | [E49] 鼠标触控板辅助（大指针/轨迹） · 端到端跨域联测（三类窗口矩阵） | `pointer_assist/pointer_assist_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E976-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E977 | [E49] 鼠标触控板辅助（大指针/轨迹） · 边界与 fuzz（乱操作不崩） | `pointer_assist/pointer_assist_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E977-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E978 | [E49] 鼠标触控板辅助（大指针/轨迹） · 恢复路径（断电/休眠唤醒状态保全） | `pointer_assist/pointer_assist_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E978-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E979 | [E49] 鼠标触控板辅助（大指针/轨迹） · 文档与交付三件套（使用/接口/CHANGELOG） | `pointer_assist/pointer_assist_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E979-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |
| UNX-F4-E980 | [E49] 鼠标触控板辅助（大指针/轨迹） · 域内总日志接入与收官自检 | `pointer_assist/pointer_assist_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E980-J1：构造内核事件通道与 vxwm surface stub，调 pointer_assist_probe() 断言） | 增补 |

### 批 E50 · 色盲滤镜与色觉辅助（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E981 | [E50] 色盲滤镜与色觉辅助 · 基础功能面（域内核锚定主路径） | `colorblind_filter/colorblind_filter_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E981-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E982 | [E50] 色盲滤镜与色觉辅助 · 参数化配置面（开放格式持久化） | `colorblind_filter/colorblind_filter_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E982-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E983 | [E50] 色盲滤镜与色觉辅助 · 用户设置面板（即时生效预览） | `colorblind_filter/colorblind_filter_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E983-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E984 | [E50] 色盲滤镜与色觉辅助 · 状态查询与自省接口（lxprocfs 投影） | `colorblind_filter/colorblind_filter_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E984-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E985 | [E50] 色盲滤镜与色觉辅助 · 异常三要素呈现（错误码→人话映射） | `colorblind_filter/colorblind_filter_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E985-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E986 | [E50] 色盲滤镜与色觉辅助 · 隐蔽异常捕获（异步回调/静默失败探针） | `colorblind_filter/colorblind_filter_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E986-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E987 | [E50] 色盲滤镜与色觉辅助 · 体验日志埋点（交互细节层） | `colorblind_filter/colorblind_filter_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E987-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E988 | [E50] 色盲滤镜与色觉辅助 · 挫败信号自动标记（rage/dead click） | `colorblind_filter/colorblind_filter_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E988-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E989 | [E50] 色盲滤镜与色觉辅助 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `colorblind_filter/colorblind_filter_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E989-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E990 | [E50] 色盲滤镜与色觉辅助 · 屏幕阅读器投影（UIA 节点语义） | `colorblind_filter/colorblind_filter_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E990-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E991 | [E50] 色盲滤镜与色觉辅助 · 高对比度与色弱兼容（非仅色相区分） | `colorblind_filter/colorblind_filter_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E991-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E992 | [E50] 色盲滤镜与色觉辅助 · DPI 与多屏适配（Per-Monitor v2 感知） | `colorblind_filter/colorblind_filter_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E992-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E993 | [E50] 色盲滤镜与色觉辅助 · 性能账（P95/内存/帧率埋点入账） | `colorblind_filter/colorblind_filter_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E993-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E994 | [E50] 色盲滤镜与色觉辅助 · 与内核健康账联动（心跳/看门狗） | `colorblind_filter/colorblind_filter_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E994-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E995 | [E50] 色盲滤镜与色觉辅助 · 开放接口与版本化（十年不变承诺） | `colorblind_filter/colorblind_filter_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E995-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E996 | [E50] 色盲滤镜与色觉辅助 · 端到端跨域联测（三类窗口矩阵） | `colorblind_filter/colorblind_filter_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E996-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E997 | [E50] 色盲滤镜与色觉辅助 · 边界与 fuzz（乱操作不崩） | `colorblind_filter/colorblind_filter_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E997-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E998 | [E50] 色盲滤镜与色觉辅助 · 恢复路径（断电/休眠唤醒状态保全） | `colorblind_filter/colorblind_filter_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E998-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E999 | [E50] 色盲滤镜与色觉辅助 · 文档与交付三件套（使用/接口/CHANGELOG） | `colorblind_filter/colorblind_filter_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E999-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |
| UNX-F4-E1000 | [E50] 色盲滤镜与色觉辅助 · 域内总日志接入与收官自检 | `colorblind_filter/colorblind_filter_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1000-J1：构造内核事件通道与 vxwm surface stub，调 colorblind_filter_probe() 断言） | 增补 |

### 批 E51 · 专注辅助与免打扰主题面（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1001 | [E51] 专注辅助与免打扰主题面 · 基础功能面（域内核锚定主路径） | `focus_assist_theme/focus_assist_theme_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1001-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1002 | [E51] 专注辅助与免打扰主题面 · 参数化配置面（开放格式持久化） | `focus_assist_theme/focus_assist_theme_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1002-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1003 | [E51] 专注辅助与免打扰主题面 · 用户设置面板（即时生效预览） | `focus_assist_theme/focus_assist_theme_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1003-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1004 | [E51] 专注辅助与免打扰主题面 · 状态查询与自省接口（lxprocfs 投影） | `focus_assist_theme/focus_assist_theme_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1004-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1005 | [E51] 专注辅助与免打扰主题面 · 异常三要素呈现（错误码→人话映射） | `focus_assist_theme/focus_assist_theme_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1005-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1006 | [E51] 专注辅助与免打扰主题面 · 隐蔽异常捕获（异步回调/静默失败探针） | `focus_assist_theme/focus_assist_theme_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1006-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1007 | [E51] 专注辅助与免打扰主题面 · 体验日志埋点（交互细节层） | `focus_assist_theme/focus_assist_theme_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1007-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1008 | [E51] 专注辅助与免打扰主题面 · 挫败信号自动标记（rage/dead click） | `focus_assist_theme/focus_assist_theme_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1008-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1009 | [E51] 专注辅助与免打扰主题面 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `focus_assist_theme/focus_assist_theme_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1009-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1010 | [E51] 专注辅助与免打扰主题面 · 屏幕阅读器投影（UIA 节点语义） | `focus_assist_theme/focus_assist_theme_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1010-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1011 | [E51] 专注辅助与免打扰主题面 · 高对比度与色弱兼容（非仅色相区分） | `focus_assist_theme/focus_assist_theme_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1011-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1012 | [E51] 专注辅助与免打扰主题面 · DPI 与多屏适配（Per-Monitor v2 感知） | `focus_assist_theme/focus_assist_theme_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1012-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1013 | [E51] 专注辅助与免打扰主题面 · 性能账（P95/内存/帧率埋点入账） | `focus_assist_theme/focus_assist_theme_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1013-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1014 | [E51] 专注辅助与免打扰主题面 · 与内核健康账联动（心跳/看门狗） | `focus_assist_theme/focus_assist_theme_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1014-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1015 | [E51] 专注辅助与免打扰主题面 · 开放接口与版本化（十年不变承诺） | `focus_assist_theme/focus_assist_theme_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1015-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1016 | [E51] 专注辅助与免打扰主题面 · 端到端跨域联测（三类窗口矩阵） | `focus_assist_theme/focus_assist_theme_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1016-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1017 | [E51] 专注辅助与免打扰主题面 · 边界与 fuzz（乱操作不崩） | `focus_assist_theme/focus_assist_theme_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1017-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1018 | [E51] 专注辅助与免打扰主题面 · 恢复路径（断电/休眠唤醒状态保全） | `focus_assist_theme/focus_assist_theme_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1018-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1019 | [E51] 专注辅助与免打扰主题面 · 文档与交付三件套（使用/接口/CHANGELOG） | `focus_assist_theme/focus_assist_theme_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1019-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |
| UNX-F4-E1020 | [E51] 专注辅助与免打扰主题面 · 域内总日志接入与收官自检 | `focus_assist_theme/focus_assist_theme_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1020-J1：构造内核事件通道与 vxwm surface stub，调 focus_assist_theme_probe() 断言） | 增补 |

### 批 E52 · IME 双语混排与中英切换 UI（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1021 | [E52] IME 双语混排与中英切换 UI · 基础功能面（域内核锚定主路径） | `ime_bilingual_toggle/ime_bilingual_toggle_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1021-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1022 | [E52] IME 双语混排与中英切换 UI · 参数化配置面（开放格式持久化） | `ime_bilingual_toggle/ime_bilingual_toggle_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1022-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1023 | [E52] IME 双语混排与中英切换 UI · 用户设置面板（即时生效预览） | `ime_bilingual_toggle/ime_bilingual_toggle_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1023-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1024 | [E52] IME 双语混排与中英切换 UI · 状态查询与自省接口（lxprocfs 投影） | `ime_bilingual_toggle/ime_bilingual_toggle_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1024-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1025 | [E52] IME 双语混排与中英切换 UI · 异常三要素呈现（错误码→人话映射） | `ime_bilingual_toggle/ime_bilingual_toggle_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1025-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1026 | [E52] IME 双语混排与中英切换 UI · 隐蔽异常捕获（异步回调/静默失败探针） | `ime_bilingual_toggle/ime_bilingual_toggle_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1026-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1027 | [E52] IME 双语混排与中英切换 UI · 体验日志埋点（交互细节层） | `ime_bilingual_toggle/ime_bilingual_toggle_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1027-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1028 | [E52] IME 双语混排与中英切换 UI · 挫败信号自动标记（rage/dead click） | `ime_bilingual_toggle/ime_bilingual_toggle_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1028-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1029 | [E52] IME 双语混排与中英切换 UI · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `ime_bilingual_toggle/ime_bilingual_toggle_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1029-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1030 | [E52] IME 双语混排与中英切换 UI · 屏幕阅读器投影（UIA 节点语义） | `ime_bilingual_toggle/ime_bilingual_toggle_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1030-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1031 | [E52] IME 双语混排与中英切换 UI · 高对比度与色弱兼容（非仅色相区分） | `ime_bilingual_toggle/ime_bilingual_toggle_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1031-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1032 | [E52] IME 双语混排与中英切换 UI · DPI 与多屏适配（Per-Monitor v2 感知） | `ime_bilingual_toggle/ime_bilingual_toggle_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1032-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1033 | [E52] IME 双语混排与中英切换 UI · 性能账（P95/内存/帧率埋点入账） | `ime_bilingual_toggle/ime_bilingual_toggle_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1033-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1034 | [E52] IME 双语混排与中英切换 UI · 与内核健康账联动（心跳/看门狗） | `ime_bilingual_toggle/ime_bilingual_toggle_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1034-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1035 | [E52] IME 双语混排与中英切换 UI · 开放接口与版本化（十年不变承诺） | `ime_bilingual_toggle/ime_bilingual_toggle_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1035-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1036 | [E52] IME 双语混排与中英切换 UI · 端到端跨域联测（三类窗口矩阵） | `ime_bilingual_toggle/ime_bilingual_toggle_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1036-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1037 | [E52] IME 双语混排与中英切换 UI · 边界与 fuzz（乱操作不崩） | `ime_bilingual_toggle/ime_bilingual_toggle_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1037-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1038 | [E52] IME 双语混排与中英切换 UI · 恢复路径（断电/休眠唤醒状态保全） | `ime_bilingual_toggle/ime_bilingual_toggle_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1038-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1039 | [E52] IME 双语混排与中英切换 UI · 文档与交付三件套（使用/接口/CHANGELOG） | `ime_bilingual_toggle/ime_bilingual_toggle_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1039-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |
| UNX-F4-E1040 | [E52] IME 双语混排与中英切换 UI · 域内总日志接入与收官自检 | `ime_bilingual_toggle/ime_bilingual_toggle_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1040-J1：构造内核事件通道与 vxwm surface stub，调 ime_bilingual_toggle_probe() 断言） | 增补 |

### 批 E53 · 候选词学习与预测设置 UI（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1041 | [E53] 候选词学习与预测设置 UI · 基础功能面（域内核锚定主路径） | `candidate_learning_ui/candidate_learning_ui_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1041-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1042 | [E53] 候选词学习与预测设置 UI · 参数化配置面（开放格式持久化） | `candidate_learning_ui/candidate_learning_ui_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1042-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1043 | [E53] 候选词学习与预测设置 UI · 用户设置面板（即时生效预览） | `candidate_learning_ui/candidate_learning_ui_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1043-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1044 | [E53] 候选词学习与预测设置 UI · 状态查询与自省接口（lxprocfs 投影） | `candidate_learning_ui/candidate_learning_ui_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1044-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1045 | [E53] 候选词学习与预测设置 UI · 异常三要素呈现（错误码→人话映射） | `candidate_learning_ui/candidate_learning_ui_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1045-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1046 | [E53] 候选词学习与预测设置 UI · 隐蔽异常捕获（异步回调/静默失败探针） | `candidate_learning_ui/candidate_learning_ui_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1046-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1047 | [E53] 候选词学习与预测设置 UI · 体验日志埋点（交互细节层） | `candidate_learning_ui/candidate_learning_ui_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1047-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1048 | [E53] 候选词学习与预测设置 UI · 挫败信号自动标记（rage/dead click） | `candidate_learning_ui/candidate_learning_ui_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1048-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1049 | [E53] 候选词学习与预测设置 UI · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `candidate_learning_ui/candidate_learning_ui_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1049-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1050 | [E53] 候选词学习与预测设置 UI · 屏幕阅读器投影（UIA 节点语义） | `candidate_learning_ui/candidate_learning_ui_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1050-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1051 | [E53] 候选词学习与预测设置 UI · 高对比度与色弱兼容（非仅色相区分） | `candidate_learning_ui/candidate_learning_ui_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1051-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1052 | [E53] 候选词学习与预测设置 UI · DPI 与多屏适配（Per-Monitor v2 感知） | `candidate_learning_ui/candidate_learning_ui_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1052-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1053 | [E53] 候选词学习与预测设置 UI · 性能账（P95/内存/帧率埋点入账） | `candidate_learning_ui/candidate_learning_ui_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1053-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1054 | [E53] 候选词学习与预测设置 UI · 与内核健康账联动（心跳/看门狗） | `candidate_learning_ui/candidate_learning_ui_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1054-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1055 | [E53] 候选词学习与预测设置 UI · 开放接口与版本化（十年不变承诺） | `candidate_learning_ui/candidate_learning_ui_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1055-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1056 | [E53] 候选词学习与预测设置 UI · 端到端跨域联测（三类窗口矩阵） | `candidate_learning_ui/candidate_learning_ui_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1056-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1057 | [E53] 候选词学习与预测设置 UI · 边界与 fuzz（乱操作不崩） | `candidate_learning_ui/candidate_learning_ui_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1057-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1058 | [E53] 候选词学习与预测设置 UI · 恢复路径（断电/休眠唤醒状态保全） | `candidate_learning_ui/candidate_learning_ui_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1058-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1059 | [E53] 候选词学习与预测设置 UI · 文档与交付三件套（使用/接口/CHANGELOG） | `candidate_learning_ui/candidate_learning_ui_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1059-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |
| UNX-F4-E1060 | [E53] 候选词学习与预测设置 UI · 域内总日志接入与收官自检 | `candidate_learning_ui/candidate_learning_ui_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1060-J1：构造内核事件通道与 vxwm surface stub，调 candidate_learning_ui_probe() 断言） | 增补 |

### 批 E54 · 输入法皮肤市场与开放格式（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1061 | [E54] 输入法皮肤市场与开放格式 · 基础功能面（域内核锚定主路径） | `ime_skin_market/ime_skin_market_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1061-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1062 | [E54] 输入法皮肤市场与开放格式 · 参数化配置面（开放格式持久化） | `ime_skin_market/ime_skin_market_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1062-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1063 | [E54] 输入法皮肤市场与开放格式 · 用户设置面板（即时生效预览） | `ime_skin_market/ime_skin_market_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1063-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1064 | [E54] 输入法皮肤市场与开放格式 · 状态查询与自省接口（lxprocfs 投影） | `ime_skin_market/ime_skin_market_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1064-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1065 | [E54] 输入法皮肤市场与开放格式 · 异常三要素呈现（错误码→人话映射） | `ime_skin_market/ime_skin_market_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1065-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1066 | [E54] 输入法皮肤市场与开放格式 · 隐蔽异常捕获（异步回调/静默失败探针） | `ime_skin_market/ime_skin_market_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1066-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1067 | [E54] 输入法皮肤市场与开放格式 · 体验日志埋点（交互细节层） | `ime_skin_market/ime_skin_market_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1067-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1068 | [E54] 输入法皮肤市场与开放格式 · 挫败信号自动标记（rage/dead click） | `ime_skin_market/ime_skin_market_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1068-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1069 | [E54] 输入法皮肤市场与开放格式 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `ime_skin_market/ime_skin_market_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1069-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1070 | [E54] 输入法皮肤市场与开放格式 · 屏幕阅读器投影（UIA 节点语义） | `ime_skin_market/ime_skin_market_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1070-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1071 | [E54] 输入法皮肤市场与开放格式 · 高对比度与色弱兼容（非仅色相区分） | `ime_skin_market/ime_skin_market_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1071-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1072 | [E54] 输入法皮肤市场与开放格式 · DPI 与多屏适配（Per-Monitor v2 感知） | `ime_skin_market/ime_skin_market_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1072-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1073 | [E54] 输入法皮肤市场与开放格式 · 性能账（P95/内存/帧率埋点入账） | `ime_skin_market/ime_skin_market_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1073-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1074 | [E54] 输入法皮肤市场与开放格式 · 与内核健康账联动（心跳/看门狗） | `ime_skin_market/ime_skin_market_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1074-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1075 | [E54] 输入法皮肤市场与开放格式 · 开放接口与版本化（十年不变承诺） | `ime_skin_market/ime_skin_market_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1075-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1076 | [E54] 输入法皮肤市场与开放格式 · 端到端跨域联测（三类窗口矩阵） | `ime_skin_market/ime_skin_market_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1076-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1077 | [E54] 输入法皮肤市场与开放格式 · 边界与 fuzz（乱操作不崩） | `ime_skin_market/ime_skin_market_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1077-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1078 | [E54] 输入法皮肤市场与开放格式 · 恢复路径（断电/休眠唤醒状态保全） | `ime_skin_market/ime_skin_market_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1078-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1079 | [E54] 输入法皮肤市场与开放格式 · 文档与交付三件套（使用/接口/CHANGELOG） | `ime_skin_market/ime_skin_market_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1079-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |
| UNX-F4-E1080 | [E54] 输入法皮肤市场与开放格式 · 域内总日志接入与收官自检 | `ime_skin_market/ime_skin_market_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1080-J1：构造内核事件通道与 vxwm surface stub，调 ime_skin_market_probe() 断言） | 增补 |

### 批 E55 · 语音合成播报 TTS 主题集成（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1081 | [E55] 语音合成播报 TTS 主题集成 · 基础功能面（域内核锚定主路径） | `tts_theme_integration/tts_theme_integration_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1081-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1082 | [E55] 语音合成播报 TTS 主题集成 · 参数化配置面（开放格式持久化） | `tts_theme_integration/tts_theme_integration_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1082-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1083 | [E55] 语音合成播报 TTS 主题集成 · 用户设置面板（即时生效预览） | `tts_theme_integration/tts_theme_integration_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1083-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1084 | [E55] 语音合成播报 TTS 主题集成 · 状态查询与自省接口（lxprocfs 投影） | `tts_theme_integration/tts_theme_integration_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1084-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1085 | [E55] 语音合成播报 TTS 主题集成 · 异常三要素呈现（错误码→人话映射） | `tts_theme_integration/tts_theme_integration_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1085-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1086 | [E55] 语音合成播报 TTS 主题集成 · 隐蔽异常捕获（异步回调/静默失败探针） | `tts_theme_integration/tts_theme_integration_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1086-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1087 | [E55] 语音合成播报 TTS 主题集成 · 体验日志埋点（交互细节层） | `tts_theme_integration/tts_theme_integration_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1087-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1088 | [E55] 语音合成播报 TTS 主题集成 · 挫败信号自动标记（rage/dead click） | `tts_theme_integration/tts_theme_integration_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1088-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1089 | [E55] 语音合成播报 TTS 主题集成 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `tts_theme_integration/tts_theme_integration_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1089-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1090 | [E55] 语音合成播报 TTS 主题集成 · 屏幕阅读器投影（UIA 节点语义） | `tts_theme_integration/tts_theme_integration_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1090-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1091 | [E55] 语音合成播报 TTS 主题集成 · 高对比度与色弱兼容（非仅色相区分） | `tts_theme_integration/tts_theme_integration_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1091-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1092 | [E55] 语音合成播报 TTS 主题集成 · DPI 与多屏适配（Per-Monitor v2 感知） | `tts_theme_integration/tts_theme_integration_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1092-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1093 | [E55] 语音合成播报 TTS 主题集成 · 性能账（P95/内存/帧率埋点入账） | `tts_theme_integration/tts_theme_integration_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1093-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1094 | [E55] 语音合成播报 TTS 主题集成 · 与内核健康账联动（心跳/看门狗） | `tts_theme_integration/tts_theme_integration_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1094-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1095 | [E55] 语音合成播报 TTS 主题集成 · 开放接口与版本化（十年不变承诺） | `tts_theme_integration/tts_theme_integration_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1095-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1096 | [E55] 语音合成播报 TTS 主题集成 · 端到端跨域联测（三类窗口矩阵） | `tts_theme_integration/tts_theme_integration_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1096-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1097 | [E55] 语音合成播报 TTS 主题集成 · 边界与 fuzz（乱操作不崩） | `tts_theme_integration/tts_theme_integration_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1097-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1098 | [E55] 语音合成播报 TTS 主题集成 · 恢复路径（断电/休眠唤醒状态保全） | `tts_theme_integration/tts_theme_integration_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1098-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1099 | [E55] 语音合成播报 TTS 主题集成 · 文档与交付三件套（使用/接口/CHANGELOG） | `tts_theme_integration/tts_theme_integration_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1099-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |
| UNX-F4-E1100 | [E55] 语音合成播报 TTS 主题集成 · 域内总日志接入与收官自检 | `tts_theme_integration/tts_theme_integration_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1100-J1：构造内核事件通道与 vxwm surface stub，调 tts_theme_integration_probe() 断言） | 增补 |

### 批 E56 · 全局字号缩放与文本缩放（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1101 | [E56] 全局字号缩放与文本缩放 · 基础功能面（域内核锚定主路径） | `global_text_scaling/global_text_scaling_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1101-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1102 | [E56] 全局字号缩放与文本缩放 · 参数化配置面（开放格式持久化） | `global_text_scaling/global_text_scaling_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1102-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1103 | [E56] 全局字号缩放与文本缩放 · 用户设置面板（即时生效预览） | `global_text_scaling/global_text_scaling_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1103-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1104 | [E56] 全局字号缩放与文本缩放 · 状态查询与自省接口（lxprocfs 投影） | `global_text_scaling/global_text_scaling_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1104-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1105 | [E56] 全局字号缩放与文本缩放 · 异常三要素呈现（错误码→人话映射） | `global_text_scaling/global_text_scaling_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1105-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1106 | [E56] 全局字号缩放与文本缩放 · 隐蔽异常捕获（异步回调/静默失败探针） | `global_text_scaling/global_text_scaling_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1106-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1107 | [E56] 全局字号缩放与文本缩放 · 体验日志埋点（交互细节层） | `global_text_scaling/global_text_scaling_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1107-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1108 | [E56] 全局字号缩放与文本缩放 · 挫败信号自动标记（rage/dead click） | `global_text_scaling/global_text_scaling_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1108-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1109 | [E56] 全局字号缩放与文本缩放 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `global_text_scaling/global_text_scaling_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1109-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1110 | [E56] 全局字号缩放与文本缩放 · 屏幕阅读器投影（UIA 节点语义） | `global_text_scaling/global_text_scaling_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1110-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1111 | [E56] 全局字号缩放与文本缩放 · 高对比度与色弱兼容（非仅色相区分） | `global_text_scaling/global_text_scaling_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1111-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1112 | [E56] 全局字号缩放与文本缩放 · DPI 与多屏适配（Per-Monitor v2 感知） | `global_text_scaling/global_text_scaling_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1112-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1113 | [E56] 全局字号缩放与文本缩放 · 性能账（P95/内存/帧率埋点入账） | `global_text_scaling/global_text_scaling_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1113-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1114 | [E56] 全局字号缩放与文本缩放 · 与内核健康账联动（心跳/看门狗） | `global_text_scaling/global_text_scaling_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1114-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1115 | [E56] 全局字号缩放与文本缩放 · 开放接口与版本化（十年不变承诺） | `global_text_scaling/global_text_scaling_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1115-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1116 | [E56] 全局字号缩放与文本缩放 · 端到端跨域联测（三类窗口矩阵） | `global_text_scaling/global_text_scaling_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1116-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1117 | [E56] 全局字号缩放与文本缩放 · 边界与 fuzz（乱操作不崩） | `global_text_scaling/global_text_scaling_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1117-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1118 | [E56] 全局字号缩放与文本缩放 · 恢复路径（断电/休眠唤醒状态保全） | `global_text_scaling/global_text_scaling_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1118-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1119 | [E56] 全局字号缩放与文本缩放 · 文档与交付三件套（使用/接口/CHANGELOG） | `global_text_scaling/global_text_scaling_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1119-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |
| UNX-F4-E1120 | [E56] 全局字号缩放与文本缩放 · 域内总日志接入与收官自检 | `global_text_scaling/global_text_scaling_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1120-J1：构造内核事件通道与 vxwm surface stub，调 global_text_scaling_probe() 断言） | 增补 |

### 批 E57 · 动画减弱与前庭障碍模式（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1121 | [E57] 动画减弱与前庭障碍模式 · 基础功能面（域内核锚定主路径） | `reduced_motion_vestibular/reduced_motion_vestibular_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1121-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1122 | [E57] 动画减弱与前庭障碍模式 · 参数化配置面（开放格式持久化） | `reduced_motion_vestibular/reduced_motion_vestibular_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1122-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1123 | [E57] 动画减弱与前庭障碍模式 · 用户设置面板（即时生效预览） | `reduced_motion_vestibular/reduced_motion_vestibular_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1123-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1124 | [E57] 动画减弱与前庭障碍模式 · 状态查询与自省接口（lxprocfs 投影） | `reduced_motion_vestibular/reduced_motion_vestibular_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1124-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1125 | [E57] 动画减弱与前庭障碍模式 · 异常三要素呈现（错误码→人话映射） | `reduced_motion_vestibular/reduced_motion_vestibular_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1125-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1126 | [E57] 动画减弱与前庭障碍模式 · 隐蔽异常捕获（异步回调/静默失败探针） | `reduced_motion_vestibular/reduced_motion_vestibular_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1126-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1127 | [E57] 动画减弱与前庭障碍模式 · 体验日志埋点（交互细节层） | `reduced_motion_vestibular/reduced_motion_vestibular_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1127-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1128 | [E57] 动画减弱与前庭障碍模式 · 挫败信号自动标记（rage/dead click） | `reduced_motion_vestibular/reduced_motion_vestibular_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1128-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1129 | [E57] 动画减弱与前庭障碍模式 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `reduced_motion_vestibular/reduced_motion_vestibular_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1129-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1130 | [E57] 动画减弱与前庭障碍模式 · 屏幕阅读器投影（UIA 节点语义） | `reduced_motion_vestibular/reduced_motion_vestibular_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1130-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1131 | [E57] 动画减弱与前庭障碍模式 · 高对比度与色弱兼容（非仅色相区分） | `reduced_motion_vestibular/reduced_motion_vestibular_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1131-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1132 | [E57] 动画减弱与前庭障碍模式 · DPI 与多屏适配（Per-Monitor v2 感知） | `reduced_motion_vestibular/reduced_motion_vestibular_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1132-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1133 | [E57] 动画减弱与前庭障碍模式 · 性能账（P95/内存/帧率埋点入账） | `reduced_motion_vestibular/reduced_motion_vestibular_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1133-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1134 | [E57] 动画减弱与前庭障碍模式 · 与内核健康账联动（心跳/看门狗） | `reduced_motion_vestibular/reduced_motion_vestibular_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1134-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1135 | [E57] 动画减弱与前庭障碍模式 · 开放接口与版本化（十年不变承诺） | `reduced_motion_vestibular/reduced_motion_vestibular_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1135-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1136 | [E57] 动画减弱与前庭障碍模式 · 端到端跨域联测（三类窗口矩阵） | `reduced_motion_vestibular/reduced_motion_vestibular_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1136-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1137 | [E57] 动画减弱与前庭障碍模式 · 边界与 fuzz（乱操作不崩） | `reduced_motion_vestibular/reduced_motion_vestibular_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1137-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1138 | [E57] 动画减弱与前庭障碍模式 · 恢复路径（断电/休眠唤醒状态保全） | `reduced_motion_vestibular/reduced_motion_vestibular_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1138-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1139 | [E57] 动画减弱与前庭障碍模式 · 文档与交付三件套（使用/接口/CHANGELOG） | `reduced_motion_vestibular/reduced_motion_vestibular_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1139-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |
| UNX-F4-E1140 | [E57] 动画减弱与前庭障碍模式 · 域内总日志接入与收官自检 | `reduced_motion_vestibular/reduced_motion_vestibular_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1140-J1：构造内核事件通道与 vxwm surface stub，调 reduced_motion_vestibular_probe() 断言） | 增补 |

### 批 E58 · 单手模式与辅助触控（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1141 | [E58] 单手模式与辅助触控 · 基础功能面（域内核锚定主路径） | `onehand_assist_touch/onehand_assist_touch_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1141-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1142 | [E58] 单手模式与辅助触控 · 参数化配置面（开放格式持久化） | `onehand_assist_touch/onehand_assist_touch_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1142-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1143 | [E58] 单手模式与辅助触控 · 用户设置面板（即时生效预览） | `onehand_assist_touch/onehand_assist_touch_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1143-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1144 | [E58] 单手模式与辅助触控 · 状态查询与自省接口（lxprocfs 投影） | `onehand_assist_touch/onehand_assist_touch_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1144-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1145 | [E58] 单手模式与辅助触控 · 异常三要素呈现（错误码→人话映射） | `onehand_assist_touch/onehand_assist_touch_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1145-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1146 | [E58] 单手模式与辅助触控 · 隐蔽异常捕获（异步回调/静默失败探针） | `onehand_assist_touch/onehand_assist_touch_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1146-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1147 | [E58] 单手模式与辅助触控 · 体验日志埋点（交互细节层） | `onehand_assist_touch/onehand_assist_touch_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1147-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1148 | [E58] 单手模式与辅助触控 · 挫败信号自动标记（rage/dead click） | `onehand_assist_touch/onehand_assist_touch_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1148-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1149 | [E58] 单手模式与辅助触控 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `onehand_assist_touch/onehand_assist_touch_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1149-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1150 | [E58] 单手模式与辅助触控 · 屏幕阅读器投影（UIA 节点语义） | `onehand_assist_touch/onehand_assist_touch_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1150-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1151 | [E58] 单手模式与辅助触控 · 高对比度与色弱兼容（非仅色相区分） | `onehand_assist_touch/onehand_assist_touch_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1151-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1152 | [E58] 单手模式与辅助触控 · DPI 与多屏适配（Per-Monitor v2 感知） | `onehand_assist_touch/onehand_assist_touch_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1152-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1153 | [E58] 单手模式与辅助触控 · 性能账（P95/内存/帧率埋点入账） | `onehand_assist_touch/onehand_assist_touch_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1153-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1154 | [E58] 单手模式与辅助触控 · 与内核健康账联动（心跳/看门狗） | `onehand_assist_touch/onehand_assist_touch_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1154-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1155 | [E58] 单手模式与辅助触控 · 开放接口与版本化（十年不变承诺） | `onehand_assist_touch/onehand_assist_touch_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1155-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1156 | [E58] 单手模式与辅助触控 · 端到端跨域联测（三类窗口矩阵） | `onehand_assist_touch/onehand_assist_touch_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1156-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1157 | [E58] 单手模式与辅助触控 · 边界与 fuzz（乱操作不崩） | `onehand_assist_touch/onehand_assist_touch_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1157-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1158 | [E58] 单手模式与辅助触控 · 恢复路径（断电/休眠唤醒状态保全） | `onehand_assist_touch/onehand_assist_touch_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1158-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1159 | [E58] 单手模式与辅助触控 · 文档与交付三件套（使用/接口/CHANGELOG） | `onehand_assist_touch/onehand_assist_touch_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1159-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |
| UNX-F4-E1160 | [E58] 单手模式与辅助触控 · 域内总日志接入与收官自检 | `onehand_assist_touch/onehand_assist_touch_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1160-J1：构造内核事件通道与 vxwm surface stub，调 onehand_assist_touch_probe() 断言） | 增补 |

### 批 E59 · 主题崩溃恢复与安全模式 UI（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1161 | [E59] 主题崩溃恢复与安全模式 UI · 基础功能面（域内核锚定主路径） | `theme_crash_safe_mode/theme_crash_safe_mode_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1161-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1162 | [E59] 主题崩溃恢复与安全模式 UI · 参数化配置面（开放格式持久化） | `theme_crash_safe_mode/theme_crash_safe_mode_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1162-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1163 | [E59] 主题崩溃恢复与安全模式 UI · 用户设置面板（即时生效预览） | `theme_crash_safe_mode/theme_crash_safe_mode_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1163-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1164 | [E59] 主题崩溃恢复与安全模式 UI · 状态查询与自省接口（lxprocfs 投影） | `theme_crash_safe_mode/theme_crash_safe_mode_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1164-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1165 | [E59] 主题崩溃恢复与安全模式 UI · 异常三要素呈现（错误码→人话映射） | `theme_crash_safe_mode/theme_crash_safe_mode_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1165-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1166 | [E59] 主题崩溃恢复与安全模式 UI · 隐蔽异常捕获（异步回调/静默失败探针） | `theme_crash_safe_mode/theme_crash_safe_mode_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1166-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1167 | [E59] 主题崩溃恢复与安全模式 UI · 体验日志埋点（交互细节层） | `theme_crash_safe_mode/theme_crash_safe_mode_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1167-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1168 | [E59] 主题崩溃恢复与安全模式 UI · 挫败信号自动标记（rage/dead click） | `theme_crash_safe_mode/theme_crash_safe_mode_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1168-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1169 | [E59] 主题崩溃恢复与安全模式 UI · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `theme_crash_safe_mode/theme_crash_safe_mode_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1169-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1170 | [E59] 主题崩溃恢复与安全模式 UI · 屏幕阅读器投影（UIA 节点语义） | `theme_crash_safe_mode/theme_crash_safe_mode_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1170-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1171 | [E59] 主题崩溃恢复与安全模式 UI · 高对比度与色弱兼容（非仅色相区分） | `theme_crash_safe_mode/theme_crash_safe_mode_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1171-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1172 | [E59] 主题崩溃恢复与安全模式 UI · DPI 与多屏适配（Per-Monitor v2 感知） | `theme_crash_safe_mode/theme_crash_safe_mode_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1172-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1173 | [E59] 主题崩溃恢复与安全模式 UI · 性能账（P95/内存/帧率埋点入账） | `theme_crash_safe_mode/theme_crash_safe_mode_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1173-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1174 | [E59] 主题崩溃恢复与安全模式 UI · 与内核健康账联动（心跳/看门狗） | `theme_crash_safe_mode/theme_crash_safe_mode_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1174-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1175 | [E59] 主题崩溃恢复与安全模式 UI · 开放接口与版本化（十年不变承诺） | `theme_crash_safe_mode/theme_crash_safe_mode_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1175-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1176 | [E59] 主题崩溃恢复与安全模式 UI · 端到端跨域联测（三类窗口矩阵） | `theme_crash_safe_mode/theme_crash_safe_mode_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1176-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1177 | [E59] 主题崩溃恢复与安全模式 UI · 边界与 fuzz（乱操作不崩） | `theme_crash_safe_mode/theme_crash_safe_mode_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1177-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1178 | [E59] 主题崩溃恢复与安全模式 UI · 恢复路径（断电/休眠唤醒状态保全） | `theme_crash_safe_mode/theme_crash_safe_mode_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1178-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1179 | [E59] 主题崩溃恢复与安全模式 UI · 文档与交付三件套（使用/接口/CHANGELOG） | `theme_crash_safe_mode/theme_crash_safe_mode_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1179-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |
| UNX-F4-E1180 | [E59] 主题崩溃恢复与安全模式 UI · 域内总日志接入与收官自检 | `theme_crash_safe_mode/theme_crash_safe_mode_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1180-J1：构造内核事件通道与 vxwm surface stub，调 theme_crash_safe_mode_probe() 断言） | 增补 |

### 批 E60 · F4 卷四治理收官与四卷联轧总账（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1181 | [E60] F4 卷四治理收官与四卷联轧总账 · 基础功能面（域内核锚定主路径） | `f4_vol4_governance/f4_vol4_governance_00.rs` | Varix 内核锚定：主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄（UNX-F4-E1181-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1182 | [E60] F4 卷四治理收官与四卷联轧总账 · 参数化配置面（开放格式持久化） | `f4_vol4_governance/f4_vol4_governance_01.rs` | Varix 内核锚定：配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏（UNX-F4-E1182-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1183 | [E60] F4 卷四治理收官与四卷联轧总账 · 用户设置面板（即时生效预览） | `f4_vol4_governance/f4_vol4_governance_02.rs` | Varix 内核锚定：设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变（UNX-F4-E1183-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1184 | [E60] F4 卷四治理收官与四卷联轧总账 · 状态查询与自省接口（lxprocfs 投影） | `f4_vol4_governance/f4_vol4_governance_03.rs` | Varix 内核锚定：状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON（UNX-F4-E1184-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1185 | [E60] F4 卷四治理收官与四卷联轧总账 · 异常三要素呈现（错误码→人话映射） | `f4_vol4_governance/f4_vol4_governance_04.rs` | Varix 内核锚定：全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码（UNX-F4-E1185-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1186 | [E60] F4 卷四治理收官与四卷联轧总账 · 隐蔽异常捕获（异步回调/静默失败探针） | `f4_vol4_governance/f4_vol4_governance_05.rs` | Varix 内核锚定：异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件（UNX-F4-E1186-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1187 | [E60] F4 卷四治理收官与四卷联轧总账 · 体验日志埋点（交互细节层） | `f4_vol4_governance/f4_vol4_governance_06.rs` | Varix 内核锚定：每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容（UNX-F4-E1187-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1188 | [E60] F4 卷四治理收官与四卷联轧总账 · 挫败信号自动标记（rage/dead click） | `f4_vol4_governance/f4_vol4_governance_07.rs` | Varix 内核锚定：同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记（UNX-F4-E1188-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1189 | [E60] F4 卷四治理收官与四卷联轧总账 · 键盘可达性（Tab 序/焦点环/快捷键承诺） | `f4_vol4_governance/f4_vol4_governance_08.rs` | Varix 内核锚定：全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角（UNX-F4-E1189-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1190 | [E60] F4 卷四治理收官与四卷联轧总账 · 屏幕阅读器投影（UIA 节点语义） | `f4_vol4_governance/f4_vol4_governance_09.rs` | Varix 内核锚定：控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致（UNX-F4-E1190-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1191 | [E60] F4 卷四治理收官与四卷联轧总账 · 高对比度与色弱兼容（非仅色相区分） | `f4_vol4_governance/f4_vol4_governance_10.rs` | Varix 内核锚定：状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨（UNX-F4-E1191-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1192 | [E60] F4 卷四治理收官与四卷联轧总账 · DPI 与多屏适配（Per-Monitor v2 感知） | `f4_vol4_governance/f4_vol4_governance_11.rs` | Varix 内核锚定：跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿（UNX-F4-E1192-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1193 | [E60] F4 卷四治理收官与四卷联轧总账 · 性能账（P95/内存/帧率埋点入账） | `f4_vol4_governance/f4_vol4_governance_12.rs` | Varix 内核锚定：操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB（UNX-F4-E1193-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1194 | [E60] F4 卷四治理收官与四卷联轧总账 · 与内核健康账联动（心跳/看门狗） | `f4_vol4_governance/f4_vol4_governance_13.rs` | Varix 内核锚定：功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警（UNX-F4-E1194-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1195 | [E60] F4 卷四治理收官与四卷联轧总账 · 开放接口与版本化（十年不变承诺） | `f4_vol4_governance/f4_vol4_governance_14.rs` | Varix 内核锚定：对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断（UNX-F4-E1195-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1196 | [E60] F4 卷四治理收官与四卷联轧总账 · 端到端跨域联测（三类窗口矩阵） | `f4_vol4_governance/f4_vol4_governance_15.rs` | Varix 内核锚定：普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS（UNX-F4-E1196-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1197 | [E60] F4 卷四治理收官与四卷联轧总账 · 边界与 fuzz（乱操作不崩） | `f4_vol4_governance/f4_vol4_governance_16.rs` | Varix 内核锚定：fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿（UNX-F4-E1197-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1198 | [E60] F4 卷四治理收官与四卷联轧总账 · 恢复路径（断电/休眠唤醒状态保全） | `f4_vol4_governance/f4_vol4_governance_17.rs` | Varix 内核锚定：状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点（UNX-F4-E1198-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1199 | [E60] F4 卷四治理收官与四卷联轧总账 · 文档与交付三件套（使用/接口/CHANGELOG） | `f4_vol4_governance/f4_vol4_governance_18.rs` | Varix 内核锚定：三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等（UNX-F4-E1199-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |
| UNX-F4-E1200 | [E60] F4 卷四治理收官与四卷联轧总账 · 域内总日志接入与收官自检 | `f4_vol4_governance/f4_vol4_governance_19.rs` | Varix 内核锚定：全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0（UNX-F4-E1200-J1：构造内核事件通道与 vxwm surface stub，调 f4_vol4_governance_probe() 断言） | 增补 |

> 卷四小计：300 条。四卷联轧：UNX-F4-E001–E1200 共 1200 项增补，域账 F22401–F23200 零触碰。
