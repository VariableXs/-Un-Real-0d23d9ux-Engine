
---

## 增补卷五 · AI-29 · UNX-F4-E1201–E1500（300 项新功能 · E61–E75 批 · 15 批 × 20 条）

> 收录纪律：独立增补编号，不占域账 F22401–F23200；状态列统一「增补」；
> 全部围绕 Varix 内核锚定（checks.rs 自检面 / lxprocfs.rs 自省面 / 内核事件·会话·IPC 通道 / vxwm surface·popup·特效通道）；
> 与卷一至卷四判据颗粒零重复；保管母本 docs/unxreal/supp/UNX-F4-SUPP-E1201-E1500.md（R-PROC-002 生成器重生成后须回播）。

### 批 E61 · 多用户与账户主题隔离面（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1201 | [E61] 多用户与账户主题隔离面 · 功能主路径（内核 IPC 会话建链） | `multiuser_theme_isolation/multiuser_theme_isolation_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1201-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1202 | [E61] 多用户与账户主题隔离面 · 开放格式配置（原子写+可迁移导出） | `multiuser_theme_isolation/multiuser_theme_isolation_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1202-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1203 | [E61] 多用户与账户主题隔离面 · 设置三态 UI（占位/校验/纠错提示） | `multiuser_theme_isolation/multiuser_theme_isolation_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1203-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1204 | [E61] 多用户与账户主题隔离面 · 浮层完整出路清单（点外/Esc/再点/失焦） | `multiuser_theme_isolation/multiuser_theme_isolation_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1204-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1205 | [E61] 多用户与账户主题隔离面 · 交互状态机全覆盖（连点/打断/拖半/长按） | `multiuser_theme_isolation/multiuser_theme_isolation_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1205-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1206 | [E61] 多用户与账户主题隔离面 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `multiuser_theme_isolation/multiuser_theme_isolation_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1206-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1207 | [E61] 多用户与账户主题隔离面 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `multiuser_theme_isolation/multiuser_theme_isolation_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1207-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1208 | [E61] 多用户与账户主题隔离面 · IME 组合期安全（不触发快捷键不误提交） | `multiuser_theme_isolation/multiuser_theme_isolation_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1208-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1209 | [E61] 多用户与账户主题隔离面 · undo/redo 链（粒度合理可回溯） | `multiuser_theme_isolation/multiuser_theme_isolation_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1209-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1210 | [E61] 多用户与账户主题隔离面 · UIA 双侧投影（Provider 语义+Client 遍历） | `multiuser_theme_isolation/multiuser_theme_isolation_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1210-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1211 | [E61] 多用户与账户主题隔离面 · 文本缩放与 DPI 复检（超大字号不截断） | `multiuser_theme_isolation/multiuser_theme_isolation_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1211-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1212 | [E61] 多用户与账户主题隔离面 · 高对比/色弱/灰度三态复检 | `multiuser_theme_isolation/multiuser_theme_isolation_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1212-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1213 | [E61] 多用户与账户主题隔离面 · 动画节奏与打断（对称缓动/打断续接） | `multiuser_theme_isolation/multiuser_theme_isolation_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1213-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1214 | [E61] 多用户与账户主题隔离面 · 错误三要素+详情折叠（零裸异常码） | `multiuser_theme_isolation/multiuser_theme_isolation_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1214-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1215 | [E61] 多用户与账户主题隔离面 · 体验日志与总日志中心接入 | `multiuser_theme_isolation/multiuser_theme_isolation_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1215-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1216 | [E61] 多用户与账户主题隔离面 · 隐私红线复检（不记输入内容/异步批量写入） | `multiuser_theme_isolation/multiuser_theme_isolation_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1216-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1217 | [E61] 多用户与账户主题隔离面 · 性能账（P95/内存上限/帧率实测入账） | `multiuser_theme_isolation/multiuser_theme_isolation_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1217-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1218 | [E61] 多用户与账户主题隔离面 · 崩溃恢复与看门狗（心跳/自动重建） | `multiuser_theme_isolation/multiuser_theme_isolation_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1218-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1219 | [E61] 多用户与账户主题隔离面 · 开放接口版本化+签名校验+卸载清洁 | `multiuser_theme_isolation/multiuser_theme_isolation_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1219-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |
| UNX-F4-E1220 | [E61] 多用户与账户主题隔离面 · 收官自检（checks.rs 全域 PASS+文档三件套） | `multiuser_theme_isolation/multiuser_theme_isolation_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1220-J1：内核事件通道+vxwm surface stub 下调 multiuser_theme_isolation_probe() 断言） | 增补 |

### 批 E62 · 来宾与会话临时主题档（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1221 | [E62] 来宾与会话临时主题档 · 功能主路径（内核 IPC 会话建链） | `guest_temp_theme/guest_temp_theme_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1221-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1222 | [E62] 来宾与会话临时主题档 · 开放格式配置（原子写+可迁移导出） | `guest_temp_theme/guest_temp_theme_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1222-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1223 | [E62] 来宾与会话临时主题档 · 设置三态 UI（占位/校验/纠错提示） | `guest_temp_theme/guest_temp_theme_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1223-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1224 | [E62] 来宾与会话临时主题档 · 浮层完整出路清单（点外/Esc/再点/失焦） | `guest_temp_theme/guest_temp_theme_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1224-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1225 | [E62] 来宾与会话临时主题档 · 交互状态机全覆盖（连点/打断/拖半/长按） | `guest_temp_theme/guest_temp_theme_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1225-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1226 | [E62] 来宾与会话临时主题档 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `guest_temp_theme/guest_temp_theme_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1226-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1227 | [E62] 来宾与会话临时主题档 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `guest_temp_theme/guest_temp_theme_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1227-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1228 | [E62] 来宾与会话临时主题档 · IME 组合期安全（不触发快捷键不误提交） | `guest_temp_theme/guest_temp_theme_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1228-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1229 | [E62] 来宾与会话临时主题档 · undo/redo 链（粒度合理可回溯） | `guest_temp_theme/guest_temp_theme_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1229-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1230 | [E62] 来宾与会话临时主题档 · UIA 双侧投影（Provider 语义+Client 遍历） | `guest_temp_theme/guest_temp_theme_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1230-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1231 | [E62] 来宾与会话临时主题档 · 文本缩放与 DPI 复检（超大字号不截断） | `guest_temp_theme/guest_temp_theme_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1231-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1232 | [E62] 来宾与会话临时主题档 · 高对比/色弱/灰度三态复检 | `guest_temp_theme/guest_temp_theme_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1232-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1233 | [E62] 来宾与会话临时主题档 · 动画节奏与打断（对称缓动/打断续接） | `guest_temp_theme/guest_temp_theme_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1233-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1234 | [E62] 来宾与会话临时主题档 · 错误三要素+详情折叠（零裸异常码） | `guest_temp_theme/guest_temp_theme_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1234-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1235 | [E62] 来宾与会话临时主题档 · 体验日志与总日志中心接入 | `guest_temp_theme/guest_temp_theme_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1235-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1236 | [E62] 来宾与会话临时主题档 · 隐私红线复检（不记输入内容/异步批量写入） | `guest_temp_theme/guest_temp_theme_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1236-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1237 | [E62] 来宾与会话临时主题档 · 性能账（P95/内存上限/帧率实测入账） | `guest_temp_theme/guest_temp_theme_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1237-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1238 | [E62] 来宾与会话临时主题档 · 崩溃恢复与看门狗（心跳/自动重建） | `guest_temp_theme/guest_temp_theme_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1238-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1239 | [E62] 来宾与会话临时主题档 · 开放接口版本化+签名校验+卸载清洁 | `guest_temp_theme/guest_temp_theme_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1239-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |
| UNX-F4-E1240 | [E62] 来宾与会话临时主题档 · 收官自检（checks.rs 全域 PASS+文档三件套） | `guest_temp_theme/guest_temp_theme_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1240-J1：内核事件通道+vxwm surface stub 下调 guest_temp_theme_probe() 断言） | 增补 |

### 批 E63 · 主题设置漫游与设备同步（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1241 | [E63] 主题设置漫游与设备同步 · 功能主路径（内核 IPC 会话建链） | `theme_roaming_sync/theme_roaming_sync_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1241-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1242 | [E63] 主题设置漫游与设备同步 · 开放格式配置（原子写+可迁移导出） | `theme_roaming_sync/theme_roaming_sync_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1242-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1243 | [E63] 主题设置漫游与设备同步 · 设置三态 UI（占位/校验/纠错提示） | `theme_roaming_sync/theme_roaming_sync_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1243-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1244 | [E63] 主题设置漫游与设备同步 · 浮层完整出路清单（点外/Esc/再点/失焦） | `theme_roaming_sync/theme_roaming_sync_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1244-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1245 | [E63] 主题设置漫游与设备同步 · 交互状态机全覆盖（连点/打断/拖半/长按） | `theme_roaming_sync/theme_roaming_sync_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1245-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1246 | [E63] 主题设置漫游与设备同步 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `theme_roaming_sync/theme_roaming_sync_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1246-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1247 | [E63] 主题设置漫游与设备同步 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `theme_roaming_sync/theme_roaming_sync_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1247-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1248 | [E63] 主题设置漫游与设备同步 · IME 组合期安全（不触发快捷键不误提交） | `theme_roaming_sync/theme_roaming_sync_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1248-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1249 | [E63] 主题设置漫游与设备同步 · undo/redo 链（粒度合理可回溯） | `theme_roaming_sync/theme_roaming_sync_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1249-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1250 | [E63] 主题设置漫游与设备同步 · UIA 双侧投影（Provider 语义+Client 遍历） | `theme_roaming_sync/theme_roaming_sync_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1250-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1251 | [E63] 主题设置漫游与设备同步 · 文本缩放与 DPI 复检（超大字号不截断） | `theme_roaming_sync/theme_roaming_sync_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1251-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1252 | [E63] 主题设置漫游与设备同步 · 高对比/色弱/灰度三态复检 | `theme_roaming_sync/theme_roaming_sync_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1252-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1253 | [E63] 主题设置漫游与设备同步 · 动画节奏与打断（对称缓动/打断续接） | `theme_roaming_sync/theme_roaming_sync_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1253-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1254 | [E63] 主题设置漫游与设备同步 · 错误三要素+详情折叠（零裸异常码） | `theme_roaming_sync/theme_roaming_sync_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1254-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1255 | [E63] 主题设置漫游与设备同步 · 体验日志与总日志中心接入 | `theme_roaming_sync/theme_roaming_sync_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1255-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1256 | [E63] 主题设置漫游与设备同步 · 隐私红线复检（不记输入内容/异步批量写入） | `theme_roaming_sync/theme_roaming_sync_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1256-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1257 | [E63] 主题设置漫游与设备同步 · 性能账（P95/内存上限/帧率实测入账） | `theme_roaming_sync/theme_roaming_sync_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1257-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1258 | [E63] 主题设置漫游与设备同步 · 崩溃恢复与看门狗（心跳/自动重建） | `theme_roaming_sync/theme_roaming_sync_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1258-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1259 | [E63] 主题设置漫游与设备同步 · 开放接口版本化+签名校验+卸载清洁 | `theme_roaming_sync/theme_roaming_sync_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1259-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |
| UNX-F4-E1260 | [E63] 主题设置漫游与设备同步 · 收官自检（checks.rs 全域 PASS+文档三件套） | `theme_roaming_sync/theme_roaming_sync_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1260-J1：内核事件通道+vxwm surface stub 下调 theme_roaming_sync_probe() 断言） | 增补 |

### 批 E64 · 定时主题计划与场景自动化（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1261 | [E64] 定时主题计划与场景自动化 · 功能主路径（内核 IPC 会话建链） | `theme_schedule_scene/theme_schedule_scene_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1261-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1262 | [E64] 定时主题计划与场景自动化 · 开放格式配置（原子写+可迁移导出） | `theme_schedule_scene/theme_schedule_scene_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1262-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1263 | [E64] 定时主题计划与场景自动化 · 设置三态 UI（占位/校验/纠错提示） | `theme_schedule_scene/theme_schedule_scene_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1263-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1264 | [E64] 定时主题计划与场景自动化 · 浮层完整出路清单（点外/Esc/再点/失焦） | `theme_schedule_scene/theme_schedule_scene_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1264-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1265 | [E64] 定时主题计划与场景自动化 · 交互状态机全覆盖（连点/打断/拖半/长按） | `theme_schedule_scene/theme_schedule_scene_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1265-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1266 | [E64] 定时主题计划与场景自动化 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `theme_schedule_scene/theme_schedule_scene_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1266-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1267 | [E64] 定时主题计划与场景自动化 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `theme_schedule_scene/theme_schedule_scene_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1267-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1268 | [E64] 定时主题计划与场景自动化 · IME 组合期安全（不触发快捷键不误提交） | `theme_schedule_scene/theme_schedule_scene_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1268-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1269 | [E64] 定时主题计划与场景自动化 · undo/redo 链（粒度合理可回溯） | `theme_schedule_scene/theme_schedule_scene_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1269-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1270 | [E64] 定时主题计划与场景自动化 · UIA 双侧投影（Provider 语义+Client 遍历） | `theme_schedule_scene/theme_schedule_scene_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1270-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1271 | [E64] 定时主题计划与场景自动化 · 文本缩放与 DPI 复检（超大字号不截断） | `theme_schedule_scene/theme_schedule_scene_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1271-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1272 | [E64] 定时主题计划与场景自动化 · 高对比/色弱/灰度三态复检 | `theme_schedule_scene/theme_schedule_scene_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1272-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1273 | [E64] 定时主题计划与场景自动化 · 动画节奏与打断（对称缓动/打断续接） | `theme_schedule_scene/theme_schedule_scene_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1273-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1274 | [E64] 定时主题计划与场景自动化 · 错误三要素+详情折叠（零裸异常码） | `theme_schedule_scene/theme_schedule_scene_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1274-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1275 | [E64] 定时主题计划与场景自动化 · 体验日志与总日志中心接入 | `theme_schedule_scene/theme_schedule_scene_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1275-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1276 | [E64] 定时主题计划与场景自动化 · 隐私红线复检（不记输入内容/异步批量写入） | `theme_schedule_scene/theme_schedule_scene_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1276-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1277 | [E64] 定时主题计划与场景自动化 · 性能账（P95/内存上限/帧率实测入账） | `theme_schedule_scene/theme_schedule_scene_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1277-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1278 | [E64] 定时主题计划与场景自动化 · 崩溃恢复与看门狗（心跳/自动重建） | `theme_schedule_scene/theme_schedule_scene_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1278-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1279 | [E64] 定时主题计划与场景自动化 · 开放接口版本化+签名校验+卸载清洁 | `theme_schedule_scene/theme_schedule_scene_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1279-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |
| UNX-F4-E1280 | [E64] 定时主题计划与场景自动化 · 收官自检（checks.rs 全域 PASS+文档三件套） | `theme_schedule_scene/theme_schedule_scene_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1280-J1：内核事件通道+vxwm surface stub 下调 theme_schedule_scene_probe() 断言） | 增补 |

### 批 E65 · 应用级主题覆写清单管理（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1281 | [E65] 应用级主题覆写清单管理 · 功能主路径（内核 IPC 会话建链） | `app_theme_override/app_theme_override_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1281-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1282 | [E65] 应用级主题覆写清单管理 · 开放格式配置（原子写+可迁移导出） | `app_theme_override/app_theme_override_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1282-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1283 | [E65] 应用级主题覆写清单管理 · 设置三态 UI（占位/校验/纠错提示） | `app_theme_override/app_theme_override_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1283-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1284 | [E65] 应用级主题覆写清单管理 · 浮层完整出路清单（点外/Esc/再点/失焦） | `app_theme_override/app_theme_override_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1284-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1285 | [E65] 应用级主题覆写清单管理 · 交互状态机全覆盖（连点/打断/拖半/长按） | `app_theme_override/app_theme_override_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1285-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1286 | [E65] 应用级主题覆写清单管理 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `app_theme_override/app_theme_override_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1286-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1287 | [E65] 应用级主题覆写清单管理 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `app_theme_override/app_theme_override_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1287-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1288 | [E65] 应用级主题覆写清单管理 · IME 组合期安全（不触发快捷键不误提交） | `app_theme_override/app_theme_override_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1288-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1289 | [E65] 应用级主题覆写清单管理 · undo/redo 链（粒度合理可回溯） | `app_theme_override/app_theme_override_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1289-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1290 | [E65] 应用级主题覆写清单管理 · UIA 双侧投影（Provider 语义+Client 遍历） | `app_theme_override/app_theme_override_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1290-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1291 | [E65] 应用级主题覆写清单管理 · 文本缩放与 DPI 复检（超大字号不截断） | `app_theme_override/app_theme_override_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1291-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1292 | [E65] 应用级主题覆写清单管理 · 高对比/色弱/灰度三态复检 | `app_theme_override/app_theme_override_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1292-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1293 | [E65] 应用级主题覆写清单管理 · 动画节奏与打断（对称缓动/打断续接） | `app_theme_override/app_theme_override_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1293-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1294 | [E65] 应用级主题覆写清单管理 · 错误三要素+详情折叠（零裸异常码） | `app_theme_override/app_theme_override_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1294-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1295 | [E65] 应用级主题覆写清单管理 · 体验日志与总日志中心接入 | `app_theme_override/app_theme_override_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1295-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1296 | [E65] 应用级主题覆写清单管理 · 隐私红线复检（不记输入内容/异步批量写入） | `app_theme_override/app_theme_override_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1296-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1297 | [E65] 应用级主题覆写清单管理 · 性能账（P95/内存上限/帧率实测入账） | `app_theme_override/app_theme_override_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1297-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1298 | [E65] 应用级主题覆写清单管理 · 崩溃恢复与看门狗（心跳/自动重建） | `app_theme_override/app_theme_override_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1298-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1299 | [E65] 应用级主题覆写清单管理 · 开放接口版本化+签名校验+卸载清洁 | `app_theme_override/app_theme_override_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1299-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |
| UNX-F4-E1300 | [E65] 应用级主题覆写清单管理 · 收官自检（checks.rs 全域 PASS+文档三件套） | `app_theme_override/app_theme_override_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1300-J1：内核事件通道+vxwm surface stub 下调 app_theme_override_probe() 断言） | 增补 |

### 批 E66 · 主题资源压缩与增量更新（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1301 | [E66] 主题资源压缩与增量更新 · 功能主路径（内核 IPC 会话建链） | `theme_pack_delta_update/theme_pack_delta_update_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1301-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1302 | [E66] 主题资源压缩与增量更新 · 开放格式配置（原子写+可迁移导出） | `theme_pack_delta_update/theme_pack_delta_update_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1302-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1303 | [E66] 主题资源压缩与增量更新 · 设置三态 UI（占位/校验/纠错提示） | `theme_pack_delta_update/theme_pack_delta_update_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1303-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1304 | [E66] 主题资源压缩与增量更新 · 浮层完整出路清单（点外/Esc/再点/失焦） | `theme_pack_delta_update/theme_pack_delta_update_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1304-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1305 | [E66] 主题资源压缩与增量更新 · 交互状态机全覆盖（连点/打断/拖半/长按） | `theme_pack_delta_update/theme_pack_delta_update_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1305-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1306 | [E66] 主题资源压缩与增量更新 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `theme_pack_delta_update/theme_pack_delta_update_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1306-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1307 | [E66] 主题资源压缩与增量更新 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `theme_pack_delta_update/theme_pack_delta_update_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1307-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1308 | [E66] 主题资源压缩与增量更新 · IME 组合期安全（不触发快捷键不误提交） | `theme_pack_delta_update/theme_pack_delta_update_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1308-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1309 | [E66] 主题资源压缩与增量更新 · undo/redo 链（粒度合理可回溯） | `theme_pack_delta_update/theme_pack_delta_update_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1309-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1310 | [E66] 主题资源压缩与增量更新 · UIA 双侧投影（Provider 语义+Client 遍历） | `theme_pack_delta_update/theme_pack_delta_update_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1310-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1311 | [E66] 主题资源压缩与增量更新 · 文本缩放与 DPI 复检（超大字号不截断） | `theme_pack_delta_update/theme_pack_delta_update_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1311-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1312 | [E66] 主题资源压缩与增量更新 · 高对比/色弱/灰度三态复检 | `theme_pack_delta_update/theme_pack_delta_update_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1312-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1313 | [E66] 主题资源压缩与增量更新 · 动画节奏与打断（对称缓动/打断续接） | `theme_pack_delta_update/theme_pack_delta_update_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1313-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1314 | [E66] 主题资源压缩与增量更新 · 错误三要素+详情折叠（零裸异常码） | `theme_pack_delta_update/theme_pack_delta_update_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1314-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1315 | [E66] 主题资源压缩与增量更新 · 体验日志与总日志中心接入 | `theme_pack_delta_update/theme_pack_delta_update_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1315-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1316 | [E66] 主题资源压缩与增量更新 · 隐私红线复检（不记输入内容/异步批量写入） | `theme_pack_delta_update/theme_pack_delta_update_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1316-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1317 | [E66] 主题资源压缩与增量更新 · 性能账（P95/内存上限/帧率实测入账） | `theme_pack_delta_update/theme_pack_delta_update_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1317-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1318 | [E66] 主题资源压缩与增量更新 · 崩溃恢复与看门狗（心跳/自动重建） | `theme_pack_delta_update/theme_pack_delta_update_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1318-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1319 | [E66] 主题资源压缩与增量更新 · 开放接口版本化+签名校验+卸载清洁 | `theme_pack_delta_update/theme_pack_delta_update_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1319-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |
| UNX-F4-E1320 | [E66] 主题资源压缩与增量更新 · 收官自检（checks.rs 全域 PASS+文档三件套） | `theme_pack_delta_update/theme_pack_delta_update_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1320-J1：内核事件通道+vxwm surface stub 下调 theme_pack_delta_update_probe() 断言） | 增补 |

### 批 E67 · 第三方主题签名与安全审计（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1321 | [E67] 第三方主题签名与安全审计 · 功能主路径（内核 IPC 会话建链） | `third_party_theme_signing/third_party_theme_signing_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1321-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1322 | [E67] 第三方主题签名与安全审计 · 开放格式配置（原子写+可迁移导出） | `third_party_theme_signing/third_party_theme_signing_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1322-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1323 | [E67] 第三方主题签名与安全审计 · 设置三态 UI（占位/校验/纠错提示） | `third_party_theme_signing/third_party_theme_signing_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1323-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1324 | [E67] 第三方主题签名与安全审计 · 浮层完整出路清单（点外/Esc/再点/失焦） | `third_party_theme_signing/third_party_theme_signing_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1324-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1325 | [E67] 第三方主题签名与安全审计 · 交互状态机全覆盖（连点/打断/拖半/长按） | `third_party_theme_signing/third_party_theme_signing_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1325-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1326 | [E67] 第三方主题签名与安全审计 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `third_party_theme_signing/third_party_theme_signing_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1326-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1327 | [E67] 第三方主题签名与安全审计 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `third_party_theme_signing/third_party_theme_signing_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1327-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1328 | [E67] 第三方主题签名与安全审计 · IME 组合期安全（不触发快捷键不误提交） | `third_party_theme_signing/third_party_theme_signing_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1328-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1329 | [E67] 第三方主题签名与安全审计 · undo/redo 链（粒度合理可回溯） | `third_party_theme_signing/third_party_theme_signing_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1329-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1330 | [E67] 第三方主题签名与安全审计 · UIA 双侧投影（Provider 语义+Client 遍历） | `third_party_theme_signing/third_party_theme_signing_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1330-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1331 | [E67] 第三方主题签名与安全审计 · 文本缩放与 DPI 复检（超大字号不截断） | `third_party_theme_signing/third_party_theme_signing_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1331-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1332 | [E67] 第三方主题签名与安全审计 · 高对比/色弱/灰度三态复检 | `third_party_theme_signing/third_party_theme_signing_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1332-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1333 | [E67] 第三方主题签名与安全审计 · 动画节奏与打断（对称缓动/打断续接） | `third_party_theme_signing/third_party_theme_signing_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1333-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1334 | [E67] 第三方主题签名与安全审计 · 错误三要素+详情折叠（零裸异常码） | `third_party_theme_signing/third_party_theme_signing_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1334-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1335 | [E67] 第三方主题签名与安全审计 · 体验日志与总日志中心接入 | `third_party_theme_signing/third_party_theme_signing_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1335-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1336 | [E67] 第三方主题签名与安全审计 · 隐私红线复检（不记输入内容/异步批量写入） | `third_party_theme_signing/third_party_theme_signing_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1336-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1337 | [E67] 第三方主题签名与安全审计 · 性能账（P95/内存上限/帧率实测入账） | `third_party_theme_signing/third_party_theme_signing_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1337-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1338 | [E67] 第三方主题签名与安全审计 · 崩溃恢复与看门狗（心跳/自动重建） | `third_party_theme_signing/third_party_theme_signing_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1338-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1339 | [E67] 第三方主题签名与安全审计 · 开放接口版本化+签名校验+卸载清洁 | `third_party_theme_signing/third_party_theme_signing_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1339-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |
| UNX-F4-E1340 | [E67] 第三方主题签名与安全审计 · 收官自检（checks.rs 全域 PASS+文档三件套） | `third_party_theme_signing/third_party_theme_signing_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1340-J1：内核事件通道+vxwm surface stub 下调 third_party_theme_signing_probe() 断言） | 增补 |

### 批 E68 · 主题卸载与残留清洁审计（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1341 | [E68] 主题卸载与残留清洁审计 · 功能主路径（内核 IPC 会话建链） | `theme_uninstall_cleanup/theme_uninstall_cleanup_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1341-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1342 | [E68] 主题卸载与残留清洁审计 · 开放格式配置（原子写+可迁移导出） | `theme_uninstall_cleanup/theme_uninstall_cleanup_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1342-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1343 | [E68] 主题卸载与残留清洁审计 · 设置三态 UI（占位/校验/纠错提示） | `theme_uninstall_cleanup/theme_uninstall_cleanup_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1343-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1344 | [E68] 主题卸载与残留清洁审计 · 浮层完整出路清单（点外/Esc/再点/失焦） | `theme_uninstall_cleanup/theme_uninstall_cleanup_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1344-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1345 | [E68] 主题卸载与残留清洁审计 · 交互状态机全覆盖（连点/打断/拖半/长按） | `theme_uninstall_cleanup/theme_uninstall_cleanup_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1345-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1346 | [E68] 主题卸载与残留清洁审计 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `theme_uninstall_cleanup/theme_uninstall_cleanup_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1346-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1347 | [E68] 主题卸载与残留清洁审计 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `theme_uninstall_cleanup/theme_uninstall_cleanup_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1347-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1348 | [E68] 主题卸载与残留清洁审计 · IME 组合期安全（不触发快捷键不误提交） | `theme_uninstall_cleanup/theme_uninstall_cleanup_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1348-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1349 | [E68] 主题卸载与残留清洁审计 · undo/redo 链（粒度合理可回溯） | `theme_uninstall_cleanup/theme_uninstall_cleanup_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1349-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1350 | [E68] 主题卸载与残留清洁审计 · UIA 双侧投影（Provider 语义+Client 遍历） | `theme_uninstall_cleanup/theme_uninstall_cleanup_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1350-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1351 | [E68] 主题卸载与残留清洁审计 · 文本缩放与 DPI 复检（超大字号不截断） | `theme_uninstall_cleanup/theme_uninstall_cleanup_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1351-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1352 | [E68] 主题卸载与残留清洁审计 · 高对比/色弱/灰度三态复检 | `theme_uninstall_cleanup/theme_uninstall_cleanup_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1352-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1353 | [E68] 主题卸载与残留清洁审计 · 动画节奏与打断（对称缓动/打断续接） | `theme_uninstall_cleanup/theme_uninstall_cleanup_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1353-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1354 | [E68] 主题卸载与残留清洁审计 · 错误三要素+详情折叠（零裸异常码） | `theme_uninstall_cleanup/theme_uninstall_cleanup_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1354-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1355 | [E68] 主题卸载与残留清洁审计 · 体验日志与总日志中心接入 | `theme_uninstall_cleanup/theme_uninstall_cleanup_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1355-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1356 | [E68] 主题卸载与残留清洁审计 · 隐私红线复检（不记输入内容/异步批量写入） | `theme_uninstall_cleanup/theme_uninstall_cleanup_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1356-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1357 | [E68] 主题卸载与残留清洁审计 · 性能账（P95/内存上限/帧率实测入账） | `theme_uninstall_cleanup/theme_uninstall_cleanup_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1357-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1358 | [E68] 主题卸载与残留清洁审计 · 崩溃恢复与看门狗（心跳/自动重建） | `theme_uninstall_cleanup/theme_uninstall_cleanup_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1358-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1359 | [E68] 主题卸载与残留清洁审计 · 开放接口版本化+签名校验+卸载清洁 | `theme_uninstall_cleanup/theme_uninstall_cleanup_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1359-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |
| UNX-F4-E1360 | [E68] 主题卸载与残留清洁审计 · 收官自检（checks.rs 全域 PASS+文档三件套） | `theme_uninstall_cleanup/theme_uninstall_cleanup_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1360-J1：内核事件通道+vxwm surface stub 下调 theme_uninstall_cleanup_probe() 断言） | 增补 |

### 批 E69 · 屏幕键盘预测条与滑行输入 UI（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1361 | [E69] 屏幕键盘预测条与滑行输入 UI · 功能主路径（内核 IPC 会话建链） | `flick_typing_ui/flick_typing_ui_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1361-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1362 | [E69] 屏幕键盘预测条与滑行输入 UI · 开放格式配置（原子写+可迁移导出） | `flick_typing_ui/flick_typing_ui_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1362-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1363 | [E69] 屏幕键盘预测条与滑行输入 UI · 设置三态 UI（占位/校验/纠错提示） | `flick_typing_ui/flick_typing_ui_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1363-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1364 | [E69] 屏幕键盘预测条与滑行输入 UI · 浮层完整出路清单（点外/Esc/再点/失焦） | `flick_typing_ui/flick_typing_ui_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1364-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1365 | [E69] 屏幕键盘预测条与滑行输入 UI · 交互状态机全覆盖（连点/打断/拖半/长按） | `flick_typing_ui/flick_typing_ui_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1365-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1366 | [E69] 屏幕键盘预测条与滑行输入 UI · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `flick_typing_ui/flick_typing_ui_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1366-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1367 | [E69] 屏幕键盘预测条与滑行输入 UI · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `flick_typing_ui/flick_typing_ui_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1367-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1368 | [E69] 屏幕键盘预测条与滑行输入 UI · IME 组合期安全（不触发快捷键不误提交） | `flick_typing_ui/flick_typing_ui_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1368-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1369 | [E69] 屏幕键盘预测条与滑行输入 UI · undo/redo 链（粒度合理可回溯） | `flick_typing_ui/flick_typing_ui_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1369-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1370 | [E69] 屏幕键盘预测条与滑行输入 UI · UIA 双侧投影（Provider 语义+Client 遍历） | `flick_typing_ui/flick_typing_ui_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1370-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1371 | [E69] 屏幕键盘预测条与滑行输入 UI · 文本缩放与 DPI 复检（超大字号不截断） | `flick_typing_ui/flick_typing_ui_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1371-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1372 | [E69] 屏幕键盘预测条与滑行输入 UI · 高对比/色弱/灰度三态复检 | `flick_typing_ui/flick_typing_ui_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1372-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1373 | [E69] 屏幕键盘预测条与滑行输入 UI · 动画节奏与打断（对称缓动/打断续接） | `flick_typing_ui/flick_typing_ui_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1373-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1374 | [E69] 屏幕键盘预测条与滑行输入 UI · 错误三要素+详情折叠（零裸异常码） | `flick_typing_ui/flick_typing_ui_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1374-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1375 | [E69] 屏幕键盘预测条与滑行输入 UI · 体验日志与总日志中心接入 | `flick_typing_ui/flick_typing_ui_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1375-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1376 | [E69] 屏幕键盘预测条与滑行输入 UI · 隐私红线复检（不记输入内容/异步批量写入） | `flick_typing_ui/flick_typing_ui_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1376-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1377 | [E69] 屏幕键盘预测条与滑行输入 UI · 性能账（P95/内存上限/帧率实测入账） | `flick_typing_ui/flick_typing_ui_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1377-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1378 | [E69] 屏幕键盘预测条与滑行输入 UI · 崩溃恢复与看门狗（心跳/自动重建） | `flick_typing_ui/flick_typing_ui_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1378-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1379 | [E69] 屏幕键盘预测条与滑行输入 UI · 开放接口版本化+签名校验+卸载清洁 | `flick_typing_ui/flick_typing_ui_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1379-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |
| UNX-F4-E1380 | [E69] 屏幕键盘预测条与滑行输入 UI · 收官自检（checks.rs 全域 PASS+文档三件套） | `flick_typing_ui/flick_typing_ui_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1380-J1：内核事件通道+vxwm surface stub 下调 flick_typing_ui_probe() 断言） | 增补 |

### 批 E70 · 剪贴板历史与云剪贴板 UI 面（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1381 | [E70] 剪贴板历史与云剪贴板 UI 面 · 功能主路径（内核 IPC 会话建链） | `clipboard_history_ui/clipboard_history_ui_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1381-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1382 | [E70] 剪贴板历史与云剪贴板 UI 面 · 开放格式配置（原子写+可迁移导出） | `clipboard_history_ui/clipboard_history_ui_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1382-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1383 | [E70] 剪贴板历史与云剪贴板 UI 面 · 设置三态 UI（占位/校验/纠错提示） | `clipboard_history_ui/clipboard_history_ui_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1383-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1384 | [E70] 剪贴板历史与云剪贴板 UI 面 · 浮层完整出路清单（点外/Esc/再点/失焦） | `clipboard_history_ui/clipboard_history_ui_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1384-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1385 | [E70] 剪贴板历史与云剪贴板 UI 面 · 交互状态机全覆盖（连点/打断/拖半/长按） | `clipboard_history_ui/clipboard_history_ui_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1385-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1386 | [E70] 剪贴板历史与云剪贴板 UI 面 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `clipboard_history_ui/clipboard_history_ui_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1386-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1387 | [E70] 剪贴板历史与云剪贴板 UI 面 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `clipboard_history_ui/clipboard_history_ui_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1387-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1388 | [E70] 剪贴板历史与云剪贴板 UI 面 · IME 组合期安全（不触发快捷键不误提交） | `clipboard_history_ui/clipboard_history_ui_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1388-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1389 | [E70] 剪贴板历史与云剪贴板 UI 面 · undo/redo 链（粒度合理可回溯） | `clipboard_history_ui/clipboard_history_ui_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1389-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1390 | [E70] 剪贴板历史与云剪贴板 UI 面 · UIA 双侧投影（Provider 语义+Client 遍历） | `clipboard_history_ui/clipboard_history_ui_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1390-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1391 | [E70] 剪贴板历史与云剪贴板 UI 面 · 文本缩放与 DPI 复检（超大字号不截断） | `clipboard_history_ui/clipboard_history_ui_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1391-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1392 | [E70] 剪贴板历史与云剪贴板 UI 面 · 高对比/色弱/灰度三态复检 | `clipboard_history_ui/clipboard_history_ui_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1392-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1393 | [E70] 剪贴板历史与云剪贴板 UI 面 · 动画节奏与打断（对称缓动/打断续接） | `clipboard_history_ui/clipboard_history_ui_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1393-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1394 | [E70] 剪贴板历史与云剪贴板 UI 面 · 错误三要素+详情折叠（零裸异常码） | `clipboard_history_ui/clipboard_history_ui_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1394-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1395 | [E70] 剪贴板历史与云剪贴板 UI 面 · 体验日志与总日志中心接入 | `clipboard_history_ui/clipboard_history_ui_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1395-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1396 | [E70] 剪贴板历史与云剪贴板 UI 面 · 隐私红线复检（不记输入内容/异步批量写入） | `clipboard_history_ui/clipboard_history_ui_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1396-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1397 | [E70] 剪贴板历史与云剪贴板 UI 面 · 性能账（P95/内存上限/帧率实测入账） | `clipboard_history_ui/clipboard_history_ui_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1397-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1398 | [E70] 剪贴板历史与云剪贴板 UI 面 · 崩溃恢复与看门狗（心跳/自动重建） | `clipboard_history_ui/clipboard_history_ui_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1398-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1399 | [E70] 剪贴板历史与云剪贴板 UI 面 · 开放接口版本化+签名校验+卸载清洁 | `clipboard_history_ui/clipboard_history_ui_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1399-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |
| UNX-F4-E1400 | [E70] 剪贴板历史与云剪贴板 UI 面 · 收官自检（checks.rs 全域 PASS+文档三件套） | `clipboard_history_ui/clipboard_history_ui_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1400-J1：内核事件通道+vxwm surface stub 下调 clipboard_history_ui_probe() 断言） | 增补 |

### 批 E71 · 表情连击与颜文字快捷面板（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1401 | [E71] 表情连击与颜文字快捷面板 · 功能主路径（内核 IPC 会话建链） | `kaomoji_quick_panel/kaomoji_quick_panel_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1401-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1402 | [E71] 表情连击与颜文字快捷面板 · 开放格式配置（原子写+可迁移导出） | `kaomoji_quick_panel/kaomoji_quick_panel_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1402-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1403 | [E71] 表情连击与颜文字快捷面板 · 设置三态 UI（占位/校验/纠错提示） | `kaomoji_quick_panel/kaomoji_quick_panel_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1403-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1404 | [E71] 表情连击与颜文字快捷面板 · 浮层完整出路清单（点外/Esc/再点/失焦） | `kaomoji_quick_panel/kaomoji_quick_panel_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1404-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1405 | [E71] 表情连击与颜文字快捷面板 · 交互状态机全覆盖（连点/打断/拖半/长按） | `kaomoji_quick_panel/kaomoji_quick_panel_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1405-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1406 | [E71] 表情连击与颜文字快捷面板 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `kaomoji_quick_panel/kaomoji_quick_panel_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1406-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1407 | [E71] 表情连击与颜文字快捷面板 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `kaomoji_quick_panel/kaomoji_quick_panel_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1407-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1408 | [E71] 表情连击与颜文字快捷面板 · IME 组合期安全（不触发快捷键不误提交） | `kaomoji_quick_panel/kaomoji_quick_panel_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1408-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1409 | [E71] 表情连击与颜文字快捷面板 · undo/redo 链（粒度合理可回溯） | `kaomoji_quick_panel/kaomoji_quick_panel_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1409-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1410 | [E71] 表情连击与颜文字快捷面板 · UIA 双侧投影（Provider 语义+Client 遍历） | `kaomoji_quick_panel/kaomoji_quick_panel_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1410-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1411 | [E71] 表情连击与颜文字快捷面板 · 文本缩放与 DPI 复检（超大字号不截断） | `kaomoji_quick_panel/kaomoji_quick_panel_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1411-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1412 | [E71] 表情连击与颜文字快捷面板 · 高对比/色弱/灰度三态复检 | `kaomoji_quick_panel/kaomoji_quick_panel_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1412-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1413 | [E71] 表情连击与颜文字快捷面板 · 动画节奏与打断（对称缓动/打断续接） | `kaomoji_quick_panel/kaomoji_quick_panel_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1413-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1414 | [E71] 表情连击与颜文字快捷面板 · 错误三要素+详情折叠（零裸异常码） | `kaomoji_quick_panel/kaomoji_quick_panel_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1414-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1415 | [E71] 表情连击与颜文字快捷面板 · 体验日志与总日志中心接入 | `kaomoji_quick_panel/kaomoji_quick_panel_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1415-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1416 | [E71] 表情连击与颜文字快捷面板 · 隐私红线复检（不记输入内容/异步批量写入） | `kaomoji_quick_panel/kaomoji_quick_panel_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1416-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1417 | [E71] 表情连击与颜文字快捷面板 · 性能账（P95/内存上限/帧率实测入账） | `kaomoji_quick_panel/kaomoji_quick_panel_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1417-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1418 | [E71] 表情连击与颜文字快捷面板 · 崩溃恢复与看门狗（心跳/自动重建） | `kaomoji_quick_panel/kaomoji_quick_panel_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1418-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1419 | [E71] 表情连击与颜文字快捷面板 · 开放接口版本化+签名校验+卸载清洁 | `kaomoji_quick_panel/kaomoji_quick_panel_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1419-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |
| UNX-F4-E1420 | [E71] 表情连击与颜文字快捷面板 · 收官自检（checks.rs 全域 PASS+文档三件套） | `kaomoji_quick_panel/kaomoji_quick_panel_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1420-J1：内核事件通道+vxwm surface stub 下调 kaomoji_quick_panel_probe() 断言） | 增补 |

### 批 E72 · 输入统计仪表盘（打字速度/词频）（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1421 | [E72] 输入统计仪表盘（打字速度/词频） · 功能主路径（内核 IPC 会话建链） | `typing_stats_dashboard/typing_stats_dashboard_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1421-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1422 | [E72] 输入统计仪表盘（打字速度/词频） · 开放格式配置（原子写+可迁移导出） | `typing_stats_dashboard/typing_stats_dashboard_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1422-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1423 | [E72] 输入统计仪表盘（打字速度/词频） · 设置三态 UI（占位/校验/纠错提示） | `typing_stats_dashboard/typing_stats_dashboard_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1423-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1424 | [E72] 输入统计仪表盘（打字速度/词频） · 浮层完整出路清单（点外/Esc/再点/失焦） | `typing_stats_dashboard/typing_stats_dashboard_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1424-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1425 | [E72] 输入统计仪表盘（打字速度/词频） · 交互状态机全覆盖（连点/打断/拖半/长按） | `typing_stats_dashboard/typing_stats_dashboard_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1425-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1426 | [E72] 输入统计仪表盘（打字速度/词频） · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `typing_stats_dashboard/typing_stats_dashboard_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1426-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1427 | [E72] 输入统计仪表盘（打字速度/词频） · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `typing_stats_dashboard/typing_stats_dashboard_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1427-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1428 | [E72] 输入统计仪表盘（打字速度/词频） · IME 组合期安全（不触发快捷键不误提交） | `typing_stats_dashboard/typing_stats_dashboard_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1428-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1429 | [E72] 输入统计仪表盘（打字速度/词频） · undo/redo 链（粒度合理可回溯） | `typing_stats_dashboard/typing_stats_dashboard_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1429-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1430 | [E72] 输入统计仪表盘（打字速度/词频） · UIA 双侧投影（Provider 语义+Client 遍历） | `typing_stats_dashboard/typing_stats_dashboard_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1430-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1431 | [E72] 输入统计仪表盘（打字速度/词频） · 文本缩放与 DPI 复检（超大字号不截断） | `typing_stats_dashboard/typing_stats_dashboard_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1431-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1432 | [E72] 输入统计仪表盘（打字速度/词频） · 高对比/色弱/灰度三态复检 | `typing_stats_dashboard/typing_stats_dashboard_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1432-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1433 | [E72] 输入统计仪表盘（打字速度/词频） · 动画节奏与打断（对称缓动/打断续接） | `typing_stats_dashboard/typing_stats_dashboard_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1433-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1434 | [E72] 输入统计仪表盘（打字速度/词频） · 错误三要素+详情折叠（零裸异常码） | `typing_stats_dashboard/typing_stats_dashboard_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1434-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1435 | [E72] 输入统计仪表盘（打字速度/词频） · 体验日志与总日志中心接入 | `typing_stats_dashboard/typing_stats_dashboard_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1435-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1436 | [E72] 输入统计仪表盘（打字速度/词频） · 隐私红线复检（不记输入内容/异步批量写入） | `typing_stats_dashboard/typing_stats_dashboard_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1436-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1437 | [E72] 输入统计仪表盘（打字速度/词频） · 性能账（P95/内存上限/帧率实测入账） | `typing_stats_dashboard/typing_stats_dashboard_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1437-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1438 | [E72] 输入统计仪表盘（打字速度/词频） · 崩溃恢复与看门狗（心跳/自动重建） | `typing_stats_dashboard/typing_stats_dashboard_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1438-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1439 | [E72] 输入统计仪表盘（打字速度/词频） · 开放接口版本化+签名校验+卸载清洁 | `typing_stats_dashboard/typing_stats_dashboard_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1439-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |
| UNX-F4-E1440 | [E72] 输入统计仪表盘（打字速度/词频） · 收官自检（checks.rs 全域 PASS+文档三件套） | `typing_stats_dashboard/typing_stats_dashboard_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1440-J1：内核事件通道+vxwm surface stub 下调 typing_stats_dashboard_probe() 断言） | 增补 |

### 批 E73 · 盲文显示与点字输出桥（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1441 | [E73] 盲文显示与点字输出桥 · 功能主路径（内核 IPC 会话建链） | `braille_display_bridge/braille_display_bridge_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1441-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1442 | [E73] 盲文显示与点字输出桥 · 开放格式配置（原子写+可迁移导出） | `braille_display_bridge/braille_display_bridge_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1442-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1443 | [E73] 盲文显示与点字输出桥 · 设置三态 UI（占位/校验/纠错提示） | `braille_display_bridge/braille_display_bridge_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1443-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1444 | [E73] 盲文显示与点字输出桥 · 浮层完整出路清单（点外/Esc/再点/失焦） | `braille_display_bridge/braille_display_bridge_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1444-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1445 | [E73] 盲文显示与点字输出桥 · 交互状态机全覆盖（连点/打断/拖半/长按） | `braille_display_bridge/braille_display_bridge_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1445-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1446 | [E73] 盲文显示与点字输出桥 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `braille_display_bridge/braille_display_bridge_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1446-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1447 | [E73] 盲文显示与点字输出桥 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `braille_display_bridge/braille_display_bridge_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1447-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1448 | [E73] 盲文显示与点字输出桥 · IME 组合期安全（不触发快捷键不误提交） | `braille_display_bridge/braille_display_bridge_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1448-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1449 | [E73] 盲文显示与点字输出桥 · undo/redo 链（粒度合理可回溯） | `braille_display_bridge/braille_display_bridge_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1449-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1450 | [E73] 盲文显示与点字输出桥 · UIA 双侧投影（Provider 语义+Client 遍历） | `braille_display_bridge/braille_display_bridge_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1450-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1451 | [E73] 盲文显示与点字输出桥 · 文本缩放与 DPI 复检（超大字号不截断） | `braille_display_bridge/braille_display_bridge_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1451-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1452 | [E73] 盲文显示与点字输出桥 · 高对比/色弱/灰度三态复检 | `braille_display_bridge/braille_display_bridge_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1452-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1453 | [E73] 盲文显示与点字输出桥 · 动画节奏与打断（对称缓动/打断续接） | `braille_display_bridge/braille_display_bridge_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1453-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1454 | [E73] 盲文显示与点字输出桥 · 错误三要素+详情折叠（零裸异常码） | `braille_display_bridge/braille_display_bridge_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1454-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1455 | [E73] 盲文显示与点字输出桥 · 体验日志与总日志中心接入 | `braille_display_bridge/braille_display_bridge_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1455-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1456 | [E73] 盲文显示与点字输出桥 · 隐私红线复检（不记输入内容/异步批量写入） | `braille_display_bridge/braille_display_bridge_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1456-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1457 | [E73] 盲文显示与点字输出桥 · 性能账（P95/内存上限/帧率实测入账） | `braille_display_bridge/braille_display_bridge_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1457-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1458 | [E73] 盲文显示与点字输出桥 · 崩溃恢复与看门狗（心跳/自动重建） | `braille_display_bridge/braille_display_bridge_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1458-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1459 | [E73] 盲文显示与点字输出桥 · 开放接口版本化+签名校验+卸载清洁 | `braille_display_bridge/braille_display_bridge_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1459-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |
| UNX-F4-E1460 | [E73] 盲文显示与点字输出桥 · 收官自检（checks.rs 全域 PASS+文档三件套） | `braille_display_bridge/braille_display_bridge_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1460-J1：内核事件通道+vxwm surface stub 下调 braille_display_bridge_probe() 断言） | 增补 |

### 批 E74 · 字幕与实时转写覆盖层（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1461 | [E74] 字幕与实时转写覆盖层 · 功能主路径（内核 IPC 会话建链） | `caption_live_overlay/caption_live_overlay_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1461-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1462 | [E74] 字幕与实时转写覆盖层 · 开放格式配置（原子写+可迁移导出） | `caption_live_overlay/caption_live_overlay_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1462-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1463 | [E74] 字幕与实时转写覆盖层 · 设置三态 UI（占位/校验/纠错提示） | `caption_live_overlay/caption_live_overlay_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1463-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1464 | [E74] 字幕与实时转写覆盖层 · 浮层完整出路清单（点外/Esc/再点/失焦） | `caption_live_overlay/caption_live_overlay_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1464-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1465 | [E74] 字幕与实时转写覆盖层 · 交互状态机全覆盖（连点/打断/拖半/长按） | `caption_live_overlay/caption_live_overlay_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1465-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1466 | [E74] 字幕与实时转写覆盖层 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `caption_live_overlay/caption_live_overlay_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1466-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1467 | [E74] 字幕与实时转写覆盖层 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `caption_live_overlay/caption_live_overlay_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1467-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1468 | [E74] 字幕与实时转写覆盖层 · IME 组合期安全（不触发快捷键不误提交） | `caption_live_overlay/caption_live_overlay_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1468-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1469 | [E74] 字幕与实时转写覆盖层 · undo/redo 链（粒度合理可回溯） | `caption_live_overlay/caption_live_overlay_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1469-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1470 | [E74] 字幕与实时转写覆盖层 · UIA 双侧投影（Provider 语义+Client 遍历） | `caption_live_overlay/caption_live_overlay_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1470-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1471 | [E74] 字幕与实时转写覆盖层 · 文本缩放与 DPI 复检（超大字号不截断） | `caption_live_overlay/caption_live_overlay_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1471-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1472 | [E74] 字幕与实时转写覆盖层 · 高对比/色弱/灰度三态复检 | `caption_live_overlay/caption_live_overlay_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1472-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1473 | [E74] 字幕与实时转写覆盖层 · 动画节奏与打断（对称缓动/打断续接） | `caption_live_overlay/caption_live_overlay_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1473-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1474 | [E74] 字幕与实时转写覆盖层 · 错误三要素+详情折叠（零裸异常码） | `caption_live_overlay/caption_live_overlay_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1474-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1475 | [E74] 字幕与实时转写覆盖层 · 体验日志与总日志中心接入 | `caption_live_overlay/caption_live_overlay_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1475-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1476 | [E74] 字幕与实时转写覆盖层 · 隐私红线复检（不记输入内容/异步批量写入） | `caption_live_overlay/caption_live_overlay_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1476-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1477 | [E74] 字幕与实时转写覆盖层 · 性能账（P95/内存上限/帧率实测入账） | `caption_live_overlay/caption_live_overlay_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1477-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1478 | [E74] 字幕与实时转写覆盖层 · 崩溃恢复与看门狗（心跳/自动重建） | `caption_live_overlay/caption_live_overlay_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1478-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1479 | [E74] 字幕与实时转写覆盖层 · 开放接口版本化+签名校验+卸载清洁 | `caption_live_overlay/caption_live_overlay_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1479-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |
| UNX-F4-E1480 | [E74] 字幕与实时转写覆盖层 · 收官自检（checks.rs 全域 PASS+文档三件套） | `caption_live_overlay/caption_live_overlay_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1480-J1：内核事件通道+vxwm surface stub 下调 caption_live_overlay_probe() 断言） | 增补 |

### 批 E75 · F4 卷五治理收官与五卷联轧总账（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-F4-E1481 | [E75] F4 卷五治理收官与五卷联轧总账 · 功能主路径（内核 IPC 会话建链） | `f4_vol5_governance/f4_vol5_governance_00.rs` | Varix 内核锚定：经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册（UNX-F4-E1481-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1482 | [E75] F4 卷五治理收官与五卷联轧总账 · 开放格式配置（原子写+可迁移导出） | `f4_vol5_governance/f4_vol5_governance_01.rs` | Varix 内核锚定：配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致（UNX-F4-E1482-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1483 | [E75] F4 卷五治理收官与五卷联轧总账 · 设置三态 UI（占位/校验/纠错提示） | `f4_vol5_governance/f4_vol5_governance_02.rs` | Varix 内核锚定：错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落（UNX-F4-E1483-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1484 | [E75] F4 卷五治理收官与五卷联轧总账 · 浮层完整出路清单（点外/Esc/再点/失焦） | `f4_vol5_governance/f4_vol5_governance_03.rs` | Varix 内核锚定：四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零（UNX-F4-E1484-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1485 | [E75] F4 卷五治理收官与五卷联轧总账 · 交互状态机全覆盖（连点/打断/拖半/长按） | `f4_vol5_governance/f4_vol5_governance_04.rs` | Varix 内核锚定：状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态（UNX-F4-E1485-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1486 | [E75] F4 卷五治理收官与五卷联轧总账 · 键盘对等可达（Tab 序/焦点归还/快捷键词典） | `f4_vol5_governance/f4_vol5_governance_05.rs` | Varix 内核锚定：浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS（UNX-F4-E1486-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1487 | [E75] F4 卷五治理收官与五卷联轧总账 · 悬停/按压/拖拽微观手感（100ms 反馈红线） | `f4_vol5_governance/f4_vol5_governance_06.rs` | Varix 内核锚定：全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms（UNX-F4-E1487-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1488 | [E75] F4 卷五治理收官与五卷联轧总账 · IME 组合期安全（不触发快捷键不误提交） | `f4_vol5_governance/f4_vol5_governance_07.rs` | Varix 内核锚定：composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发（UNX-F4-E1488-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1489 | [E75] F4 卷五治理收官与五卷联轧总账 · undo/redo 链（粒度合理可回溯） | `f4_vol5_governance/f4_vol5_governance_08.rs` | Varix 内核锚定：用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致（UNX-F4-E1489-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1490 | [E75] F4 卷五治理收官与五卷联轧总账 · UIA 双侧投影（Provider 语义+Client 遍历） | `f4_vol5_governance/f4_vol5_governance_09.rs` | Varix 内核锚定：控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差（UNX-F4-E1490-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1491 | [E75] F4 卷五治理收官与五卷联轧总账 · 文本缩放与 DPI 复检（超大字号不截断） | `f4_vol5_governance/f4_vol5_governance_10.rs` | Varix 内核锚定：200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断（UNX-F4-E1491-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1492 | [E75] F4 卷五治理收官与五卷联轧总账 · 高对比/色弱/灰度三态复检 | `f4_vol5_governance/f4_vol5_governance_11.rs` | Varix 内核锚定：非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS（UNX-F4-E1492-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1493 | [E75] F4 卷五治理收官与五卷联轧总账 · 动画节奏与打断（对称缓动/打断续接） | `f4_vol5_governance/f4_vol5_governance_12.rs` | Varix 内核锚定：动画打断从中断点续接零跳变；判据：注入打断后帧差连续（UNX-F4-E1493-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1494 | [E75] F4 卷五治理收官与五卷联轧总账 · 错误三要素+详情折叠（零裸异常码） | `f4_vol5_governance/f4_vol5_governance_13.rs` | Varix 内核锚定：技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出（UNX-F4-E1494-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1495 | [E75] F4 卷五治理收官与五卷联轧总账 · 体验日志与总日志中心接入 | `f4_vol5_governance/f4_vol5_governance_14.rs` | Varix 内核锚定：交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单（UNX-F4-E1495-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1496 | [E75] F4 卷五治理收官与五卷联轧总账 · 隐私红线复检（不记输入内容/异步批量写入） | `f4_vol5_governance/f4_vol5_governance_15.rs` | Varix 内核锚定：日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms（UNX-F4-E1496-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1497 | [E75] F4 卷五治理收官与五卷联轧总账 · 性能账（P95/内存上限/帧率实测入账） | `f4_vol5_governance/f4_vol5_governance_16.rs` | Varix 内核锚定：长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界（UNX-F4-E1497-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1498 | [E75] F4 卷五治理收官与五卷联轧总账 · 崩溃恢复与看门狗（心跳/自动重建） | `f4_vol5_governance/f4_vol5_governance_17.rs` | Varix 内核锚定：功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照（UNX-F4-E1498-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1499 | [E75] F4 卷五治理收官与五卷联轧总账 · 开放接口版本化+签名校验+卸载清洁 | `f4_vol5_governance/f4_vol5_governance_18.rs` | Varix 内核锚定：接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中（UNX-F4-E1499-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |
| UNX-F4-E1500 | [E75] F4 卷五治理收官与五卷联轧总账 · 收官自检（checks.rs 全域 PASS+文档三件套） | `f4_vol5_governance/f4_vol5_governance_19.rs` | Varix 内核锚定：自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS（UNX-F4-E1500-J1：内核事件通道+vxwm surface stub 下调 f4_vol5_governance_probe() 断言） | 增补 |

> 卷五小计：300 条。五卷联轧：UNX-F4-E001–E1500 共 1500 项增补，域账 F22401–F23200 零触碰。
