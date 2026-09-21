# -*- coding: utf-8 -*-
"""S4.1 缺口二最终修复：ERDP 惰性推进（防 QEMU 满环丢弃分支吞掉完成事件）。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\drivers\xhci.rs")
s = p.read_text(encoding="utf-8")

def rep(old, new, n=1):
    global s
    c = s.count(old)
    assert c == n, "count=%d (want %d) for: %r" % (c, n, old[:70])
    s = s.replace(old, new)

# 1) 结构体：加 erdp_written 槽号（惰性推进的落点记录）
rep("""    // 事件环软件侧。
    evt_phys: u64,
    evt_idx: usize,
    evt_cycle: bool,""",
    """    // 事件环软件侧。
    evt_phys: u64,
    evt_idx: usize,
    evt_cycle: bool,
    /// ERDP 已写入的槽号（惰性推进：落后 evt_idx 若干槽，防 QEMU 满环
    /// 丢弃分支命中——dp_idx 贴近 er_ep_idx 时完成事件会被静默丢弃）。
    erdp_written: usize,""")

rep("""        evt_phys: 0,
        evt_idx: 0,
        evt_cycle: true,
        ictx_phys: 0,""",
    """        evt_phys: 0,
        evt_idx: 0,
        evt_cycle: true,
        erdp_written: 0,
        ictx_phys: 0,""")

# 2) evt_step：只推游标，标记 dirty，不再立即写 ERDP
rep("""    /// 推进事件环游标并把 ERDP 让位到下一个待填条目（EHB 顺手清 IP）。
    fn evt_step(&mut self) {
        self.evt_idx += 1;
        if self.evt_idx >= EVENT_ENTRIES {
            self.evt_idx = 0;
            self.evt_cycle = !self.evt_cycle;
        }
        let erdp_tok = self.evt_phys + 64 + self.evt_idx as u64 * 32;
        let erdp = self.mem.bus_addr(erdp_tok);
        self.rtw32(RT_ERDP, erdp as u32 | ERDP_EHB);
        self.rtw32(RT_ERDP + 4, (erdp >> 32) as u32);
    }""",
    """    /// 推进事件环游标（惰性：ERDP 不立即前推，见 `erdp_flush_if_needed`）。
    fn evt_step(&mut self) {
        self.evt_idx += 1;
        if self.evt_idx >= EVENT_ENTRIES {
            self.evt_idx = 0;
            self.evt_cycle = !self.evt_cycle;
        }
    }

    /// ERDP 惰性推进：落后达到阈值（8 槽）或调用方要求时，把 ERDP 让位
    /// 到游标处。**设计依据**：QEMU `xhci_event` 在 `dp_idx == er_ep_idx+1`
    /// 时静默丢弃事件（满环分支）——急切推进会让 dp_idx 恰好追平
    /// er_ep_idx+1，完成事件被吞；保持 ERDP 落后 ≥2 槽即绕开该分支。
    fn erdp_flush_if_needed(&mut self, force: bool) {
        let lag = (self.evt_idx + EVENT_ENTRIES - self.erdp_written) % EVENT_ENTRIES;
        if !force && lag < 8 {
            return;
        }
        self.erdp_written = self.evt_idx;
        let erdp_tok = self.evt_phys + 64 + self.evt_idx as u64 * 32;
        let erdp = self.mem.bus_addr(erdp_tok);
        self.rtw32(RT_ERDP, erdp as u32 | ERDP_EHB);
        self.rtw32(RT_ERDP + 4, (erdp >> 32) as u32);
    }""")

# 3) evt_scan_for 里的 ERDP 推进 → 同步 erdp_written（不立即写）
rep("""            self.evt_idx = (slot + 1) % EVENT_ENTRIES;
            if slot + 1 >= EVENT_ENTRIES {
                self.evt_cycle = !self.evt_cycle;
            }
            let erdp_tok = self.evt_phys + 64 + self.evt_idx as u64 * 32;
            let erdp = self.mem.bus_addr(erdp_tok);
            self.rtw32(RT_ERDP, erdp as u32 | ERDP_EHB);
            self.rtw32(RT_ERDP + 4, (erdp >> 32) as u32);""",
    """            self.evt_idx = (slot + 1) % EVENT_ENTRIES;
            if slot + 1 >= EVENT_ENTRIES {
                self.evt_cycle = !self.evt_cycle;
            }
            self.erdp_flush_if_needed(false);""")

# 4) program_rings：初始 ERDP 写后记录 erdp_written=0
rep("""        self.rtw32(RT_ERDP, seg_bus as u32);
        self.rtw32(RT_ERDP + 4, (seg_bus >> 32) as u32);""",
    """        self.rtw32(RT_ERDP, seg_bus as u32);
        self.rtw32(RT_ERDP + 4, (seg_bus >> 32) as u32);
        self.erdp_written = 0;""")

# 5) cmd_submit 的循环：每轮末尾惰性冲刷（命令等待期 ring 小，force 不必要）
rep("""            if !re_rung && (self.now)() >= rerun_at {
                re_rung = true;
                self.doorbell(0, 0); // 门铃丢失类异常兜底：重振铃一次。
            }
            if (self.now)() > deadline {""",
    """            if !re_rung && (self.now)() >= rerun_at {
                re_rung = true;
                self.doorbell(0, 0); // 门铃丢失类异常兜底：重振铃一次。
            }
            self.erdp_flush_if_needed(false);
            if (self.now)() > deadline {""")

# 6) pump 末尾：force 冲刷一次（把消费进度批量让位给控制器）
rep("""            if let Some(ev) = ev_out {
                if n < out.len() {
                    out[n] = Some(ev);
                    n += 1;
                }
            }
        }
        n
    }""",
    """            if let Some(ev) = ev_out {
                if n < out.len() {
                    out[n] = Some(ev);
                    n += 1;
                }
            }
        }
        self.erdp_flush_if_needed(true);
        n
    }""")

p.write_text(s, encoding="utf-8")
print("ALL WRITTEN")
