# UNX-C4-B06 · termios 与 ioctl 面：四组标志/TIOCGWINSZ/SIGWINCH（F10501–F10520 · 20 条）

> AI-14 承办｜域账累计：B01–B05 28,600 + 本批 5,600 = 34,200 / 240,000｜嫁接源：Linux termios(3)/tty_ioctl(2)/ioctl_tty(2)（注出处，禁凭记忆）｜防重：termios 状态面挂 pty（B05）与行规程（B07）之间——本批立"状态本体与 ioctl 面"，输入处理行为本体归 B07，逐条判据可辨；SIGWINCH 事件源在 B04（F10474），本批 F10511 为事件源的消费反侧（winsize 账本体）｜批注：原始模式（F10514）是判据主轴"vim 类全屏程序"的直通路径——B07 全屏录制基线与本批联签；F10520 联签 C2 挂号（termios ioctl 族号段核验）。

### UNX-F10501 · termios 结构体与四组标志
- 域/批：C4/B06｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10501-J1 结构体字段/位宽/枚举值与 termios(3) 注出定义逐字段一致，offsetof 断言全过，四组标志（iflag/oflag/cflag/lflag）位表落账
### UNX-F10502 · tcgetattr/tcsetattr 与三动作
- 域/批：C4/B06｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10502-J1 get/set 往返逐字段一致（100/100），TCSANOW/DRAIN/FLUSH 三动作时序语义正确（排空/冲刷探针 10/10 次）
### UNX-F10503 · 波特率面（存而不用声明）
- 域/批：C4/B06｜纯功能行数：220｜状态：[骨架]｜判据：UNX-F10503-J1 cfgetispeed/cfsetispeed 存取往返一致 100/100，"不调制实际传输"声明落账，非法速度码 BEINVAL
### UNX-F10504 · c_iflag 输入标志全位语义
- 域/批：C4/B06｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10504-J1 IGNBRK/BRKINT/INPCK/ISTRIP/IXON/IXANY/ICRNL/INLCR/IGNCR 九位逐一行为判据过（每位正反用例各 10 次）
### UNX-F10505 · c_oflag OPOST/ONLCR 输出处理
- 域/批：C4/B06｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10505-J1 OPOST 关=字节直通（零改写），OPOST|ONLCR 下 NL→CR-NL 逐字节可验（1 万行零错）
### UNX-F10506 · c_cflag 控制标志（无真实 UART 声明）
- 域/批：C4/B06｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10506-J1 CS8/CREAD/CLOCAL/PARENB 存取与位账一致，"无 UART 调制面"声明落账，位面供未来串口域消费
### UNX-F10507 · c_lflag 本地标志全位语义
- 域/批：C4/B06｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10507-J1 ICANON/ECHO/ISIG/TOSTOP/IEXTEN 位行为逐条判据过（正反用例各 10 次），位组合矩阵关键 8 格全对
### UNX-F10508 · VMIN/VTIME 四象限矩阵
- 域/批：C4/B06｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10508-J1 VMIN/VTIME 四象限（阻塞/轮询/限时/字节计数）行为与 termios(3) 注出表逐格一致，限时精度账落 ktest 时钟粒度
### UNX-F10509 · 特殊字符集 c_cc
- 域/批：C4/B06｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10509-J1 VINTR/VQUIT/VERASE/VEOF/VSUSP/VSTART/VSTOP/VEOL 默认值与注出表一致，重定义往返生效（10/10），_POSIX_VDISABLE 面可用
### UNX-F10510 · ioctl TIOCGETA/TIOCSETA 面
- 域/批：C4/B06｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10510-J1 两命令与 tcgetattr/tcsetattr 同源同判（同一实现双入口），uaccess 判界 EFAULT 面 10/10
### UNX-F10511 · TIOCGWINSZ/TIOCSWINSZ 与 SIGWINCH（事件源消费侧）
- 域/批：C4/B06｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10511-J1 winsize 写读往返一致（100/100），SET 同值零事件（防抖）、变值触发 F10474 事件源 10/10，全零 winsize 合法态
### UNX-F10512 · TIOCSCTTY/TIOCNOTTY（B08 联签）
- 域/批：C4/B06｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10512-J1 B06 阶段两命令挂分派但行为本体委托 B08（调用得 ENOTTY/预埋占位探针），B08 收口后清单引用本条
### UNX-F10513 · tcflush/tcdrain/tcflow
- 域/批：C4/B06｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10513-J1 TCIFLUSH/TCOFLUSH/TCIOFLUSH 三冲刷位行为可验（冲后读空/写位恢复 10/10），tcdrain 立即返回声明，tcflow 三动作落账
### UNX-F10514 · 原始模式路径与 vim 全屏前置（判据主轴）
- 域/批：C4/B06｜纯功能行数：360｜状态：[骨架]｜判据：UNX-F10514-J1 cfmakeraw 类原始态下字节零加工直通（控制符不产信号不回显），1 MB 二进制流往返逐字节一致，判据主轴场景账引用本条
### UNX-F10515 · termios 默认值与行规程默认策略
- 域/批：C4/B06｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10515-J1 新开 pty 默认 termios 与注出默认表逐字段一致（含 ICRNL/ICANON/ECHO/ISIG 置位、OPOST|ONLCR），B07 行规程按默认态起跑
### UNX-F10516 · exec 时 termios 保留语义
- 域/批：C4/B06｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10516-J1 exec 后 termios 逐字段保留（100/100 不复位），与信号面 exec 清空（F10475）形成"状态两面"对账
### UNX-F10517 · echo 控制位（ECHO/ECHOE/ECHOK/ECHONL）
- 域/批：C4/B06｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10517-J1 四位组合下回显行为逐格可验（回显字节序与位语义一致），ECHO 关=零回显（控制符含），与 B07 回显管线联签
### UNX-F10518 · 非阻塞与 termios 交错
- 域/批：C4/B06｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10518-J1 O_NONBLOCK 与 VMIN/VTIME 同 fd 交错时行为优先级明确（NONBLOCK 压倒限时面），交错序列 100 轮零矛盾
### UNX-F10519 · termios 错误码面
- 域/批：C4/B06｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10519-J1 ENOTTY（非 tty fd）/EINVAL（坏旗标/坏参数）/EFAULT（坏指针）三错误逐条可触发（各 10 次），错误码走 errno.rs 单点
### UNX-F10520 · B06 批压测与 C2 挂号联签
- 域/批：C4/B06｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10520-J1 termios 全位翻转回归（位面 × 动作 × 特殊字符组合 500 态）全过，termios ioctl 族挂号登记项落 table.rs 可查
