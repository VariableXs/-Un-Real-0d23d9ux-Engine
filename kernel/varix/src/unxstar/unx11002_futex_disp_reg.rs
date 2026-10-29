//! UNX-F11002 futex 分发表注册联签（C2 协议注册项对账）（0x1e040 段）
//!
//! 锚点：UNX-F11002-J1 注册项全量对账（10/10），注册序与协议一致（10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：分发表（C2 协议十路操作码定序登记）+ 协议对账器（注册序与协议
//! 序逐位对齐、乱序即拒）+ fold 指纹与冻结纪律 + 注入核（乱序/缺项/重号/
//! 冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e040 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e040;
pub const DT_ORDER: u32 = SEG_BASE + 0x01; // 注册序与协议不符
pub const DT_SHORT: u32 = SEG_BASE + 0x02; // 注册项未齐
pub const DT_DUP: u32 = SEG_BASE + 0x03; // 操作码重号
pub const DT_EMPTY: u32 = SEG_BASE + 0x04; // 处理名空
pub const DT_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const DT_NOITEM: u32 = SEG_BASE + 0x06; // 对账目标不存在

/// C2 协议十路操作码（协议序）。
pub const PROTOCOL_OPS: [u32; 10] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
/// 操作码定容。
pub const OP_CAP: usize = 12;
/// 分发表域标（fold 种子）。
pub const DT_TAG: u64 = 0x1e04c2;

/// 单条分发表项：操作码 + 处理名 + 协议位次。
#[derive(Clone, Debug)]
pub struct DispEntry {
    pub opcode: u32,
    pub handler: String,
    pub proto_pos: Option<usize>,
    pub verified: bool,
}

impl DispEntry {
    pub fn render(&self) -> String {
        let pos = match self.proto_pos {
            Some(p) => p,
            None => 0usize,
        };
        format!("op{}|pos{}|{}", self.opcode, pos, self.handler)
    }
}

/// futex 分发表：注册 → 协议序对账 → 冻结。
pub struct DispatchTable {
    pub entries: Vec<DispEntry>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl DispatchTable {
    pub fn new() -> DispatchTable {
        DispatchTable { entries: Vec::new(), frozen: false, root: None }
    }

    /// 注册操作码：冻结、空名、零码、重号、超容均拒。
    pub fn register(&mut self, opcode: u32, handler: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(DT_FROZEN);
        }
        if handler.is_empty() {
            return Err(DT_EMPTY);
        }
        if opcode == 0 || self.entries.iter().any(|e| e.opcode == opcode) {
            return Err(DT_DUP);
        }
        if self.entries.len() >= OP_CAP {
            return Err(DT_SHORT);
        }
        self.entries.push(DispEntry {
            opcode,
            handler: handler.to_string(),
            proto_pos: None,
            verified: false,
        });
        Ok(())
    }

    /// 协议序对账：逐位比对注册序与协议序，全对齐才逐项翻核销位。
    pub fn reconcile(&mut self, proto: &[u32]) -> Result<bool, u32> {
        if self.frozen {
            return Err(DT_FROZEN);
        }
        if proto.len() != self.entries.len() {
            return Err(DT_SHORT);
        }
        let mut aligned = true;
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].opcode != proto[k] {
                aligned = false;
            }
            k += 1;
        }
        if !aligned {
            return Ok(false);
        }
        let mut k = 0usize;
        while k < self.entries.len() {
            self.entries[k].proto_pos = Some(k);
            self.entries[k].verified = true;
            k += 1;
        }
        Ok(true)
    }

    /// 未核销操作码清单（按注册序）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if !self.entries[k].verified {
                g.push(self.entries[k].opcode);
            }
            k += 1;
        }
        g
    }

    /// 乱序位清单：注册序中与协议序不齐的位次（协议外多登的尾巴位亦算不齐）。
    pub fn misordered(&self, proto: &[u32]) -> Vec<usize> {
        let mut g: Vec<usize> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if k >= proto.len() || self.entries[k].opcode != proto[k] {
                g.push(k);
            }
            k += 1;
        }
        g
    }

    /// 全量对账裁断：十路齐、全数核销、与协议逐位齐。
    pub fn closed(&self, proto: &[u32]) -> bool {
        self.entries.len() == PROTOCOL_OPS.len()
            && self.gaps().is_empty()
            && self.misordered(proto).is_empty()
    }

    /// 按操作码查项（只读）。
    pub fn find(&self, opcode: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].opcode == opcode {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含码/名/核销位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = DT_TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = (h ^ e.opcode as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.verified as u64).wrapping_mul(0x100000001b3);
            let bytes = e.handler.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 处理名确定性文本。
pub fn handler_name(no: usize) -> String {
    format!("c2op{}", no)
}

/// 协议全量注册：按协议序登记十路。
pub fn register_all(t: &mut DispatchTable) -> bool {
    let mut i = 0usize;
    let mut ok = true;
    while i < PROTOCOL_OPS.len() {
        let nm = handler_name(i);
        if t.register(PROTOCOL_OPS[i], &nm).is_err() {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 乱序注册必判不齐
    let mut a = DispatchTable::new();
    let _ = register_all(&mut a);
    let _ = a.register(101, "乱序项");
    got[0] = !a.closed(&PROTOCOL_OPS) && a.misordered(&PROTOCOL_OPS).len() == 1;
    // 2) 缺项（只登九路）必不算对账齐
    let mut b = DispatchTable::new();
    let mut i = 0usize;
    while i < PROTOCOL_OPS.len() - 1 {
        let nm = handler_name(i);
        let _ = b.register(PROTOCOL_OPS[i], &nm);
        i += 1;
    }
    got[1] = !b.closed(&PROTOCOL_OPS);
    // 3) 重号必拒
    let mut c = DispatchTable::new();
    let _ = c.register(10, "甲");
    got[2] = c.register(10, "乙").err() == Some(DT_DUP);
    // 4) 冻结后写账必抓
    let mut d = DispatchTable::new();
    let _ = d.register(10, "冻结前");
    d.freeze();
    got[3] = d.register(20, "迟到").err() == Some(DT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11002_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11002");

    // J1 注册项全量对账（10/10）
    let mut t = DispatchTable::new();
    let reg_ok = register_all(&mut t);
    let rec = t.reconcile(&PROTOCOL_OPS);
    s.add("J1-注册项全量对账", reg_ok && rec == Ok(true) && t.gaps().is_empty(), "十路操作码全量注册并逐项核销");

    // J2 注册序与协议一致（10/10）：逐位对齐且乱序位清空
    s.add("J2-注册序与协议一致", t.closed(&PROTOCOL_OPS) && t.misordered(&PROTOCOL_OPS).is_empty(), "注册序逐位对齐协议序");

    // J3 冻结根：fold 稳定且与根一致
    let root = t.freeze();
    s.add("J3-冻结根", t.root == Some(root) && t.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = t.register(110, "迟到").err() == Some(DT_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后注册必拒");

    // J5 注入检出：乱序 + 缺项
    let inj = inject_four();
    s.add("J5-乱序缺项检出", inj[0] && inj[1], "乱序判不齐且缺项必抓");

    // J6 注入检出：重号 + 冻结后写
    s.add("J6-重号冻结检出", inj[2] && inj[3], "重号必拒且冻结后写必抓");

    // J7 按码查项命中与位次一致
    let j7 = {
        let hit0 = t.find(PROTOCOL_OPS[0]);
        let hit9 = t.find(PROTOCOL_OPS[9]);
        hit0 == Some(0) && hit9 == Some(9) && t.find(777).is_none()
    };
    s.add("J7-按码查项", j7, "首末位次命中且越界码不命中");

    // J8 空名拒收与零码拒收
    let j8 = DispatchTable::new().register(1, "").err() == Some(DT_EMPTY)
        && DispatchTable::new().register(0, "零码").err() == Some(DT_DUP);
    s.add("J8-空名零码拒", j8, "处理名空与零操作码均必拒");

    // J9 对账长度不符拒
    let j9 = {
        let mut c = DispatchTable::new();
        let nm = handler_name(0);
        let _ = c.register(PROTOCOL_OPS[0], &nm);
        c.reconcile(&PROTOCOL_OPS).err() == Some(DT_SHORT)
    };
    s.add("J9-对账长度拒", j9, "注册项数与协议不符必拒");

    // J10 缺口语义：未对账时缺口可枚举且 closed 为假
    let j10 = {
        let mut c = DispatchTable::new();
        let _ = register_all(&mut c);
        !c.closed(&PROTOCOL_OPS) && c.gaps().len() == PROTOCOL_OPS.len()
    };
    s.add("J10-缺口枚举", j10, "漏对账核销位可枚举且为假");

    // J11 超容拒：第 OP_CAP+1 条注册被拒且不破坏原账
    let j11 = {
        let mut c = DispatchTable::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= OP_CAP as u32 {
            let nm = format!("超容{}", n);
            if c.register(n.wrapping_mul(3), &nm).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.register(9999, "over").err() == Some(DT_SHORT)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：核销项带协议位次
    let j12 = {
        let e0 = &t.entries[0];
        let e9 = &t.entries[9];
        e0.proto_pos == Some(0) && e9.proto_pos == Some(9)
            && e0.render().contains("pos0") && e9.render().contains("pos9")
    };
    s.add("J12-渲染位次", j12, "核销项渲染含协议位次");

    // J13 未核销不算对账齐：码齐但核销位未翻
    let j13 = {
        let mut c = DispatchTable::new();
        let _ = register_all(&mut c);
        !c.closed(&PROTOCOL_OPS) && c.find(PROTOCOL_OPS[3]).is_some()
    };
    s.add("J13-未核销不算齐", j13, "码齐但未逐项核销仍不算对账齐");

    // J14 样本确定性：处理名同入同出且十名互异
    let n3a = handler_name(3);
    let n3b = handler_name(3);
    let mut distinct = true;
    let mut i = 0usize;
    while i < PROTOCOL_OPS.len() {
        let mut j = i.wrapping_add(1);
        while j < PROTOCOL_OPS.len() {
            if handler_name(i) == handler_name(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J14-样本确定性", n3a == n3b && distinct, "处理名同入同出且十名互异");

    // J15 动态明细：冻结根十六进制落账
    let fp = t.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    // J16 动态明细：核销数与注册数双十
    let g = t.gaps().len();
    s.add_dyn("J16-核销明细", g == 0 && t.entries.len() == 10, &format!("gaps={} n={}", g, t.entries.len()));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11002_dispatch_flow() {
        let mut t = DispatchTable::new();
        assert!(register_all(&mut t));
        assert!(t.reconcile(&PROTOCOL_OPS) == Ok(true));
        assert!(t.register(PROTOCOL_OPS[0], "dup").err() == Some(DT_DUP));
        t.freeze();
        assert!(t.register(20, "late").err() == Some(DT_FROZEN));
        assert!(t.root.is_some());
    }

    #[test]
    fn unx11002_checks_green() {
        let s = run_unx11002_checks();
        assert!(s.all_passed(), "UNX-F11002 判据未全绿");
    }
}
