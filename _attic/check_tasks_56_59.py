# -*- coding: utf-8 -*-
"""任务表勾选任务56/59 并附验收注记"""
import io

p = r"docs/VARIX双域系统AI分工任务表.md"
s = io.open(p, encoding="utf-8").read()

old56 = "- [ ] **任务 56**（AI-K）：配额服务——三方分配矩阵/水位/回收。前置：任务 21。"
new56 = ("- [x] **任务 56**（AI-K）：配额服务——三方分配矩阵/水位/回收。前置：任务 21。"
         "✅2026-09-18 新建 quota.rs：**CPU 三方分配矩阵**（Front 400‰/w5·Engine 350‰/w3·Wine 150‰/w1，min_permil 保底+weight 超额→比例保底求解 targets 500/350/150；"
         "apply 只写既有 slice_total/slice_left 对接 engine::on_tick/pick 同优先级 RR 份额∝slice——**零旁路**；assign_tid/clear_tid/set_quota=tid 级覆写+运行时重配 Σmin≤1000 校验）＋"
         "**MemWatermark 三档滞回水位**（Pressure 只级1 ramcache/Critical 级1→2→3 顺序回收/Normal 逆序归还；owed 账本全程记账，归还=回收账目两清；滞回带 [250,300) 双侧维持）＋"
         "**GpuChannel trait 冻结**（reserve/release/submit 七方法契约，SoftGpuChannel=软件渲染兜底唯一实现，kind=\"soft\" 纯记账不在渲染路径）。"
         "宿主 11 用例+实机探针 6 行证据全 verdict=ok：三方饱和份额 507/342/150（偏差≤10‰ 全过保底=保底核不被抢占）；独占压测 Wine×2 下 Front/Engine max-gap 21/27≤40 交互不卡顿；"
         "水位 reclaimed=restored=320 页顺序 [ram,engine,wine] 算术唯一确定；GPU 预留 400/350/150+越限拒绝+记账 90/10/0。"
         "**实机坑**：Box::new(ThreadTable) 栈中转 ~115KB 撑爆 64KB 引导栈静默崩溃→探针表改 .bss static（SpinProtected，sched::SCHED 同款）+探针尾恢复全局 current。"
         "ktest 2876 绿（2865+11）/kcheck 0 告警/kbuild 成功。分配策略文档+回收策略表 docs/双域-任务56-配额分配策略-2026-09-17.md；凭证 docs/acceptance/2026-09-17-任务56-配额服务/。")
assert s.count(old56) == 1, ("56", s.count(old56))
s = s.replace(old56, new56, 1)

old59 = "- [ ] **任务 59**（AI-K）：GPU Vulkan 加速立项评估（软件渲染兜底不变）。前置：任务 21。"
new59 = ("- [x] **任务 59**（AI-K）：GPU Vulkan 加速立项评估（软件渲染兜底不变）。前置：任务 21。"
         "✅2026-09-18 `docs/双域-任务59-GPU-Vulkan立项评估报告-2026-09-17.md`：**分阶段立项可行，当前只启动 M1**（virtio-gpu 2D 显示通路加速，软件合成保持）；"
         "M2 起=QEMU virtio-gpu-gl/venus 直通实测，数据不达标则 M3 搁置、M1 成果保留；范围 M1-M4（显示通路→直通评估→同 trait Vulkan 后端→双后端 A/B 像素回归）+可复用基建（GpuChannel 冻结 trait/PCI ECAM/BarMmio/DMA 清零规范）+风险 R1-R5（venus 依赖实测前置/无 MSI 纯轮询/SoftGpuChannel 永久兜底/明确不做 compute·3D 管线·GPU 多租户）。"
         "**双落点声明**：报告结论节 + quota.rs 模块注释与 SoftGpuChannel（kind=\"soft\" 永久默认后端，失败回退不阻塞引导）。凭证 docs/acceptance/2026-09-17-任务59-Vulkan立项评估/。")
assert s.count(old59) == 1, ("59", s.count(old59))
s = s.replace(old59, new59, 1)

io.open(p, "w", encoding="utf-8", newline="").write(s)
s2 = io.open(p, encoding="utf-8").read()
assert "- [x] **任务 56**" in s2 and "- [x] **任务 59**" in s2
assert s2.count("- [ ] **任务 56**") == 0 and s2.count("- [ ] **任务 59**") == 0
print("task table updated ok")
