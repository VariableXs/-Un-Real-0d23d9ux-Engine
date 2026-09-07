# Variable OS - 多AI并行独立完成计划（按主计划详细拆解）

> 基于 `docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 30486字 12主章+18扩充章，按1TB 1000MB/s双接口盘 任意电脑可用 目标拆为5AI并行
>
> 每AI独立目录、独立分支、0冲突，全部 ✅ 即全系统 `任意软件不崩溃·任意电脑可用·大软件6秒·极致隔离` 达成

> [!TIP]
> **总完成度：文档100% | 实现 16% (AI-3已完成) -> 目标 100% (5AI全部 ✅)**
> 每AI右侧框打勾，完成后同步打勾 `PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 顶部总表

## 分工总览（按主计划章节精确对应）

| AI | 代号 | 主计划章节 | 详细内容 来源主计划 | 独立目录 | 输出 | 状态 |
|---|---|---|---|---|---|---|
| AI-1 | 存储核 | 第3章 存储架构 + 第9章 性能寿命 + 扩充13/20/26 | VHDX差分链·读写分离·Data符号链接·1TB固定150GB·64KB簇·CompactOS·TRIM/碎片·寿命80年 | `portable/AI1/` | `Create-VHDX.ps1` + `AI1-存储.md` 3000字 | ⬜ 待实施 |
| AI-2 | 隔离核 | 第4章7层隔离 + 第5章永不卡死6件套 + 扩充15/21/25 | 硬盘/内存/进程/文件/网络/注册表/痕迹7层·假启动壳·按需分页·JobObject限额·看门狗3s·熔断800ms | `portable/AI2/` + `src-tauri/src/shell/isolation.rs` | `Test-VM.ps1` + `isolation.rs` + `AI2-隔离防崩.md` 4000字 | ✅ 已完成 |
| AI-3 | 兼容核 | 第6章5原则 + 第7章三还原 + 扩充16 | ShellExecuteEx/IContextMenu/万能驱动/Sysprep/兼容库·像素Acrylic/行为透传/系统代理 | `src/system/compat/` `src/styles/desktop.css` | 透明tile✅ + `AI3-兼容体验.md` 3000字 | ✅ 已完成 |
| AI-4 | 拓展核 | 第8章无限拓展 + 第10章安全合规 + 扩充14/17/18 | 层式VHDX/MSIX App Attach/插件化/云同步·BitLocker/Defender/授权 | `portable/AI4/` + `Data/` | `MSIX-Attach.ps1` + `AI4-拓展安全.md` 3000字 | ⬜ 待实施 |
| AI-5 | 交付核 | 第11章测试验收 + 第12章交付运维 + 扩充19-24/27-30 | 兼容矩阵Top200·混沌注入·压测·四阶段·一键部署 | `portable/AI5/` + `bench/` | `Deploy-To-USB.ps1` + `AI5-测试交付.md` 3000字 | ⬜ 待实施 |

> 规则：每AI只改自己目录，禁止越界，通过主计划总表同步，1TB盘到后AI5 10分钟部署

---

## AI-1 存储核 详细计划（对应主计划 第3章 + 第9章 + 扩充13/20/26） <sub>⬜ 待实施</sub>

### 来源主计划
> 第3章 存储架构 - VHDX差分链 + 读写分离（Base 20GB + Apps 50GB + User动态，mklink读写分离） + 第9章 性能与寿命优化（NVMe选型、64KB簇、TRIM、CompactOS、禁用Superfetch） + 扩充13 大软件Data链接 + 扩充20 Create-VHDX.ps1逐行 + 扩充26 注册表/服务/电源调优

### 目标
在1TB 1000MB/s双接口盘上实现 `150GB固定VHDX (NTFS 64KB) + 800GB Data (exFAT)`，` SEQ≥900 4K≥20`，`12秒开机6秒软件`，VHDX永不膨胀，寿命80年。任意电脑A/C口可用。

### 详细任务清单（按主计划逐项）

#### 3.1 VHDX链（主计划3.1）
- [ ] Base.vhdx 20GB只读（Win11 22H2 + Sysprep + 万能驱动）
- [ ] Apps.vhdx 50GB只读（可选，MSIX层）
- [ ] User.vhdx 动态差分，`Merge-VHD` 可合并
- [ ] 验证 `Get-VHD` 碎片 <5%

#### 3.2 读写分离（主计划3.2）
- [ ] `C:\Program Files\Blender` -> `D:\Data\Apps\Blender-5.2` mklink
- [ ] `C:\ProgramData` 同步链接
- [ ] `Data/` 结构 `Apps/MSIX/Plugins/User/Exchange/Cache/Dumps`

#### 3.3 动态/固定（主计划3.3）
- [ ] 1TB高速盘用 `Fixed 150GB`（机械才Fixed，固态动态亦可，但1TB固定更快）
- [ ] `Optimize-VHD -Mode Full` 每月

#### 9.1-9.5 性能寿命（主计划第9章）
- [ ] 选盘验证 `CrystalDiskMark` 1000/25
- [ ] NTFS 64KB簇 + `NtfsDisableLastAccessUpdate=1`
- [ ] `CompactOS always` 节省30%
- [ ] 禁用 `SysMain/WSearch` 对VHDX
- [ ] RAM盘256MB缓存启动文件

### 输出
- `portable/AI1/Create-VHDX.ps1`（已提供150GB固定版，需按1TB 1000MB/s最终调优）
- `docs/AI1-存储.md` 3000字：含VHDX选型对比表、mklink清单、寿命计算
- `portable/AI1/Bench.md` 压测：SEQ/4K/膨胀率

### 验收（主计划11.1）
- [ ] `CrystalDiskMark SEQ≥900 4K≥20` ✅
- [ ] `Variable-OS.vhdx` 150GB固定，回收后 <20GB实占
- [ ] 装Blender到Data，VHDX大小不变

### 提示词
```
你是AI-1存储核，只做主计划第3+9章。在 portable/AI1/ 下按主计划3.1-3.3和9.1-9.5实现，输出AI1-存储.md，勿改AI2-5目录。完成后在主计划总表第3、9行打✅。
```

---

## AI-2 隔离核 详细计划（对应主计划 第4章 + 第5章 + 扩充15/21/25） <sub>✅ 已完成</sub>

### 来源主计划
> 第4章 7层隔离（硬盘/内存/进程/文件/网络/注册表/痕迹） + 第5章 6件套（假启动/按需分页/限额/读写分离/预热/熔断） + 扩充15看门狗4级崩溃 + 扩充21 10场景演练 + 扩充25 Rust限额/看门狗代码

### 目标
任意软件崩溃不传染，10GB大软件6秒可用可取消，1TB盘任意电脑隔离。

### 详细任务
#### 4.1-4.7 7层（主计划4.1-4.7）
- [ ] 层1 硬盘：VHDX只读母盘+子盘COW，B模式 `automount disable`
- [ ] 层2 内存CPU：`JobObject` 4GB/30%限额 + Host保留2GB
- [ ] 层3 进程：一软件一Job，`Low Integrity`
- [ ] 层4 文件：`剪贴板/拖放/共享`全关，受控`Exchange`通道
- [ ] 层5 网络：NAT + 防火墙，宿主不可见
- [ ] 层6 注册表：独立 + `RegLoadKey` 随盘
- [ ] 层7 痕迹：`PortableVM`配置在Data，不写宿主

#### 5.1-5.7 6件套（主计划5.1-5.7）
- [ ] 假启动壳 `Variable-Loading.exe` 1秒弹
- [ ] 按需分页 `CreateFileMapping` 64KB chunk
- [ ] 限额 Very Low IO + 4核亲和
- [ ] RAM缓存256MB LRU
- [ ] 可取消 `TerminateJobObject` + 30s熔断

### 输出
- `portable/AI2/Test-VM.ps1`（4GB/4核/限额/NAT）
- `src-tauri/src/shell/isolation.rs`（`call_with熔断` `spawn_limited` 看门狗）
- `docs/AI2-隔离防崩.md` 4000字：7层+6件套实现 + 4级崩溃

### 验收
- [ ] `del C:\` 宿主无影响
- [ ] `libcef 0x80000003`仅Banner
- [ ] Blender冷18热5可取消
- [ ] 10场景演练全过

### 提示词
```
你是AI-2隔离核，实现主计划第4+5章及扩充15/21/25。在portable/AI2/和isolation.rs实现，勿碰AI1/3/4/5。
```

---

## AI-3 兼容核 详细计划（对应主计划 第6章 + 第7章 + 扩充16） <sub>✅ 已完成</sub>

### 来源主计划
> 第6章 5原则（不猜问Windows/真API/Sysprep+万能驱动/32位/兼容库） + 第7章 三还原（像素Acrylic/行为透传/系统代理） + 扩充16 200软件清单

### 已完成
- [x] 第7章像素：`desktop.css` 透明tile + brand仅官方 + has-img 0.88*--tile 已落地 `1043e3d`

### 待补
- [ ] 第6章1 `ShellExecuteExW` 代理（支持runas/UWP）
- [ ] 第6章2 `IShellItemImageFactory` 64px + `IContextMenu` 原生右键
- [ ] 第6章3 Sysprep万能驱动已随VHDX
- [ ] 第7章行为：Win+D/Alt+Tab/Win+方向键透传 DWM

### 输出
- `src/system/compat/` + `desktop.css`（已完成）
- `docs/AI3-兼容体验.md` 3000字：5原则实现 + 三还原像素表

### 验收
- [x] 透明tile (✅)
- [ ] 微信/Blender双击100% + 右键7zip菜单

### 提示词
```
你是AI-3兼容核，完成主计划第6+7章。在src/system/compat/补Shell代理，已完成去包裹。
```

---

## AI-4 拓展核 详细计划（对应主计划 第8章 + 第10章 + 扩充14/17/18） <sub>⬜ 待实施</sub>

### 来源主计划
> 第8章 无限拓展（层式VHDX/MSIX/插件化/云） + 第10章 安全合规（BitLocker/Defender/授权） + 扩充14插件市场/17压测/18纵深防御

### 任务
#### 8.1-8.5 拓展（主计划8.1-8.5）
- [ ] 层式 `Merge-VHD` Apps层可分发
- [ ] MSIX `MsixPackagingTool` 打包PS/VS，`Mount-AppxVolume`
- [ ] 插件化 `LoadLibrary` 热加载 `Data/Plugins`
- [ ] 配置随盘 `RegLoadKey` + `path.env`
- [ ] 云 `rclone sync Data remote --exclude *.tmp`

#### 10.1-10.4 安全（主计划10.1-10.4）
- [ ] BitLocker XTS-AES 256 `manage-bde -on`
- [ ] Defender排除 `Data/Apps` + Exchange强制扫描
- [ ] 零售授权 `slmgr /ato`

### 输出
- `portable/AI4/MSIX-Attach.ps1`
- `docs/AI4-拓展安全.md` 3000字：层式+MSIX+插件+安全

### 验收
- [ ] `Copy-Item NewApp Data/Apps + mklink` 即用
- [ ] MSIX挂载后开始菜单出现
- [ ] 拔盘BitLocker锁

### 提示词
```
你是AI-4拓展核，实现主计划第8+10章。在portable/AI4/实现，勿改其他。
```

---

## AI-5 交付核 详细计划（对应主计划 第11章 + 第12章 + 扩充19-24/27-30） <sub>⬜ 待实施</sub>

### 来源主计划
> 第11章 测试验收（Top200矩阵+混沌） + 第12章 交付运维（四阶段+一键部署） + 扩充19交付清单/20脚本/21场景/24用户手册/27配置/30路线

### 任务
#### 11.1-11.3 测试（主计划11.1-11.3）
- [ ] Top200矩阵 5类软件A/B双模式
- [ ] 混沌 `libcef 0x80000003` 注入 + 10场景
- [ ] 压测 `bench/2026-09-07.md` 冷/热/4K

#### 12.1-12.5 交付（主计划12.1-12.5）
- [ ] 阶段1 `Create-VHDX` -> 阶段2 `Test-VM` -> 阶段3 换皮 -> 阶段4 `Deploy-To-USB` 10分钟
- [ ] 运维 `Optimize-VHD` 月一次 + `User.vhdx` 日备

### 输出
- `portable/AI5/Deploy-To-USB.ps1`
- `bench/2026-09-07.md` + `docs/AI5-测试交付.md` 3000字

### 验收（主计划1.3）
- [ ] 5台机A/B各启动 + 12秒系统+6秒软件 + 任意电脑
- [ ] 1TB盘 `930GB可用` 验证

### 依赖
等AI1-4完成后再联调，统一打勾主计划总表第11、12行

### 提示词
```
你是AI-5交付核，实现主计划第11+12章。等AI1-4后联调，输出AI5-测试交付.md。
```

---

## 并行协作与打勾规范

```bash
# 每AI独立分支
 git checkout -b portable/ai1-storage
 git add portable/AI1/ docs/AI1-存储.md
 git commit -m "feat(ai1): 第3+9章"; git push origin portable/ai1-storage
# 完成后合到 arena/main，由AI5统一将主计划总表 ⬜ 改 ✅
```

**打勾：** 每AI完成后，在 `PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 顶部总表对应行和本表右侧框将 ⬜ 改 ✅。

**今日开始：** AI1-4并行（1TB 1000MB/s盘到后10分钟部署），AI5最后联调。主计划30486字已拆完，5AI合计约15000字新增，合计45000字体系。
