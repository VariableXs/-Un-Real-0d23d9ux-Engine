# UNX-C2-B19 · 资源族：setrlimit/getrlimit/setpriority（F9161-F9180 · 20 条）

> AI-12 承办｜批次类型：M 型（任务书 B09-B20 M 型收尾轴 · 资源族）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B19.md 全六要素收口）｜域账累计：B01-B15 66,870 + B16 6,500 + B17 6,400 + B18 6,450 + 本批 6,400 = 92,620 / 240,000｜嫁接源：getrlimit(2)/setpriority(2)/times(2) man-pages 语义对齐；A2 计时基座冻结接口消费；与 B15 F9091/F9092 getrusage/prlimit64 档单源联动；A1 调度消费（sched_yield）｜防重声明：资源族 20 号为号面新铺（B01-B15 getrusage/prlimit64 号面补位已铺于 B15，本批铺 rlimit 全集与优先级族，零重复）；扩号段 F9161-F9180 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；限额硬限提升走 EPERM 矩阵（零越权路径）；零硬件频率/电压类改动（硬件红线条款默认适用）｜批注（AI-12）：B19 是资源族轴：rlimit 全集逐格是 shell ulimit 与 POSIX spawn 的地基，软硬限翻转协议（软可降硬不可升）是安全边界核心，nice 值域与 A1 调度消费单源，RLIMIT_NPROC 与 B25 fork EAGAIN 格联动｜判据与后续 deepen/C2-B19.md 逐条同名同判据同 ID

### UNX-F9161 · setrlimit/getrlimit 语义档总纲（rlimit 结构四元组）
- 域/批：C2/B19｜纯功能行数：420｜状态：｜判据：UNX-F9161-J1 软硬限读写回环判据过，结构四元组探针格过，RLIM_INFINITY 编码格过
### UNX-F9162 · RLIMIT_NOFILE/CORE/STACK 三限额逐格
- 域/批：C2/B19｜纯功能行数：360｜状态：｜判据：UNX-F9162-J1 三限额逐格判据过，NOFILE 与 fd 分配器联动可观测，CORE 零值禁转储格过
### UNX-F9163 · RLIMIT_AS/DATA/RSS 内存限额族
- 域/批：C2/B19｜纯功能行数：340｜状态：｜判据：UNX-F9163-J1 三限额判据过，AS 与 mmap 分配联动可观测，RSS 提示性语义声明格过
### UNX-F9164 · RLIMIT_CPU/NPROC/SIGPENDING 调度限额族
- 域/批：C2/B19｜纯功能行数：340｜状态：｜判据：UNX-F9164-J1 三限额判据过，CPU 超限 SIGXCPU 投递挂点格过，NPROC 与 fork EAGAIN 联动格过
### UNX-F9165 · RLIMIT_FSIZE/LOCKS/MSGQUEUE/RTPRIO 补位族
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9165-J1 四限额补位判据过，FSIZE 超限 SIGXFSZ 挂点格过，未实装限额零静默声明格过
### UNX-F9166 · prlimit64 与 setrlimit 协同（新旧 ABI 双轨）
- 域/批：C2/B19｜纯功能行数：340｜状态：｜判据：UNX-F9166-J1 双轨协同判据过，新旧 ABI 行为一致可观测，与 B15 prlimit64 档对账一致
### UNX-F9167 · 限额错误矩阵（EINVAL/EPERM/ENOMEM）
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9167-J1 三错误码注入矩阵判据过，软硬序倒置全 EINVAL 格过，硬限提升全 EPERM 格过
### UNX-F9168 · RLIM_INFINITY 与软硬限翻转协议
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9168-J1 翻转协议判据过，软限可降可升到硬限判据过，硬限非特权只降格过
### UNX-F9169 · setpriority/getpriority 语义档（nice 值域）
- 域/批：C2/B19｜纯功能行数：340｜状态：｜判据：UNX-F9169-J1 值域 [-20,19] 判据过，越界钳制语义格过，PRIO_PROCESS/PRIO_PGRP/PRIO_USER 三档格过
### UNX-F9170 · 优先级 EACCES/EPERM 矩阵（特权降级规则）
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9170-J1 降权越级矩阵判据过，nice 降级（更优先）非特权全 EACCES 格过，与 B24 分界声明格过
### UNX-F9171 · sched_yield 挂点（A1 调度消费）
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9171-J1 让出挂点判据过，A1 调度消费声明格过，单任务零副作用格过
### UNX-F9172 · times 语义档（tms 四字段与 CLK_TCK）
- 域/批：C2/B19｜纯功能行数：320｜状态：｜判据：UNX-F9172-J1 四字段判据过，CLK_TCK 换算一致格过，与 getrusage 对账一致格过
### UNX-F9173 · getrusage CHILDREN 联动（wait 收割累计）
- 域/批：C2/B19｜纯功能行数：340｜状态：｜判据：UNX-F9173-J1 收割累计判据过，与 wait 族 rusage 联动一致，二次累计零重复格过
### UNX-F9174 · 资源族与 A2 计时基座消费协议
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9174-J1 消费协议落账判据过，时钟源单源声明格过，上游依赖登记格过
### UNX-F9175 · 资源族参数探针总成
- 域/批：C2/B19｜纯功能行数：280｜状态：｜判据：UNX-F9175-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源
### UNX-F9176 · 资源族与 shell ulimit 消费核
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9176-J1 ulimit 内建消费链判据过，软硬限透传可观测，消费号登记格过
### UNX-F9177 · 资源族升级接管注记
- 域/批：C2/B19｜纯功能行数：280｜状态：｜判据：UNX-F9177-J1 接管边界声明判据过，存量零重写承诺格过，升级增量清单落账
### UNX-F9178 · 资源族与 Windows 工作集/配额对照格
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9178-J1 工作集对照判据过，Job Object 配额映射表落账，差异账三条全列
### UNX-F9179 · getrusage RUSAGE_SELF 静态账（与 B15 对账）
- 域/批：C2/B19｜纯功能行数：300｜状态：｜判据：UNX-F9179-J1 静态账判据过，与 getrusage 语义档逐字段对账一致，零重复声明格过
### UNX-F9180 · ktest syscall 面 B19 批断言集
- 域/批：C2/B19｜纯功能行数：340｜状态：｜判据：UNX-F9180-J1 本批 19 条判据聚合判据过，rlimit 全集与优先级族一次跑通，域累计 92,620 收口断言
