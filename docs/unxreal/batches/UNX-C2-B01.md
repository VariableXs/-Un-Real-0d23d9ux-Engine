# UNX-C2-B01 · 分发表骨架与调用 ABI（F8801–F8820 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：本批 5,200 / 240,000｜嫁接源：Linux man-pages（syscalls 章节）、x86-64 System V ABI、Intel SDM Vol.2 只跟随｜防重：与现存 `kernel/varix/src/syscall/`（table.rs 18 号注册面 + COMPAT_TABLE、entry.rs SYSCALL/int 0x80 双门、errno.rs、guard.rs、meter.rs、uaccess.rs、calls.rs、userlib.rs）为升级接管扩容，接管对象逐条声明，非重复实现｜判据与 deepen/C2-B01.md 逐条同名同判据同 ID｜本批为试产批 2（跨组验证依赖协议，数据回填 ADR-UNX-005）

### UNX-F8801 · syscall 分发表骨架与注册协议
- 域/批：C2/B01｜纯功能行数：350｜状态：[已深化]｜判据：UNX-F8801-J1 300+ 表项注册唯一性断言全过（重复注册构建失败即捕获），未知号返回 -ENOSYS 与 Linux 基准一致，抽查 50 号逐号断言零混码
### UNX-F8802 · syscall 入口上下文保存与参数提取
- 域/批：C2/B01｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8802-J1 6 参 syscall 寄存器逐位传递正确（rdi/rsi/rdx/r10/r8/r9），r10 槽位约定专项断言通过，EFAULT 探针注入不崩内核 10/10 次
### UNX-F8803 · write/read 循环语义与错误矩阵
- 域/批：C2/B01｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F8803-J1 EBADF/EFAULT/短写/零长四路径行为与 Linux 逐码一致，O_APPEND 追加位生效可观测，短写返回值计数准确
### UNX-F8804 · mmap/munmap/mprotect 三联语义
- 域/批：C2/B01｜纯功能行数：380｜状态：[已深化]｜判据：UNX-F8804-J1 对齐/越界/权限翻转错误码矩阵全对齐 Linux 基准，mprotect 翻只读后写触发 #PF 且进程收 SIGSEGV（与 C4 联测）
### UNX-F8805 · -ENOSYS 与保留号段行为
- 域/批：C2/B01｜纯功能行数：150｜状态：[已深化]｜判据：UNX-F8805-J1 全部未实现号返回 -ENOSYS，抽查 50 号逐号断言无混码，保留带（Reserved/Out）与未注册带（NotYet）分档留痕
### UNX-F8806 · 号段治理四带路由升级接管
- 域/批：C2/B01｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8806-J1 band_of 四带判定（Reserved/Native/Compat/Vendor）与 Linux 号面路由回归 1,000 号零误判，band miss 与垃圾号分档入账
### UNX-F8807 · Linux 兼容号面转接器扩容
- 域/批：C2/B01｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F8807-J1 COMPAT_TABLE 15 号存量 + 扩容新号转接路由 300 号全通，RouteNote 四态（Exact/Permuted/ExtraArgsIgnored/ReturnDiffers）逐号核对一致
### UNX-F8808 · ABI 版本协商与特性位面升级接管
- 域/批：C2/B01｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8808-J1 abi_negotiate 对 32 位调用方/未来版本/过旧版本三拒绝路径各 100 次断言正确，AbiInfo 字段逐项与注册面实态一致
### UNX-F8809 · 分发表注册协议冻结单与下游联签文书
- 域/批：C2/B01｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8809-J1 冻结接口 {nr,handler,nargs,flags,persona} 五字段协议文书落盘，C3/C4/C5/E1 四消费方联签栏就位，接口变更走 mini-ADR 有据可查
### UNX-F8810 · 能力位 guard 预挂点（CAP_* 矩阵）
- 域/批：C2/B01｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8810-J1 每 syscall 声明所需 CAP_* 位与 guard 闸门判定回归 300 号全通，未知号 fail-closed（CAP_ADMIN）判据保持，缺位拒绝留痕
### UNX-F8811 · 延迟计量 meter 挂点与五指标采集
- 域/批：C2/B01｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8811-J1 SyscallMeter 对 P50/P99 五指标采集落账，空调用百万次 P95 与 lxerrno 转译开销 ≤2μs 对账一致，账随批可导出
### UNX-F8812 · uaccess 用户指针探针协议升级接管
- 域/批：C2/B01｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F8812-J1 copy_from_user/copy_to_user/UserRegion 三原语探针注入坏指针 10/10 次返回 EFAULT 不崩内核，部分拷贝语义计数准确
### UNX-F8813 · errno 编码器全集与负值编码规约
- 域/批：C2/B01｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8813-J1 Errno 全集正反向编码（sysret_decode/sysret_err）往返一致率 100%，-4095..-1 边界带逐值断言，Errno::Ok 误用被拒
### UNX-F8814 · 快路径标记与二分查找优化
- 域/批：C2/B01｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8814-J1 表升序不变量断言保持（windows(2) 全过），fast 位号面快路径命中账可导出，查找开销对账在预算内
### UNX-F8815 · 双门入口（SYSCALL/int 0x80）并轨路由
- 域/批：C2/B01｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8815-J1 SYSCALL 与 int 0x80 双门同号路由结果一致 1,000 对断言零差，门选择入账可观测，LSTAR/SFMASK 配置快照落账
### UNX-F8816 · 每任务内核栈与 TSS.RSP0 切换账
- 域/批：C2/B01｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8816-J1 入口栈切换后 RSP0 指向每任务内核栈断言通过，栈溢出保护页触发可观测，切换路径无泄漏 10^5 次验证
### UNX-F8817 · syscall 返回路径与 sysret 单点
- 域/批：C2/B01｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8817-J1 返回路径寄存器恢复断言（rcx/rip/r11 语义）全过，sysret_decode 单点消费判据入账，双向路径对称可审计
### UNX-F8818 · 号面自描述导出（manifest 与 nr_count）
- 域/批：C2/B01｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8818-J1 号面 manifest（每号名/arity/caps/fast/audited）导出与 SYSCALLS 表逐项一致，nr_count 双账核平，导出可机读
### UNX-F8819 · seccomp 过滤器前置挂点协议
- 域/批：C2/B01｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8819-J1 SeccompFilter 前置拦截协议：过滤拒绝的号不进分发表统计（账分离）断言通过，SeccompVerdict 三态判定回归全绿
### UNX-F8820 · ktest syscall 面 B01 批断言集
- 域/批：C2/B01｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F8820-J1 本批 19 条判据全部聚合入 ktest syscall 面，一次命令全跑，CheckSet 三态（pass/fail/skip）落账，skip 必带原因，通过率 100% 才算绿
