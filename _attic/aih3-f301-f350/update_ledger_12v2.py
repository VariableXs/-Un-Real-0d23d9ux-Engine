# -*- coding: utf-8 -*-
"""批次十二账本更新 v2（UTF-8 全程 + 按行号修表头）。"""
import io

p = "_attic/aih3-f301-f350/行数对账与缺陷账本.md"
lines = io.open(p, encoding="utf-8").read().splitlines(keepends=True)

# 1. 修表头三行（按行首标签匹配，任何编码状态下按行号+标签双保险）
for i, l in enumerate(lines):
    if l.startswith("| ") and ("验证基线" in l or "楠岃瘉鍩虹嚎" in l):
        lines[i] = ("| 验证基线 | **421 通过 / 0 失败 / 0 警告**（域内单测实数 = cargo test 424 − 3 舱内探针；"
                    "隔离舱 `_attic/aih3-f301-f350/isolation/` 经 `#[path]` 直挂实测，深化批次十二收口复核） |\n")
    if l.startswith("| ") and ("源码规模" in l or "婧愮爜瑙勬ā" in l):
        lines[i] = ("| 源码规模 | 总 **30,659** 行；纯功能 **18,835** 行（口径：总行 − 空行 − 注释行 − 测试区行；"
                    "其中 50 项 18,201 + hbase 531 + 域聚合器 103。**计数器 v2 程序化实数**（字符串感知——见 D-30），"
                    "脚本随仓 `_attic/aih3-f301-f350/count_pure_v2.py`） |\n")
    if l.startswith("| ") and ("域内单元测试" in l or "鍩熷唴鍗曞厓娴嬭瘯" in l):
        lines[i] = ("| 域内单元测试 | **421 个**（cargo test 424 − 3 舱内探针） | 宿主 cargo test 直跑（注入钟确定复现）；"
                    "批次十一收口 408 → 本批 +13 |\n")

s = "".join(lines)

# 2. 逐项行数（锚点直接取自 grep 实际行文本）
pairs = [
    ("| F310 关闭前保存三问 | `saveask` | 910 | 398 | 44% |",
     "| F310 关闭前保存三问 | `saveask` | 910 | 454 | 50% |"),
    ("| F311 会话自动恢复 | `sesrestore` | 1040 | 319 | 31% |",
     "| F311 会话自动恢复 | `sesrestore` | 1040 | 362 | 35% |"),
    ("| F322 麦克风隐私指示 | `micind` | 520 | 113 | 22% |",
     "| F322 麦克风隐私指示 | `micind` | 520 | 390 | 75% |"),
    ("| F331 动画降级不卡逻辑 | `animdegrade` | 780 | 487 | 62% |",
     "| F331 动画降级不卡逻辑 | `animdegrade` | 780 | 508 | 65% |"),
    ("| F332 帧率自适应 | `animdegrade` | 6175 | 487 | 8% |",
     "| F332 帧率自适应 | `animdegrade` | 6175 | 508 | 8% |"),
    ("| F333 低电量视觉模式 | `animdegrade` | 910 | 486 | 53% |",
     "| F333 低电量视觉模式 | `animdegrade` | 910 | 507 | 56% |"),
    ("| F336 复制文件地址 | `copypath` | 715 | 775 | 108% |",
     "| F336 复制文件地址 | `copypath` | 715 | 829 | 116% |"),
    ("| F337 命令行与图形互通 | `copypath` | 2860 | 774 | 27% |",
     "| F337 命令行与图形互通 | `copypath` | 2860 | 829 | 29% |"),
    ("| F342 磁盘检查与修复 | `sysgov` | 585 | 506 | 86% |",
     "| F342 磁盘检查与修复 | `sysgov` | 585 | 524 | 90% |"),
    ("| F343 内存不足出路 | `sysgov` | 1170 | 506 | 43% |",
     "| F343 内存不足出路 | `sysgov` | 1170 | 524 | 45% |"),
    ("| F344 vxapp 卸载三步 | `sysgov` | 3640 | 505 | 14% |",
     "| F344 vxapp 卸载三步 | `sysgov` | 3640 | 523 | 14% |"),
    ("| F345 默认应用矩阵 | `sysgov` | 585 | 506 | 86% |",
     "| F345 默认应用矩阵 | `sysgov` | 585 | 524 | 90% |"),
    ("| F346 自启动管理 | `sysgov` | 1105 | 505 | 46% |",
     "| F346 自启动管理 | `sysgov` | 1105 | 523 | 47% |"),
    ("| **合计（50 项）** | — | **60710** | **17790（另有 hbase 531 + 域聚合器 103）** | **29.3%** |",
     "| **合计（50 项）** | — | **60710** | **18201（另有 hbase 531 + 域聚合器 103）** | **30.0%** |"),
]
for a, b in pairs:
    assert a in s, "missing: " + a[:30]
    s = s.replace(a, b)

# 3. 批次十二章节
tail = "50 项口径 17,338 → **17,790**（29.3%）。F336 达成率 108%（第二超额项）。"
assert tail in s
s = s.replace(tail, tail + """

## 四·十四、深化批次十二（2026-09-26 · 决策解释器 / 协议版本化 / 管道语义）

**本批新增功能核**（全部注入钟化/纯计算/零 unsafe）：

| 深化件 | 落位 | 判据锚 |
| --- | --- | --- |
| `DecisionExplainer` 调速器决策解释器（当前模式为什么——机制/证据/让位三段人话链，四模式文案分岔互异——机制不黑盒） | animdegrade | F332 |
| `ProtocolVersion` 卸载协议版本化（输出带版本头/未知版本显性拒绝+升级指引——接口十年不变）+ `PreinstallManifest` 预置应用清单导出（重装迁移面）+ `QuotaWarn` 体积配额三档预警（告急才出横幅） | sysgov | F344 |
| `ConvertPipeline` 转换管道（normal→quote→audit 一条链——记的与看到的一致+凭据路径 redacted 标记）+ `BookmarkFreq` 书签频率账（高频置顶零命中殿后）+ `InputGuard` 超长输入拒绝（4096 边界+DoS 面防呆） | copypath | F337/F336 |
| `AutosaveLedger` 草稿自动保存间隔账（10s 间隔门+不脏不存+未保存编辑合计） | saveask | F310 |
| `RecoveryPointDedup` 恢复点去重（FNV 指纹+空间节省估算——暂存区卫生可测） | sesrestore | F311 |
| `CrossDeviceLedger` 指示器交叉占用（双设备独立账+关一不动另一——设备语义不越界） | micind | F322 |

**基础设施**：`MAX_CHECKS` 64 → 96（copypath/sysgov 深化至 12 层后单项
检查数超 64，域聚合截断丢红——容量提升留 headroom）；mod.rs 检查树
改程序化平衡生成（rebuild 脚本随仓——手数括号时代结束）。

**隔离验证**：`_attic/aih3-f301-f350/isolation/` cargo test **421 域内通过 / 0 失败 /
0 警告**（cargo test 总 424 含 3 个舱内探针——口径沿 D-20）。

**行数**：纯功能 18,424 → **18,835**（+411）；域内单测 408 → **421**（+13）；
50 项口径 17,790 → **18,201**（**30.0% 破三成**）。""")
io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("ledger completed v2")
