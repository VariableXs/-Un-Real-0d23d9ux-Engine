# Variable OS 术语与 FAQ — 主计划扩充 24 + 扩充 29

> 归属：`portable/AI5/`（AI-5 交付核）。答复口径与主计划一致，涉及未实测的项会直说「未实测」。

## 术语表（扩充 24 / 附录 D）

| 术语 | 含义 |
|---|---|
| **VHDX** | 微软虚拟硬盘格式，支持动态、差分、TRIM。本项目用 `Base`(只读) + `Apps`(共享) + `User`(差分) 三层 |
| **差分盘 (Differencing)** | 只记录相对母盘的改动。写时复制、读时合并，丢弃即还原 |
| **Sysprep** | 系统准备工具，抹掉硬件相关信息，让同一镜像能上不同电脑 |
| **JobObject** | Windows 作业对象，用于限制内存/CPU/进程数，是隔离层的执行手段 |
| **MSIX App Attach** | 按需挂载应用包，不写注册表、不污染 C 盘、删文件即卸载 |
| **Ventoy** | 多启动U盘工具，可直接引导 VHDX |
| **A 模式 / B 模式** | A = 在宿主里跑虚拟机（隔离强、反作弊受限）；B = U盘原生启动（性能与兼容性最好） |
| **摆渡通道 (Exchange)** | `Data\Exchange`，虚拟系统与宿主之间**唯一**的受控文件通道，强制杀软扫描 |
| **熔断** | IPC 超时 800ms 即断开并回退默认值，避免一个慢调用卡死整个桌面 |
| **看门狗** | 周期巡检 Worker，死了就在 3s 内重启并提示「已恢复」 |

## FAQ

**Q：U盘要多大？**
A：1TB 固态U盘。Base 20GB + Apps 50GB + User 动态 + Data 约 900GB，实测能装 50 个大软件。

**Q：会不会把宿主电脑搞坏？**
A：不会。三重保护：① 便携系统在 VHDX 里，写操作落在差分盘；② 目标盘安全闸会拒绝把宿主系统盘当部署目标（`Test-Ai5UsbTarget`）；③ 上盘用 `robocopy /E` 而不是 `/MIR`，不删目标盘已有文件。宿主 MBR/GPT 全程不改（主计划 10.4）。

**Q：Mac 能用吗？**
A：Intel Mac 可走 B 模式；Apple Silicon 只能用 QEMU 慢速跑，仅应急，不是设计目标。

**Q：游戏能玩吗？**
A：单机与网游在 B 模式（原生启动 + 显卡直通）可玩。A 模式下反作弊会检测到虚拟化驱动 —— 矩阵里英雄联盟/无畏契约/CS2/PUBG/永劫无间/穿越火线/WeGame 已标 `aMode=warn`，脚本会提示切 B 模式（混沌场景 S06）。

**Q：U盘丢了怎么办？**
A：BitLocker XTS-AES256 加密（AI-4 `Security-Manager.ps1`）+ 云同步（AI-4 `Cloud-Sync.ps1`）。新盘 `rclone copy` + VHDX 合并，约 10 分钟恢复。**恢复密钥必须离线另存**，留在盘里等于没加密。

**Q：为什么 Data 分区用 exFAT 而不是 NTFS？**（扩充 29.1）
A：Data 要跨 Win/Mac/Linux 且无日志写放大，exFAT 合适；VHDX 所在卷用 NTFS，才能支持稀疏文件与权限。两者分开，各取所长。

**Q：为什么不用 Docker / WSL2？**（扩充 29.2 / 29.3）
A：Docker 共享宿主内核，跑不了 Windows GUI 与驱动；WSL2 是 Linux 内核，跑不了 exe。VHDX 里是完整 Windows 内核，兼容性 100%。

**Q：4K 对齐为什么重要？**（扩充 29.4）
A：分区起始偏移必须是 4096 的整数倍，否则 4K 随机读写会掉约 50%。`Chaos-Inject.ps1 -Action Run -Scenario S09` 会逐分区检查 `StartingOffset % 4096`。

**Q：兼容矩阵里为什么大多是 `todo`？**
A：因为 `pass` 必须来自实测。当前只有主计划 11.1 / 16.2 明确记录过的 14 条带既有结论，其余 186 条等真机跑 `Compat-Matrix.ps1 -Action Run` 后回填。**不会因为软件有名就打 ✅。**

**Q：混沌测试会不会把我的电脑搞崩？**
A：不会。12 个场景里 6 个标 `manual`（拔盘、宿主蓝屏、虚拟机内删 C 盘、驱动回退等），脚本**只出步骤卡，绝不代为执行**；另外 6 个 `auto` 场景要么只读（S06/S08/S09），要么只操作脚本自己启动的一次性子进程（S11/S12），要么默认 dry-run 且真写盘需 `-AllowFill` 并受 512MB 上限保护（S05）。

**Q：性能数字能直接对标 CrystalDiskMark 吗？**
A：不能。`Bench-Perf.ps1` 用 .NET `FileStream` 用户态读写，只能横向对比同机器不同盘/簇大小，报告里会写明口径。要裸盘指标请在真机上跑 CrystalDiskMark 并把结果附到 `Data/Tests/`。

**Q：脚本能在非 Windows 上跑吗？**
A：不能。全套脚本依赖 Hyper-V、BitLocker、`robocopy`、`reg`、`manage-bde` 等 Windows 组件。在 Linux 沙箱里最多能做结构校验，语法与运行时验证必须在 Windows 上执行 `portable/tests/Run-PortableTests.ps1`。
