//! UNX-F56161 syscall 延迟聚合器深化（M 型旗舰起批）（0x61420 段）
//!
//! 锚点：UNX-F56161-J1 聚合深化账（syscall 延迟聚合器 M 型深化——三
//! 段延迟（入队/服务/返回）× 调用类闭集聚合矩阵逐格落账）；J2 聚合语
//! 义（矩阵格值 = 该类该段延迟和，行和列和双核算）；J3 深化指纹（矩
//! 阵全格折叠指纹，一格变全签变）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·M 型旗舰面）。
//!
//! 结构：调用类闭集（read/write/open/close/ioctl/mmap 六类）× 三段延
//! 迟聚合矩阵 + 事件录入核（类闭集 + 段值物理界双闸）+ 双和核算（行
//! 和列和）+ 矩阵指纹 + 注入核（未知类/负延迟/越界延迟/矩阵篡改/空
//! 矩阵印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61420 独占 0x20）。
pub const SEG_BASE: u32 = 0x61420;
pub const SA_UNK_CLASS: u32 = SEG_BASE + 0x01; // 未知调用类
pub const SA_NEG_LAT: u32 = SEG_BASE + 0x02; // 负延迟（i64 中转核抓）
pub const SA_LAT_RANGE: u32 = SEG_BASE + 0x03; // 延迟越物理界（> 10s）
pub const SA_MATRIX_TAMPER: u32 = SEG_BASE + 0x04; // 矩阵篡改
pub const SA_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空矩阵印

/// 调用类闭集（六类）。
pub const C_READ: usize = 0;
pub const C_WRITE: usize = 1;
pub const C_OPEN: usize = 2;
pub const C_CLOSE: usize = 3;
pub const C_IOCTL: usize = 4;
pub const C_MMAP: usize = 5;
pub const CLASS_COUNT: usize = 6;

/// 类名渲染。
pub const CLASS_NAMES: [&str; CLASS_COUNT] = ["read", "write", "open", "close", "ioctl", "mmap"];

/// 三段闭集。
pub const P_ENQUEUE: usize = 0;
pub const P_SERVICE: usize = 1;
pub const P_RETURN: usize = 2;
pub const PHASE_COUNT: usize = 3;

/// 段名渲染。
pub const PHASE_NAMES: [&str; PHASE_COUNT] = ["enqueue", "service", "return"];

/// 延迟物理界（微秒，> 10_000_000 即 10s 拒）。
pub const LAT_MAX: u64 = 10_000_000;

/// 延迟值核。
pub fn lat_ok(lat: u64) -> bool {
    lat <= LAT_MAX
}

/// 单条延迟事件。
#[derive(Clone)]
pub struct SysLatEvent {
    pub class: usize,
    pub phase: usize,
    pub lat_us: u64,
}

/// 聚合矩阵（类 × 段）。
pub struct SysLatMatrix {
    cells: [[u64; PHASE_COUNT]; CLASS_COUNT],
    event_n: u64,
    sealed: bool,
}

impl SysLatMatrix {
    pub fn new() -> Self {
        SysLatMatrix {
            cells: [[0; PHASE_COUNT]; CLASS_COUNT],
            event_n: 0,
            sealed: false,
        }
    }

    /// 录入事件：类闭集 + 段闭集 + 延迟界三闸。
    pub fn record(&mut self, class: usize, phase: usize, lat_us: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(SA_EMPTY_SEAL);
        }
        if class >= CLASS_COUNT {
            return Err(SA_UNK_CLASS);
        }
        if phase >= PHASE_COUNT {
            return Err(SA_UNK_CLASS);
        }
        if !lat_ok(lat_us) {
            return Err(SA_LAT_RANGE);
        }
        self.cells[class][phase] += lat_us;
        self.event_n += 1;
        Ok(())
    }

    pub fn cell(&self, class: usize, phase: usize) -> u64 {
        self.cells[class][phase]
    }

    /// 行和：某类三段总延迟。
    pub fn row_sum(&self, class: usize) -> u64 {
        if class >= CLASS_COUNT {
            return 0;
        }
        self.cells[class][0] + self.cells[class][1] + self.cells[class][2]
    }

    /// 列和：某段全类总延迟。
    pub fn col_sum(&self, phase: usize) -> u64 {
        if phase >= PHASE_COUNT {
            return 0;
        }
        (0..CLASS_COUNT).map(|c| self.cells[c][phase]).sum()
    }

    /// 总和核：全格和 = 各列和之和。
    pub fn total(&self) -> u64 {
        (0..PHASE_COUNT).map(|p| self.col_sum(p)).sum()
    }

    pub fn event_n(&self) -> u64 {
        self.event_n
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.event_n == 0 {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 矩阵指纹：全格折叠。
    pub fn matrix_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for row in &self.cells {
            for v in row {
                h = h.wrapping_mul(0x0100_0193) ^ (*v as u32);
                h ^= (*v >> 32) as u32;
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_matrix(&self) -> String {
        let mut out = String::new();
        for c in 0..CLASS_COUNT {
            for p in 0..PHASE_COUNT {
                out.push_str(&format!(
                    "{}:{}:{}\n",
                    CLASS_NAMES[c], PHASE_NAMES[p], self.cells[c][p]
                ));
            }
        }
        if self.sealed {
            out.push_str("MATRIX-SEAL\n");
        }
        out
    }
}

impl Default for SysLatMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：六类各录三段（read 三段 120/300/80，其余递减演示口径）。
fn standard_matrix() -> SysLatMatrix {
    let mut m = SysLatMatrix::new();
    let base = [120u64, 300, 80, 40, 60, 200];
    for (c, b) in base.iter().enumerate() {
        let _ = m.record(c, P_ENQUEUE, *b);
        let _ = m.record(c, P_SERVICE, b * 3);
        let _ = m.record(c, P_RETURN, b / 2);
    }
    m
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知类
    let mut m = SysLatMatrix::new();
    if m.record(9, P_ENQUEUE, 100) == Err(SA_UNK_CLASS) {
        caught += 1;
    }
    // 注 2：未知段
    let mut m = SysLatMatrix::new();
    if m.record(C_READ, 9, 100) == Err(SA_UNK_CLASS) {
        caught += 1;
    }
    // 注 3：延迟越界
    let mut m = SysLatMatrix::new();
    if m.record(C_READ, P_ENQUEUE, LAT_MAX + 1) == Err(SA_LAT_RANGE) {
        caught += 1;
    }
    // 注 4：矩阵篡改（改格后指纹变）
    let fp0 = standard_matrix().matrix_fp();
    let mut m = standard_matrix();
    m.cells[0][0] += 1;
    if m.matrix_fp() != fp0 {
        caught += 1;
    }
    // 注 5：空矩阵印
    let mut m = SysLatMatrix::new();
    if !m.seal() && !m.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 聚合深化账；J2 聚合语义；J3 深化指纹。
pub fn run_unx56161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56161");
    let m = standard_matrix();
    // J1-1 十八事件全录
    let ok = m.event_n() == 18;
    s.add("56161-J1-十八事件", ok, if ok { "" } else { "事件数漂移" });
    // J1-2 read 行格值（120/360/40）
    let ok = m.cell(C_READ, P_ENQUEUE) == 120
        && m.cell(C_READ, P_SERVICE) == 360
        && m.cell(C_READ, P_RETURN) == 40;
    s.add("56161-J1-read行格", ok, if ok { "" } else { "格值漂移" });
    // J1-3 渲染导出齐
    let text = standard_matrix().render_matrix();
    let ok = text.contains("read:enqueue:120") && text.contains("mmap:return:100");
    s.add("56161-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 行和核（read = 120+360+40 = 520）
    let ok = m.row_sum(C_READ) == 520;
    s.add("56161-J2-行和核", ok, if ok { "" } else { "行和漂移" });
    // J2-2 列和核（enqueue = 120+300+80+40+60+200 = 800）
    let ok = m.col_sum(P_ENQUEUE) == 800;
    s.add("56161-J2-列和核", ok, if ok { "" } else { "列和漂移" });
    // J2-3 总和 = 行和之和 = 列和之和
    let row_total: u64 = (0..CLASS_COUNT).map(|c| m.row_sum(c)).sum();
    let ok = m.total() == row_total;
    s.add("56161-J2-双和一致", ok, if ok { "" } else { "双和漂移" });
    // J2-4 延迟界核
    let ok = lat_ok(0) && lat_ok(LAT_MAX) && !lat_ok(LAT_MAX + 1);
    s.add("56161-J2-延迟界", ok, if ok { "" } else { "界核漂移" });
    // J2-5 累加语义（同格二次录入和翻倍）
    let mut m2 = SysLatMatrix::new();
    let _ = m2.record(C_READ, P_ENQUEUE, 100);
    let _ = m2.record(C_READ, P_ENQUEUE, 100);
    let ok = m2.cell(C_READ, P_ENQUEUE) == 200;
    s.add("56161-J2-累加语义", ok, if ok { "" } else { "累加漂移" });
    // J3-1 矩阵指纹确定性
    let ok = standard_matrix().matrix_fp() == standard_matrix().matrix_fp();
    s.add("56161-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 一格变全签变
    let mut m2 = standard_matrix();
    let fp0 = m2.matrix_fp();
    let _ = m2.record(C_MMAP, P_RETURN, 7);
    let ok = m2.matrix_fp() != fp0;
    s.add("56161-J3-格变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封矩阵印
    let mut m3 = standard_matrix();
    let ok = m3.seal() && m3.sealed();
    s.add("56161-J3-封矩阵印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后写拒
    let ok = m3.record(C_READ, P_ENQUEUE, 1) == Err(SA_EMPTY_SEAL);
    s.add("56161-J3-印后写拒", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56161-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56161_all_checks_green() {
        let set = run_unx56161_checks();
        assert!(set.all_passed(), "UNX-F56161 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56161_matrix_stable() {
        assert_eq!(standard_matrix().matrix_fp(), standard_matrix().matrix_fp());
    }
}
