# -*- coding: utf-8 -*-
"""S4.1 容错补丁：事件环全槽扫描 + 命令门铃重振铃。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\drivers\xhci.rs")
s = p.read_text(encoding="utf-8")

def rep(old, new, n=1):
    global s
    c = s.count(old)
    assert c == n, "count=%d (want %d) for: %r" % (c, n, old[:70])
    s = s.replace(old, new)

# 1) evt_step 之后插入全槽扫描函数
rep("""    // ---- 命令通道（串行，自旋等待完成；仅初始化期使用）-------------------""",
    """    /// 全槽扫描：在事件环 64 槽内找「类型+TRB 指针」双匹配的事件。
    /// 每槽合法性按其位置的期望周期位判定（游标之前的槽=翻转周期，其后=当前周期）。
    /// 命中 → 游标推进到命中槽之后（沿途事件按类型记账）；未命中 → 游标不动。
    /// **这是对 QEMU 写序/游标错位类异常的工程容错**（真驱动对多段事件环
    /// 本就全段扫描）。沿途未匹配的有效事件也一并消费记账，防游标卡死。
    fn evt_scan_for(&mut self, typ: u8, want_ptr: u64) -> Option<Event> {
        let start = self.evt_idx;
        let base_cycle = self.evt_cycle;
        let mut hit = None;
        for step in 0..EVENT_ENTRIES {
            let slot = (start + step) % EVENT_ENTRIES;
            let wrapped = slot < start;
            let slot_cycle = if wrapped { !base_cycle } else { base_cycle };
            let addr = self.evt_phys + 64 + slot as u64 * 32;
            let mut raw = [0u8; 32];
            self.mem.read_bytes(addr, 0, &mut raw);
            let ev = parse_event(&raw);
            if ev.cycle != slot_cycle {
                continue; // 该槽无有效事件。
            }
            let is_match = ev.typ == typ && ev.ptr == want_ptr;
            match ev.typ {
                ER_COMMAND_COMPLETE => self.cmd_events += 1,
                ER_PORT_STATUS_CHANGE => self.port_events += 1,
                ER_TRANSFER => self.transfer_events += 1,
                _ => self.unknown_events += 1,
            }
            self.evt_idx = (slot + 1) % EVENT_ENTRIES;
            if slot + 1 >= EVENT_ENTRIES {
                self.evt_cycle = !self.evt_cycle;
            }
            self.advance_erdp();
            if is_match {
                hit = Some(ev);
                break;
            }
        }
        hit
    }

    // ---- 命令通道（串行，自旋等待完成；仅初始化期使用）-------------------""")

# 2) cmd_submit：扫描兜底 + 重振铃（保留超时取证）
rep("""        self.doorbell(0, 0); // QEMU：命令门铃写值必须为 0。
        // 自旋消费事件环直到本命令完成（初始化期串行、无并发消费者）。
        let deadline = self.deadline();
        loop {
            if let Some(ev) = self.evt_peek() {
                match ev.typ {
                    ER_COMMAND_COMPLETE => {
                        self.cmd_events += 1;
                        self.evt_step();
                        if ev.ptr == trb_bus {
                            self.cmd_outstanding = None;
                            return Ok((ev.slotid, ev.ccode));
                        }
                        // 非本命令的完成：继续等（串行契约下不应出现）。
                    }
                    ER_PORT_STATUS_CHANGE => {
                        self.port_events += 1;
                        self.evt_step();
                    }
                    ER_TRANSFER => {
                        self.transfer_events += 1;
                        self.evt_step();
                    }
                    _ => {
                        self.unknown_events += 1;
                        self.evt_step();
                    }
                }
            }
            if (self.now)() > deadline {""",
    """        self.doorbell(0, 0); // QEMU：命令门铃写值必须为 0。
        // 自旋等待本命令完成：顺序游标优先，未见则全槽扫描（QEMU 写序
        // 容错），等待过半重振铃一次（门铃丢失类异常兜底）。
        let deadline = self.deadline();
        let rerun_at = (self.now)() + self.timeout_ns / 2;
        let mut re_rung = false;
        loop {
            if let Some(ev) = self.evt_peek() {
                match ev.typ {
                    ER_COMMAND_COMPLETE => {
                        self.cmd_events += 1;
                        self.evt_step();
                        if ev.ptr == trb_bus {
                            self.cmd_outstanding = None;
                            return Ok((ev.slotid, ev.ccode));
                        }
                        // 非本命令的完成：继续等（串行契约下不应出现）。
                    }
                    ER_PORT_STATUS_CHANGE => {
                        self.port_events += 1;
                        self.evt_step();
                    }
                    ER_TRANSFER => {
                        self.transfer_events += 1;
                        self.evt_step();
                    }
                    _ => {
                        self.unknown_events += 1;
                        self.evt_step();
                    }
                }
                continue;
            }
            if let Some(ev) = self.evt_scan_for(ER_COMMAND_COMPLETE, trb_bus) {
                self.cmd_outstanding = None;
                return Ok((ev.slotid, ev.ccode));
            }
            if !re_rung && (self.now)() >= rerun_at {
                re_rung = true;
                self.doorbell(0, 0); // 门铃丢失类异常兜底：重振铃一次。
            }
            if (self.now)() > deadline {""")

p.write_text(s, encoding="utf-8")
print("ALL WRITTEN")
