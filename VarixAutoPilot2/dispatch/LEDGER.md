# 派工台账（LEDGER）

> 调度塔唯一事实来源。每次派工/收工/补位各记一行，格式固定，异常零静默。

| 时间 | 事件 | WP | 工人 | 结果/原因 |
|---|---|---|---|---|
| 10-06 19:13 | 产线上线 | - | 领单塔 | 积分切号+卡死归档：TreeCode API（/api/quota 35号32599.9可切29 / /api/accounts/login）接成 account_pool；watch 加 dead-min 10min 卡死归档重建（release_worker 退单+同编号重建）、连败≥3 切号、无号停机；技能扩到 10 个 exact 匹配 |
| 10-06 19:13 | 缺陷🟡 | - | 调度塔 | 「新建任务」不丢弃上轮草稿——残留文件节点让 / trigger 失效（自检 0/10 假绿）；修：run() 技能挂载前强制清空 + add_skill 面板未开自愈清空重试；selftest 复跑 10/10 exact 全绿 |
| 10-06 18:37 | 引导升级 | - | 调度塔 | Variable 指路「/」面板=技能正解（probe12~14 实测）：按显示名过滤+scrollIntoView+点选→真 mention 节点；bootstrap.mount_skills 上线，内联 @skill: 降为兜底；selftest 5/5 技能全 UI 挂载零兜底 |
| 10-06 18:01 | 引导上线 | - | 调度塔 | 三件套实装：新会话自动选工作空间 -Un-Real-0d23d9ux-Engine-main（chip 验证）+ 拖拽挂 4 文件（resource_link）+ 技能 @skill: 内联首条提示词；bootstrap_selftest 全绿 |
| 10-06 18:01 | 缺陷🟢 | - | 调度塔 | 宿主行为记录：隐藏 input[type=file] 条件挂载（空白视图 null）；DOM 域 nodeId 跨调用不稳；「+」技能子面板点击/悬停/键盘全竞态——三条路线弃用，探针归档 _attic/2026-10-06-bootstrap/ |
| 10-06 16:52 | 派工(新会话) | WP-00001 | W02 | E1 发送键进停止态（AI 生成中=已受理） |
| 10-06 16:52 | 派工(新会话) | WP-00001 | W01 | E1 发送键进停止态（AI 生成中=已受理） |
| 10-06 16:4x | **领单产线塔上线（claim_tower.py · VTaskBoard 版）** | - | 调度塔 | Variable 令建 18 路领单产线：工人 AI 自去任务板（8767）领单，塔只管建会话/巡检/续跑。与 dispatch_tower（PLAN 派工）并行两套模式。新文件：`tools/claim_tower.py` + `dispatch/CLAIM_WORKER_PROMPT.md`（协议 MD，正文全文+路径双通道发送）+ `领单发车.bat`。要点：① 空闲判定继承 D15 修订（唯一忙票=可见停止按钮）② 会话丢失按首条提示词 `VARIX-Wxx` 标记侧栏重链（实测标题取首条消息开头）③ BUSY 超 25min 自动补发状态对账（自愈假死）④ 发送失败 45s 冷却（限流防轰炸）⑤ 收口=板上待领/已领/阻塞全 0 且塔内无 BUSY，否则永续运行。dry-run+probe 双验证 PASS；任务板实测 57854 待领 |
| 10-06 15:2x | **D13 修复（打断发送断线）+ D14 修复（切会话裸 click）** | - | 调度塔 | 用户实测「打断发送失败 api.forceSend is not a function」「队列等空闲」。日志实证：worker 活着、217 次重试、三判据判定**正确**（页面真忙——队列目标会话正是与助手对话的会话，助手在生成长回复）。真缺陷 2 处：D13 `api.forceSend` 只加了 SNAKE 映射、漏了 IS_TAURI 装配块（15 个 api 方法审计仅缺此 1 个）；D14 Rust `switch_conversation` 用裸 `e.click()`（D3/D9 同源，React 不响应）——18 工人场景切不动会话会把消息填进错误会话。均修复：装配一行 + `_card_` MouseEvent 序列与选中态确认轮询 |
| 10-06 15:5x | **D15 修复（空闲判定三重假阳性 → worker 恒忙不发）** | - | 调度塔 | 用户再报「还是卡死没发送」。日志 15:48~15:52 连续 **569 次「0/3 全忙」** vs 同屏顶部徽章「空闲」自相矛盾（用户截图实证），且上轮助手回复 15:47 前已结束——空闲态被系统性误判。三处修复（见 D15）+ verdict 附带 dbg 原始明细进日志，重新打包上线 |
| 10-06 15:08 | **重新打包 + D12 时区修复** | - | 调度塔 | 打包走查抓到「源预览 {{时间}} 显示 06:43（UTC），实际 14:43」。单元测试实测 `local_offset_seconds` 返回 0（系统真实 28800）：旧实现假设 SystemTimeToFileTime 做时区换算，实际 SYSTEMTIME/FILETIME 均为 UTC 语义、往返墙钟不变、a−b 恒 0。正解 `GetTimeZoneInformation` bias（含夏令时）。修复 + 2 个永久回归测试（offset 系统对账 / now_hms 本地日期）全 PASS。新 exe（md5 fa1d1fa2）已替换并实启动验证：源预览显示 `15:08:42` 本地时间 ✓ |
| 10-06 14:2x | **根因修复 D11 + 端到端实弹 PASS** | - | 调度塔 | 14:12 发车 5 连败的根因不是限流——execCommand 填充只写 DOM 不进 Slate 状态，React 认为编辑器为空 → 发送键黑、点击被静默吞掉（用户截图实证）。修复：`fill_cdp`（CDP Input.insertText，trusted 输入管线）+ `send_text` 证据重写（E1 停止态/E2 tag 入流/E3 charsReal 归零）+ `clear_editor_cdp`（trusted 按键清理）。**端到端实弹 PASS**：消息入流 + AI 生成回复「好」+ 新会话 `ee4447bf` 创建。限流已于 03:17 重置解除 |
| 10-06 14:13 | 发车失败 | WP-00001 | W04 | 首条发送失败：15s 内三重证据均未成立（D11 填充失效所致，非限流） |
| 10-06 14:12 | 发车失败 | WP-00001 | W03 | 发送成功但没捕捉到新会话 id |
| 10-06 14:12 | 发车失败 | WP-00001 | W02 | 发送成功但没捕捉到新会话 id |
| 10-06 14:12 | 发车失败 | WP-00001 | W01 | 发送成功但没捕捉到新会话 id |
| 10-06 05:47 | 验收结论 | - | W99 | PARTIAL(1/2) 1/2 |
| 10-06 05:36 | 验收结论 | - | W99 | PARTIAL(1/2) 1/2 |
| 10-06 05:28 | 验收结论 | - | W99 | **PARTIAL(1/2)** 1/2 |
| 2026-10-06 03:3x | 系统初始化 | - | 调度塔 | 架构就绪，等待 PLAN 切块与发车口令 |
| 2026-10-06 05:00 | 工具链就位 | - | 调度塔 | dispatch_tower.py + check_tower_dom.py + 产线发车.bat；DOM 体检 PASS（13 会话项/新建入口/输入框/发送键全部在位） |
| 10-06 05:14 | 上一轮验收中断 | - | 调度塔 | 助手回合 2s 即被 `429 使用量超出频率限制（2026-10-07 03:17:03 UTC+8 重置）` 打断，上下文丢失 |
| 10-06 05:47 | 第三轮实弹 | - | W99 | Variable 令再试：复验 D8/D9 **双双生效**（新建 19s 到位、失败路径自动还原视图）；三重证据 15s 不成立判 FAIL 无误报；事后抓提示仍 `(无)`；配额未恢复（13→13）。验收脚本失败路径已完全自愈，可无人值守跑 |
| 10-06 05:42 | 第二轮实弹 | - | W99 | 换 `GLM-5.3-Flash` 再打一次，仍 FAIL：填 370 字、发送键 ok、编辑器清空，但 30s 内 `active` 恒 `''`、`hasList` 恒 false、发送键从未进「停止」→ **消息连受理都没被受理**；`W99.state` 180s 未出现 |
| 10-06 05:43 | 静默失败取证 | - | 调度塔 | 抓页面 toast/banner/alert：`(无可见提示元素)` —— 被限流时客户端静默吞掉发送，用户侧零反馈（记 D10） |
| 10-06 05:44 | 缺陷修复 | - | 调度塔 | 第二轮又暴露 2 处：D8 `JS_CLICK_NEW` 裸 `b.click()` 不稳定 / D9 空白页切回首次点击被吞。均已修并实测（含双向切换验证），视图已还原到用户会话 |
| 10-06 05:36 | 第二轮实机 | - | W99 | S1 被**防污染安全闸**拦下：点「新建任务」后视图仍在原会话 → 主动放弃填充。安全设计生效（没有污染用户会话） |
| 10-06 05:40 | 第二轮实机 | - | W99 | 诊断确认：`JS_CLICK_NEW` 用原生 `b.click()` 时灵时不灵（与 D3 同源，React 不吃裸 click）；改用完整 MouseEvent 序列后 0.7s 即切到空白会话。另发现模型已自动切到 `GLM-5.3-Flash`（原为被限流的 `Hy4 preview`），据此再发一轮实弹 |
| 10-06 05:3x | 缺陷修复 | - | 调度塔 | 验收暴露 5 处判定失效 + 1 处 UX 缺陷，全部修复并实测复核：D1 发送双重证据全死 / D2 活跃会话判据失效 / D3 switch_conv 点击不生效 / D4 new_conversation 自锁 / D5 probe_state.chars 含占位语 / D6 失败不还原视图 |
| 10-06 05:3x | 结论推翻 | - | 调度塔 | 旧结论「忙闲恒忙、发送必须回合间进行」**错误**。实测：本会话生成中，新建的空白会话发送键为「发送」= 空闲 → 忙闲是**按会话**的。18 路并行机制上成立 |
| 10-06 05:3x | 发车判据 | - | 调度塔 | **不可发车**：① 账号 429 限流至 2026-10-07 03:17:03 UTC+8 ② PLAN 仍为 0 个任务包（待 50000 计划原文/规格 MD） |

## 缺陷账本

> 工人报告中提取，分级：🔴 即时修 / 🟡 本包收口前修 / 🟢 攒批。攒满 80~100 条整理工单派修理 AI。

| 编号 | 现象 | 位置 | 级别 | 建议修法 | 状态 |
|---|---|---|---|---|---|
| D1 | 发送证据两条路都是死路（`[data-message-author-role]` 在本版 DOM 恒 0 个；`chars==0` 因占位语恒 25 字）→ 真发送也报失败（假阴性） | `tools/dispatch_tower.py::send_text` | 🟡 | 改三重证据：tag 入流 / 会话数+1 / 真实字数归零且非 stop | ✅ 已修+实测 |
| D2 | 活跃会话识别失效（侧栏项自身无选中标记，只有恒定 `conversation-item`） | `tools/accept_tower.py::JS_CURRENT` | 🟡 | 选中态在后代的 `_selected_` CSS-module 类上；新增 `active_conv()` | ✅ 已修+实测 |
| D3 | `switch_conv` 用 `el.click()` 不生效，切不回原会话，把用户留在空白页 | `tools/dispatch_tower.py::switch_conv` | 🟡 | 对内层 `_card_` 派发完整 MouseEvent 序列；返回前用 `active_conv()` 确认真的切过去了 | ✅ 已修+实测（双向） |
| D4 | `new_conversation` 先 `wait_idle()` 后点新建 → 全局忙时自锁 900s | `tools/dispatch_tower.py::new_conversation` | 🟡 | 先点新建再判目标会话空闲 + 安全闸（未离开原会话则放弃填充） | ✅ 已修 |
| D5 | `probe_state().chars` 把占位语当内容（空框 = 25 字） | `tools/send_selftest.py::probe_state` | 🟢 | 保留 `chars` 兼容旧调用，新增 `charsReal` | ✅ 已修+实测 |
| D6 | 验收脚本失败时不还原视图 | `tools/accept_tower.py::main` | 🟡 | 失败路径也 `switch_conv(anchor)` | ✅ 已修 |
| D7 | 账号 429 限流，新会话后端不受理、不派 AI | 环境（账号） | 🔴 | 切其他模型，或等 2026-10-07 03:17:03 UTC+8 重置 | ✅ 已解除（03:17 重置；14:2x 实弹 AI 正常回复） |
| D8 | `JS_CLICK_NEW` 用原生 `b.click()` 时灵时不灵 → 点「新建任务」后视图没切走 | `tools/dispatch_tower.py::JS_CLICK_NEW` | 🟡 | 与 D3 同源：改完整 MouseEvent 序列；并轮询 8s 确认真的进入空白会话（`active=''` 且 `.cr-message-list` 消失） | ✅ 已修+实测 |
| D9 | 从空白新会话切回原会话时首次点击偶发被吞 | `tools/dispatch_tower.py::switch_conv` | 🟡 | 点一轮不成就再点一轮（外层 `for attempt in range(2)`） | ✅ 已修+实测 |
| D10 | 被限流时客户端**静默吞掉发送**：输入框清空、无 toast/banner/alert，用户点了发送后什么也没发生 | 宿主产品行为（WorkBuddy 5.6.2） | 🔴 | 塔侧够不到根因；靠三重证据全不成立间接判定"后端未受理"并在日志显式提示限流 | ⚠️ 已记录（非本仓库可修） |
| D11 | **填充不进 Slate 状态**：`execCommand('insertText')` 写的文字只存在于 DOM，Slate 内部 model 不认 → React 判定编辑器为空 → 发送键呈黑色视觉、点击被空内容逻辑吞掉。**「编辑器有字但发不出」的唯一根因**（手动粘贴不受影响，自动化填充全中招；14:12 发车 5 连败同因） | `tools/send_selftest.py::fill_js`（已删） | 🔴 | 改 `fill_cdp`：CDP `Input.insertText`（模拟真实 IME，trusted 事件走完整输入管线）+ `charsReal` 验证；`restore` 升级为 `clear_editor_cdp`（trusted Ctrl+A+Backspace）。端到端实弹 PASS | ✅ 已修+实弹验证 |
| D12 | **提示词时间戳是 UTC**：源预览/发送的 `{{时间}}` 比真实时间差 8 小时。旧 `local_offset_seconds` 用「GetLocalTime 墙钟 − SystemTimeToFileTime 往返墙钟」算偏移，但 SYSTEMTIME/FILETIME 都是 UTC 语义、SystemTimeToFileTime 不做时区换算 → 往返恒等、a−b **恒 0** | `src-tauri/src/template.rs::local_offset_seconds` | 🔴 | 改 `GetTimeZoneInformation` 取 bias（`本地−UTC = −bias`，夏令时叠加 daylight_bias）；补 2 个永久回归测试（与系统 TZI 对账 / now_hms 本地日期）。测试驱动：先实测复现 off=0 → 修复 → 2/2 PASS | ✅ 已修+测试验证+实机截图 |
| D13 | **「打断发送」报 `api.forceSend is not a function`**：SNAKE 映射表加了 `forceSend: 'force_send'`，但 IS_TAURI 装配块漏了 `api.forceSend = ...` ⇒ 方法 undefined。后端 force_send 完好却永远收不到调用 | `src/app.js`（IS_TAURI 块） | 🔴 | 补装配一行 `api.forceSend = (text) => getCore().invoke('forceSend', { text })`；并全量审计 15 个 api 方法确认无其他漏配 | ✅ 已修+审计 |
| D14 | **Rust `switch_conversation` 用裸 `e.click()`**（D3/D9 同源）：React 不响应脚本派发的裸 click ⇒ 队列项指向**别的会话**时切不动视图，fill_prompt 会把内容填进当前打开的错误会话。现在队列目标=当前会话所以未触发，18 工人场景必炸 | `src-tauri/src/engine.rs::switch_conversation` | 🟡 | 改 `_card_` 内层 + 完整 MouseEvent 序列 + 选中态（后代 `_selected_` 类）确认轮询 3s，未确认即放弃（防填错会话） | ✅ 已修 |
| D15 | **空闲判定三重假阳性 → worker 恒忙、队列永不发送**（用户连报「卡死没发送」，日志 569 次 0/3 全忙 vs 徽章「空闲」矛盾）：① `idle_verdict.by_btn` 把「发送键不存在」判忙——空编辑器时按钮条件渲染整个消失（fill_methods 实验实锤），而「空闲+空编辑器」恰是最标准空闲态；② `by_stop_btn` 选择器过宽（`[title*="停止"]` 无标签限定）且不过滤不可见残留元素；③ `busy_state` 单票否决含 `anims>0`——消息区存在未知常驻大块 loading/typing 类元素（569 次「动画忙」实证），worker 判闲后会在发送入口自锁。连带 force_send 在页面空闲时误点「停止」 | `src-tauri/src/engine.rs::idle_verdict` / `busy_state` | 🔴 | ① by_btn：按钮不存在 ⇒ 空闲（生成中按钮必以 `--stop` 存在，安全）② stopEls 限定 button + rect 可见性过滤 ③ anims 退出 busy_state 否决、降级日志字段（发送键 `--stop` 单票已足够）④ verdict 返回 `dbg` 原始明细（btn/cls/stop 元素列表/anim 元素列表）进日志——再出恒忙一眼定位捣乱元素 | ✅ 已修+上线 |

## 阻塞台账

> 卡住的永远是任务，不是人。每条阻塞写清：卡在哪/需要什么/谁能解。

| WP | 卡在哪 | 需要什么 | 谁能解 | 状态 |
|---|---|---|---|---|
| ~~发车~~ | ~~账号 429 限流~~ | ~~切换其他模型，或等到 2026-10-07 03:17:03 UTC+8~~ | - | ✅ 已解除（03:17 重置 + D11 修复，14:2x 实弹 PASS） |
| 切块 | `dispatch/PLAN.md` 仍是 0 个任务包（只有示例条目） | 50000 条计划原文，或指定规格 MD 由调度塔生成；产物根目录 | Variable | ⏳ 阻塞中（示例 WP-00001 可用于试发闭环） |
