//! UNX-F11006 futex 错误矩阵挂点对账（E 型 B22 面联签）（0x1e0c0 段）
//!
//! 锚点：UNX-F11006-J1 错误码挂点逐项对账（10/10），E 型判据挂接零冲突
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：错误矩阵（B22 面十路错误码逐路挂点登记）+ 挂接对账器（每码恰一
//! 挂点、重挂即冲突即拒）+ 探针器（逐码注入探针必回对应诊断）+ fold 指纹
//! 与冻结 + 注入核（未挂码/重挂/空名/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e0c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e0c0;
pub const EM_UNMAPPED: u32 = SEG_BASE + 0x01; // 错误码未挂点
pub const EM_CONFLICT: u32 = SEG_BASE + 0x02; // 挂点重挂冲突
pub const EM_EMPTY: u32 = SEG_BASE + 0x03; // 诊断名空
pub const EM_CODE0: u32 = SEG_BASE + 0x04; // 零错误码
pub const EM_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const EM_CAP: u32 = SEG_BASE + 0x06; // 矩阵超容

/// B22 面十路错误码。
pub const ECODES: [u32; 10] = [11, 12, 22, 28, 61, 75, 84, 95, 98, 110];
/// 矩阵定容。
pub const MATRIX_CAP: usize = 14;
/// 错误矩阵域标（fold 种子）。
pub const EM_TAG: u64 = 0x1e0c_e6;

/// 单条挂点：错误码 + 挂点名 + 探针命中位。
#[derive(Clone, Debug)]
pub struct ErrHook {
    pub code: u32,
    pub name: String,
    pub probed: bool,
}

impl ErrHook {
    pub fn render(&self) -> String {
        let flag = if self.probed { "P" } else { "H" };
        format!("e{}|{}|{}", self.code, flag, self.name)
    }
}

/// futex 错误矩阵：逐码挂点 → 逐项对账 → 逐码探针 → 冻结。
pub struct ErrMatrix {
    pub hooks: Vec<ErrHook>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ErrMatrix {
    pub fn new() -> ErrMatrix {
        ErrMatrix { hooks: Vec::new(), frozen: false, root: None }
    }

    /// 挂点登记：冻结、空名、零码、重码、重挂点、超容均拒。
    pub fn hook(&mut self, code: u32, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(EM_FROZEN);
        }
        if name.is_empty() {
            return Err(EM_EMPTY);
        }
        if code == 0 {
            return Err(EM_CODE0);
        }
        if self.hooks.iter().any(|h| h.code == code) {
            return Err(EM_CONFLICT);
        }
        if self.hooks.iter().any(|h| h.name == name) {
            return Err(EM_CONFLICT);
        }
        if self.hooks.len() >= MATRIX_CAP {
            return Err(EM_CAP);
        }
        self.hooks.push(ErrHook { code, name: name.to_string(), probed: false });
        Ok(())
    }

    /// 逐码探针：命中挂点翻探针位并回报诊断名；未挂码必拒。
    pub fn probe(&mut self, code: u32) -> Result<String, u32> {
        if self.frozen {
            return Err(EM_FROZEN);
        }
        let mut k = 0usize;
        while k < self.hooks.len() {
            if self.hooks[k].code == code {
                self.hooks[k].probed = true;
                return Ok(self.hooks[k].name.to_string());
            }
            k += 1;
        }
        Err(EM_UNMAPPED)
    }

    /// 未探针码清单（按登记序）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.hooks.len() {
            if !self.hooks[k].probed {
                g.push(self.hooks[k].code);
            }
            k += 1;
        }
        g
    }

    /// 零冲突裁断：十路码全挂、全探针、重码零。
    pub fn closed(&self) -> bool {
        if self.hooks.len() != ECODES.len() || !self.gaps().is_empty() {
            return false;
        }
        let mut i = 0usize;
        while i < self.hooks.len() {
            let mut j = i.wrapping_add(1);
            while j < self.hooks.len() {
                if self.hooks[i].code == self.hooks[j].code
                    || self.hooks[i].name == self.hooks[j].name
                {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 按码查挂点位次。
    pub fn find(&self, code: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.hooks.len() {
            if self.hooks[k].code == code {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含码/名/探针位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = EM_TAG;
        let mut k = 0usize;
        while k < self.hooks.len() {
            let e = &self.hooks[k];
            h = (h ^ e.code as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.probed as u64).wrapping_mul(0x100000001b3);
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

/// 挂点名确定性文本。
pub fn hook_name(no: usize) -> String {
    format!("e型挂点{}", no)
}

/// 全矩阵搭建：十路码逐路挂点并逐码探针。
pub fn build_matrix() -> (ErrMatrix, bool) {
    let mut m = ErrMatrix::new();
    let mut i = 0usize;
    let mut ok = true;
    while i < ECODES.len() {
        let nm = hook_name(i);
        if m.hook(ECODES[i], &nm).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < ECODES.len() {
        if m.probe(ECODES[i]) != Ok(hook_name(i)) {
            ok = false;
        }
        i += 1;
    }
    let m_ok = ok && m.closed();
    (m, m_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未挂码探针必拒
    let mut a = ErrMatrix::new();
    let _ = a.hook(ECODES[0], "已挂码");
    got[0] = a.probe(12345).err() == Some(EM_UNMAPPED);
    // 2) 重挂码必拒（冲突）
    let mut b = ErrMatrix::new();
    let _ = b.hook(ECODES[0], "甲挂点");
    got[1] = b.hook(ECODES[0], "乙挂点").err() == Some(EM_CONFLICT);
    // 3) 重挂点必拒（同名异码）
    let mut c = ErrMatrix::new();
    let _ = c.hook(ECODES[0], "同名");
    got[2] = c.hook(ECODES[1], "同名").err() == Some(EM_CONFLICT);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_matrix();
    d.freeze();
    got[3] = d.hook(777, "迟到").err() == Some(EM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11006_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11006");

    // J1 错误码挂点逐项对账（10/10）
    let (mut m, built) = build_matrix();
    s.add("J1-挂点逐项对账", built && m.hooks.len() == 10 && m.gaps().is_empty(), "十路错误码全挂全探针");

    // J2 E 型判据挂接零冲突：码与名双重唯一
    s.add("J2-挂接零冲突", m.closed(), "码唯一且挂点名唯一零冲突");

    // J3 冻结根：fold 稳定且与根一致
    let root = m.freeze();
    s.add("J3-冻结根", m.root == Some(root) && m.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = m.hook(888, "迟到").err() == Some(EM_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后挂点必拒");

    // J5 注入检出：未挂码 + 重挂码
    let inj = inject_four();
    s.add("J5-未挂重挂检出", inj[0] && inj[1], "未挂码探针与重挂码两路必抓");

    // J6 注入检出：重挂点 + 冻结后写
    s.add("J6-重名冻结检出", inj[2] && inj[3], "同名异码必拒且冻结后写必抓");

    // J7 按码查点：首末命中且越界码不命中
    let j7 = m.find(ECODES[0]) == Some(0) && m.find(ECODES[9]) == Some(9) && m.find(424242).is_none();
    s.add("J7-按码查点", j7, "首末位次命中且未知码不命中");

    // J8 空名与零码拒
    let j8 = ErrMatrix::new().hook(1, "").err() == Some(EM_EMPTY)
        && ErrMatrix::new().hook(0, "零码").err() == Some(EM_CODE0);
    s.add("J8-空名零码拒", j8, "诊断名空与零错误码均必拒");

    // J9 缺口语义：漏探针三路时 gaps 可枚举且 closed 为假
    let j9 = {
        let mut c = ErrMatrix::new();
        let mut i = 0usize;
        while i < ECODES.len() {
            let nm = hook_name(i);
            let _ = c.hook(ECODES[i], &nm);
            i += 1;
        }
        let mut i = 0usize;
        while i < ECODES.len() - 3 {
            let _ = c.probe(ECODES[i]);
            i += 1;
        }
        c.gaps().len() == 3 && !c.closed()
    };
    s.add("J9-缺口枚举", j9, "漏探针三路可枚举且矩阵为假");

    // J10 渲染面：已探针与待探针标记可区分
    let j10 = {
        let a = ErrHook { code: 22, name: "渲染".to_string(), probed: true };
        let b = ErrHook { code: 22, name: "渲染".to_string(), probed: false };
        a.render() != b.render() && a.render().contains("P") && b.render().contains("H")
    };
    s.add("J10-渲染分旗", j10, "探针/待探渲染可区分");

    // J11 超容拒：第 MATRIX_CAP+1 条必拒
    let j11 = {
        let mut c = ErrMatrix::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= MATRIX_CAP as u32 {
            let nm = format!("超容{}", n);
            if c.hook(n.wrapping_mul(5), &nm).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.hook(99999, "over").err() == Some(EM_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 探针后码名回读一致：逐码回读与登记名一致
    let j12 = {
        let mut c = ErrMatrix::new();
        let mut i = 0usize;
        while i < ECODES.len() {
            let nm = hook_name(i);
            let _ = c.hook(ECODES[i], &nm);
            i += 1;
        }
        let mut i = 0usize;
        let mut ok = true;
        while i < ECODES.len() {
            if c.probe(ECODES[i]) != Ok(hook_name(i)) {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J12-回读一致", j12, "逐码探针回读与登记名一致");

    // J13 样本确定性：挂点名同入同出且十名互异
    let n3a = hook_name(3);
    let n3b = hook_name(3);
    let mut distinct = true;
    let mut i = 0usize;
    while i < ECODES.len() {
        let mut j = i.wrapping_add(1);
        while j < ECODES.len() {
            if hook_name(i) == hook_name(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J13-样本确定性", n3a == n3b && distinct, "挂点名同入同出且互异");

    // J14 动态明细：矩阵在册数与缺口数落账
    let g = m.gaps().len();
    s.add_dyn("J14-矩阵明细", g == 0 && m.hooks.len() == 10, &format!("hooks=10 gaps={}", g));

    // J15 动态明细：冻结根十六进制落账
    let fp = m.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11006_matrix_flow() {
        let (mut m, built) = build_matrix();
        assert!(built);
        assert!(m.hook(ECODES[0], "dup").err() == Some(EM_CONFLICT));
        assert!(m.probe(424242).err() == Some(EM_UNMAPPED));
        m.freeze();
        assert!(m.hook(1, "late").err() == Some(EM_FROZEN));
        assert!(m.root.is_some());
    }

    #[test]
    fn unx11006_checks_green() {
        let s = run_unx11006_checks();
        assert!(s.all_passed(), "UNX-F11006 判据未全绿");
    }
}
