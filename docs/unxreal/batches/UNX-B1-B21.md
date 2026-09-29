# UNX-B1-B21 · fcntl 文件描述符旗标（F4401–F4420 · 20 条）

> AI-06 承办｜域账累计：B01–B20 108,860 + 本批 5,320 = 114,180 / 240,000｜嫁接源：—（POSIX fcntl FD_CLOEXEC 语义参照，只跟随）｜防重声明：与 m700vfs.rs/storage.rs/fs/fs23_mount.rs/vfsguard.rs 为升级接管分层；与 B20 分层：前批立 dup 族，本批立 fcntl fd 旗标面（F_GETFD/F_SETFD/CLOEXEC）——正交分段｜批注：任务书样板 F4260"exec 后 CLOEXEC 全关账"判据成分在本批 F4403 承接兑现；fcntl 命令字全集在本批裁断（F4411——域支持清单外零接受，防泛化打法）。

### UNX-F4401 · F_GETFD/F_SETFD 基础语义
- 域/批：B1/B21｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4401-J1 GETFD 读得每 fd 旗标真值（1,000 次读-改-回读一致），SETFD 写位生效（位级 diff=0），两命令只作用于本 fd（邻 fd 零扰动 100 例）
### UNX-F4402 · FD_CLOEXEC 位语义
- 域/批：B1/B21｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4402-J1 CLOEXEC 置位后 exec 时该 fd 自动关闭（关闭时机==exec 成功时点——失败不关账 10 例），位语义钉死
### UNX-F4403 · exec 后 CLOEXEC 全关账
- 域/批：B1/B21｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4403-J1 exec 后携 CLOEXEC 位 fd 全关零残留（存活清单==非 CLOEXEC 清单 diff=0）——任务书样板 F4260"exec 后 CLOEXEC 全关账"承接兑现（C1 R-C1-001 同判例登记）
### UNX-F4404 · 非 CLOEXEC exec 存活账
- 域/批：B1/B21｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4404-J1 exec 后无 CLOEXEC fd 存活且可续用（读写续通 1,000 次），存活清单与预期 diff=0
### UNX-F4405 · CLOEXEC 标准库纪律锚
- 域/批：B1/B21｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4405-J1 popen/system 类隐式进程创建的 CLOEXEC 纪律锚位登记（C2/libc 消费注记——纪律声明账）
### UNX-F4406 · CLOEXEC 与 dup 交互口径
- 域/批：B1/B21｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4406-J1 dup/dup2/F_DUPFD 复制不携带源 fd CLOEXEC 位（新 fd 位==0——POSIX 口径 1,000 次零违背），dup3/O_CLOEXEC 例外账（F4385）
### UNX-F4407 · CLOEXEC 与 fork 交互口径
- 域/批：B1/B21｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4407-J1 fork 后子进程 fd 的 CLOEXEC 位保真复制（位级 diff=0——fork 不清位），exec 时点处置账
### UNX-F4408 · open O_CLOEXEC 原子置位
- 域/批：B1/B21｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4408-J1 open(path,O_CLOEXEC|...) 返回 fd 位==1（原子无窗——1,000 次），与 open+SETFD 两步法对照账
### UNX-F4409 · O_CLOEXEC 竞态消解账
- 域/批：B1/B21｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4409-J1 两步法（open+fcntl）竞态窗（他线程 exec 注入）vs 单步 O_CLOEXEC 零窗——竞态消解口径账（100 例对照全过）
### UNX-F4410 · F_SETFD 非法位注入
- 域/批：B1/B21｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4410-J1 未定义位掩码注入 SETFD 被裁断（fd 旗标位全集==FD_CLOEXEC 单位——域裁断账），越位写入拒绝 10/10
### UNX-F4411 · fcntl 命令字全集裁断
- 域/批：B1/B21｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4411-J1 域实现命令清单（F_GETFD/F_SETFD/F_DUPFD/F_DUPFD_CLOEXEC/F_GETFL/F_SETFL/F_GETLK/F_SETLK/F_SETLKW 九命令）外注入 EINVAL（10/10——防泛化裁断账）
### UNX-F4412 · fcntl 错误码矩阵
- 域/批：B1/B21｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4412-J1 四格矩阵（EBADF/EINVAL/EMFILE/EPERM）逐格注入断言全过，错误码透传零改写（F4155 口径）
### UNX-F4413 · CLOEXEC 统计账
- 域/批：B1/B21｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4413-J1 置位数（open/SETFD/dup3 三源）与 exec 关闭数守恒（万次零漂移），账可导出
### UNX-F4414 · CLOEXEC 压力回归
- 域/批：B1/B21｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4414-J1 万次 open-exec 循环风暴——关闭账零漂移零死锁（守恒残差=0），风暴后表 diff=0
### UNX-F4415 · C2 exec 联签锚
- 域/批：B1/B21｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4415-J1 CLOEXEC 处置→C1/C2 exec 路径消费联签锚位登记（F4256 契约方法学——锚位生命周期账）
### UNX-F4416 · F_GETFL/F_SETFL 入口锚
- 域/批：B1/B21｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4416-J1 文件状态旗标读改入口（GETFL 真值/SETFL 生效）锚位通（1,000 次），O_ 旗标语义展开归 B22——分工注记
### UNX-F4417 · 锁命令挂点锚
- 域/批：B1/B21｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4417-J1 F_GETLK/F_SETLK/F_SETLKW 三锁命令挂点登记（挂点签名冻结——实现深度随闸门补测，B26 联动预铺）
### UNX-F4418 · fcntl 命令分布统计账
- 域/批：B1/B21｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4418-J1 九命令调用分布计数与操作序一致（万次零漂移），账可导出
### UNX-F4419 · ktest vfs B21 面注册
- 域/批：B1/B21｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4419-J1 B21 断言面注册一次成功，B21 全部断言一次命令可跑，重复注册注入被拒绝
### UNX-F4420 · B21 批判据聚合断言集
- 域/批：B1/B21｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4420-J1 本批 19 条判据全部聚合入 ktest vfs B21 面，聚合报告 19/19 无缺项，通过率 100% 才算绿
