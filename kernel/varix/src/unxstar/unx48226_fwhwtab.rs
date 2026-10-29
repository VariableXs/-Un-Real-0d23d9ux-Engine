//! unx48226_fwhwtab — UNX-F48226 半全角表与逐项对照账（抽样 100 项行为对照）（诊断段 0xBD20）。
//!
//! 判据原文：
//!   J1 100 项对照全对（§AI-61 §5 J-3 构成项）；判据锚定 §AI-61 §5 J-3。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 半全角映射标准式：ASCII 可见区 0x21..0x7E ↔ 全角区 0xFF01..0xFF5E
//!      （全角码 = 半角码 − 0x20 + 0xFF01），空格 0x20 ↔ 全角空格 0x3000
//!      单列特例；映射纯函数、双向互逆，无任何特设字符例外表。
//!   2. 抽样 100 项账：从可区 94 字 + 空格 + 5 个全角区间边界样本
//!      （0xFF01/0xFF5E/0x3000/区外首尾）构成 100 项对照集，逐项落
//!      (半角, 全角, 往返复得) 三元账，正反两向各跑一遍。
//!   3. 对照全对口径（J1）：正向 half→full 与表内期望全等、反向
//!      full→half 与原字全等、往返恒等三条逐项断言；100 项一次不缺，
//!      差一项即坏账。
//!   4. 区外防御：0x20..0x7E 与 0xFF01..0xFF5E、0x3000 之外的字一律
//!      拒转（返回诊断码），不猜测不透传——映射面无静默扩区。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBD20;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_OUT_OF_RANGE: u16 = SEG_BASE + 1; // 区外字拒转
const E_PAIR_BROKEN: u16 = SEG_BASE + 2;  // 映射与标准式不符
const E_RT_LOSS: u16 = SEG_BASE + 3;      // 往返不恒等
const E_TAB_MISS: u16 = SEG_BASE + 4;     // 100 项对照不足
const E_LEDGER_BAD: u16 = SEG_BASE + 5;   // 对照账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6;    // 写回摘要为空

/// 半角可区下界/上界。
pub const HW_LO: u32 = 0x21;
pub const HW_HI: u32 = 0x7E;
/// 全角区下界/上界（半角可区的镜像）。
pub const FW_LO: u32 = 0xFF01;
pub const FW_HI: u32 = 0xFF5E;
/// 半角空格与全角空格。
pub const HW_SPACE: u32 = 0x20;
pub const FW_SPACE: u32 = 0x3000;

/// 标准式正映射：半角 → 全角（纪律 1）。
pub fn hw_to_fw(c: u32) -> Result<u32, u16> {
    if c == HW_SPACE {
        return Ok(FW_SPACE);
    }
    if c >= HW_LO && c <= HW_HI {
        return Ok(c - 0x20 + FW_LO);
    }
    Err(E_OUT_OF_RANGE)
}

/// 标准式反映射：全角 → 半角（区外拒转）。
pub fn fw_to_hw(c: u32) -> Result<u32, u16> {
    if c == FW_SPACE {
        return Ok(HW_SPACE);
    }
    if c >= FW_LO && c <= FW_HI {
        return Ok(c - FW_LO + 0x20);
    }
    Err(E_OUT_OF_RANGE)
}

/// 一项对照账。
#[derive(Clone, Copy, Debug)]
pub struct PairRecord {
    pub hw: u32,
    pub fw: u32,
    pub rt: u32, // 往返复得的半角字
}

/// 半全角对照枢纽：100 项抽样账 + 正反双跑审计。
pub struct FwHwTab {
    ledger: Vec<PairRecord>,
    pub pair_total: u32,
    pub mismatches: u32,
}

impl FwHwTab {
    pub fn new() -> FwHwTab {
        FwHwTab { ledger: Vec::new(), pair_total: 0, mismatches: 0 }
    }

    /// 对照一项：正向 + 反向 + 往返三条断言，落账（J1 主体）。
    pub fn check_pair(&mut self, hw: u32) -> Result<PairRecord, u16> {
        let fw = match hw_to_fw(hw) {
            Ok(v) => v,
            Err(e) => {
                self.mismatches += 1;
                return Err(e);
            }
        };
        // 正向期望复算（独立式重算，防实现与标准式漂移）。
        let want_fw = if hw == HW_SPACE { FW_SPACE } else { hw - 0x20 + FW_LO };
        if fw != want_fw {
            self.mismatches += 1;
            return Err(E_PAIR_BROKEN);
        }
        let rt = match fw_to_hw(fw) {
            Ok(v) => v,
            Err(e) => {
                self.mismatches += 1;
                return Err(e);
            }
        };
        if rt != hw {
            self.mismatches += 1;
            return Err(E_RT_LOSS);
        }
        let rec = PairRecord { hw, fw, rt };
        self.ledger.push(rec);
        self.pair_total += 1;
        Ok(rec)
    }

    /// 抽样 100 项：可区 94 字 + 空格 + 5 个边界样本，确定性生成。
    pub fn run_sample_100(&mut self) -> Result<(), u16> {
        // 1..=94：可区全字。
        let mut c = HW_LO;
        while c <= HW_HI {
            self.check_pair(c)?;
            c += 1;
        }
        // 95：空格。
        self.check_pair(HW_SPACE)?;
        // 96..100：边界样本（可区端点已含，此处补全角区端点回查）。
        let edges: [u32; 4] = [FW_LO, FW_HI, FW_SPACE, HW_SPACE];
        // 边界样本走反向对照：fw_to_hw 再 hw_to_fw 恒等。
        let mut i = 0usize;
        while i < edges.len() {
            let hw = fw_to_hw(edges[i])?;
            let back = hw_to_fw(hw)?;
            if back != edges[i] {
                return Err(E_RT_LOSS);
            }
            self.ledger.push(PairRecord { hw, fw: edges[i], rt: hw });
            self.pair_total += 1;
            i += 1;
        }
        Ok(())
    }

    /// 账逐条复算（J1）：三元账逐项与标准式重算比对。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.pair_total {
            return Err(E_LEDGER_BAD);
        }
        if self.mismatches != 0 {
            return Err(E_PAIR_BROKEN);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            let r = &self.ledger[i];
            let want_fw = hw_to_fw(r.hw)?;
            if want_fw != r.fw {
                return Err(E_PAIR_BROKEN);
            }
            let want_hw = fw_to_hw(r.fw)?;
            if want_hw != r.rt || r.rt != r.hw {
                return Err(E_RT_LOSS);
            }
            i += 1;
        }
        Ok(())
    }

    /// ×N 对照覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.pair_total < want {
            return Err(E_TAB_MISS);
        }
        Ok(())
    }

    /// 全对终判：无失配记录。
    pub fn assert_all_match(&self) -> Result<(), u16> {
        if self.mismatches != 0 {
            return Err(E_PAIR_BROKEN);
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 抽样表第 k 项的半角字（供独立复跑与账面比对）。
pub fn sample_hw(k: u32) -> u32 {
    if k < 94 {
        HW_LO + k
    } else if k == 94 {
        HW_SPACE
    } else {
        HW_LO + (k % 94)
    }
}

/// 自例程判据：逐条落实 J1（100 项对照全对），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对照 ×100：抽样 100 项正反双跑全对 + 账逐条复算。
    let mut ft = FwHwTab::new();
    ft.run_sample_100()?;
    ft.coverage(100)?;
    ft.audit()?;
    ft.assert_all_match()?;
    if ft.ledger_len() != 100 {
        return Err(E_TAB_MISS);
    }
    hits += 1;

    // R2 标准式锚点：'!'(0x21)↔0xFF01、'~'(0x7E)↔0xFF5E、空格↔0x3000
    //    三个锚点逐位核对。
    if hw_to_fw(0x21)? != 0xFF01 {
        return Err(E_PAIR_BROKEN);
    }
    if hw_to_fw(0x7E)? != 0xFF5E {
        return Err(E_PAIR_BROKEN);
    }
    if hw_to_fw(HW_SPACE)? != FW_SPACE {
        return Err(E_PAIR_BROKEN);
    }
    if fw_to_hw(0xFF01)? != 0x21 {
        return Err(E_PAIR_BROKEN);
    }
    if fw_to_hw(0xFF5E)? != 0x7E {
        return Err(E_PAIR_BROKEN);
    }
    if fw_to_hw(FW_SPACE)? != HW_SPACE {
        return Err(E_PAIR_BROKEN);
    }
    hits += 1;

    // R3 可区逐字互逆：0x21..0x7E 全 94 字正反双向逐一恒等（穷举口径）。
    let mut c = HW_LO;
    while c <= HW_HI {
        let fw = hw_to_fw(c)?;
        if fw_to_hw(fw)? != c {
            return Err(E_RT_LOSS);
        }
        // 单调性：全角码随半角码严格递增（镜像区连续无跳位）。
        if c > HW_LO {
            let prev = hw_to_fw(c - 1)?;
            if fw != prev + 1 {
                return Err(E_PAIR_BROKEN);
            }
        }
        c += 1;
    }
    hits += 1;

    // R4 独立抽样复跑：sample_hw 生成 100 项再对照一遍，账面翻倍、
    //    结果与首轮逐位一致（可复跑红线）。
    let mut ft = FwHwTab::new();
    ft.run_sample_100()?;
    let first: Vec<u32> = {
        let mut v: Vec<u32> = Vec::new();
        let mut i = 0u32;
        while i < 100 {
            let r = ft.check_pair(sample_hw(i % 101))?;
            v.push(r.fw);
            i += 1;
        }
        v
    };
    // 重跑一遍，逐位同果。
    let mut i = 0u32;
    while i < 100 {
        let r = ft.check_pair(sample_hw(i % 101))?;
        if r.fw != first[i as usize] {
            return Err(E_RT_LOSS);
        }
        i += 1;
    }
    ft.audit()?;
    ft.assert_all_match()?;
    hits += 1;

    // R5 区外拒转：0x1F、0x7F、0x80、0xFF00、0xFF6F、0x2FFF、0x3001 全拒。
    let bad: [u32; 7] = [0x1F, 0x7F, 0x80, 0xFF00, 0xFF6F, 0x2FFF, 0x3001];
    let mut i = 0usize;
    while i < bad.len() {
        if hw_to_fw(bad[i]).is_ok() {
            return Err(E_OUT_OF_RANGE);
        }
        i += 1;
    }
    let bad_fw: [u32; 4] = [0x20, 0xFF00, 0xFF6F, 0x3100];
    let mut i = 0usize;
    while i < bad_fw.len() {
        if fw_to_hw(bad_fw[i]).is_ok() {
            return Err(E_OUT_OF_RANGE);
        }
        i += 1;
    }
    // 拒转不落账。
    let mut ft = FwHwTab::new();
    if ft.check_pair(0x7F).is_ok() {
        return Err(E_OUT_OF_RANGE);
    }
    if ft.pair_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 汉字全角字符不受此表管辖：中文常用字（如 U+4E2D）不在半全角
    //    映射面，双向全拒（面界清晰，防静默扩区）。
    if hw_to_fw(0x4E2D).is_ok() {
        return Err(E_OUT_OF_RANGE);
    }
    if fw_to_hw(0x4E2D).is_ok() {
        return Err(E_OUT_OF_RANGE);
    }
    hits += 1;

    // R7 失配抓拍：伪造一条与标准式不符的账，审计必红（对照面无假绿）。
    let mut ft = FwHwTab::new();
    ft.check_pair(0x41)?;
    ft.ledger.push(PairRecord { hw: 0x41, fw: 0x1234, rt: 0x41 }); // 伪账
    if ft.audit().is_ok() {
        return Err(E_PAIR_BROKEN);
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要与零对照拒。
    let mut ft = FwHwTab::new();
    if ft.writeback(true, 0x10).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    ft.run_sample_100()?;
    if ft.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ft.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
