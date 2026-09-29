# UNX-C2-B27 · E 型错误矩阵七：时间/资源族（EINVAL/EAGAIN）（F9321-F9340 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 非法值域全 EINVAL、定时器耗尽全 EAGAIN 时间资源族矩阵）｜本批 [骨架] 20 条全为本会话新铺设｜域账累计：B01–B15 66,870 + B16–B26 73,030 + 本批 6,900 = 146,800 / 240,000｜嫁接源：man-pages clock_getres(2)/timer_create(2)/setrlimit(2)/sched(7) 错误段语义对齐；与 B06 时间族/B19 资源族/B20 规范化对账；A2 计时基座消费｜防重声明：时间/资源族 E 型 20 号为号面新铺（B01–B15 时间资源正判据已深化，本批只铺错误注入矩阵号，零重复）；扩号段 F9321–F9340 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；零时钟源直改承诺（clock_settime 特权检查挂点声明）；零硬件频率改动；双轨产线条款默认适用｜批注（AI-12）：B27 是时间/资源族错误矩阵批：tv_nsec 规范化边界（≥1e9 全 EINVAL）与 POSIX 定时器耗尽 EAGAIN（RLIMIT_SIGPENDING 联动）是判据重心，sched_setaffinity/setscheduler 错误矩阵为 A1 调度消费预留对装面｜判据与后续 deepen/C2-B27.md 逐条同名同判据同 ID

### UNX-F9321 · 时间资源错误面总纲（EINVAL 判定域）
- 域/批：C2/B27｜纯功能行数：400｜状态：｜判据：UNX-F9321-J1 时间资源错误面判据过，EINVAL 触发域（非法时钟/非法标志/值域越界）逐格落账，与 Linux 语义一致格过
### UNX-F9322 · gettimeofday/clock_gettime 坏指针矩阵（EFAULT）
- 域/批：C2/B27｜纯功能行数：380｜状态：｜判据：UNX-F9322-J1 坏指针注入判据过，全 EFAULT 格过，10/10 次检出格过，与 B06 时间族对账格过
### UNX-F9323 · clock_settime 非法时钟 EINVAL 矩阵
- 域/批：C2/B27｜纯功能行数：360｜状态：｜判据：UNX-F9323-J1 非法 clockid 注入判据过，全 EINVAL 格过，CLOCK_REALTIME 正判据格过，特权检查挂点格过
### UNX-F9324 · timer_create/timer_settime 错误矩阵（EINVAL/EAGAIN）
- 域/批：C2/B27｜纯功能行数：380｜状态：｜判据：UNX-F9324-J1 两错误码注入矩阵判据过，非法 sigevent 全 EINVAL 格过，定时器耗尽全 EAGAIN 格过
### UNX-F9325 · POSIX 定时器耗尽 EAGAIN 矩阵（RLIMIT_SIGPENDING 联动）
- 域/批：C2/B27｜纯功能行数：360｜状态：｜判据：UNX-F9325-J1 定时器上限注入判据过，全 EAGAIN 格过，与 B19 SIGPENDING 联动格过
### UNX-F9326 · setitimer 非法间隔矩阵
- 域/批：C2/B27｜纯功能行数：320｜状态：｜判据：UNX-F9326-J1 负值/超上界注入判据过，全 EINVAL 格过，ITIMER_REAL 三类正判据格过
### UNX-F9327 · clock_nanosleep 非法标志/时钟矩阵
- 域/批：C2/B27｜纯功能行数：360｜状态：｜判据：UNX-F9327-J1 非法标志注入判据过，全 EINVAL/ENOTSUP 逐格检出，非法时钟全 EINVAL 格过，与 B20 对账格过
### UNX-F9328 · getrusage 坏指针矩阵（联 B15/B19 对账）
- 域/批：C2/B27｜纯功能行数：340｜状态：｜判据：UNX-F9328-J1 坏指针注入判据过，全 EFAULT 格过，非法 who 全 EINVAL 格过，与 B19 对账零重复格过
### UNX-F9329 · setrlimit 坏结构指针矩阵
- 域/批：C2/B27｜纯功能行数：340｜状态：｜判据：UNX-F9329-J1 坏 rlim 指针注入判据过，全 EFAULT 格过，10/10 次检出格过
### UNX-F9330 · rlimit 软硬倒置 EINVAL 矩阵（联 B19 对账）
- 域/批：C2/B27｜纯功能行数：340｜状态：｜判据：UNX-F9330-J1 软硬倒置注入判据过，全 EINVAL 格过，与 B19 矩阵零重复分层格过
### UNX-F9331 · sched_setaffinity 坏掩码 EINVAL 矩阵
- 域/批：C2/B27｜纯功能行数：340｜状态：｜判据：UNX-F9331-J1 全零掩码注入判据过，全 EINVAL 格过，坏 mask 指针全 EFAULT 格过
### UNX-F9332 · sched_setscheduler 非法策略 EINVAL 矩阵
- 域/批：C2/B27｜纯功能行数：340｜状态：｜判据：UNX-F9332-J1 非法策略注入判据过，全 EINVAL 格过，非法 param 全 EINVAL 格过，A1 消费声明格过
### UNX-F9333 · 时间值规范化边界矩阵（tv_nsec 上限 1e9）
- 域/批：C2/B27｜纯功能行数：380｜状态：｜判据：UNX-F9333-J1 tv_nsec 越界注入判据过，≥1e9 全 EINVAL 格过，规范化（tv_sec 进位）语义格过，与 B20 规范化对账一致格过
### UNX-F9334 · 时间/资源族 E 型参数探针总成
- 域/批：C2/B27｜纯功能行数：300｜状态：｜判据：UNX-F9334-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过
### UNX-F9335 · 时间/资源族 E 型与 B06/B19 深化册对账格
- 域/批：C2/B27｜纯功能行数：300｜状态：｜判据：UNX-F9335-J1 对账判据过，与时间族/资源族深化册逐条零冲突格过，正反分层声明格过
### UNX-F9336 · 时间/资源族 E 型与 Windows 计时器错误对照格
- 域/批：C2/B27｜纯功能行数：300｜状态：｜判据：UNX-F9336-J1 ERROR_INVALID_PARAMETER 映射判据过，差异账三条全列格过
### UNX-F9337 · E 型判据可复测性声明（10/10 次检出）
- 域/批：C2/B27｜纯功能行数：300｜状态：｜判据：UNX-F9337-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过
### UNX-F9338 · 时间/资源族 E 型升级接管注记
- 域/批：C2/B27｜纯功能行数：280｜状态：｜判据：UNX-F9338-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过
### UNX-F9339 · 时间/资源族 E 型与 glibc clock 家族消费核
- 域/批：C2/B27｜纯功能行数：340｜状态：｜判据：UNX-F9339-J1 clock_gettime errno 透传判据过，失效时钟全 EINVAL 可观测，消费号登记格过
### UNX-F9340 · ktest syscall 面 B27 批断言集
- 域/批：C2/B27｜纯功能行数：440｜状态：｜判据：UNX-F9340-J1 本批 19 条判据聚合判据过，时间资源族 E 型矩阵一次跑通，域累计 146,800 收口断言
