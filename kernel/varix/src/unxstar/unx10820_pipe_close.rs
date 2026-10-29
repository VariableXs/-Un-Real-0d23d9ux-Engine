//! UNX-F10820 管道 FIFO 域收口（B21 批次聚合）（0x1bf00 段）
//!
//! 锚点：UNX-F10820-J1 域收口对平——B21 段 19 张锚（UNX-F10801~UNX-F10819）
//! 全量登记、段号步进 0x20 单调、全绿后封账（红锚必阻断）、封账后写拒绝、
//! 计数对平 19/19 五线全对平；注入坏账（重复登记/步进断裂/红锚封账/封账
//! 后写/短账）必被检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：锚账（id × seg × 绿位）+ 收口判定器（步进/单调/计数/绿位四查）
//! + 封账状态机（红锚阻断、封后写拒）+ 折叠指纹 + 注入核（五类坏账必
//! 分歧）+ 聚合断言矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// B21 段锚范围：UNX-F10801 ~ UNX-F10819（19 张）。
pub const ANCHOR_FIRST: u32 = 10801;
pub const ANCHOR_LAST: u32 = 10819;
pub const ANCHOR_COUNT: usize = 19;

/// 段号步进：每卡独占 0x20。
pub const SEG_STEP: u32 = 0x20;

/// 诊断段（0x1bf00 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bf00;
pub const PC_AGG_DUP_ID: u32 = SEG_BASE + 0x01; // 锚 id 重复登记
pub const PC_AGG_STRIDE: u32 = SEG_BASE + 0x02; // 段号步进断裂
pub const PC_AGG_RED_SEAL: u32 = SEG_BASE + 0x03; // 红锚未阻断封账
pub const PC_AGG_SEALED: u32 = SEG_BASE + 0x04; // 封账后写未拒
pub const PC_AGG_COUNT: u32 = SEG_BASE + 0x05; // 计数对不平
pub const PC_AGG_MISS: u32 = SEG_BASE + 0x06; // 锚缺失

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PC_AGG_DUP_ID => "锚 id 重复登记",
        PC_AGG_STRIDE => "段号步进断裂",
        PC_AGG_RED_SEAL => "红锚未阻断封账",
        PC_AGG_SEALED => "封账后写未拒",
        PC_AGG_COUNT => "计数对不平",
        PC_AGG_MISS => "锚缺失",
        _ => "未知诊断码",
    }
}

/// 段号核：UNX-F10801=0x1bca0 起，步进 0x20。
pub fn seg_of(n: u32) -> u32 {
    0x1bca0 + SEG_STEP * (n - ANCHOR_FIRST)
}

/// 锚条目。
#[derive(Clone)]
pub struct Anchor {
    pub id: u32,
    pub seg: u32,
    pub green: bool,
}

/// 锚账：登记、置绿、对账、封账。
pub struct AnchorLedger {
    pub rows: Vec<Anchor>,
    pub sealed: bool,
    pub note: String,
}

impl AnchorLedger {
    pub fn new() -> AnchorLedger {
        AnchorLedger { rows: Vec::new(), sealed: false, note: "open".to_string() }
    }

    /// 登记锚：id 重复拒绝（PC_AGG_DUP_ID）；封账后写拒绝（PC_AGG_SEALED）。
    pub fn enroll(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PC_AGG_SEALED);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                return Err(PC_AGG_DUP_ID);
            }
            i += 1;
        }
        self.rows.push(Anchor { id, seg: seg_of(id), green: false });
        Ok(())
    }

    /// 置绿：按 id 定位。
    pub fn set_green(&mut self, id: u32) -> bool {
        let mut hit = false;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].id == id {
                self.rows[i].green = true;
                hit = true;
            }
            i += 1;
        }
        hit
    }

    /// 全绿判定。
    pub fn all_green(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].green {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 计数对账：登记数与期望锚数对平。
    pub fn count_ok(&self) -> bool {
        self.rows.len() == ANCHOR_COUNT
    }

    /// 锚范围对账：id 连续覆盖 ANCHOR_FIRST..=ANCHOR_LAST 无缺无重。
    pub fn coverage_ok(&self) -> bool {
        if self.rows.len() != ANCHOR_COUNT {
            return false;
        }
        let mut ok = true;
        let mut want = ANCHOR_FIRST;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].id != want {
                ok = false;
            }
            want += 1;
            i += 1;
        }
        ok
    }

    /// 步进对账：seg 单调递增且步进恒 0x20。
    pub fn stride_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 1usize;
        while i < self.rows.len() {
            if self.rows[i].seg != self.rows[i - 1].seg + SEG_STEP {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 封账：全绿才落 seal；红锚时红账阻断（PC_AGG_RED_SEAL）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if !self.all_green() {
            self.note = "red-blocked".to_string();
            return Err(PC_AGG_RED_SEAL);
        }
        self.sealed = true;
        self.note = "sealed".to_string();
        Ok(())
    }

    /// 折叠指纹：id/seg/绿位全量折入。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        let mut i = 0usize;
        while i < self.rows.len() {
            h = h.wrapping_mul(31).wrapping_add(self.rows[i].id);
            h = h.wrapping_mul(31).wrapping_add(self.rows[i].seg);
            h = h.wrapping_mul(31).wrapping_add(self.rows[i].green as u32);
            i += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 规范登记账：B21 全量 19 锚全绿封账。
pub fn canonical() -> AnchorLedger {
    let mut lg = AnchorLedger::new();
    let mut n = ANCHOR_FIRST;
    while n <= ANCHOR_LAST {
        let _ = lg.enroll(n);
        lg.set_green(n);
        n += 1;
    }
    let _ = lg.seal();
    lg
}

/// 审计行：登记账逐行 (id, seg, 绿位) 渲染。
pub fn audit_rows(lg: &AnchorLedger) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < lg.rows.len() {
        out.push(format!(
            "F{} seg={:#x} {}",
            lg.rows[i].id,
            lg.rows[i].seg,
            if lg.rows[i].green { "green" } else { "red" }
        ));
        i += 1;
    }
    out
}

/// 失配行：id 偏移或 seg 偏移的行数。
pub fn mismatches(lg: &AnchorLedger) -> usize {
    let mut n = 0usize;
    let mut want = ANCHOR_FIRST;
    let mut i = 0usize;
    while i < lg.rows.len() {
        if lg.rows[i].id != want || lg.rows[i].seg != seg_of(want) {
            n += 1;
        }
        want += 1;
        i += 1;
    }
    n
}

/// 注入 A：重复登记必拒。
pub fn inject_dup() -> bool {
    let mut lg = AnchorLedger::new();
    let r1 = lg.enroll(10801);
    let r2 = lg.enroll(10801);
    r1.is_ok() && r2.err() == Some(PC_AGG_DUP_ID) && lg.rows.len() == 1
}

/// 注入 B：步进断裂必检出（手改账面 seg）。
pub fn inject_stride() -> bool {
    let mut lg = AnchorLedger::new();
    let _ = lg.enroll(10801);
    let _ = lg.enroll(10802);
    let _ = lg.enroll(10803);
    lg.rows[1].seg += 0x10;
    !lg.stride_ok()
}

/// 注入 C：红锚封账必阻断。
pub fn inject_red_seal() -> bool {
    let mut lg = AnchorLedger::new();
    let mut n = ANCHOR_FIRST;
    while n <= ANCHOR_LAST {
        let _ = lg.enroll(n);
        if n != 10810 {
            lg.set_green(n);
        }
        n += 1;
    }
    lg.seal().err() == Some(PC_AGG_RED_SEAL) && !lg.sealed && lg.note == "red-blocked"
}

/// 注入 D：封账后写必拒。
pub fn inject_post_seal() -> bool {
    let mut lg = canonical();
    let before = lg.rows.len();
    lg.enroll(10830).err() == Some(PC_AGG_SEALED) && lg.rows.len() == before
}

/// 注入 E：短账（缺 10807）必计数/覆盖双失。
pub fn inject_short_count() -> bool {
    let mut lg = AnchorLedger::new();
    let mut n = ANCHOR_FIRST;
    while n <= ANCHOR_LAST {
        if n != 10807 {
            let _ = lg.enroll(n);
            lg.set_green(n);
        }
        n += 1;
    }
    !lg.count_ok() && !lg.coverage_ok() && lg.rows.len() == ANCHOR_COUNT - 1
}

/// 域收口判据（对应卡验收 J 判据）。
pub fn run_unx10820_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10820");
    let lg = canonical();
    let rows = audit_rows(&lg);

    // J1 聚合对平：19 锚全登记全绿封账
    s.add("J1-收口对平", lg.count_ok() && lg.all_green() && lg.sealed, "19/19 全绿封账");

    // J2 锚范围覆盖：10801..10819 连续无缺
    s.add("J2-覆盖核", lg.coverage_ok(), "id 连续无缺无重");

    // J3 段号步进：0x20 单调
    s.add("J3-步进核", lg.stride_ok(), "seg 单调步进 0x20");

    // J4 首尾锚段号字面：10801=0x1bca0 / 10819=0x1bee0
    let j4 = seg_of(10801) == 0x1bca0 && seg_of(10819) == 0x1bee0;
    s.add("J4-首尾段号", j4, "0x1bca0/0x1bee0 双锚");

    // J5 失配行零
    s.add("J5-失配零", mismatches(&lg) == 0, "id/seg 全对");

    // J6 审计行数与账行数一致
    s.add("J6-审计行核", rows.len() == ANCHOR_COUNT && rows[0].contains("F10801"), "行账一致");

    // J7 注入1 重复登记检出
    s.add("J7-重复登记检出", inject_dup(), "重复 id 必拒");

    // J8 注入2 步进断裂检出
    s.add("J8-步进断裂检出", inject_stride(), "手改 seg 必失平");

    // J9 注入3 红锚封账检出
    s.add("J9-红锚阻断检出", inject_red_seal(), "红锚封账必拒");

    // J10 注入4 封账后写检出
    s.add("J10-封后写检出", inject_post_seal(), "seal 后 enroll 必拒");

    // J11 注入5 短账检出
    s.add("J11-短账检出", inject_short_count(), "缺锚必计数双失");

    // J12 封账态注记闭集
    let j12 = lg.note == "sealed"
        && diag_text(PC_AGG_RED_SEAL) == "红锚未阻断封账"
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J12-注记闭集", j12, "sealed/red-blocked 两态可辨");

    // J13 (dyn) 折叠指纹落账
    let h = lg.fold();
    s.add_dyn("J13-折叠指纹", h != 0, &format!("fold={:#x} rows={}", h, rows.len()));

    // J14 (dyn) 计数明细
    s.add_dyn(
        "J14-计数明细",
        lg.count_ok(),
        &format!("anchors={}/{} sealed={}", lg.rows.len(), ANCHOR_COUNT, lg.sealed),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10820_ledger_flow() {
        let mut lg = AnchorLedger::new();
        assert!(lg.enroll(10801).is_ok());
        assert!(lg.enroll(10801).err() == Some(PC_AGG_DUP_ID));
        assert!(lg.seal().err() == Some(PC_AGG_RED_SEAL));
        lg.set_green(10801);
        assert!(lg.seal().is_ok() && lg.sealed);
        assert!(lg.enroll(10802).err() == Some(PC_AGG_SEALED));
        let lg2 = canonical();
        assert_eq!(seg_of(10801), 0x1bca0);
        assert_eq!(seg_of(10810), 0x1bca0 + 0x20 * 9);
        assert!(mismatches(&lg2) == 0);
        assert!(audit_rows(&lg2).len() == 19);
    }

    #[test]
    fn unx10820_checks_green() {
        let s = run_unx10820_checks();
        assert!(s.all_passed(), "UNX-F10820 判据未全绿");
    }
}
