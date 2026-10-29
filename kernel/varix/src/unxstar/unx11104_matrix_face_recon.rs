//! UNX-F11104 回归矩阵面总对账（B34 总账终核）（0x1f584 段）
//!
//! 锚点：UNX-F11104-J1 矩阵总账终核对平（10/10），常态化协议挂接终核
//! （10/10）。
//! 域：CoRun·UNX-C4-B36（回归矩阵面）。
//!
//! 结构：回归矩阵（RegMatrix：六批 × 四面的格点账，格态 0 未标/1 覆盖/
//! 2 豁免）+ 常态化协议挂接（ProtoHook：挂接位 + 复核位）+ 覆盖率闸 +
//! 冻结根 + 注入核（越界格/非法格态/重挂协议/覆盖不足四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f584 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f584;
pub const MX_RANGE: u32 = SEG_BASE + 0x01; // 格点越界
pub const MX_STATE: u32 = SEG_BASE + 0x02; // 非法格态
pub const MX_DUP: u32 = SEG_BASE + 0x03; // 协议重挂
pub const MX_UNMARKED: u32 = SEG_BASE + 0x04; // 有空格
pub const MX_COVER: u32 = SEG_BASE + 0x05; // 覆盖率不足
pub const MX_PROTO: u32 = SEG_BASE + 0x06; // 常态协议未挂齐
pub const MX_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const MX_OVER: u32 = SEG_BASE + 0x08; // 协议超容

/// 矩阵规模：六批 × 四面。
pub const ROWS: u32 = 6;
pub const COLS: u32 = 4;
/// 格点总数（24）。
pub const CELLS: usize = 24;
/// 覆盖率闸（千分率 950 = 95%）。
pub const COVER_GATE: u32 = 950;
/// 常态化协议条数。
pub const PROTOS: usize = 4;
/// 协议挂接定容。
pub const PROTO_CAP: usize = 8;
/// 矩阵面标（fold 种子）。
pub const MX_TAG: u64 = 0x1f5_8484;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单格三态：0 未标 / 1 覆盖 / 2 豁免。
pub fn cell_name(state: u8) -> &'static str {
    match state {
        0 => "未标",
        1 => "覆盖",
        2 => "豁免",
        _ => "非法",
    }
}

/// 常态化协议挂接项：挂接位 + 复核位。
#[derive(Clone, Debug, PartialEq)]
pub struct ProtoHook {
    pub no: u32,
    pub name: String,
    pub attached: bool,
    pub verified: bool,
}

impl ProtoHook {
    pub fn ready(&self) -> bool {
        self.attached && self.verified
    }

    pub fn render(&self) -> String {
        let a = if self.attached { "A" } else { "-" };
        let v = if self.verified { "V" } else { "-" };
        format!("N{}|{}|{}|{}", self.no, a, v, self.name)
    }
}

/// 回归矩阵：格点账 + 协议挂接账 + 冻结根。
#[derive(Clone, Debug)]
pub struct RegMatrix {
    pub rows: u32,
    pub cols: u32,
    pub cells: Vec<u8>,
    pub protos: Vec<ProtoHook>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl RegMatrix {
    pub fn new(rows: u32, cols: u32) -> RegMatrix {
        let n = (rows as usize).saturating_mul(cols as usize);
        RegMatrix {
            rows,
            cols,
            cells: alloc::vec![0u8; n],
            protos: Vec::new(),
            frozen: false,
            root: None,
        }
    }

    fn idx(&self, r: u32, c: u32) -> Option<usize> {
        if r >= self.rows || c >= self.cols {
            return None;
        }
        let i = (r as usize).saturating_mul(self.cols as usize).saturating_add(c as usize);
        if i < self.cells.len() {
            Some(i)
        } else {
            None
        }
    }

    /// 标格：越界拒、非法态拒、重复标同一态拒。
    pub fn mark(&mut self, r: u32, c: u32, state: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(MX_FROZEN);
        }
        if state > 2 {
            return Err(MX_STATE);
        }
        let i = self.idx(r, c).ok_or(MX_RANGE)?;
        if self.cells[i] == state && state != 0 {
            return Err(MX_DUP);
        }
        self.cells[i] = state;
        Ok(())
    }

    pub fn covered(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cells.len() {
            if self.cells[k] == 1 {
                n += 1;
            }
            k += 1;
        }
        n
    }

    pub fn exempt(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cells.len() {
            if self.cells[k] == 2 {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 行完成：该行无未标格。
    pub fn row_done(&self, r: u32) -> bool {
        if r >= self.rows {
            return false;
        }
        let mut c = 0u32;
        while c < self.cols {
            match self.idx(r, c) {
                Some(i) => {
                    if self.cells[i] == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            c += 1;
        }
        true
    }

    /// 覆盖率千分率（含豁免格）。
    pub fn coverage_permille(&self) -> u32 {
        if self.cells.is_empty() {
            return 0;
        }
        let marked = self.covered().saturating_add(self.exempt());
        (marked as u32).saturating_mul(1000) / self.cells.len() as u32
    }

    /// 常态化协议登记：重号拒、超容拒。
    pub fn hook(&mut self, no: u32, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(MX_FROZEN);
        }
        if name.is_empty() {
            return Err(MX_PROTO);
        }
        if self.protos.iter().any(|p| p.no == no) {
            return Err(MX_DUP);
        }
        if self.protos.len() >= PROTO_CAP {
            return Err(MX_OVER);
        }
        self.protos.push(ProtoHook {
            no,
            name: name.to_string(),
            attached: false,
            verified: false,
        });
        Ok(())
    }

    /// 挂接：未登记拒、重复挂接拒。
    pub fn attach(&mut self, no: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MX_FROZEN);
        }
        let p = self.proto_mut(no).ok_or(MX_PROTO)?;
        if p.attached {
            return Err(MX_DUP);
        }
        p.attached = true;
        Ok(())
    }

    /// 复核：未挂接不得复核。
    pub fn verify(&mut self, no: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MX_FROZEN);
        }
        let p = self.proto_mut(no).ok_or(MX_PROTO)?;
        if !p.attached {
            return Err(MX_PROTO);
        }
        p.verified = true;
        Ok(())
    }

    fn proto_mut(&mut self, no: u32) -> Option<&mut ProtoHook> {
        let mut k = 0usize;
        while k < self.protos.len() {
            if self.protos[k].no == no {
                return Some(&mut self.protos[k]);
            }
            k += 1;
        }
        None
    }

    /// 常态协议挂接终核：四条齐、全挂接、全复核。
    pub fn protos_ready(&self) -> bool {
        if self.protos.len() != PROTOS {
            return false;
        }
        let mut k = 0usize;
        while k < self.protos.len() {
            if !self.protos[k].ready() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 空格点名（行优先）。
    pub fn unmarked(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut r = 0u32;
        while r < self.rows {
            let mut c = 0u32;
            while c < self.cols {
                if let Some(i) = self.idx(r, c) {
                    if self.cells[i] == 0 {
                        v.push(r.saturating_mul(10).saturating_add(c));
                    }
                }
                c += 1;
            }
            r += 1;
        }
        v
    }

    /// 矩阵总账闭合：格点全标、覆盖率达闸、常态协议挂接齐。
    pub fn closed(&self) -> bool {
        self.cells.len() == CELLS
            && self.unmarked().is_empty()
            && self.coverage_permille() >= COVER_GATE
            && self.covered() > 0
            && self.protos_ready()
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 矩阵指纹：格态序 + 协议挂接链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = MX_TAG;
        h = (h ^ self.rows as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.cols as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.cells.len() {
            h = (h ^ self.cells[k] as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        let mut p = 0usize;
        while p < self.protos.len() {
            let it = &self.protos[p];
            h = (h ^ it.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ it.attached as u64).wrapping_mul(FP_STEP);
            h = (h ^ it.verified as u64).wrapping_mul(FP_STEP);
            let bytes = it.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            p += 1;
        }
        h
    }
}

/// 协议名样本。
pub fn proto_name(no: u32) -> String {
    format!("常态化回归协议 {}", no)
}

/// 标准矩阵：24 格全标（每行 3 覆盖 + 1 豁免），四条常态协议挂接并复核。
pub fn build_matrix() -> (RegMatrix, bool) {
    let mut m = RegMatrix::new(ROWS, COLS);
    let mut ok = true;
    let mut r = 0u32;
    while r < ROWS {
        let mut c = 0u32;
        while c < COLS {
            let st: u8 = if c == COLS - 1 { 2 } else { 1 };
            if m.mark(r, c, st).is_err() {
                ok = false;
            }
            c += 1;
        }
        r += 1;
    }
    let mut no = 1u32;
    while no <= PROTOS as u32 {
        if m.hook(no, &proto_name(no)).is_err() {
            ok = false;
        }
        if m.attach(no).is_err() {
            ok = false;
        }
        if m.verify(no).is_err() {
            ok = false;
        }
        no += 1;
    }
    (m, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 越界格必拒
    let mut a = RegMatrix::new(ROWS, COLS);
    got[0] = a.mark(ROWS, 0, 1).err() == Some(MX_RANGE)
        && a.mark(0, COLS, 1).err() == Some(MX_RANGE);
    // 2) 非法格态必拒
    let mut b = RegMatrix::new(ROWS, COLS);
    got[1] = b.mark(0, 0, 3).err() == Some(MX_STATE) && b.cells[0] == 0;
    // 3) 协议重挂必拒
    let mut c = RegMatrix::new(ROWS, COLS);
    let _ = c.hook(1, "甲");
    got[2] = c.hook(1, "乙").err() == Some(MX_DUP);
    // 4) 覆盖不足闭合为假（只标前 10 格）
    let mut d = RegMatrix::new(ROWS, COLS);
    let mut i = 0u32;
    while i < 10 {
        let _ = d.mark(i / COLS, i % COLS, 1);
        i += 1;
    }
    let mut no = 1u32;
    while no <= PROTOS as u32 {
        let _ = d.hook(no, &proto_name(no));
        let _ = d.attach(no);
        let _ = d.verify(no);
        no += 1;
    }
    got[3] = !d.closed() && d.unmarked().len() == 14 && d.coverage_permille() < COVER_GATE;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11104_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11104");

    let (mut m, built) = build_matrix();
    // J1 矩阵总账终核对平
    s.add(
        "J1-矩阵总账对平",
        built && m.cells.len() == CELLS && m.unmarked().is_empty(),
        "6×4=24 格逐格标定且无空格",
    );

    // J2 覆盖率终核达标
    s.add(
        "J2-覆盖率达闸",
        m.coverage_permille() == 1000 && m.covered() == 18 && m.exempt() == 6,
        "18 覆盖 + 6 豁免 = 1000‰",
    );

    // J3 常态化协议挂接终核
    s.add(
        "J3-常态挂接终核",
        m.protos_ready() && m.protos.len() == PROTOS,
        "四条常态协议全挂接并全复核",
    );

    // J4 矩阵闭合
    s.add("J4-矩阵闭合", m.closed(), "格点全标、覆盖达闸、协议挂齐");

    // J5 越界格拒
    let j5 = {
        let mut c = RegMatrix::new(ROWS, COLS);
        c.mark(ROWS, 0, 1).err() == Some(MX_RANGE)
            && c.mark(0, COLS, 1).err() == Some(MX_RANGE)
    };
    s.add("J5-越界格拒", j5, "行越界与列越界两路必拒");

    // J6 非法格态拒
    let j6 = {
        let mut c = RegMatrix::new(ROWS, COLS);
        c.mark(0, 0, 9).err() == Some(MX_STATE) && c.cells[0] == 0
    };
    s.add("J6-非法格态拒", j6, "格态大于 2 必拒且不改格");

    // J7 协议重挂拒
    let j7 = {
        let mut c = RegMatrix::new(ROWS, COLS);
        let _ = c.hook(1, "甲");
        let _ = c.attach(1);
        c.attach(1).err() == Some(MX_DUP) && c.hook(1, "乙").err() == Some(MX_DUP)
    };
    s.add("J7-协议重挂拒", j7, "同一协议二次登记与二次挂接必拒");

    // J8 未复核不算常态
    let j8 = {
        let mut c = RegMatrix::new(ROWS, COLS);
        let _ = c.hook(1, "未复核");
        let _ = c.attach(1);
        !c.protos_ready() && c.protos.len() == 1
    };
    s.add("J8-未复核不算", j8, "只挂接不复核不算常态齐备");

    // J9 覆盖不足不闭合
    let j9 = {
        let mut c = RegMatrix::new(ROWS, COLS);
        let mut i = 0u32;
        while i < 12 {
            let _ = c.mark(i / COLS, i % COLS, 1);
            i += 1;
        }
        !c.closed() && c.coverage_permille() == 500
    };
    s.add("J9-覆盖不足不闭合", j9, "半格覆盖时闭合为假");

    // J10 注入检出：越界 + 非法态
    let inj = inject_four();
    s.add("J10-越界非法态检出", inj[0] && inj[1], "越界格与非法格态两路必抓");

    // J11 注入检出：重挂 + 覆盖不足
    s.add("J11-重挂覆盖检出", inj[2] && inj[3], "协议重挂与覆盖不足两路必抓");

    // J12 逐行完成断言
    let j12 = {
        let mut r = 0u32;
        let mut all = true;
        while r < ROWS {
            if !m.row_done(r) {
                all = false;
            }
            r += 1;
        }
        all
    };
    s.add("J12-逐行完成", j12, "六行各自无未标格");

    // J13 冻结拒写
    let root = m.seal();
    let j13 = m.mark(0, 0, 1).err() == Some(MX_FROZEN)
        && m.hook(9, "追挂").err() == Some(MX_FROZEN)
        && m.attach(1).err() == Some(MX_FROZEN);
    s.add("J13-冻结拒写", j13, "封存后标格/登记/挂接三路必拒");

    // J14 冻结根稳定
    s.add(
        "J14-冻结根稳定",
        m.root == Some(root) && root == m.fold() && root != 0,
        "矩阵冻结根可重算零漂移",
    );

    // J15 格态名与渲染
    let j15 = cell_name(0) == "未标"
        && cell_name(1) == "覆盖"
        && cell_name(2) == "豁免"
        && m.protos[0].render().contains("A")
        && m.protos[0].render().contains("V");
    s.add("J15-格态渲染", j15, "三格态名与协议双旗渲染可区分");

    // J16 动态明细
    s.add_dyn(
        "J16-矩阵明细",
        m.closed() && m.fold() == root,
        &format!(
            "cov={} ex={} rate={} root={:#x}",
            m.covered(),
            m.exempt(),
            m.coverage_permille(),
            root
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11104_matrix_flow() {
        let (mut m, built) = build_matrix();
        assert!(built);
        assert!(m.closed());
        assert!(m.coverage_permille() == 1000);
        assert!(m.protos_ready());
        assert!(m.seal() != 0);
        assert!(m.mark(0, 0, 1).err() == Some(MX_FROZEN));
        assert!(RegMatrix::new(1, 1).mark(1, 0, 1).err() == Some(MX_RANGE));
    }

    #[test]
    fn unx11104_checks_green() {
        let s = run_unx11104_checks();
        assert!(s.all_passed(), "UNX-F11104 判据未全绿");
    }
}
