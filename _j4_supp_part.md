
---

## 增补卷 · AI-29 · UNX-J4-E001–E300（沙箱与隔离域 300 项新功能 · S01–S15 批 · 15 批 × 20 条）

> 收录纪律：独立增补编号，不占 J4 域账 F38401–F39200（40 批守恒不动）；状态列统一「增补」；
> 全部围绕 Varix 内核锚定（checks.rs 自检面 / lxprocfs.rs 自省面 / 内核事件·会话·IPC 通道 / vxwm surface·popup·特效通道）；
> 与 F4 六卷及他卷判据颗粒零重复；保管母本 docs/unxreal/supp/UNX-J4-SUPP-E001-E300.md（R-PROC-002 生成器重生成后须回播）。

### 批 S01 · 沙箱会话生命周期管理（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E001 | [S01] 沙箱会话生命周期管理 · 功能主路径（内核会话建链+能力位申请） | `sandbox_session_lifecycle/sandbox_session_lifecycle_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E001-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E002 | [S01] 沙箱会话生命周期管理 · 开放格式清单（原子写/导出迁移） | `sandbox_session_lifecycle/sandbox_session_lifecycle_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E002-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E003 | [S01] 沙箱会话生命周期管理 · 默认拒绝原则（未声明能力一律拒） | `sandbox_session_lifecycle/sandbox_session_lifecycle_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E003-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E004 | [S01] 沙箱会话生命周期管理 · 越界请求三要素呈现（零裸异常码） | `sandbox_session_lifecycle/sandbox_session_lifecycle_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E004-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E005 | [S01] 沙箱会话生命周期管理 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `sandbox_session_lifecycle/sandbox_session_lifecycle_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E005-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E006 | [S01] 沙箱会话生命周期管理 · 体验日志埋点（沙箱操作四元组） | `sandbox_session_lifecycle/sandbox_session_lifecycle_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E006-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E007 | [S01] 沙箱会话生命周期管理 · 拉起/关闭完整出路（孤儿会话回收） | `sandbox_session_lifecycle/sandbox_session_lifecycle_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E007-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E008 | [S01] 沙箱会话生命周期管理 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `sandbox_session_lifecycle/sandbox_session_lifecycle_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E008-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E009 | [S01] 沙箱会话生命周期管理 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `sandbox_session_lifecycle/sandbox_session_lifecycle_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E009-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E010 | [S01] 沙箱会话生命周期管理 · 100ms 反馈红线（沙箱内交互可见反馈） | `sandbox_session_lifecycle/sandbox_session_lifecycle_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E010-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E011 | [S01] 沙箱会话生命周期管理 · 跨界审计（跨界部件清单式声明+越界告警） | `sandbox_session_lifecycle/sandbox_session_lifecycle_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E011-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E012 | [S01] 沙箱会话生命周期管理 · 配额超限优雅降级（不崩宿主不丢数据） | `sandbox_session_lifecycle/sandbox_session_lifecycle_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E012-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E013 | [S01] 沙箱会话生命周期管理 · 资源泄漏看门狗（句柄/内存/线程三账） | `sandbox_session_lifecycle/sandbox_session_lifecycle_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E013-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E014 | [S01] 沙箱会话生命周期管理 · UIA 双侧投影（沙箱内控件树语义可见） | `sandbox_session_lifecycle/sandbox_session_lifecycle_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E014-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E015 | [S01] 沙箱会话生命周期管理 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `sandbox_session_lifecycle/sandbox_session_lifecycle_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E015-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E016 | [S01] 沙箱会话生命周期管理 · DPI 与多屏矩阵复检 | `sandbox_session_lifecycle/sandbox_session_lifecycle_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E016-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E017 | [S01] 沙箱会话生命周期管理 · 性能账（沙箱开销 P95/内存上限入账） | `sandbox_session_lifecycle/sandbox_session_lifecycle_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E017-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E018 | [S01] 沙箱会话生命周期管理 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `sandbox_session_lifecycle/sandbox_session_lifecycle_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E018-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E019 | [S01] 沙箱会话生命周期管理 · 开放接口版本化+策略签名验证 | `sandbox_session_lifecycle/sandbox_session_lifecycle_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E019-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |
| UNX-J4-E020 | [S01] 沙箱会话生命周期管理 · 收官自检（checks.rs 全域+文档三件套一致性） | `sandbox_session_lifecycle/sandbox_session_lifecycle_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E020-J1：内核事件通道+沙箱 stub 下调 sandbox_session_lifecycle_probe() 断言） | 增补 |

### 批 S02 · 能力位掩码与最小权限引擎（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E021 | [S02] 能力位掩码与最小权限引擎 · 功能主路径（内核会话建链+能力位申请） | `capability_bitmask_engine/capability_bitmask_engine_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E021-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E022 | [S02] 能力位掩码与最小权限引擎 · 开放格式清单（原子写/导出迁移） | `capability_bitmask_engine/capability_bitmask_engine_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E022-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E023 | [S02] 能力位掩码与最小权限引擎 · 默认拒绝原则（未声明能力一律拒） | `capability_bitmask_engine/capability_bitmask_engine_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E023-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E024 | [S02] 能力位掩码与最小权限引擎 · 越界请求三要素呈现（零裸异常码） | `capability_bitmask_engine/capability_bitmask_engine_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E024-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E025 | [S02] 能力位掩码与最小权限引擎 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `capability_bitmask_engine/capability_bitmask_engine_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E025-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E026 | [S02] 能力位掩码与最小权限引擎 · 体验日志埋点（沙箱操作四元组） | `capability_bitmask_engine/capability_bitmask_engine_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E026-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E027 | [S02] 能力位掩码与最小权限引擎 · 拉起/关闭完整出路（孤儿会话回收） | `capability_bitmask_engine/capability_bitmask_engine_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E027-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E028 | [S02] 能力位掩码与最小权限引擎 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `capability_bitmask_engine/capability_bitmask_engine_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E028-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E029 | [S02] 能力位掩码与最小权限引擎 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `capability_bitmask_engine/capability_bitmask_engine_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E029-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E030 | [S02] 能力位掩码与最小权限引擎 · 100ms 反馈红线（沙箱内交互可见反馈） | `capability_bitmask_engine/capability_bitmask_engine_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E030-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E031 | [S02] 能力位掩码与最小权限引擎 · 跨界审计（跨界部件清单式声明+越界告警） | `capability_bitmask_engine/capability_bitmask_engine_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E031-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E032 | [S02] 能力位掩码与最小权限引擎 · 配额超限优雅降级（不崩宿主不丢数据） | `capability_bitmask_engine/capability_bitmask_engine_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E032-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E033 | [S02] 能力位掩码与最小权限引擎 · 资源泄漏看门狗（句柄/内存/线程三账） | `capability_bitmask_engine/capability_bitmask_engine_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E033-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E034 | [S02] 能力位掩码与最小权限引擎 · UIA 双侧投影（沙箱内控件树语义可见） | `capability_bitmask_engine/capability_bitmask_engine_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E034-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E035 | [S02] 能力位掩码与最小权限引擎 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `capability_bitmask_engine/capability_bitmask_engine_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E035-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E036 | [S02] 能力位掩码与最小权限引擎 · DPI 与多屏矩阵复检 | `capability_bitmask_engine/capability_bitmask_engine_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E036-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E037 | [S02] 能力位掩码与最小权限引擎 · 性能账（沙箱开销 P95/内存上限入账） | `capability_bitmask_engine/capability_bitmask_engine_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E037-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E038 | [S02] 能力位掩码与最小权限引擎 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `capability_bitmask_engine/capability_bitmask_engine_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E038-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E039 | [S02] 能力位掩码与最小权限引擎 · 开放接口版本化+策略签名验证 | `capability_bitmask_engine/capability_bitmask_engine_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E039-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |
| UNX-J4-E040 | [S02] 能力位掩码与最小权限引擎 · 收官自检（checks.rs 全域+文档三件套一致性） | `capability_bitmask_engine/capability_bitmask_engine_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E040-J1：内核事件通道+沙箱 stub 下调 capability_bitmask_engine_probe() 断言） | 增补 |

### 批 S03 · 文件系统虚拟化隔离层（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E041 | [S03] 文件系统虚拟化隔离层 · 功能主路径（内核会话建链+能力位申请） | `fs_virtualization_isolation/fs_virtualization_isolation_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E041-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E042 | [S03] 文件系统虚拟化隔离层 · 开放格式清单（原子写/导出迁移） | `fs_virtualization_isolation/fs_virtualization_isolation_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E042-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E043 | [S03] 文件系统虚拟化隔离层 · 默认拒绝原则（未声明能力一律拒） | `fs_virtualization_isolation/fs_virtualization_isolation_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E043-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E044 | [S03] 文件系统虚拟化隔离层 · 越界请求三要素呈现（零裸异常码） | `fs_virtualization_isolation/fs_virtualization_isolation_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E044-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E045 | [S03] 文件系统虚拟化隔离层 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `fs_virtualization_isolation/fs_virtualization_isolation_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E045-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E046 | [S03] 文件系统虚拟化隔离层 · 体验日志埋点（沙箱操作四元组） | `fs_virtualization_isolation/fs_virtualization_isolation_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E046-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E047 | [S03] 文件系统虚拟化隔离层 · 拉起/关闭完整出路（孤儿会话回收） | `fs_virtualization_isolation/fs_virtualization_isolation_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E047-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E048 | [S03] 文件系统虚拟化隔离层 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `fs_virtualization_isolation/fs_virtualization_isolation_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E048-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E049 | [S03] 文件系统虚拟化隔离层 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `fs_virtualization_isolation/fs_virtualization_isolation_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E049-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E050 | [S03] 文件系统虚拟化隔离层 · 100ms 反馈红线（沙箱内交互可见反馈） | `fs_virtualization_isolation/fs_virtualization_isolation_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E050-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E051 | [S03] 文件系统虚拟化隔离层 · 跨界审计（跨界部件清单式声明+越界告警） | `fs_virtualization_isolation/fs_virtualization_isolation_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E051-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E052 | [S03] 文件系统虚拟化隔离层 · 配额超限优雅降级（不崩宿主不丢数据） | `fs_virtualization_isolation/fs_virtualization_isolation_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E052-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E053 | [S03] 文件系统虚拟化隔离层 · 资源泄漏看门狗（句柄/内存/线程三账） | `fs_virtualization_isolation/fs_virtualization_isolation_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E053-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E054 | [S03] 文件系统虚拟化隔离层 · UIA 双侧投影（沙箱内控件树语义可见） | `fs_virtualization_isolation/fs_virtualization_isolation_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E054-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E055 | [S03] 文件系统虚拟化隔离层 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `fs_virtualization_isolation/fs_virtualization_isolation_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E055-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E056 | [S03] 文件系统虚拟化隔离层 · DPI 与多屏矩阵复检 | `fs_virtualization_isolation/fs_virtualization_isolation_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E056-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E057 | [S03] 文件系统虚拟化隔离层 · 性能账（沙箱开销 P95/内存上限入账） | `fs_virtualization_isolation/fs_virtualization_isolation_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E057-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E058 | [S03] 文件系统虚拟化隔离层 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `fs_virtualization_isolation/fs_virtualization_isolation_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E058-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E059 | [S03] 文件系统虚拟化隔离层 · 开放接口版本化+策略签名验证 | `fs_virtualization_isolation/fs_virtualization_isolation_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E059-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |
| UNX-J4-E060 | [S03] 文件系统虚拟化隔离层 · 收官自检（checks.rs 全域+文档三件套一致性） | `fs_virtualization_isolation/fs_virtualization_isolation_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E060-J1：内核事件通道+沙箱 stub 下调 fs_virtualization_isolation_probe() 断言） | 增补 |

### 批 S04 · 注册表虚拟化与写时复制（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E061 | [S04] 注册表虚拟化与写时复制 · 功能主路径（内核会话建链+能力位申请） | `registry_cow_virtualization/registry_cow_virtualization_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E061-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E062 | [S04] 注册表虚拟化与写时复制 · 开放格式清单（原子写/导出迁移） | `registry_cow_virtualization/registry_cow_virtualization_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E062-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E063 | [S04] 注册表虚拟化与写时复制 · 默认拒绝原则（未声明能力一律拒） | `registry_cow_virtualization/registry_cow_virtualization_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E063-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E064 | [S04] 注册表虚拟化与写时复制 · 越界请求三要素呈现（零裸异常码） | `registry_cow_virtualization/registry_cow_virtualization_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E064-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E065 | [S04] 注册表虚拟化与写时复制 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `registry_cow_virtualization/registry_cow_virtualization_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E065-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E066 | [S04] 注册表虚拟化与写时复制 · 体验日志埋点（沙箱操作四元组） | `registry_cow_virtualization/registry_cow_virtualization_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E066-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E067 | [S04] 注册表虚拟化与写时复制 · 拉起/关闭完整出路（孤儿会话回收） | `registry_cow_virtualization/registry_cow_virtualization_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E067-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E068 | [S04] 注册表虚拟化与写时复制 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `registry_cow_virtualization/registry_cow_virtualization_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E068-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E069 | [S04] 注册表虚拟化与写时复制 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `registry_cow_virtualization/registry_cow_virtualization_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E069-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E070 | [S04] 注册表虚拟化与写时复制 · 100ms 反馈红线（沙箱内交互可见反馈） | `registry_cow_virtualization/registry_cow_virtualization_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E070-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E071 | [S04] 注册表虚拟化与写时复制 · 跨界审计（跨界部件清单式声明+越界告警） | `registry_cow_virtualization/registry_cow_virtualization_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E071-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E072 | [S04] 注册表虚拟化与写时复制 · 配额超限优雅降级（不崩宿主不丢数据） | `registry_cow_virtualization/registry_cow_virtualization_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E072-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E073 | [S04] 注册表虚拟化与写时复制 · 资源泄漏看门狗（句柄/内存/线程三账） | `registry_cow_virtualization/registry_cow_virtualization_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E073-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E074 | [S04] 注册表虚拟化与写时复制 · UIA 双侧投影（沙箱内控件树语义可见） | `registry_cow_virtualization/registry_cow_virtualization_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E074-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E075 | [S04] 注册表虚拟化与写时复制 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `registry_cow_virtualization/registry_cow_virtualization_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E075-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E076 | [S04] 注册表虚拟化与写时复制 · DPI 与多屏矩阵复检 | `registry_cow_virtualization/registry_cow_virtualization_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E076-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E077 | [S04] 注册表虚拟化与写时复制 · 性能账（沙箱开销 P95/内存上限入账） | `registry_cow_virtualization/registry_cow_virtualization_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E077-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E078 | [S04] 注册表虚拟化与写时复制 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `registry_cow_virtualization/registry_cow_virtualization_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E078-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E079 | [S04] 注册表虚拟化与写时复制 · 开放接口版本化+策略签名验证 | `registry_cow_virtualization/registry_cow_virtualization_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E079-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |
| UNX-J4-E080 | [S04] 注册表虚拟化与写时复制 · 收官自检（checks.rs 全域+文档三件套一致性） | `registry_cow_virtualization/registry_cow_virtualization_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E080-J1：内核事件通道+沙箱 stub 下调 registry_cow_virtualization_probe() 断言） | 增补 |

### 批 S05 · 进程间通信通道隔离（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E081 | [S05] 进程间通信通道隔离 · 功能主路径（内核会话建链+能力位申请） | `ipc_channel_isolation/ipc_channel_isolation_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E081-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E082 | [S05] 进程间通信通道隔离 · 开放格式清单（原子写/导出迁移） | `ipc_channel_isolation/ipc_channel_isolation_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E082-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E083 | [S05] 进程间通信通道隔离 · 默认拒绝原则（未声明能力一律拒） | `ipc_channel_isolation/ipc_channel_isolation_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E083-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E084 | [S05] 进程间通信通道隔离 · 越界请求三要素呈现（零裸异常码） | `ipc_channel_isolation/ipc_channel_isolation_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E084-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E085 | [S05] 进程间通信通道隔离 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `ipc_channel_isolation/ipc_channel_isolation_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E085-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E086 | [S05] 进程间通信通道隔离 · 体验日志埋点（沙箱操作四元组） | `ipc_channel_isolation/ipc_channel_isolation_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E086-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E087 | [S05] 进程间通信通道隔离 · 拉起/关闭完整出路（孤儿会话回收） | `ipc_channel_isolation/ipc_channel_isolation_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E087-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E088 | [S05] 进程间通信通道隔离 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `ipc_channel_isolation/ipc_channel_isolation_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E088-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E089 | [S05] 进程间通信通道隔离 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `ipc_channel_isolation/ipc_channel_isolation_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E089-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E090 | [S05] 进程间通信通道隔离 · 100ms 反馈红线（沙箱内交互可见反馈） | `ipc_channel_isolation/ipc_channel_isolation_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E090-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E091 | [S05] 进程间通信通道隔离 · 跨界审计（跨界部件清单式声明+越界告警） | `ipc_channel_isolation/ipc_channel_isolation_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E091-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E092 | [S05] 进程间通信通道隔离 · 配额超限优雅降级（不崩宿主不丢数据） | `ipc_channel_isolation/ipc_channel_isolation_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E092-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E093 | [S05] 进程间通信通道隔离 · 资源泄漏看门狗（句柄/内存/线程三账） | `ipc_channel_isolation/ipc_channel_isolation_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E093-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E094 | [S05] 进程间通信通道隔离 · UIA 双侧投影（沙箱内控件树语义可见） | `ipc_channel_isolation/ipc_channel_isolation_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E094-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E095 | [S05] 进程间通信通道隔离 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `ipc_channel_isolation/ipc_channel_isolation_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E095-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E096 | [S05] 进程间通信通道隔离 · DPI 与多屏矩阵复检 | `ipc_channel_isolation/ipc_channel_isolation_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E096-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E097 | [S05] 进程间通信通道隔离 · 性能账（沙箱开销 P95/内存上限入账） | `ipc_channel_isolation/ipc_channel_isolation_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E097-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E098 | [S05] 进程间通信通道隔离 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `ipc_channel_isolation/ipc_channel_isolation_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E098-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E099 | [S05] 进程间通信通道隔离 · 开放接口版本化+策略签名验证 | `ipc_channel_isolation/ipc_channel_isolation_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E099-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |
| UNX-J4-E100 | [S05] 进程间通信通道隔离 · 收官自检（checks.rs 全域+文档三件套一致性） | `ipc_channel_isolation/ipc_channel_isolation_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E100-J1：内核事件通道+沙箱 stub 下调 ipc_channel_isolation_probe() 断言） | 增补 |

### 批 S06 · 窗口与剪贴板跨界管控（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E101 | [S06] 窗口与剪贴板跨界管控 · 功能主路径（内核会话建链+能力位申请） | `window_clipboard_boundary/window_clipboard_boundary_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E101-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E102 | [S06] 窗口与剪贴板跨界管控 · 开放格式清单（原子写/导出迁移） | `window_clipboard_boundary/window_clipboard_boundary_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E102-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E103 | [S06] 窗口与剪贴板跨界管控 · 默认拒绝原则（未声明能力一律拒） | `window_clipboard_boundary/window_clipboard_boundary_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E103-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E104 | [S06] 窗口与剪贴板跨界管控 · 越界请求三要素呈现（零裸异常码） | `window_clipboard_boundary/window_clipboard_boundary_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E104-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E105 | [S06] 窗口与剪贴板跨界管控 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `window_clipboard_boundary/window_clipboard_boundary_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E105-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E106 | [S06] 窗口与剪贴板跨界管控 · 体验日志埋点（沙箱操作四元组） | `window_clipboard_boundary/window_clipboard_boundary_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E106-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E107 | [S06] 窗口与剪贴板跨界管控 · 拉起/关闭完整出路（孤儿会话回收） | `window_clipboard_boundary/window_clipboard_boundary_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E107-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E108 | [S06] 窗口与剪贴板跨界管控 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `window_clipboard_boundary/window_clipboard_boundary_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E108-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E109 | [S06] 窗口与剪贴板跨界管控 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `window_clipboard_boundary/window_clipboard_boundary_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E109-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E110 | [S06] 窗口与剪贴板跨界管控 · 100ms 反馈红线（沙箱内交互可见反馈） | `window_clipboard_boundary/window_clipboard_boundary_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E110-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E111 | [S06] 窗口与剪贴板跨界管控 · 跨界审计（跨界部件清单式声明+越界告警） | `window_clipboard_boundary/window_clipboard_boundary_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E111-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E112 | [S06] 窗口与剪贴板跨界管控 · 配额超限优雅降级（不崩宿主不丢数据） | `window_clipboard_boundary/window_clipboard_boundary_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E112-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E113 | [S06] 窗口与剪贴板跨界管控 · 资源泄漏看门狗（句柄/内存/线程三账） | `window_clipboard_boundary/window_clipboard_boundary_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E113-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E114 | [S06] 窗口与剪贴板跨界管控 · UIA 双侧投影（沙箱内控件树语义可见） | `window_clipboard_boundary/window_clipboard_boundary_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E114-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E115 | [S06] 窗口与剪贴板跨界管控 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `window_clipboard_boundary/window_clipboard_boundary_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E115-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E116 | [S06] 窗口与剪贴板跨界管控 · DPI 与多屏矩阵复检 | `window_clipboard_boundary/window_clipboard_boundary_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E116-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E117 | [S06] 窗口与剪贴板跨界管控 · 性能账（沙箱开销 P95/内存上限入账） | `window_clipboard_boundary/window_clipboard_boundary_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E117-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E118 | [S06] 窗口与剪贴板跨界管控 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `window_clipboard_boundary/window_clipboard_boundary_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E118-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E119 | [S06] 窗口与剪贴板跨界管控 · 开放接口版本化+策略签名验证 | `window_clipboard_boundary/window_clipboard_boundary_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E119-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |
| UNX-J4-E120 | [S06] 窗口与剪贴板跨界管控 · 收官自检（checks.rs 全域+文档三件套一致性） | `window_clipboard_boundary/window_clipboard_boundary_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E120-J1：内核事件通道+沙箱 stub 下调 window_clipboard_boundary_probe() 断言） | 增补 |

### 批 S07 · 网络访问白名单与代理审计（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E121 | [S07] 网络访问白名单与代理审计 · 功能主路径（内核会话建链+能力位申请） | `network_whitelist_audit/network_whitelist_audit_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E121-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E122 | [S07] 网络访问白名单与代理审计 · 开放格式清单（原子写/导出迁移） | `network_whitelist_audit/network_whitelist_audit_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E122-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E123 | [S07] 网络访问白名单与代理审计 · 默认拒绝原则（未声明能力一律拒） | `network_whitelist_audit/network_whitelist_audit_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E123-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E124 | [S07] 网络访问白名单与代理审计 · 越界请求三要素呈现（零裸异常码） | `network_whitelist_audit/network_whitelist_audit_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E124-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E125 | [S07] 网络访问白名单与代理审计 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `network_whitelist_audit/network_whitelist_audit_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E125-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E126 | [S07] 网络访问白名单与代理审计 · 体验日志埋点（沙箱操作四元组） | `network_whitelist_audit/network_whitelist_audit_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E126-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E127 | [S07] 网络访问白名单与代理审计 · 拉起/关闭完整出路（孤儿会话回收） | `network_whitelist_audit/network_whitelist_audit_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E127-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E128 | [S07] 网络访问白名单与代理审计 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `network_whitelist_audit/network_whitelist_audit_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E128-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E129 | [S07] 网络访问白名单与代理审计 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `network_whitelist_audit/network_whitelist_audit_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E129-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E130 | [S07] 网络访问白名单与代理审计 · 100ms 反馈红线（沙箱内交互可见反馈） | `network_whitelist_audit/network_whitelist_audit_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E130-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E131 | [S07] 网络访问白名单与代理审计 · 跨界审计（跨界部件清单式声明+越界告警） | `network_whitelist_audit/network_whitelist_audit_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E131-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E132 | [S07] 网络访问白名单与代理审计 · 配额超限优雅降级（不崩宿主不丢数据） | `network_whitelist_audit/network_whitelist_audit_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E132-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E133 | [S07] 网络访问白名单与代理审计 · 资源泄漏看门狗（句柄/内存/线程三账） | `network_whitelist_audit/network_whitelist_audit_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E133-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E134 | [S07] 网络访问白名单与代理审计 · UIA 双侧投影（沙箱内控件树语义可见） | `network_whitelist_audit/network_whitelist_audit_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E134-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E135 | [S07] 网络访问白名单与代理审计 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `network_whitelist_audit/network_whitelist_audit_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E135-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E136 | [S07] 网络访问白名单与代理审计 · DPI 与多屏矩阵复检 | `network_whitelist_audit/network_whitelist_audit_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E136-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E137 | [S07] 网络访问白名单与代理审计 · 性能账（沙箱开销 P95/内存上限入账） | `network_whitelist_audit/network_whitelist_audit_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E137-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E138 | [S07] 网络访问白名单与代理审计 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `network_whitelist_audit/network_whitelist_audit_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E138-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E139 | [S07] 网络访问白名单与代理审计 · 开放接口版本化+策略签名验证 | `network_whitelist_audit/network_whitelist_audit_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E139-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |
| UNX-J4-E140 | [S07] 网络访问白名单与代理审计 · 收官自检（checks.rs 全域+文档三件套一致性） | `network_whitelist_audit/network_whitelist_audit_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E140-J1：内核事件通道+沙箱 stub 下调 network_whitelist_audit_probe() 断言） | 增补 |

### 批 S08 · 设备与硬件访问闸门（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E141 | [S08] 设备与硬件访问闸门 · 功能主路径（内核会话建链+能力位申请） | `device_hardware_gate/device_hardware_gate_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E141-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E142 | [S08] 设备与硬件访问闸门 · 开放格式清单（原子写/导出迁移） | `device_hardware_gate/device_hardware_gate_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E142-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E143 | [S08] 设备与硬件访问闸门 · 默认拒绝原则（未声明能力一律拒） | `device_hardware_gate/device_hardware_gate_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E143-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E144 | [S08] 设备与硬件访问闸门 · 越界请求三要素呈现（零裸异常码） | `device_hardware_gate/device_hardware_gate_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E144-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E145 | [S08] 设备与硬件访问闸门 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `device_hardware_gate/device_hardware_gate_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E145-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E146 | [S08] 设备与硬件访问闸门 · 体验日志埋点（沙箱操作四元组） | `device_hardware_gate/device_hardware_gate_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E146-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E147 | [S08] 设备与硬件访问闸门 · 拉起/关闭完整出路（孤儿会话回收） | `device_hardware_gate/device_hardware_gate_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E147-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E148 | [S08] 设备与硬件访问闸门 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `device_hardware_gate/device_hardware_gate_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E148-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E149 | [S08] 设备与硬件访问闸门 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `device_hardware_gate/device_hardware_gate_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E149-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E150 | [S08] 设备与硬件访问闸门 · 100ms 反馈红线（沙箱内交互可见反馈） | `device_hardware_gate/device_hardware_gate_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E150-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E151 | [S08] 设备与硬件访问闸门 · 跨界审计（跨界部件清单式声明+越界告警） | `device_hardware_gate/device_hardware_gate_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E151-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E152 | [S08] 设备与硬件访问闸门 · 配额超限优雅降级（不崩宿主不丢数据） | `device_hardware_gate/device_hardware_gate_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E152-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E153 | [S08] 设备与硬件访问闸门 · 资源泄漏看门狗（句柄/内存/线程三账） | `device_hardware_gate/device_hardware_gate_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E153-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E154 | [S08] 设备与硬件访问闸门 · UIA 双侧投影（沙箱内控件树语义可见） | `device_hardware_gate/device_hardware_gate_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E154-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E155 | [S08] 设备与硬件访问闸门 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `device_hardware_gate/device_hardware_gate_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E155-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E156 | [S08] 设备与硬件访问闸门 · DPI 与多屏矩阵复检 | `device_hardware_gate/device_hardware_gate_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E156-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E157 | [S08] 设备与硬件访问闸门 · 性能账（沙箱开销 P95/内存上限入账） | `device_hardware_gate/device_hardware_gate_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E157-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E158 | [S08] 设备与硬件访问闸门 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `device_hardware_gate/device_hardware_gate_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E158-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E159 | [S08] 设备与硬件访问闸门 · 开放接口版本化+策略签名验证 | `device_hardware_gate/device_hardware_gate_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E159-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |
| UNX-J4-E160 | [S08] 设备与硬件访问闸门 · 收官自检（checks.rs 全域+文档三件套一致性） | `device_hardware_gate/device_hardware_gate_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E160-J1：内核事件通道+沙箱 stub 下调 device_hardware_gate_probe() 断言） | 增补 |

### 批 S09 · 内存与资源配额强制（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E161 | [S09] 内存与资源配额强制 · 功能主路径（内核会话建链+能力位申请） | `memory_quota_enforcement/memory_quota_enforcement_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E161-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E162 | [S09] 内存与资源配额强制 · 开放格式清单（原子写/导出迁移） | `memory_quota_enforcement/memory_quota_enforcement_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E162-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E163 | [S09] 内存与资源配额强制 · 默认拒绝原则（未声明能力一律拒） | `memory_quota_enforcement/memory_quota_enforcement_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E163-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E164 | [S09] 内存与资源配额强制 · 越界请求三要素呈现（零裸异常码） | `memory_quota_enforcement/memory_quota_enforcement_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E164-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E165 | [S09] 内存与资源配额强制 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `memory_quota_enforcement/memory_quota_enforcement_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E165-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E166 | [S09] 内存与资源配额强制 · 体验日志埋点（沙箱操作四元组） | `memory_quota_enforcement/memory_quota_enforcement_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E166-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E167 | [S09] 内存与资源配额强制 · 拉起/关闭完整出路（孤儿会话回收） | `memory_quota_enforcement/memory_quota_enforcement_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E167-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E168 | [S09] 内存与资源配额强制 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `memory_quota_enforcement/memory_quota_enforcement_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E168-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E169 | [S09] 内存与资源配额强制 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `memory_quota_enforcement/memory_quota_enforcement_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E169-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E170 | [S09] 内存与资源配额强制 · 100ms 反馈红线（沙箱内交互可见反馈） | `memory_quota_enforcement/memory_quota_enforcement_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E170-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E171 | [S09] 内存与资源配额强制 · 跨界审计（跨界部件清单式声明+越界告警） | `memory_quota_enforcement/memory_quota_enforcement_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E171-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E172 | [S09] 内存与资源配额强制 · 配额超限优雅降级（不崩宿主不丢数据） | `memory_quota_enforcement/memory_quota_enforcement_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E172-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E173 | [S09] 内存与资源配额强制 · 资源泄漏看门狗（句柄/内存/线程三账） | `memory_quota_enforcement/memory_quota_enforcement_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E173-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E174 | [S09] 内存与资源配额强制 · UIA 双侧投影（沙箱内控件树语义可见） | `memory_quota_enforcement/memory_quota_enforcement_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E174-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E175 | [S09] 内存与资源配额强制 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `memory_quota_enforcement/memory_quota_enforcement_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E175-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E176 | [S09] 内存与资源配额强制 · DPI 与多屏矩阵复检 | `memory_quota_enforcement/memory_quota_enforcement_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E176-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E177 | [S09] 内存与资源配额强制 · 性能账（沙箱开销 P95/内存上限入账） | `memory_quota_enforcement/memory_quota_enforcement_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E177-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E178 | [S09] 内存与资源配额强制 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `memory_quota_enforcement/memory_quota_enforcement_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E178-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E179 | [S09] 内存与资源配额强制 · 开放接口版本化+策略签名验证 | `memory_quota_enforcement/memory_quota_enforcement_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E179-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |
| UNX-J4-E180 | [S09] 内存与资源配额强制 · 收官自检（checks.rs 全域+文档三件套一致性） | `memory_quota_enforcement/memory_quota_enforcement_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E180-J1：内核事件通道+沙箱 stub 下调 memory_quota_enforcement_probe() 断言） | 增补 |

### 批 S10 · 子进程派生与沙箱继承策略（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E181 | [S10] 子进程派生与沙箱继承策略 · 功能主路径（内核会话建链+能力位申请） | `child_inherit_policy/child_inherit_policy_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E181-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E182 | [S10] 子进程派生与沙箱继承策略 · 开放格式清单（原子写/导出迁移） | `child_inherit_policy/child_inherit_policy_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E182-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E183 | [S10] 子进程派生与沙箱继承策略 · 默认拒绝原则（未声明能力一律拒） | `child_inherit_policy/child_inherit_policy_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E183-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E184 | [S10] 子进程派生与沙箱继承策略 · 越界请求三要素呈现（零裸异常码） | `child_inherit_policy/child_inherit_policy_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E184-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E185 | [S10] 子进程派生与沙箱继承策略 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `child_inherit_policy/child_inherit_policy_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E185-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E186 | [S10] 子进程派生与沙箱继承策略 · 体验日志埋点（沙箱操作四元组） | `child_inherit_policy/child_inherit_policy_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E186-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E187 | [S10] 子进程派生与沙箱继承策略 · 拉起/关闭完整出路（孤儿会话回收） | `child_inherit_policy/child_inherit_policy_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E187-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E188 | [S10] 子进程派生与沙箱继承策略 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `child_inherit_policy/child_inherit_policy_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E188-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E189 | [S10] 子进程派生与沙箱继承策略 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `child_inherit_policy/child_inherit_policy_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E189-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E190 | [S10] 子进程派生与沙箱继承策略 · 100ms 反馈红线（沙箱内交互可见反馈） | `child_inherit_policy/child_inherit_policy_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E190-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E191 | [S10] 子进程派生与沙箱继承策略 · 跨界审计（跨界部件清单式声明+越界告警） | `child_inherit_policy/child_inherit_policy_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E191-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E192 | [S10] 子进程派生与沙箱继承策略 · 配额超限优雅降级（不崩宿主不丢数据） | `child_inherit_policy/child_inherit_policy_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E192-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E193 | [S10] 子进程派生与沙箱继承策略 · 资源泄漏看门狗（句柄/内存/线程三账） | `child_inherit_policy/child_inherit_policy_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E193-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E194 | [S10] 子进程派生与沙箱继承策略 · UIA 双侧投影（沙箱内控件树语义可见） | `child_inherit_policy/child_inherit_policy_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E194-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E195 | [S10] 子进程派生与沙箱继承策略 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `child_inherit_policy/child_inherit_policy_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E195-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E196 | [S10] 子进程派生与沙箱继承策略 · DPI 与多屏矩阵复检 | `child_inherit_policy/child_inherit_policy_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E196-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E197 | [S10] 子进程派生与沙箱继承策略 · 性能账（沙箱开销 P95/内存上限入账） | `child_inherit_policy/child_inherit_policy_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E197-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E198 | [S10] 子进程派生与沙箱继承策略 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `child_inherit_policy/child_inherit_policy_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E198-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E199 | [S10] 子进程派生与沙箱继承策略 · 开放接口版本化+策略签名验证 | `child_inherit_policy/child_inherit_policy_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E199-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |
| UNX-J4-E200 | [S10] 子进程派生与沙箱继承策略 · 收官自检（checks.rs 全域+文档三件套一致性） | `child_inherit_policy/child_inherit_policy_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E200-J1：内核事件通道+沙箱 stub 下调 child_inherit_policy_probe() 断言） | 增补 |

### 批 S11 · 沙箱逃逸检测与告警（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E201 | [S11] 沙箱逃逸检测与告警 · 功能主路径（内核会话建链+能力位申请） | `escape_detection_alert/escape_detection_alert_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E201-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E202 | [S11] 沙箱逃逸检测与告警 · 开放格式清单（原子写/导出迁移） | `escape_detection_alert/escape_detection_alert_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E202-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E203 | [S11] 沙箱逃逸检测与告警 · 默认拒绝原则（未声明能力一律拒） | `escape_detection_alert/escape_detection_alert_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E203-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E204 | [S11] 沙箱逃逸检测与告警 · 越界请求三要素呈现（零裸异常码） | `escape_detection_alert/escape_detection_alert_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E204-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E205 | [S11] 沙箱逃逸检测与告警 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `escape_detection_alert/escape_detection_alert_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E205-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E206 | [S11] 沙箱逃逸检测与告警 · 体验日志埋点（沙箱操作四元组） | `escape_detection_alert/escape_detection_alert_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E206-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E207 | [S11] 沙箱逃逸检测与告警 · 拉起/关闭完整出路（孤儿会话回收） | `escape_detection_alert/escape_detection_alert_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E207-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E208 | [S11] 沙箱逃逸检测与告警 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `escape_detection_alert/escape_detection_alert_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E208-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E209 | [S11] 沙箱逃逸检测与告警 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `escape_detection_alert/escape_detection_alert_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E209-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E210 | [S11] 沙箱逃逸检测与告警 · 100ms 反馈红线（沙箱内交互可见反馈） | `escape_detection_alert/escape_detection_alert_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E210-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E211 | [S11] 沙箱逃逸检测与告警 · 跨界审计（跨界部件清单式声明+越界告警） | `escape_detection_alert/escape_detection_alert_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E211-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E212 | [S11] 沙箱逃逸检测与告警 · 配额超限优雅降级（不崩宿主不丢数据） | `escape_detection_alert/escape_detection_alert_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E212-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E213 | [S11] 沙箱逃逸检测与告警 · 资源泄漏看门狗（句柄/内存/线程三账） | `escape_detection_alert/escape_detection_alert_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E213-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E214 | [S11] 沙箱逃逸检测与告警 · UIA 双侧投影（沙箱内控件树语义可见） | `escape_detection_alert/escape_detection_alert_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E214-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E215 | [S11] 沙箱逃逸检测与告警 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `escape_detection_alert/escape_detection_alert_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E215-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E216 | [S11] 沙箱逃逸检测与告警 · DPI 与多屏矩阵复检 | `escape_detection_alert/escape_detection_alert_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E216-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E217 | [S11] 沙箱逃逸检测与告警 · 性能账（沙箱开销 P95/内存上限入账） | `escape_detection_alert/escape_detection_alert_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E217-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E218 | [S11] 沙箱逃逸检测与告警 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `escape_detection_alert/escape_detection_alert_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E218-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E219 | [S11] 沙箱逃逸检测与告警 · 开放接口版本化+策略签名验证 | `escape_detection_alert/escape_detection_alert_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E219-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |
| UNX-J4-E220 | [S11] 沙箱逃逸检测与告警 · 收官自检（checks.rs 全域+文档三件套一致性） | `escape_detection_alert/escape_detection_alert_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E220-J1：内核事件通道+沙箱 stub 下调 escape_detection_alert_probe() 断言） | 增补 |

### 批 S12 · 沙箱快照与状态回滚（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E221 | [S12] 沙箱快照与状态回滚 · 功能主路径（内核会话建链+能力位申请） | `snapshot_rollback/snapshot_rollback_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E221-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E222 | [S12] 沙箱快照与状态回滚 · 开放格式清单（原子写/导出迁移） | `snapshot_rollback/snapshot_rollback_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E222-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E223 | [S12] 沙箱快照与状态回滚 · 默认拒绝原则（未声明能力一律拒） | `snapshot_rollback/snapshot_rollback_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E223-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E224 | [S12] 沙箱快照与状态回滚 · 越界请求三要素呈现（零裸异常码） | `snapshot_rollback/snapshot_rollback_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E224-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E225 | [S12] 沙箱快照与状态回滚 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `snapshot_rollback/snapshot_rollback_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E225-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E226 | [S12] 沙箱快照与状态回滚 · 体验日志埋点（沙箱操作四元组） | `snapshot_rollback/snapshot_rollback_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E226-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E227 | [S12] 沙箱快照与状态回滚 · 拉起/关闭完整出路（孤儿会话回收） | `snapshot_rollback/snapshot_rollback_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E227-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E228 | [S12] 沙箱快照与状态回滚 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `snapshot_rollback/snapshot_rollback_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E228-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E229 | [S12] 沙箱快照与状态回滚 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `snapshot_rollback/snapshot_rollback_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E229-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E230 | [S12] 沙箱快照与状态回滚 · 100ms 反馈红线（沙箱内交互可见反馈） | `snapshot_rollback/snapshot_rollback_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E230-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E231 | [S12] 沙箱快照与状态回滚 · 跨界审计（跨界部件清单式声明+越界告警） | `snapshot_rollback/snapshot_rollback_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E231-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E232 | [S12] 沙箱快照与状态回滚 · 配额超限优雅降级（不崩宿主不丢数据） | `snapshot_rollback/snapshot_rollback_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E232-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E233 | [S12] 沙箱快照与状态回滚 · 资源泄漏看门狗（句柄/内存/线程三账） | `snapshot_rollback/snapshot_rollback_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E233-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E234 | [S12] 沙箱快照与状态回滚 · UIA 双侧投影（沙箱内控件树语义可见） | `snapshot_rollback/snapshot_rollback_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E234-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E235 | [S12] 沙箱快照与状态回滚 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `snapshot_rollback/snapshot_rollback_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E235-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E236 | [S12] 沙箱快照与状态回滚 · DPI 与多屏矩阵复检 | `snapshot_rollback/snapshot_rollback_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E236-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E237 | [S12] 沙箱快照与状态回滚 · 性能账（沙箱开销 P95/内存上限入账） | `snapshot_rollback/snapshot_rollback_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E237-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E238 | [S12] 沙箱快照与状态回滚 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `snapshot_rollback/snapshot_rollback_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E238-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E239 | [S12] 沙箱快照与状态回滚 · 开放接口版本化+策略签名验证 | `snapshot_rollback/snapshot_rollback_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E239-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |
| UNX-J4-E240 | [S12] 沙箱快照与状态回滚 · 收官自检（checks.rs 全域+文档三件套一致性） | `snapshot_rollback/snapshot_rollback_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E240-J1：内核事件通道+沙箱 stub 下调 snapshot_rollback_probe() 断言） | 增补 |

### 批 S13 · 沙箱日志隔离与父域审计（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E241 | [S13] 沙箱日志隔离与父域审计 · 功能主路径（内核会话建链+能力位申请） | `sandbox_log_audit/sandbox_log_audit_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E241-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E242 | [S13] 沙箱日志隔离与父域审计 · 开放格式清单（原子写/导出迁移） | `sandbox_log_audit/sandbox_log_audit_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E242-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E243 | [S13] 沙箱日志隔离与父域审计 · 默认拒绝原则（未声明能力一律拒） | `sandbox_log_audit/sandbox_log_audit_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E243-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E244 | [S13] 沙箱日志隔离与父域审计 · 越界请求三要素呈现（零裸异常码） | `sandbox_log_audit/sandbox_log_audit_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E244-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E245 | [S13] 沙箱日志隔离与父域审计 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `sandbox_log_audit/sandbox_log_audit_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E245-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E246 | [S13] 沙箱日志隔离与父域审计 · 体验日志埋点（沙箱操作四元组） | `sandbox_log_audit/sandbox_log_audit_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E246-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E247 | [S13] 沙箱日志隔离与父域审计 · 拉起/关闭完整出路（孤儿会话回收） | `sandbox_log_audit/sandbox_log_audit_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E247-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E248 | [S13] 沙箱日志隔离与父域审计 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `sandbox_log_audit/sandbox_log_audit_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E248-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E249 | [S13] 沙箱日志隔离与父域审计 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `sandbox_log_audit/sandbox_log_audit_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E249-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E250 | [S13] 沙箱日志隔离与父域审计 · 100ms 反馈红线（沙箱内交互可见反馈） | `sandbox_log_audit/sandbox_log_audit_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E250-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E251 | [S13] 沙箱日志隔离与父域审计 · 跨界审计（跨界部件清单式声明+越界告警） | `sandbox_log_audit/sandbox_log_audit_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E251-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E252 | [S13] 沙箱日志隔离与父域审计 · 配额超限优雅降级（不崩宿主不丢数据） | `sandbox_log_audit/sandbox_log_audit_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E252-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E253 | [S13] 沙箱日志隔离与父域审计 · 资源泄漏看门狗（句柄/内存/线程三账） | `sandbox_log_audit/sandbox_log_audit_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E253-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E254 | [S13] 沙箱日志隔离与父域审计 · UIA 双侧投影（沙箱内控件树语义可见） | `sandbox_log_audit/sandbox_log_audit_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E254-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E255 | [S13] 沙箱日志隔离与父域审计 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `sandbox_log_audit/sandbox_log_audit_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E255-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E256 | [S13] 沙箱日志隔离与父域审计 · DPI 与多屏矩阵复检 | `sandbox_log_audit/sandbox_log_audit_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E256-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E257 | [S13] 沙箱日志隔离与父域审计 · 性能账（沙箱开销 P95/内存上限入账） | `sandbox_log_audit/sandbox_log_audit_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E257-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E258 | [S13] 沙箱日志隔离与父域审计 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `sandbox_log_audit/sandbox_log_audit_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E258-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E259 | [S13] 沙箱日志隔离与父域审计 · 开放接口版本化+策略签名验证 | `sandbox_log_audit/sandbox_log_audit_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E259-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |
| UNX-J4-E260 | [S13] 沙箱日志隔离与父域审计 · 收官自检（checks.rs 全域+文档三件套一致性） | `sandbox_log_audit/sandbox_log_audit_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E260-J1：内核事件通道+沙箱 stub 下调 sandbox_log_audit_probe() 断言） | 增补 |

### 批 S14 · 插件沙箱与第三方扩展隔离（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E261 | [S14] 插件沙箱与第三方扩展隔离 · 功能主路径（内核会话建链+能力位申请） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E261-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E262 | [S14] 插件沙箱与第三方扩展隔离 · 开放格式清单（原子写/导出迁移） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E262-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E263 | [S14] 插件沙箱与第三方扩展隔离 · 默认拒绝原则（未声明能力一律拒） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E263-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E264 | [S14] 插件沙箱与第三方扩展隔离 · 越界请求三要素呈现（零裸异常码） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E264-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E265 | [S14] 插件沙箱与第三方扩展隔离 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E265-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E266 | [S14] 插件沙箱与第三方扩展隔离 · 体验日志埋点（沙箱操作四元组） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E266-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E267 | [S14] 插件沙箱与第三方扩展隔离 · 拉起/关闭完整出路（孤儿会话回收） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E267-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E268 | [S14] 插件沙箱与第三方扩展隔离 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E268-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E269 | [S14] 插件沙箱与第三方扩展隔离 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E269-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E270 | [S14] 插件沙箱与第三方扩展隔离 · 100ms 反馈红线（沙箱内交互可见反馈） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E270-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E271 | [S14] 插件沙箱与第三方扩展隔离 · 跨界审计（跨界部件清单式声明+越界告警） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E271-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E272 | [S14] 插件沙箱与第三方扩展隔离 · 配额超限优雅降级（不崩宿主不丢数据） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E272-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E273 | [S14] 插件沙箱与第三方扩展隔离 · 资源泄漏看门狗（句柄/内存/线程三账） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E273-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E274 | [S14] 插件沙箱与第三方扩展隔离 · UIA 双侧投影（沙箱内控件树语义可见） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E274-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E275 | [S14] 插件沙箱与第三方扩展隔离 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E275-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E276 | [S14] 插件沙箱与第三方扩展隔离 · DPI 与多屏矩阵复检 | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E276-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E277 | [S14] 插件沙箱与第三方扩展隔离 · 性能账（沙箱开销 P95/内存上限入账） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E277-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E278 | [S14] 插件沙箱与第三方扩展隔离 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E278-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E279 | [S14] 插件沙箱与第三方扩展隔离 · 开放接口版本化+策略签名验证 | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E279-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |
| UNX-J4-E280 | [S14] 插件沙箱与第三方扩展隔离 · 收官自检（checks.rs 全域+文档三件套一致性） | `plugin_sandbox_thirdparty/plugin_sandbox_thirdparty_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E280-J1：内核事件通道+沙箱 stub 下调 plugin_sandbox_thirdparty_probe() 断言） | 增补 |

### 批 S15 · J4 增补卷治理收官总账（20 条）

| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |
|---|---|---|---|---|
| UNX-J4-E281 | [S15] J4 增补卷治理收官总账 · 功能主路径（内核会话建链+能力位申请） | `j4_supp_governance/j4_supp_governance_00.rs` | Varix 内核锚定：经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见（UNX-J4-E281-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E282 | [S15] J4 增补卷治理收官总账 · 开放格式清单（原子写/导出迁移） | `j4_supp_governance/j4_supp_governance_01.rs` | Varix 内核锚定：沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致（UNX-J4-E282-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E283 | [S15] J4 增补卷治理收官总账 · 默认拒绝原则（未声明能力一律拒） | `j4_supp_governance/j4_supp_governance_02.rs` | Varix 内核锚定：未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码（UNX-J4-E283-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E284 | [S15] J4 增补卷治理收官总账 · 越界请求三要素呈现（零裸异常码） | `j4_supp_governance/j4_supp_governance_03.rs` | Varix 内核锚定：全部越界错误走人话映射；判据：越界路径 grep 裸码零命中（UNX-J4-E284-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E285 | [S15] J4 增补卷治理收官总账 · 隐蔽失败探针（静默拒绝/异步回调入总日志） | `j4_supp_governance/j4_supp_governance_04.rs` | Varix 内核锚定：隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件（UNX-J4-E285-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E286 | [S15] J4 增补卷治理收官总账 · 体验日志埋点（沙箱操作四元组） | `j4_supp_governance/j4_supp_governance_05.rs` | Varix 内核锚定：操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容（UNX-J4-E286-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E287 | [S15] J4 增补卷治理收官总账 · 拉起/关闭完整出路（孤儿会话回收） | `j4_supp_governance/j4_supp_governance_06.rs` | Varix 内核锚定：父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零（UNX-J4-E287-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E288 | [S15] J4 增补卷治理收官总账 · 状态机全覆盖（启动中/运行/暂停/终止乱序注入） | `j4_supp_governance/j4_supp_governance_07.rs` | Varix 内核锚定：fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS（UNX-J4-E288-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E289 | [S15] J4 增补卷治理收官总账 · 键盘/焦点隔离（沙箱 UI 不劫持宿主焦点） | `j4_supp_governance/j4_supp_governance_08.rs` | Varix 内核锚定：沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢（UNX-J4-E289-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E290 | [S15] J4 增补卷治理收官总账 · 100ms 反馈红线（沙箱内交互可见反馈） | `j4_supp_governance/j4_supp_governance_09.rs` | Varix 内核锚定：交互反馈埋点；判据：反馈账 P95 < 100ms（UNX-J4-E290-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E291 | [S15] J4 增补卷治理收官总账 · 跨界审计（跨界部件清单式声明+越界告警） | `j4_supp_governance/j4_supp_governance_10.rs` | Varix 内核锚定：跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账（UNX-J4-E291-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E292 | [S15] J4 增补卷治理收官总账 · 配额超限优雅降级（不崩宿主不丢数据） | `j4_supp_governance/j4_supp_governance_11.rs` | Varix 内核锚定：超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活（UNX-J4-E292-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E293 | [S15] J4 增补卷治理收官总账 · 资源泄漏看门狗（句柄/内存/线程三账） | `j4_supp_governance/j4_supp_governance_12.rs` | Varix 内核锚定：泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件（UNX-J4-E293-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E294 | [S15] J4 增补卷治理收官总账 · UIA 双侧投影（沙箱内控件树语义可见） | `j4_supp_governance/j4_supp_governance_13.rs` | Varix 内核锚定：沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点（UNX-J4-E294-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E295 | [S15] J4 增补卷治理收官总账 · 高对比/色弱/灰度复检（沙箱 UI 同标准） | `j4_supp_governance/j4_supp_governance_14.rs` | Varix 内核锚定：三态渲染达标；判据：比对脚本 PASS（UNX-J4-E295-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E296 | [S15] J4 增补卷治理收官总账 · DPI 与多屏矩阵复检 | `j4_supp_governance/j4_supp_governance_15.rs` | Varix 内核锚定：沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS（UNX-J4-E296-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E297 | [S15] J4 增补卷治理收官总账 · 性能账（沙箱开销 P95/内存上限入账） | `j4_supp_governance/j4_supp_governance_16.rs` | Varix 内核锚定：沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB（UNX-J4-E297-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E298 | [S15] J4 增补卷治理收官总账 · 崩溃恢复（沙箱崩溃零波及宿主零波及邻箱） | `j4_supp_governance/j4_supp_governance_17.rs` | Varix 内核锚定：kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS（UNX-J4-E298-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E299 | [S15] J4 增补卷治理收官总账 · 开放接口版本化+策略签名验证 | `j4_supp_governance/j4_supp_governance_18.rs` | Varix 内核锚定：策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕（UNX-J4-E299-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |
| UNX-J4-E300 | [S15] J4 增补卷治理收官总账 · 收官自检（checks.rs 全域+文档三件套一致性） | `j4_supp_governance/j4_supp_governance_19.rs` | Varix 内核锚定：自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS（UNX-J4-E300-J1：内核事件通道+沙箱 stub 下调 j4_supp_governance_probe() 断言） | 增补 |

> 卷小计：300 条。AI-29 增补总账：F4 六卷 1800 项 + J4 一卷 300 项 = 2100 项增补，域账零触碰。
