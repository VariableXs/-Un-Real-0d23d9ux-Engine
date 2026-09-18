# -*- coding: utf-8 -*-
"""记忆更新：每日日志追加任务56/59 段 + MEMORY.md 进度行"""
import io

# --- 每日日志追加（append-only）---
p = r".workbuddy/memory/2026-09-17.md"
s = io.open(p, encoding="utf-8").read()
add = """
## 任务56 完成（AI-K · 2026-09-18 凌晨）
- **配额服务全链验收通过（QUOTA PROBE DEMO PASS，实机 6 行证据全 verdict=ok PANIC=0）**：quota.rs 三件套——CpuQuota 三方矩阵（Front 400‰/w5、Engine 350‰/w3、Wine 150‰/w1；比例保底求解 targets 500/350/150；apply 只写既有 slice_total/slice_left 对接 tick 零旁路，同优先级 RR 份额∝slice_total；assign_tid 覆写）+ MemWatermark 三档滞回（owed 账本：Pressure 只级1/Critical 级1→2→3/Normal 逆序归还，reclaimed=restored=320 账目两清）+ GpuChannel trait 冻结 + SoftGpuChannel 兜底（纯记账不在渲染路径）。
- **实机核心坑：Box::new(ThreadTable) 栈中转 ~115KB 撑爆 64KB 引导栈**（串口止于 display-probe 静默崩溃）→ 探针表改 .bss static（SpinProtected<ThreadTable> const 初始化，sched::SCHED 同款模式，A/B 双表免重置）+ 探针尾恢复 engine 全局 current。教训：**内核里 >64KB 的 struct 禁止栈上/Box 中转物化，一律 static .bss**。
- 实机数字：份额 507/342/150（目标 500/350/150 偏差≤10‰）；独占压测 max-gap 21/27≤40；GPU 预留 400/350/150+越限 1150>1000 拒绝。
- 工程坑两枚：①trait 对象数组上不可见 MockStage 固有方法——账本断言进借用作用域、mock 直查在作用域外；②heredoc 含 ′\\′+中文注释被壳层截断——内核多文件修补一律脚本文件化+回读断言（Edit 静默丢盘惯例同源）。
- 测试数学三处修正（份额用例忘指派三方/GPU 越限 200→400/成员数期望 20→10）——**断言失败的先查测试自身建模**。
- 门禁：ktest 2876 绿（2865+11）/kcheck 0/kbuild+make-iso-qemu 成功。
- 凭证：docs/acceptance/2026-09-17-任务56-配额服务/ + docs/双域-任务56-配额分配策略-2026-09-17.md。

## 任务59 完成（AI-K · 2026-09-18 凌晨）
- **Vulkan 立项评估报告归档**：分阶段可行、当前只启动 M1（virtio-gpu 2D 显示通路加速）；M2 起=venus 直通实测数据前置（不达标 M3 搁置、M1 成果保留）；范围 M1-M4+风险 R1-R5+启动条件；明确不做 compute/3D 管线/GPU 多租户。双落点兜底声明（报告+quota.rs SoftGpuChannel）。
- 凭证：docs/双域-任务59-GPU-Vulkan立项评估报告-2026-09-17.md + docs/acceptance/2026-09-17-任务59-Vulkan立项评估/。
- **AI-K 阶段性收官**：任务 1-6/9/12-22/24/25/56/59 全部勾选闭环；双域任务表剩余项属 AI-P/AI-V/AI-S 线。
"""
assert "任务56 完成" not in s
io.open(p, "a", encoding="utf-8").write(add)
s2 = io.open(p, encoding="utf-8").read()
assert s2.count("任务56 完成") == 1 and s2.count("任务59 完成") == 1
print("daily log ok")

# --- MEMORY.md 进度行 ---
p = r".workbuddy/memory/MEMORY.md"
s = io.open(p, encoding="utf-8").read()
old = "- 施工文件：docs/VARIX双域系统{AI分工任务表,施工总案,总施工清单}.md。AI-K 进度：任务 1-6/9/12-20/22/24/25 已勾选闭环（21 里程碑 bace527、24 KV 存储 875862b）；进行中 56 配额服务；队列 59 Vulkan 评估。"
new = "- 施工文件：docs/VARIX双域系统{AI分工任务表,施工总案,总施工清单}.md。**AI-K 全部收官**：任务 1-6/9/12-22/24/25/56/59 勾选闭环（21=bace527、24=875862b、56/59 见验收目录）。内核新戒律：>64KB struct 禁栈上/Box 中转物化，一律 static .bss（SpinProtected const 模式）；ktest 基线 2876。"
assert old in s, "MEMORY.md progress line not found"
s = s.replace(old, new, 1)
io.open(p, "w", encoding="utf-8", newline="").write(s)
print("MEMORY.md ok")
