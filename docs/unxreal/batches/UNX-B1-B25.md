# UNX-B1-B25 · renameat2 变体与 link 纪律（F4481–F4500 · 20 条）

> AI-06 承办｜域账累计：B01–B24 130,680 + 本批 5,460 = 136,140 / 240,000｜嫁接源：—（POSIX link/unlink/Linux renameat2 语义参照，只跟随）｜防重声明：与 m700vfs.rs/storage.rs/fs/fs23_mount.rs/vfsguard.rs 为升级接管分层；与 B24 分层：前批立 rename 主体语义，本批立 renameat2 变体与 link/unlink 硬链接纪律——正交分段｜批注：任务书专题五"link/linkat 硬链接计数一致性"在本批全量落位。

### UNX-F4481 · rename 目录整体移动语义
- 域/批：B1/B25｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4481-J1 目录 rename 子树随移（移动后子树全部内容经新路径可达——1,000 项逐项 diff=0），旧路径全数 ENOENT
### UNX-F4482 · rename 自身后代复验
- 域/批：B1/B25｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4482-J1 移动目录到自身后代注入 EINVAL/ELOOP 复验（F4467 口径——多级后代 10/10），树零损伤
### UNX-F4483 · renameat 相对路径语义
- 域/批：B1/B25｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4483-J1 renameat(AT_FDCWD,…) 等价 rename（等价账 1,000 次），目录 fd 基准相对路径解析账（10/10——at 语义）
### UNX-F4484 · RENAME_NOREPLACE 原子无窗
- 域/批：B1/B25｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4484-J1 renameat2 NOREPLACE 存在注入 EEXIST（万次原子无窗——并发创建竞态零双写），无旗标回退账
### UNX-F4485 · RENAME_EXCHANGE 交换语义
- 域/批：B1/B25｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4485-J1 EXCHANGE 双路径原子互换（互换后双向内容 diff=0——1,000 次），互换原子性并发观测零中间态（10,000 次）
### UNX-F4486 · RENAME_WHITEOUT 覆盖层锚
- 域/批：B1/B25｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4486-J1 WHITEOUT 语义锚位登记（overlay 类覆盖层预留——随闸门补测，B2 联签注记），非法组合注入 EINVAL
### UNX-F4487 · link 基础语义
- 域/批：B1/B25｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4487-J1 link 后双名同 inode（双路径读同内容——1,000 次），nlink+1 账（守恒）
### UNX-F4488 · linkat 跟随语义口径
- 域/批：B1/B25｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4488-J1 linkat AT_SYMLINK_FOLLOW 跟随/不跟随双口径账（10/10 双例），AT_FDCWD 等价账
### UNX-F4489 · link 跨目录语义
- 域/批：B1/B25｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4489-J1 跨目录硬链建立（不同父目录 nlink 各自账），双目录项同 inode 映射核对（1,000 次）
### UNX-F4490 · link 目录禁令
- 域/批：B1/B25｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4490-J1 目录硬链注入 EPERM（10/10——POSIX 禁令），普通文件零误伤
### UNX-F4491 · unlink 语义
- 域/批：B1/B25｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4491-J1 unlink 后名消失（ENOENT）且 nlink−1（守恒账），nlink 归零时 inode 回收（回收账零残留）
### UNX-F4492 · unlink 打开文件存活语义
- 域/批：B1/B25｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4492-J1 unlink 后已开 fd 内容存活至关闭（读写续通 1,000 次——POSIX 语义），全 fd 关闭后存储回收账
### UNX-F4493 · unlink 目录注入分工格
- 域/批：B1/B25｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4493-J1 unlink 目录注入 EISDIR（10/10——rmdir 分工），rmdir 空目录成功账（10/10）
### UNX-F4494 · rmdir 非空注入
- 域/批：B1/B25｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4494-J1 rmdir 非空目录注入 ENOTEMPTY（10/10），拒绝后状态零变更
### UNX-F4495 · nlink 万次守恒账
- 域/批：B1/B25｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4495-J1 link/unlink 万次操作 nlink 零漂移（守恒账）——任务书"link/linkat 硬链接计数一致性"兑现
### UNX-F4496 · link 族错误码矩阵
- 域/批：B1/B25｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F4496-J1 六格矩阵（EEXIST 目标存在/ENOENT/EPERM 目录链/EMLINK 超限/EXDEV 跨载/ENOBUFS 注记）逐格注入断言全过
### UNX-F4497 · EMLINK 上限语义
- 域/批：B1/B25｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F4497-J1 nlink 达上限再 link 注入 EMLINK（10/10——限值账），上限值与 fs 报告值一致（透传账）
### UNX-F4498 · link-rename-unlink 组合回归
- 域/批：B1/B25｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4498-J1 链-改-删混合序列回归（树一致性 F4234 全量核对+nlink 双账守恒——1,000 组序列）
### UNX-F4499 · ktest vfs B25 面注册
- 域/批：B1/B25｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F4499-J1 B25 断言面注册一次成功，B25 全部断言一次命令可跑，重复注册注入被拒绝
### UNX-F4500 · B25 批判据聚合断言集
- 域/批：B1/B25｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F4500-J1 本批 19 条判据全部聚合入 ktest vfs B25 面，聚合报告 19/19 无缺项，通过率 100% 才算绿
