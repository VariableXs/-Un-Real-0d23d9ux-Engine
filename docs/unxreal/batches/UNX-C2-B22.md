# UNX-C2-B22 · E 型错误矩阵二：fd 族（EBADF/ENOENT/EMFILE/ENFILE）（F9221-F9240 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 坏 fd 全 EBADF、路径缺失全 ENOENT fd 族矩阵）｜本批 [骨架] 20 条全为本会话新铺设｜域账累计：B01–B15 66,870 + B16–B21 38,830 + 本批 6,700 = 112,400 / 240,000｜嫁接源：man-pages dup(2)/pipe(2)/open(2)/path_resolution(7) 错误段语义对齐；与 B08 dup 族/B15 F9093 管道账/B03 深化册单源联动；B1 VFS 冻结读路径消费｜防重声明：fd 族 E 型 20 号为号面新铺（B01–B15 fd 正判据已深化，本批只铺错误注入矩阵号，零重复）；扩号段 F9221–F9240 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；fd 竞态注入零越权写承诺（TOCTOU 窗口注入后无跨进程越权可观测）；双轨产线条款默认适用｜批注（AI-12）：B22 是 fd 族错误矩阵批：EBADF 判定域三条件（越界/未开/已关）与 ENOENT 判定面（路径解析三态）逐格落账，EBADF vs ENOENT 判定次序协议（先 fd 后路径）是全 E 型批的判序地基，EMFILE/ENFILE 双层耗尽矩阵与 B19 RLIMIT_NOFILE 联动｜判据与后续 deepen/C2-B22.md 逐条同名同判据同 ID

### UNX-F9221 · fd 语义域总纲（EBADF 判定域与 fd 空间模型）
- 域/批：C2/B22｜纯功能行数：400｜状态：｜判据：UNX-F9221-J1 fd 空间模型（0/1/2 基座+分配序）判据过，EBADF 判定域三条件（越界/未开/已关）逐格落账，判定先于操作语义格过
### UNX-F9222 · dup/dup2/dup3 坏 fd 矩阵
- 域/批：C2/B22｜纯功能行数：340｜状态：｜判据：UNX-F9222-J1 oldfd/newfd 坏值注入矩阵判据过，全 EBADF 逐格检出，dup2 equal-fd 返回 newfd 正判据格过
### UNX-F9223 · pipe 系统调用错误矩阵（EFAULT/EMFILE/ENFILE）
- 域/批：C2/B22｜纯功能行数：340｜状态：｜判据：UNX-F9223-J1 三错误码注入矩阵判据过，坏 pipefd 指针全 EFAULT 格过，逐码与 Linux 一致
### UNX-F9224 · fd 表耗尽 EMFILE 矩阵（RLIMIT_NOFILE 联动）
- 域/批：C2/B22｜纯功能行数：360｜状态：｜判据：UNX-F9224-J1 限额触顶注入判据过，分配全 EMFILE 格过，限额上调后恢复正判据格过，与 B19 资源族联动格过
### UNX-F9225 · 系统级 ENFILE 矩阵（文件表上限）
- 域/批：C2/B22｜纯功能行数：320｜状态：｜判据：UNX-F9225-J1 系统级上限注入判据过，全 ENFILE 格过，与进程级 EMFILE 判序（先进程后系统）格过
### UNX-F9226 · ENOENT 判定域总纲（路径解析错误面）
- 域/批：C2/B22｜纯功能行数：360｜状态：｜判据：UNX-F9226-J1 路径解析错误面判据过，ENOENT 触发三态（末段缺失/中间段缺失/空组件）逐格落账，与 B1 VFS 冻结接口消费格过
### UNX-F9227 · open/stat/unlink ENOENT 矩阵
- 域/批：C2/B22｜纯功能行数：340｜状态：｜判据：UNX-F9227-J1 三 syscall 缺失路径注入矩阵判据过，全 ENOENT 逐格检出，O_CREAT 正判据（存在即打开）格过
### UNX-F9228 · 目录操作 ENOENT/ENOTDIR/EISDIR 矩阵
- 域/批：C2/B22｜纯功能行数：340｜状态：｜判据：UNX-F9228-J1 三错误码注入矩阵判据过，目录尾段缺失全 ENOENT 格过，文件作目录全 ENOTDIR 格过
### UNX-F9229 · 相对路径与 AT_FDCWD 边界矩阵
- 域/批：C2/B22｜纯功能行数：320｜状态：｜判据：UNX-F9229-J1 AT_FDCWD 正判据判据过，坏 dirfd 全 EBADF 格过，绝对路径忽略 dirfd 语义格过
### UNX-F9230 · dirfd 坏值矩阵（AT_EMPTY_PATH/AT_SYMLINK_FOLLOW）
- 域/批：C2/B22｜纯功能行数：320｜状态：｜判据：UNX-F9230-J1 两标志判据过，坏 dirfd 全 EBADF 格过，非目录 fd 全 ENOTDIR 格过
### UNX-F9231 · EBADF vs ENOENT 判定次序协议（先 fd 后路径）
- 域/批：C2/B22｜纯功能行数：340｜状态：｜判据：UNX-F9231-J1 判序协议判据过，坏 fd 优先返回 EBADF 逐格检出，判序与 Linux 逐例一致格过
### UNX-F9232 · fd 复用竞态注入（close-then-use TOCTOU）
- 域/批：C2/B22｜纯功能行数：340｜状态：｜判据：UNX-F9232-J1 close-then-use 竞态注入判据过，槽位复用窗口可观测格过，100 次注入零越权写格过
### UNX-F9233 · 错误码返回路径探针（负值编码与 err 链路）
- 域/批：C2/B22｜纯功能行数：320｜状态：｜判据：UNX-F9233-J1 负值编码探针判据过，-Errno 逐码编码一致格过，err 链路与 B13 编码器单源格过
### UNX-F9234 · fd 族 E 型参数探针总成
- 域/批：C2/B22｜纯功能行数：300｜状态：｜判据：UNX-F9234-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过
### UNX-F9235 · fd 族 E 型与 B08/B15 深化册对账格
- 域/批：C2/B22｜纯功能行数：300｜状态：｜判据：UNX-F9235-J1 对账判据过，与 dup 族/管道深化册逐条判据零冲突格过，正反分层声明格过
### UNX-F9236 · fd 族 E 型与 Windows INVALID_HANDLE_VALUE 对照格
- 域/批：C2/B22｜纯功能行数：300｜状态：｜判据：UNX-F9236-J1 INVALID_HANDLE_VALUE 映射判据过，WSA 错误差异账三条全列格过
### UNX-F9237 · E 型判据可复测性声明（10/10 次检出）
- 域/批：C2/B22｜纯功能行数：300｜状态：｜判据：UNX-F9237-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过
### UNX-F9238 · fd 族 E 型升级接管注记
- 域/批：C2/B22｜纯功能行数：280｜状态：｜判据：UNX-F9238-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过
### UNX-F9239 · fd 族 E 型与 glibc perror/strerror 消费核
- 域/批：C2/B22｜纯功能行数：320｜状态：｜判据：UNX-F9239-J1 strerror 逐码输出判据过，与 errno 透传单源格过，消费号登记格过
### UNX-F9240 · ktest syscall 面 B22 批断言集
- 域/批：C2/B22｜纯功能行数：460｜状态：｜判据：UNX-F9240-J1 本批 19 条判据聚合判据过，fd 族 E 型矩阵一次跑通，域累计 112,400 收口断言
