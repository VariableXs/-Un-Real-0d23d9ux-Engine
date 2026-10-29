//! UNX-F11077 回归常态化协议与 B33 LTP 纳入挂接（0x1e9a0 段）
//!
//! 本账把“常态回归协议”拆成 10 个固定槽位：每槽有唯一 pid、所有者与序号，
//! 槽的相位按 Declared → Bound → Witnessed 单向推进；B33 的 LTP 用例只能
//! 通过 `bind_hook` 挂到尚未占用的槽上，且一个 LTP 号全局只能出现一次。
//! 不变式：协议槽数与挂接数相等、每个已见证槽恰有一条观测为真的 LTP 记录。
//!
//! 结构：协议槽表 + 挂接账 + 覆盖审计器（空账/越界/重号/双挂/未绑/冻结六路）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11077 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e9a0;

/// 协议账为空，无法做常态化审计 —— 尚未登记任何协议槽时抛出。
pub const RP_EMPTY: u32 = SEG_BASE + 0x01;
/// 槽号或 LTP 号越出固定窗口 —— pid 不在 1..=10 或 ltp 不在 100..=109。
pub const RP_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 协议 pid 重复，或同一 LTP 用例被挂接第二次。
pub const RP_DUP: u32 = SEG_BASE + 0x03;
/// 双挂/双占 —— 目标协议槽已经 Bound 或 Witnessed，不能再挂第二条 LTP。
pub const RP_LEAK: u32 = SEG_BASE + 0x04;
/// 悬空挂接 —— 协议槽尚未登记、或 LTP 观测记录不在账上。
pub const RP_UNBOUND: u32 = SEG_BASE + 0x05;
/// 封账后写账 —— 协议账已冻结仍尝试登记、挂接或见证。
pub const RP_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（协议挂接账）。
pub const RP_TAG: u64 = 0x1ea_77;

/// 协议槽固定容量（10/10）。
pub const RP_SLOTS: usize = 10;
/// LTP 挂接固定容量（10/10）。
pub const RP_HOOKS: usize = 10;

/// 协议槽相位；只允许向前推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookPhase {
    /// 槽已立账，尚未挂 B33 用例。
    Declared,
    /// LTP 用例已占槽，尚未拿到观测结论。
    Bound,
    /// LTP 观测为真，槽完成见证。
    Witnessed,
}

/// 一条常态化协议槽。
#[derive(Clone, Debug)]
pub struct ProtocolSlot {
    pub pid: u32,
    pub owner: u8,
    pub order: u16,
    pub phase: HookPhase,
    pub ltp_id: u32,
}

/// 一条 B33 LTP 挂接记录。
#[derive(Clone, Debug)]
pub struct HookCase {
    pub ltp_id: u32,
    pub pid: u32,
    pub seq: u16,
    pub expected: bool,
    pub observed: bool,
}

impl HookCase {
    /// 单条挂接对平：期望为真且观测为真。
    pub fn aligned(&self) -> bool {
        self.expected && self.observed
    }

    /// 供动态判据渲染的一行文本。
    pub fn render(&self) -> String {
        let mark = if self.aligned() { "A" } else { "X" };
        format!("ltp{}>p{}@{}:{}", self.ltp_id, self.pid, self.seq, mark)
    }
}

/// 常态回归协议账：协议槽与 LTP 挂接双表。
#[derive(Clone, Debug)]
pub struct ProtocolBoard {
    pub slots: Vec<ProtocolSlot>,
    pub hooks: Vec<HookCase>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ProtocolBoard {
    pub fn new() -> ProtocolBoard {
        ProtocolBoard { slots: Vec::new(), hooks: Vec::new(), frozen: false, root: None }
    }

    fn slot_at(&self, pid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].pid == pid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn hook_at(&self, ltp_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.hooks.len() {
            if self.hooks[i].ltp_id == ltp_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记一个协议槽。序号必须等于当前槽数加一，保证账面严格有序。
    pub fn enroll(&mut self, pid: u32, owner: u8, order: u16) -> Result<(), u32> {
        if self.frozen {
            return Err(RP_FROZEN);
        }
        if pid == 0 || pid > RP_SLOTS as u32 {
            return Err(RP_RANGE);
        }
        if order as usize != self.slots.len() + 1 {
            return Err(RP_RANGE);
        }
        if self.slot_at(pid).is_some() {
            return Err(RP_DUP);
        }
        self.slots.push(ProtocolSlot {
            pid,
            owner,
            order,
            phase: HookPhase::Declared,
            ltp_id: 0,
        });
        Ok(())
    }

    /// 把 B33 LTP 用例挂到协议槽。一个槽只许一条，一个 LTP 号只许一次。
    pub fn bind_hook(&mut self, pid: u32, ltp_id: u32, observed: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(RP_FROZEN);
        }
        if pid == 0 || pid > RP_SLOTS as u32 || ltp_id < 100 || ltp_id > 109 {
            return Err(RP_RANGE);
        }
        let si = match self.slot_at(pid) {
            Some(i) => i,
            None => return Err(RP_UNBOUND),
        };
        if self.slots[si].phase != HookPhase::Declared {
            return Err(RP_LEAK);
        }
        if self.hook_at(ltp_id).is_some() {
            return Err(RP_DUP);
        }
        let seq = self.slots[si].order;
        self.slots[si].phase = if observed { HookPhase::Witnessed } else { HookPhase::Bound };
        self.slots[si].ltp_id = ltp_id;
        self.hooks.push(HookCase {
            ltp_id,
            pid,
            seq,
            expected: true,
            observed,
        });
        Ok(())
    }

    /// 回填 LTP 观测；仅允许从 Bound 转 Witnessed，重复见证视为重号。
    pub fn witness(&mut self, ltp_id: u32, observed: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(RP_FROZEN);
        }
        let hi = match self.hook_at(ltp_id) {
            Some(i) => i,
            None => return Err(RP_UNBOUND),
        };
        if self.hooks[hi].observed {
            return Err(RP_DUP);
        }
        if !observed {
            return Err(RP_UNBOUND);
        }
        self.hooks[hi].observed = true;
        let pid = self.hooks[hi].pid;
        if let Some(si) = self.slot_at(pid) {
            self.slots[si].phase = HookPhase::Witnessed;
        }
        Ok(())
    }

    /// 已见证槽数 / 已绑定槽数。
    pub fn coverage(&self) -> (u32, u32) {
        let mut witnessed = 0u32;
        let mut bound = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            match self.slots[i].phase {
                HookPhase::Witnessed => {
                    witnessed = witnessed.wrapping_add(1);
                    bound = bound.wrapping_add(1);
                }
                HookPhase::Bound => bound = bound.wrapping_add(1),
                HookPhase::Declared => {}
            }
            i += 1;
        }
        (witnessed, bound)
    }

    /// 协议面 10/10：十槽全部 Witnessed。
    pub fn protocol_all_ready(&self) -> bool {
        self.slots.len() == RP_SLOTS && self.coverage().0 == RP_SLOTS as u32
    }

    /// LTP 面 10/10：十条挂接全部对平。
    pub fn hooks_all_aligned(&self) -> bool {
        if self.hooks.len() != RP_HOOKS {
            return false;
        }
        let mut i = 0usize;
        while i < self.hooks.len() {
            if !self.hooks[i].aligned() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 每槽恰一条挂接、每挂接恰指一个槽。
    pub fn one_to_one(&self) -> bool {
        if self.slots.len() != self.hooks.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.hooks.len() {
            let pid = self.hooks[i].pid;
            match self.slot_at(pid) {
                Some(si) => {
                    if self.slots[si].ltp_id != self.hooks[i].ltp_id {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 全账审计：空账、悬空、双挂都拒绝。
    pub fn audit(&self) -> Result<(), u32> {
        if self.slots.is_empty() {
            return Err(RP_EMPTY);
        }
        if self.slots.len() != RP_SLOTS {
            return Err(RP_UNBOUND);
        }
        if !self.one_to_one() {
            return Err(RP_LEAK);
        }
        if !self.protocol_all_ready() || !self.hooks_all_aligned() {
            return Err(RP_UNBOUND);
        }
        Ok(())
    }

    /// 链式 FNV-1a 指纹，覆盖槽相位与挂接观测。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RP_TAG;
        let mut i = 0usize;
        while i < self.slots.len() {
            let s = &self.slots[i];
            h = (h ^ s.pid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.owner as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.order as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.ltp_id as u64).wrapping_mul(0x100000001b3);
            h = (h ^ match s.phase {
                HookPhase::Declared => 0x11,
                HookPhase::Bound => 0x22,
                HookPhase::Witnessed => 0x33,
            }).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut k = 0usize;
        while k < self.hooks.len() {
            let c = &self.hooks[k];
            h = (h ^ c.ltp_id as u64).wrapping_mul(0x100000001b3);
            h = (h ^ c.pid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ c.seq as u64).wrapping_mul(0x100000001b3);
            h = (h ^ if c.observed { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结协议账并返回挂接根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 一行动态摘要，便于把账面容错率带进 J9。
    pub fn summary(&self) -> String {
        let kind = "protocol".to_string();
        let (w, b) = self.coverage();
        format!("{}|w={}|b={}|h={}|root={}", kind, w, b, self.hooks.len(),
            self.root.unwrap_or(0))
    }
}

/// 四路坏样本：重号、越界、双挂、冻结后写，各走不同拒绝分支。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // 1) 协议 pid 重号。
    let mut a = ProtocolBoard::new();
    let _ = a.enroll(1, 7, 1);
    got[0] = a.enroll(1, 8, 2).err() == Some(RP_DUP);

    // 2) 槽号越界。
    let mut b = ProtocolBoard::new();
    got[1] = b.enroll(11, 1, 1).err() == Some(RP_RANGE);

    // 3) 同一槽双挂 LTP。
    let mut c = ProtocolBoard::new();
    let _ = c.enroll(2, 3, 1);
    let _ = c.bind_hook(2, 102, true);
    got[2] = c.bind_hook(2, 103, true).err() == Some(RP_LEAK);

    // 4) 冻结后登记新槽。
    let _ = c.freeze();
    got[3] = c.enroll(3, 4, 2).err() == Some(RP_FROZEN);
    got
}

/// 常态化协议与 B33 挂接判据。
pub fn run_unx11077_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11077");

    // J1 两半：协议逐项立账 10/10，LTP 挂接对平 10/10。
    let mut b = ProtocolBoard::new();
    let mut pid = 1u32;
    while pid <= RP_SLOTS as u32 {
        let _ = b.enroll(pid, (pid % 4) as u8, pid as u16);
        let _ = b.bind_hook(pid, 99 + pid, true);
        pid = pid.wrapping_add(1);
    }
    let j1a = b.protocol_all_ready() && b.coverage().0 == 10;
    let j1b = b.hooks_all_aligned() && b.hooks.len() == 10;
    s.add("J1-协议逐项立账", j1a, "10 个协议槽全部见证");
    s.add("J1-LTP挂接对平", j1b, "10 条 B33 LTP 挂接全部对平");

    // J2 双面守恒：槽数、挂接数、一对一映射同时成立。
    let j2 = b.one_to_one() && b.slots.len() == b.hooks.len()
        && b.fold() == b.clone().fold();
    s.add("J2-双面守恒", j2, "槽数与挂接数相等且映射一一对应");

    // J3 四路注入逐条独立。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "协议 pid 重号必拒");
    s.add("J3-越界注入", inj[1], "槽号越界必拒");
    s.add("J3-双挂注入", inj[2], "同槽第二条 LTP 必拒");
    s.add("J3-冻结注入", inj[3], "封账后登记必拒");

    // J4 冻结/不可变：封账根稳定，且写路径全部关闭。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.fold() == root && f.root == Some(root)
        && f.enroll(1, 1, 1).err() == Some(RP_FROZEN)
        && f.witness(100, true).err() == Some(RP_FROZEN);
    s.add("J4-封账不可变", j4, "封账根稳定且登记/见证双路拒写");

    // J5 指纹敏感：只改一条观测位，指纹必须改变。
    let mut c = b.clone();
    c.hooks[4].observed = false;
    let j5 = c.fold() != b.fold() && b.fold() != 0;
    s.add("J5-指纹敏感", j5, "单条观测翻转即改变 fold 根");

    // J6 空账与范围双拒。
    let mut e = ProtocolBoard::new();
    let j6 = e.audit().err() == Some(RP_EMPTY)
        && e.enroll(10, 0, 1).is_ok()
        && e.enroll(12, 0, 2).err() == Some(RP_RANGE);
    s.add("J6-空账范围", j6, "空账审计与槽号越界均拒");

    // J7 唯一与顺序：pid 递增、LTP 号唯一、槽序号严格加一。
    let mut order_ok = true;
    let mut i = 1usize;
    while i < b.slots.len() {
        if b.slots[i].order as usize != i + 1 || b.slots[i].pid != i as u32 + 1 {
            order_ok = false;
        }
        i += 1;
    }
    let mut ltp_ok = true;
    let mut j = 0usize;
    while j < b.hooks.len() {
        let mut k = j + 1;
        while k < b.hooks.len() {
            if b.hooks[j].ltp_id == b.hooks[k].ltp_id {
                ltp_ok = false;
            }
            k += 1;
        }
        j += 1;
    }
    s.add("J7-唯一有序", order_ok && ltp_ok, "序号严格递增且 LTP 号不重复");

    // J8 负语义：只挂 9 条时绝不能误报全绿。
    let mut partial = ProtocolBoard::new();
    let mut p = 1u32;
    while p <= 9 {
        let _ = partial.enroll(p, 2, p as u16);
        let _ = partial.bind_hook(p, 99 + p, true);
        p = p.wrapping_add(1);
    }
    let j8 = !partial.protocol_all_ready() && !partial.hooks_all_aligned()
        && partial.audit().is_err();
    s.add("J8-半账非绿", j8, "九条挂接的半账必须判非全绿");

    // J9 覆盖动态账。
    s.add_dyn("J9-覆盖动态", b.coverage() == (10, 10),
        &format!("witness={} bound={} hooks={}", b.coverage().0, b.coverage().1, b.hooks.len()));

    // J10 见证路径：先 Bound 后回填观测，仍能到达 Witnessed。
    let mut w = ProtocolBoard::new();
    let _ = w.enroll(1, 9, 1);
    let _ = w.bind_hook(1, 100, false);
    let before = w.coverage().0;
    let filled = w.witness(100, true).is_ok();
    // 单槽小面只需本挂接对平，不必凑满十挂接（hooks_all_aligned 属全账口径）
    let j10 = before == 0
        && filled
        && w.coverage().0 == 1
        && w.hooks.len() == 1
        && w.hooks[0].aligned();
    s.add("J10-见证路径", j10, "Bound 后回填真观测完成见证");

    // J11 摘要可渲染且包含根。
    let text = f.summary();
    let j11 = text.starts_with("protocol|") && text.contains("root=");
    s.add_dyn("J11-摘要落账", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11077_flow() {
        let mut b = ProtocolBoard::new();
        assert!(b.audit().err() == Some(RP_EMPTY));
        assert!(b.enroll(1, 2, 1).is_ok());
        assert!(b.enroll(2, 2, 3).err() == Some(RP_RANGE));
        assert!(b.enroll(1, 3, 2).err() == Some(RP_DUP));
        assert!(b.bind_hook(1, 100, true).is_ok());
        assert!(b.bind_hook(1, 101, true).err() == Some(RP_LEAK));
        assert!(b.bind_hook(2, 101, true).err() == Some(RP_UNBOUND));
        let old = b.fold();
        let root = b.freeze();
        assert!(old == root && b.fold() == root);
        assert!(b.enroll(2, 4, 2).err() == Some(RP_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11077_checks_green() {
        assert!(run_unx11077_checks().all_passed(), "UNX-F11077 判据未全绿");
    }
}
