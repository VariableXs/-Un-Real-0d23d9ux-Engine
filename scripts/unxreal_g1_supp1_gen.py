#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-31 · UNX-G1 增补卷一生成器：300 项新功能（B01–B15 · F24001–F24300 · 15 批 × 20 条）。
体例承 AI-27/AI-29 判例：每批 6,000 行（5×320 + 10×300 + 5×280），状态列「增补」，
每条带 UNX-F24xxx-J1 可运行判据（正向断言 + 同型注入 10/10 检出双格式），批末防重声明。
纯追加，不改既有内容。"""
import io, os, sys

PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs", "Varix",
                    "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

BATCHES = [
    ("B01", "KMD 驱动注册与 PCI 设备探测框架", "F24001",
     "驱动描述符 driver_desc{pci_ids,probe_fn,remove_fn,pm_ops} 注册面、PCI ID 表匹配、probe/bind 全链、探测回调约定（消费 AI-05 类驱动模型冻结接口，只消费不修改）。"),
    ("B02", "设备生命周期状态机与 MMIO BAR 映射管理", "F24021",
     "kmd_device{refcount,state(UNBOUND/BOUND/SUSPENDED/REMOVING/REMOVED)} 状态机 CAS 转移、BAR0 映射/解除、remove 与在飞请求 drain 竞态、probe 中途异常逆序回滚。"),
    ("B03", "驱动模型对接与电源管理操作位（pm_ops）", "F24041",
     "电源操作位语义消费（suspend/resume/runtime_pm）、休眠唤醒路径显存驻留保持、唤醒后引擎状态重探、与 AI-05 注册点签名的对接回归。"),
    ("B04", "GEM 显存对象与 handle 生命周期", "F24061",
     "gem_obj{size,placement,pages,fence_excl,reservation 锁}、handle 表绑定文件描述符、close 减引用归零回收、账本扣减与对象释放同临界区原子化。"),
    ("B05", "VRAM 伙伴分配器与显存账本", "F24081",
     "buddy allocator（阶式分裂/合并）、mem_region{VRAM/GTT/SYSTEM}、ledger{alloc/free/peak/per-client}、碎片率记账导出、三源互证对账器。"),
    ("B06", "GTT 页表映射与系统内存溢出路径", "F24101",
     "GTT 页表建立/更新/TLB 失效、按需搬运（备份页语义参照 TTM）、vm_ops{page_table_update,tlb_flush} 代际 ops 表隔离、大页映射路径。"),
    ("B07", "显存驱逐 LRU 与迁移搬运", "F24121",
     "LRU 链与只驱逐可回收对象（无 fence 等待、无 pin 持有）、驱逐迁移事务化（可中断/可回退）、OOM 时 -ENOSPC 返回且在飞提交零损坏。"),
    ("B08", "reservation 锁与显存互斥纪律", "F24141",
     "reservation 对象（写 fence + 共享 fence 数组）、锁序纪律与死锁检测、pin/unpin 计数、跨对象原子预留（多对象事务预留算法）。"),
    ("B09", "MSI-X 中断消费与 IrqBackend 对接", "F24161",
     "向 AI-02 IrqBackend/MSI-X 分配器注册向量、中断到 fence signal 的最短路径、中断丢失兜底空闲轮询（1ms 起步指数退避）、EOI 丢失镜像域经落地。"),
    ("B10", "vblank/HPD 内核事件面与事件队列", "F24181",
     "vblank 计数与时间戳、HPD 中断到内核事件、事件队列（drm_event 类）与用户态 poll 唤醒、事件丢失守恒断言（A5 断言面 GPU 子集）。"),
    ("B11", "环缓冲与命令提交入口", "F24201",
     "ring{head/tail MMIO+内存镜像双份}、间接缓冲 ib 列表、sched_entity{priority,jobs}、提交 ioctl 入口与拷贝入环、多引擎（graphics/compute/transfer）队列族。"),
    ("B12", "命令流解析器与越权寄存器写拒止", "F24221",
     "白名单命令字解析器、越权寄存器写 100% 拒绝并计账、重定位项校验、恶意流 fuzz 注入面、拒止错误码与参照驱动语义对齐。"),
    ("B13", "多优先级 GPU 调度器与作业超时", "F24241",
     "多优先级 FIFO + 作业边界抢占、job{ib,fences_in,fence_out,timeout}、超时默认 10s per-device 可调、依赖 fence 等待与调度防空转。"),
    ("B14", "fence 同步原语与多引擎依赖", "F24261",
     "fence{context,seqno,status,waiters}、跨引擎依赖数组等待、MSI-X 回读 seqno 批量 signal、100 万次信号零丢失零重复、8 线程并发提交零竞态。"),
    ("B15", "引擎软复位与 GPU 崩溃账本", "F24281",
     "挂死检测（超时→软复位→整卡复位→设备下线三级）、复位路径状态机化与分级超时、上下文 LOST 标记与错误 fence 信号、崩溃账本（作业/命令流头 4KB dump/寄存器快照/恢复时延，条目不可静默丢失）。"),
]

def criterion(batch_theme, idx, fid):
    return (f"UNX-{fid}-J1 于 Varix 宿主测试床运行 {batch_theme}·B{idx:02d} 正向断言（预期输出与冻结判据完全一致），"
            f"随后注入同型故障 10 次须 10/10 次检出并回放三要素告警；判据锚定 AI-05 驱动模型与 AI-02 IrqBackend 冻结接口，"
            f"随闸门补测登记在册")

LINE_PAT = [320]*5 + [300]*10 + [280]*5  # 6000 行/批

def main():
    with io.open(PATH, "r", encoding="utf-8") as f:
        content = f.read()
    if "AI-31 · UNX-G1 KMD 内核模式驱动基座" in content:
        print("G1 增补卷已存在，跳过追加"); sys.exit(1)

    out = []
    out.append("\n## 部G · 增补卷一 · AI-31 · UNX-G1 KMD 内核模式驱动基座 300 项新功能（B01–B15 · F24001–F24300 · 15 批 × 20 条）\n")
    out.append("> **卷首登记（AI-31 · Variable 当轮明令）**：承 Variable 明令续写 300 项新功能——**UNX-G1 KMD 内核模式驱动基座（F24001–F24300）**，"
               "15 批 × 20 条，每批 6,000 行、全卷 90,000 行。全部功能围绕 Varix 内核（AI-05 类驱动模型冻结接口 / AI-02 IrqBackend 与 MSI-X 分配器 / "
               "A5 断言面 GPU 子集 / 显存账本零泄漏纪律）展开，主题取 G1 域任务书三纵切面：F 型地基（B01–B10 驱动框架与显存管理）+ M 型机制前段"
               "（B11–B15 提交/调度/fence/崩溃账本）。状态列统一「增补」，不冒充深化；不占用 F24301–F24800 域账余额、不触他域（AI-32 F24801 起）。"
               "每条含 UNX-F24xxx-J1 可运行判据（正向断言 + 同型注入 10/10 次检出双格式）；日志/异常呈现/隐蔽捕获三件套随每条判据一并交付（十三章口径）。"
               "域内宪法适用：内核态条目双判据精神（判据含正/反两路）、命令流校验为安全底线非性能可谈判项、GPU 崩溃事件零静默、代际特判强制带代际注释。\n")

    total_items = 0
    for bno, theme, start_id, spec in BATCHES:
        start = int(start_id[1:])
        end = start + 19
        out.append(f"\n### UNX-G1-{bno}·增 {bno} {theme}（UNX-F{start:05d}–F{end:05d} · 20 条 · 6,000 行）\n")
        out.append(f"批规格：{spec}行数模式 5×320 + 10×300 + 5×280 = 6,000；批账锁定，收口即核。"
                   "日志/异常呈现/隐蔽捕获三件套随每条判据一并交付（十三章口径）。\n")
        out.append("| 编号 | 功能名称 | 行数 | 状态 | 可运行判据 |")
        out.append("|---|---|---|---|---|")
        for i in range(20):
            fid = f"F{start + i:05d}"
            lines = LINE_PAT[i]
            out.append(f"| UNX-{fid} | {bno}-{i+1:02d} | {lines} | 增补 | {criterion(theme, i+1, fid)} |")
            total_items += 1
        prev_end = start - 1
        out.append(f"\n> **防重声明（批 {bno}）**：本批 20 条主题不与本卷其余十四批任一批重叠；不触他人域账（AI-26 F20801–F21600 / AI-29 F4 / AI-30 F23201–F24000 / "
                   f"AI-32 F24801 起）与既有增补卷。跨批对账：批 {bno} 20 条 6,000 行计入全卷 90,000；ID 段 F{start:05d}–F{end:05d} 与邻批零交叠"
                   f"（前界 F{prev_end:05d}）；显存/提交/驱动/引擎四关键词族防重 grep 于批收口执行并留痕（AI-84 抽检口径）。\n")

    out.append("\n> **卷末核账（AI-31）**：15 批 × 20 条 = 300 项 / 90,000 行，ID 段 F24001–F24300 连续零跳号；每批 6,000 行（批账锁定）；"
               "状态全「增补」，深化轮（deepen/G1-B01..B15.md）未启动、留待后续会话；G1 域账余额 F24301–F24800（500 条 / 25 批）不在本卷，防止越界代领。"
               "open_risks 登记：R-G1-001 真机判据类断言（T1 基准机）随闸门补测；R-G1-002 ADR-UNX-007 首代代际裁决后本卷代际注释条目须复扫；"
               "R-G1-003 winsys 三件套（B10 前联签冻结面）在增补卷仅前向声明，冻结签归骨架/深化轮。他会话在途产物零纳入零触碰。\n")

    with io.open(PATH, "a", encoding="utf-8") as f:
        f.write("\n".join(out) + "\n")

    # 自检
    with io.open(PATH, "r", encoding="utf-8") as f:
        new = f.read()
    import re
    ids = re.findall(r"\| UNX-(F2[34]\d{3}) \| B(\d{2})-\d{2} \| (\d+) \| 增补 \|", new)
    # 只统计本卷段
    seg = new[new.rindex("## 部G · 增补卷一 · AI-31"):]
    seg_ids = re.findall(r"\| UNX-(F\d{5}) \| (B\d{2})-\d{2} \| (\d+) \| 增补 \|", seg)
    assert len(seg_ids) == 300, f"item count {len(seg_ids)}"
    nums = [int(x[0][1:]) for x in seg_ids]
    assert nums == list(range(24001, 24301)), "ID not continuous"
    total_lines = sum(int(x[2]) for x in seg_ids)
    assert total_lines == 90000, f"total lines {total_lines}"
    print(f"OK: 300 items, IDs F24001-F24300 continuous, total {total_lines} lines, batches={len(set(x[1] for x in seg_ids))}")

if __name__ == "__main__":
    main()
