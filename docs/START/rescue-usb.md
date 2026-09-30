# U 盘自救援手册（救援 CLI 四命令 · 任务63）

> 适用：VARIX 便携部署（`Variable.exe` + `.portable` 标记 + `data\` 数据根）。
> 四命令 = `--export-rescue` / `--repair` / `--force-raster` / `--revoke-list`。
> 任务63 适配后：**容器/输出参数可全部省略**——自动发现数据根、输出写回 U 盘自身，
> 任意一台电脑插上 U 盘即可救援，不强依赖宿主。

## 何时用哪个命令（决策树）

```
U 盘出问题了？
│
├─ 黑屏 / 花屏 / 图形异常，但数据应该还在
│   └─ Variable.exe --force-raster
│      （写软件渲染标记，下次启动走软件渲染；确认恢复后可在设置页关闭）
│
├─ 疑似凭据泄露 / 公用机用毕
│   └─ Variable.exe --revoke-list
│      （导出紧急吊销清单 → 逐项登录吊销会话；清单落在 U 盘 rescue\）
│
├─ 容器打开失败 / 数据丢失 / 启动报容器损坏
│   ├─ 第一步永远是：Variable.exe --export-rescue [输出目录]
│   │   （只读容器、只写输出目录——救援路径绝不改写容器本体；
│   │    文件级失败自动降级 chunk 级，把能救的先救出来）
│   └─ 救出后若容器还能打开：Variable.exe --repair
│       （journal 重放 + checkpoint 固化，把已提交事务固化成新恢复点）
│
└─ 只是想确认 U 盘部署状态
    └─ Variable.exe --doctor
```

**顺序铁律：先 export-rescue（只读取证），再 repair（写容器）。**
export-rescue 永远不改容器；repair 会固化新 checkpoint——先取证再动手。

## 自动发现规则（任务63）

四命令的容器/输出缺省时按以下优先级自动发现：

1. `VARIABLE_DATA_ROOT` 环境变量（显式指定，最高优先）；
2. exe 便携根：exe 同级有 `.portable` 标记或 `VARIABLE_PORTABLE=1` → `<exe目录>\data`；
3. 全盘符扫描（A..Z）判据命中：
   - `X:\.portable`（根部署）→ `X:\data`
   - `X:\data\data.uxv`（数据盘形态）
   - `X:\Variable\.portable`（常规便携部署名）

输出缺省落 `<数据根父目录>\rescue\rescue-<时间戳>`（U 盘自救援：写回 U 盘自身，
不散落宿主）。显式给出参数时与旧版（B-33）行为完全一致——向后兼容。

## 命令速查

| 命令 | 参数（全部可选） | 行为 | 容器安全 |
|---|---|---|---|
| `--export-rescue` | `[容器] [输出] [口令]` | 文件级整树导出，失败自动降级 chunk 级裸流走查 | **只读** |
| `--repair` | `[容器] [口令]` | open（journal 重放已提交事务）+ seal（固化新 checkpoint） | 写（固化） |
| `--force-raster` | — | 写 `<数据根>\force-raster.flag`，下次启动软件渲染 | 不碰容器 |
| `--revoke-list` | `[输出]` | 导出吊销清单 Markdown（金库身份快照 → 平台吊销 URL） | 不碰容器 |

退出码：0 = 成功；1 = 执行失败；2 = 参数/发现失败（stderr 带诊断与候选列表）。

## 实测记录（2026-09-17/18，任务63 验收）

- U 盘：TU200Pro 1T（E:，exFAT，953.85GB），便携部署 `E:\Variable\`（2026-09-16 部署）。
- 新构建 exe（含自动发现）旁路部署 `E:\Variable\Variable-rescue63.exe`：
  - `--export-rescue`：自动发现容器 `E:\Variable\data\data.uxv`，输出缺省
    `E:\Variable\rescue\rescue-1789677217`，exit=0（空容器 0 文件如实报告）；
  - `--repair`：自动发现 + journal 重放固化，exit=0；
  - `--force-raster`：flag 写入 U 盘 `data\`，exit=0（验证后立即删除，无残留副作用）；
  - `--revoke-list`：清单导出 `E:\Variable\rescue\revocation-list.md`，exit=0；
  - `--doctor`：`data-dir: E:\Variable\data / writable: true / container: present`。
- 旧 exe（B-33 版）显式参数形态回归：`--export-rescue <容器> <输出>` exit=0（兼容）。
- 测试门禁：宿主 8 用例（"系统半损坏"三形态 ×3 + 自动发现 + 输出落盘）全绿；
  `cargo test -p variable --lib` 275 passed（267 基线 + 8 新增）。
- 验证后清理：`force-raster.flag`、`Variable-rescue63.exe` 已从 U 盘移除；
  `E:\Variable\rescue\` 目录保留（救援产物，用户可自取后删除）。

## 与 QEMU 先行强拔演练的关系（任务10）

- QEMU 先行（HMP `drive_del`）：介质即刻消失语义，三阶段 ×3 共 9 轮零 panic。
- 实机物理拔出（`_attic/usb-unplug-drill.py`）：U 盘直通 QEMU（卷设备只读），
  物理移除使宿主句柄失效，guest 走真实控制器/IO 错误路径——见
  `docs/acceptance/2026-09-17-任务10-强拔演练/` 实机补充记录。
- 只读介质实机观察（先导验证）：内核对 976GB U 盘 NVMe 枚举成功，全部写 IO
  收 `status=0x280` 错误后 **WARN + probe abort 优雅降级，零 panic**。
