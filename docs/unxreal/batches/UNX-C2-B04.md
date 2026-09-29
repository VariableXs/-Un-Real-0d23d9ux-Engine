# UNX-C2-B04 · 进程族地基（F8861–F8880 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B03 14,970 + 本批 4,570 = 19,540 / 240,000｜嫁接源：Linux man-pages（fork/clone/execve/wait 章节）只跟随｜防重：进程对象与装载本体归 C1 域（AI-11）防重，本域为 syscall 号面语义档与错误矩阵——对现存 `syscall::calls::WaitQueue`/`ExitStatus`、`F0039` 建模层为升级接管扩容；C1 未 finalize，跨域依赖逐条记入风险栏（§6.2 规则三）｜判据与 deepen/C2-B04.md 逐条同名同判据同 ID

### UNX-F8861 · fork 语义档与 COW 继承（A4 消费）
- 域/批：C2/B04｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F8861-J1 fork 一次调用双返回（父得子 pid/子得 0）判据过，COW 语义依赖 A4 未收口已记风险，现存最小进程建模路径判据先行全过
### UNX-F8862 · vfork 挂点与语义约束
- 域/批：C2/B04｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8862-J1 vfork 挂点注册成功，挂起父进程语义约束（子不得返回）文档化判据过，未实现档显式返 ENOSYS 不假装
### UNX-F8863 · clone/clone3 标志位面（CLONE_* 预留）
- 域/批：C2/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8863-J1 CLONE_VM/FS/FILES/SIGHAND 四标志组合矩阵 8 格挂点注册，未支持组合返 EINVAL 不静默，clone3 结构版本字段校验判据过
### UNX-F8864 · execve 装载与参数环境块
- 域/批：C2/B04｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F8864-J1 argv/envp 计数与内容逐项传递正确（探针读回对照），E2BIG 超限返错判据过，成功不返回语义（后续代码不可达）有账
### UNX-F8865 · execveat 与空路径语义
- 域/批：C2/B04｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8865-J1 dirfd + 相对路径解析正确，AT_EMPTY_PATH 特值语义挂点注册，坏 flags 返 EINVAL
### UNX-F8866 · exit/exit_group 双档退出
- 域/批：C2/B04｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8866-J1 exit 单线程退出与 exit_group 全组退出判据各 100 次正确，atexit 类清理挂点执行序有账，退出码 0–255 截断与 Linux 一致
### UNX-F8867 · wait4 回收语义与状态编码
- 域/批：C2/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8867-J1 WNOHANG 非阻塞轮询、阻塞等待、WUNTRACED 三档判据过，状态字 WEXITSTATUS/WIFEXITED 编码与 Linux 逐位一致
### UNX-F8868 · waitid 扩展等待（WEXITED/WSTOPPED/WCONTINUED）
- 域/批：C2/B04｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8868-J1 idtype 四档（P_ALL/P_PID/P_PGID/P_PIDFD 挂点）解析正确，WNOHANG 下 si_pid=0 约定判据过
### UNX-F8869 · waitpid 兼容挂点
- 域/批：C2/B04｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F8869-J1 waitpid 与 wait4 同语义转接判据 100 次一致，负 pid（进程组等待）语义挂点正确
### UNX-F8870 · getpid/getppid 双人格路由（C1 挂点）
- 域/批：C2/B04｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8870-J1 getpid 路由到本人格账本判据过（C1 人格标签挂点消费方验证），execve 后 pid 不变判据过
### UNX-F8871 · getpgrp 进程组查询
- 域/批：C2/B04｜纯功能行数：150｜状态：[已深化]｜判据：UNX-F8871-J1 getpgrp 与 getpgid(0) 等价判据过，fork 后组继承判据过
### UNX-F8872 · 孤儿进程收养与 init 语义
- 域/批：C2/B04｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8872-J1 父进程先退后子进程被收养判据过，收养目标 ppid 变更可观测，收养后可 wait 回收
### UNX-F8873 · 僵尸态生命周期与回收协议
- 域/批：C2/B04｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8873-J1 退出后进僵尸态、wait 后回收判据过，无人 wait 的僵尸驻留有账（资源占用可观测），防泄漏上限告警判据过
### UNX-F8874 · 退出码传递与 WIFEXITED 解码
- 域/批：C2/B04｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8874-J1 256 档退出码全档传递正确，被信号终止（WIFSIGNALED）挂点解码判据过
### UNX-F8875 · execve 后 fd 关闭语义（O_CLOEXEC）
- 域/批：C2/B04｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8875-J1 带 O_CLOEXEC 的 fd 在 exec 后关闭判据过，无标志 fd 存续判据过，关闭清单可导出
### UNX-F8876 · ELF interpreter 装载挂点（C1 联签）
- 域/批：C2/B04｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8876-J1 PT_INTERP 挂点注册与 C1 联签就位，解释器缺失返 ENOENT 判据过，装载本体归 C1 防重声明
### UNX-F8877 · 进程族错误矩阵（EAGAIN/EINVAL/EFAULT/EPERM）
- 域/批：C2/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8877-J1 四错误码 × fork/execve/wait4/getpid 注入矩阵 16 格逐格与 Linux 逐码一致
### UNX-F8878 · 进程数限额与 EAGAIN（RLIMIT_NPROC 挂点）
- 域/批：C2/B04｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8878-J1 超 RLIMIT_NPROC fork 返 EAGAIN 判据过，限额值读写与 B07 资源族联动判据过
### UNX-F8879 · 进程族双人格并存判据（C1 联测）
- 域/批：C2/B04｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8879-J1 POSIX 进程与 NT 语义进程并存互不干扰判据（C1 交付后复测）登记随闸门补测，号面路由分流预演判据先行全过
### UNX-F8880 · ktest syscall 面 B04 批断言集
- 域/批：C2/B04｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F8880-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，进程族断言与既有 CheckSet 命名空间隔离
