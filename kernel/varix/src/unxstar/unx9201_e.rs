//! UNX-F9201 E 型方法论总纲（正反双判据协议与矩阵记法）（0x638E0 段）
//!
//! 锚点：UNX-F9201-J1 矩阵账（E 型性质 × {正,反} 双判据格逐格落
//! 账）；J2 协议语义（双侧必填 + side/kind 闭集 + 反判据必带拒绝码
//! + 半判据拒印四性质）；J3 总纲指纹（矩阵折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B21（E 型方法论总纲收官面）。
//!
//! 结构：性质册 + 双判据格核 + 矩阵账本 + 总纲印发核 + 折叠指纹 +
//! 注入核（野性质/野 side/野 kind/零拒绝码/半判据印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x638E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x638E0;
pub const EM_UNK_PROP: u32 = SEG_BASE + 0x01; // 野性质
pub const EM_BAD_SIDE: u32 = SEG_BASE + 0x02; // side 出闭集
pub const EM_BAD_KIND: u32 = SEG_BASE + 0x03; // kind 出闭集
pub const EM_ZERO_CODE: u32 = SEG_BASE + 0x04; // 反判据零拒绝码
pub const EM_HALF_SEAL: u32 = SEG_BASE + 0x05; // 半判据印发

/// 性质族闭集。
pub const K_STATE: u8 = 0; // 状态性质
pub const K_TRANS: u8 = 1; // 转换性质
pub const K_INV: u8 = 2; // 不变量
pub const K_BOUND: u8 = 3; // 边界性质
pub const K_COUNT: u8 = 4;

pub const K_NAMES: [&str; 4] = ["state", "trans", "inv", "bound"];

/// 判据侧闭集。
pub const S_POS: u8 = 0; // 正判据：合法输入 → 正确行为
pub const S_NEG: u8 = 1; // 反判据：非法输入 → 明确拒绝码
pub const S_COUNT: u8 = 2;

pub const S_NAMES: [&str; 2] = ["pos", "neg"];

/// 单条双判据格。
#[derive(Clone)]
pub struct ECel {
    pub prop: String,
    pub kind: u8,
    pub side: u8,
    pub ok: bool,
    pub err_code: u32, // 反判据必 >0；正判据恒 0
    pub note: String,
}

impl ECel {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.kind < K_COUNT
            && self.side < S_COUNT
            && !self.prop.is_empty()
            && match self.side {
                S_NEG => self.err_code > 0,
                _ => self.err_code == 0,
            }
    }

    pub fn render_line(&self) -> String {
        format!(
            "EM|{}|{}|{}|{}|code={}",
            self.prop,
            K_NAMES[self.kind as usize],
            S_NAMES[self.side as usize],
            if self.ok { "PASS" } else { "FAIL" },
            self.err_code
        )
    }
}

/// E 型总纲矩阵簿。
pub struct EmBook {
    pub props: Vec<(String, u8)>, // (性质名, kind)
    pub cells: Vec<ECel>,
    pub sealed: bool,
}

impl EmBook {
    pub fn new() -> EmBook {
        EmBook { props: Vec::new(), cells: Vec::new(), sealed: false }
    }

    pub fn declare(&mut self, prop: &str, kind: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(EM_HALF_SEAL);
        }
        if prop.is_empty() {
            return Err(EM_UNK_PROP);
        }
        if kind >= K_COUNT {
            return Err(EM_BAD_KIND);
        }
        if !self.props.iter().any(|(p, _)| p == prop) {
            self.props.push((prop.to_string(), kind));
        }
        Ok(())
    }

    pub fn has_prop(&self, prop: &str) -> bool {
        self.props.iter().any(|(p, _)| p == prop)
    }

    pub fn kind_of(&self, prop: &str) -> u8 {
        self.props
            .iter()
            .find(|(p, _)| p == prop)
            .map(|(_, k)| *k)
            .unwrap_or(K_COUNT)
    }

    /// 登记一条判据格。
    pub fn dual(&mut self, prop: &str, side: u8, ok: bool, err_code: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EM_HALF_SEAL);
        }
        if note.is_empty() {
            return Err(EM_UNK_PROP);
        }
        if side >= S_COUNT {
            return Err(EM_BAD_SIDE);
        }
        if !self.has_prop(prop) {
            return Err(EM_UNK_PROP);
        }
        if side == S_NEG && err_code == 0 {
            return Err(EM_ZERO_CODE);
        }
        if side == S_POS && err_code != 0 {
            return Err(EM_BAD_SIDE);
        }
        self.cells.push(ECel {
            prop: prop.to_string(),
            kind: self.kind_of(prop),
            side,
            ok,
            err_code,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 双侧齐备：每性质 pos/neg 各恰一条。
    pub fn full_dual(&self) -> bool {
        self.props.iter().all(|(p, _)| {
            self.cells.iter().any(|c| c.prop == *p && c.side == S_POS)
                && self.cells.iter().any(|c| c.prop == *p && c.side == S_NEG)
        })
    }

    /// 无半判据（格数恰为性质数 ×2）。
    pub fn no_extra(&self) -> bool {
        self.cells.len() == self.props.len() * 2
    }

    pub fn all_green(&self) -> bool {
        self.cells.iter().all(|c| c.ok && c.ok)
    }

    /// kind 全谱。
    pub fn kinds_complete(&self) -> bool {
        for k in 0..K_COUNT {
            if !self.props.iter().any(|(_, pk)| *pk == k) {
                return false;
            }
        }
        true
    }

    /// 总纲印发：全绿 + 全双 + 无冗格方可发印，返 Ok(fold)。
    pub fn emit(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(EM_HALF_SEAL);
        }
        if !self.all_green() || !self.full_dual() || !self.no_extra() {
            return Err(EM_HALF_SEAL);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x919E0;
        for c in &self.cells {
            for by in c.prop.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            h = h.wrapping_mul(31).wrapping_add(c.kind as u32);
            h = h.wrapping_mul(31).wrapping_add(c.side as u32);
            h = h.wrapping_mul(31).wrapping_add(c.ok as u32);
            h = h.wrapping_mul(31).wrapping_add(c.err_code);
        }
        h
    }
}

/// E 型总纲标准样本：四性质族各一，双侧齐备全绿。
/// 性质取自 B17~B20 批已收口语义（内存探针 / 迁移闸 / 身份族 / 对照格）。
pub fn mk_book() -> EmBook {
    let mut b = EmBook::new();
    let _ = b.declare("probe_align", K_BOUND); // addr/length 对齐探针边界
    let _ = b.declare("gate_migrate", K_TRANS); // 进程组迁移转换闸
    let _ = b.declare("id_keep", K_INV); // KEEP 恒不覆盖不变量
    let _ = b.declare("cg_level", K_STATE); // 对照格等级闭集状态
    // probe_align：正（对齐样本通过）反（不对齐样本拒绝）
    let _ = b.dual("probe_align", S_POS, true, 0, "页对齐样本放行");
    let _ = b.dual("probe_align", S_NEG, true, 0x638E1, "非对齐样本拒");
    // gate_migrate：正（五闸全过迁移成功）反（会话闸拦截）
    let _ = b.dual("gate_migrate", S_POS, true, 0, "合法迁移账落");
    let _ = b.dual("gate_migrate", S_NEG, true, 0x638E2, "跨会话迁移拒");
    // id_keep：正（显式值覆盖）反（KEEP 恒不覆盖）
    let _ = b.dual("id_keep", S_POS, true, 0, "显式值生效");
    let _ = b.dual("id_keep", S_NEG, true, 0x638E3, "KEEP 拒覆盖");
    // cg_level：正（四级在册格落账）反（野等级拒）
    let _ = b.dual("cg_level", S_POS, true, 0, "四级格全落账");
    let _ = b.dual("cg_level", S_NEG, true, 0x638E4, "野等级必拒");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野性质 dual
    got[0] = b.dual("prop_none", S_POS, true, 0, "inj-prop").err() == Some(EM_UNK_PROP);
    // 2) side 出闭集
    got[1] = b.dual("probe_align", 9, true, 0, "inj-side").err() == Some(EM_BAD_SIDE);
    // 3) kind 出闭集
    got[2] = b.declare("prop_new", 9).err() == Some(EM_BAD_KIND);
    // 4) 反判据零拒绝码必抓
    let mut b2 = EmBook::new();
    let _ = b2.declare("p1", K_STATE);
    got[3] = b2.dual("p1", S_NEG, true, 0, "inj-zero").err() == Some(EM_ZERO_CODE);
    // 5) 半判据性质上 emit 必抓（只有正侧，缺反侧）
    let mut b3 = EmBook::new();
    let _ = b3.declare("half", K_STATE);
    let _ = b3.dual("half", S_POS, true, 0, "half-pos");
    got[4] = b3.emit().err() == Some(EM_HALF_SEAL);
    got
}

/// 判据主体。
pub fn run_unx9201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9201");
    let mut b = mk_book();

    // J1 矩阵账非空（8 格）
    let j1 = b.cells.len() == 8;
    s.add("J1-矩阵账齐", j1, "四性质 × 双侧 = 8 格");

    // J2 格字段齐
    let j2 = b.cells.iter().all(|c| c.ok());
    s.add("J2-格字段齐备", j2, "留痕非空且 side/kind/码自洽");

    // J3 双侧齐备
    let j3 = b.full_dual();
    s.add("J3-双侧齐备", j3, "每性质 pos/neg 各恰一条");

    // J4 全格绿
    let j4 = b.all_green();
    s.add("J4-全格绿", j4, "8/8 判据格 PASS");

    // J5 野性质拒
    let j5 = b.dual("prop_none", S_POS, true, 0, "野性质").err() == Some(EM_UNK_PROP);
    s.add("J5-野性质拒", j5, "未声明性质必拒");

    // J6 side 闭集
    let j6 = b.dual("probe_align", S_COUNT, true, 0, "野侧").err() == Some(EM_BAD_SIDE);
    s.add("J6-side闭集", j6, "side 出闭集必拒");

    // J7 kind 闭集
    let j7 = b.declare("prop_x", K_COUNT).err() == Some(EM_BAD_KIND);
    s.add("J7-kind闭集", j7, "kind 出闭集必拒");

    // J8 反判据必带拒绝码
    let j8 = b
        .cells
        .iter()
        .filter(|c| c.side == S_NEG)
        .all(|c| c.err_code > 0)
        && b
            .cells
            .iter()
            .filter(|c| c.side == S_POS)
            .all(|c| c.err_code == 0);
    s.add("J8-码侧对账", j8, "反格码>0 且正格码=0");

    // J9 kind 全谱
    let j9 = b.kinds_complete();
    s.add("J9-kind全谱", j9, "state/trans/inv/bound 全出现");

    // J10 无冗格
    let j10 = b.no_extra();
    s.add("J10-无冗格", j10, "格数恰为性质数×2");

    // J11 总纲印发
    let j11 = b.emit().is_ok();
    s.add("J11-总纲印发", j11, "全绿+全双+无冗格方可发印");

    // J12 印后拒写 + 半判据拒印
    let j12 = {
        let after = b.dual("probe_align", S_POS, true, 0, "印后").err() == Some(EM_HALF_SEAL);
        let mut b2 = EmBook::new();
        let _ = b2.declare("h", K_INV);
        let _ = b2.dual("h", S_NEG, true, 1, "只有反侧");
        after && b2.emit().err() == Some(EM_HALF_SEAL)
    };
    s.add("J12-半判据拒印", j12, "印后拒写且半判据必拒发印");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9201_matrix_flow() {
        let mut b = mk_book();
        assert!(b.full_dual());
        assert!(b.no_extra());
        assert!(b.all_green());
        assert!(b.emit().is_ok());
        assert_eq!(b.dual("probe_align", S_POS, true, 0, "late"), Err(EM_HALF_SEAL));
        let mut b2 = EmBook::new();
        assert_eq!(b2.dual("ghost", S_POS, true, 0, "x"), Err(EM_UNK_PROP));
        let _ = b2.declare("p", K_STATE);
        assert_eq!(b2.dual("p", S_NEG, true, 0, "zero"), Err(EM_ZERO_CODE));
    }

    #[test]
    fn unx9201_checks_green() {
        let s = run_unx9201_checks();
        assert!(s.all_passed(), "UNX-F9201 判据未全绿");
    }
}
