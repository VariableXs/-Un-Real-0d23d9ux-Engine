//! UNX-F11012 epoll 错误码挂点对账（E 型 B26 面联签）（0x1e180 段）
//!
//! 锚点：UNX-F11012-J1 四错误码挂点逐项对平（10/10），错误路径挂号核销
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：错误路径账（B26 面四错误码逐路挂号：码 + 路径名 + 命中位）+
//! 核销器（逐码注入探针必命中对应路径、命中即核销）+ 零冲突裁断（码与路
//! 径名双唯一）+ fold 指纹与冻结 + 注入核（未挂码/核销后重挂/空名/冻结后
//! 写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e180 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e180;
pub const EP_UNMAPPED: u32 = SEG_BASE + 0x01; // 错误码未挂点
pub const EP_CONFLICT: u32 = SEG_BASE + 0x02; // 重挂冲突
pub const EP_EMPTY: u32 = SEG_BASE + 0x03; // 路径名空
pub const EP_CODE0: u32 = SEG_BASE + 0x04; // 零错误码
pub const EP_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const EP_CAP: u32 = SEG_BASE + 0x06; // 面板超容

/// B26 面四错误码（EXIST/NOENT/INVAL/PERM 对应模拟值）。
pub const E4: [u32; 4] = [17, 2, 22, 1];
/// 四错误码语义名。
pub const E4_NAMES: [&str; 4] = ["EXIST", "NOENT", "INVAL", "PERM"];
/// 面板定容。
pub const PANEL_CAP: usize = 8;
/// 错误路径账域标（fold 种子）。
pub const EP_TAG: u64 = 0x1e18_0c;

/// 单条错误路径：码 + 路径名 + 命中位 + 核销位。
#[derive(Clone, Debug)]
pub struct ErrPath {
    pub code: u32,
    pub name: String,
    pub hit: bool,
    pub settled: bool,
}

impl ErrPath {
    pub fn render(&self) -> String {
        let flag = if self.settled { "S" } else { "H" };
        format!("e{}|{}|{}", self.code, flag, self.name)
    }
}

/// epoll 错误路径账：逐码挂号 → 逐码探针命中 → 核销 → 冻结。
pub struct ErrPathLedger {
    pub paths: Vec<ErrPath>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ErrPathLedger {
    pub fn new() -> ErrPathLedger {
        ErrPathLedger { paths: Vec::new(), frozen: false, root: None }
    }

    /// 挂号：冻结、空名、零码、重码、重名、超容均拒。
    pub fn enroll(&mut self, code: u32, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(EP_FROZEN);
        }
        if name.is_empty() {
            return Err(EP_EMPTY);
        }
        if code == 0 {
            return Err(EP_CODE0);
        }
        if self.paths.iter().any(|p| p.code == code || p.name == name) {
            return Err(EP_CONFLICT);
        }
        if self.paths.len() >= PANEL_CAP {
            return Err(EP_CAP);
        }
        self.paths.push(ErrPath { code, name: name.to_string(), hit: false, settled: false });
        Ok(())
    }

    /// 逐码探针：命中即翻命中位并回报路径名；未挂码必拒。
    pub fn probe(&mut self, code: u32) -> Result<String, u32> {
        if self.frozen {
            return Err(EP_FROZEN);
        }
        let mut k = 0usize;
        while k < self.paths.len() {
            if self.paths[k].code == code {
                self.paths[k].hit = true;
                return Ok(self.paths[k].name.to_string());
            }
            k += 1;
        }
        Err(EP_UNMAPPED)
    }

    /// 核销：命中位已翻才可核销并翻核销位。
    pub fn settle(&mut self, code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(EP_FROZEN);
        }
        let mut k = 0usize;
        while k < self.paths.len() {
            if self.paths[k].code == code {
                if !self.paths[k].hit {
                    return Ok(false);
                }
                self.paths[k].settled = true;
                return Ok(true);
            }
            k += 1;
        }
        Err(EP_UNMAPPED)
    }

    /// 未核销码清单。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.paths.len() {
            if !self.paths[k].settled {
                g.push(self.paths[k].code);
            }
            k += 1;
        }
        g
    }

    /// 未命中先核销的越权探针清单（必为零）。
    pub fn illegal_settle(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.paths.len() {
            if self.paths[k].settled && !self.paths[k].hit {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 零冲突裁断：四码全挂全核销且无越权核销。
    pub fn closed(&self) -> bool {
        self.paths.len() == E4.len()
            && self.gaps().is_empty()
            && self.illegal_settle() == 0
    }

    /// 按码查路径位次。
    pub fn find(&self, code: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.paths.len() {
            if self.paths[k].code == code {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含码/名/双位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = EP_TAG;
        let mut k = 0usize;
        while k < self.paths.len() {
            let e = &self.paths[k];
            h = (h ^ e.code as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.hit as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.settled as u64).wrapping_mul(0x100000001b3);
            let bytes = e.name.as_bytes();
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

/// 路径名确定性文本。
pub fn ep_name(no: usize) -> String {
    format!("b26路径{}", no)
}

/// 四路全链：逐码挂号、探针、核销。
pub fn build_panel() -> (ErrPathLedger, bool) {
    let mut p = ErrPathLedger::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < E4.len() {
        if p.enroll(E4[i], &ep_name(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < E4.len() {
        if p.probe(E4[i]) != Ok(ep_name(i)) {
            ok = false;
        }
        if p.settle(E4[i]) != Ok(true) {
            ok = false;
        }
        i += 1;
    }
    let p_ok = ok && p.closed();
    (p, p_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未挂码探针必拒
    let mut a = ErrPathLedger::new();
    let _ = a.enroll(E4[0], "已挂");
    got[0] = a.probe(31337).err() == Some(EP_UNMAPPED);
    // 2) 重挂码必拒
    let mut b = ErrPathLedger::new();
    let _ = b.enroll(E4[0], "甲");
    got[1] = b.enroll(E4[0], "乙").err() == Some(EP_CONFLICT);
    // 3) 未命中先核销必拒（返回假且不翻核销位）
    let mut c = ErrPathLedger::new();
    let _ = c.enroll(E4[0], "未命中");
    let s = c.settle(E4[0]);
    got[2] = s == Ok(false) && !c.paths[0].settled;
    // 4) 冻结后写账必抓
    let (mut d, _) = build_panel();
    d.freeze();
    got[3] = d.enroll(999, "迟到").err() == Some(EP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11012_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11012");

    // J1 四错误码挂点逐项对平
    let (mut p, built) = build_panel();
    s.add("J1-四码挂点对平", built && p.paths.len() == 4 && p.gaps().is_empty(), "四码全挂全核销");

    // J2 错误路径挂号核销零越权
    s.add("J2-核销零越权", p.closed() && p.illegal_settle() == 0, "命中先于核销零越权");

    // J3 探针回读一致：逐码回读与语义名对齐
    let j3 = {
        let mut c = ErrPathLedger::new();
        let mut i = 0usize;
        while i < E4.len() {
            let _ = c.enroll(E4[i], &ep_name(i));
            i += 1;
        }
        let mut i = 0usize;
        let mut ok = true;
        while i < E4.len() {
            if c.probe(E4[i]) != Ok(ep_name(i)) {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J3-回读一致", j3, "逐码探针回读与挂号名一致");

    // J4 冻结根：fold 稳定且与根一致
    let root = p.freeze();
    s.add("J4-冻结根", p.root == Some(root) && p.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：未挂码 + 重挂码
    let inj = inject_four();
    s.add("J5-未挂重挂检出", inj[0] && inj[1], "未挂码与重挂码两路必抓");

    // J6 注入检出：越权核销 + 冻结后写
    s.add("J6-越权冻结检出", inj[2] && inj[3], "未命中核销必拒且冻结后写必抓");

    // J7 重名异码必拒（零冲突双唯一）
    let j7 = {
        let mut c = ErrPathLedger::new();
        let _ = c.enroll(E4[0], "同名");
        c.enroll(E4[1], "同名").err() == Some(EP_CONFLICT)
    };
    s.add("J7-重名拒", j7, "同路径名异码必拒");

    // J8 空名与零码拒
    let j8 = ErrPathLedger::new().enroll(1, "").err() == Some(EP_EMPTY)
        && ErrPathLedger::new().enroll(0, "零码").err() == Some(EP_CODE0);
    s.add("J8-空名零码拒", j8, "路径名空与零码均必拒");

    // J9 按码查路径：首末命中且未知码不命中
    let j9 = p.find(E4[0]) == Some(0) && p.find(E4[3]) == Some(3) && p.find(555).is_none();
    s.add("J9-按码查路径", j9, "首末位次命中且未知码不命中");

    // J10 渲染面：核销与命中标记可区分
    let j10 = {
        let a = ErrPath { code: 22, name: "渲染".to_string(), hit: true, settled: true };
        let b = ErrPath { code: 22, name: "渲染".to_string(), hit: true, settled: false };
        a.render() != b.render() && a.render().contains("S") && b.render().contains("H")
    };
    s.add("J10-渲染分旗", j10, "核销/命中渲染可区分");

    // J11 缺口语义：漏核销两路时 gaps 可枚举且 closed 为假
    let j11 = {
        let mut c = ErrPathLedger::new();
        let mut i = 0usize;
        while i < E4.len() {
            let _ = c.enroll(E4[i], &ep_name(i));
            i += 1;
        }
        let mut i = 0usize;
        while i < E4.len() - 2 {
            let _ = c.probe(E4[i]);
            let _ = c.settle(E4[i]);
            i += 1;
        }
        c.gaps().len() == 2 && !c.closed()
    };
    s.add("J11-缺口枚举", j11, "漏核销两路可枚举且面板为假");

    // J12 超容拒：第 PANEL_CAP+1 条必拒
    let j12 = {
        let mut c = ErrPathLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= PANEL_CAP as u32 {
            let nm = format!("超容{}", n);
            if c.enroll(n.wrapping_mul(7), &nm).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(99999, "over").err() == Some(EP_CAP)
    };
    s.add("J12-超容边界", j12, "超出定容必拒且原账不损");

    // J13 样本确定性：路径名同入同出且四名互异
    let n1a = ep_name(1);
    let n1b = ep_name(1);
    let mut distinct = true;
    let mut i = 0usize;
    while i < E4.len() {
        let mut j = i.wrapping_add(1);
        while j < E4.len() {
            if ep_name(i) == ep_name(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J13-样本确定性", n1a == n1b && distinct, "路径名同入同出且互异");

    // J14 动态明细：面板在册数与缺口数落账
    let g = p.gaps().len();
    s.add_dyn("J14-面板明细", g == 0 && p.paths.len() == 4, &format!("paths=4 gaps={}", g));

    // J15 动态明细：冻结根十六进制落账
    let fp = p.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11012_errpath_flow() {
        let (mut p, built) = build_panel();
        assert!(built);
        assert!(p.enroll(E4[0], "dup").err() == Some(EP_CONFLICT));
        assert!(p.probe(31337).err() == Some(EP_UNMAPPED));
        p.freeze();
        assert!(p.enroll(1, "late").err() == Some(EP_FROZEN));
        assert!(p.root.is_some());
    }

    #[test]
    fn unx11012_checks_green() {
        let s = run_unx11012_checks();
        assert!(s.all_passed(), "UNX-F11012 判据未全绿");
    }
}
