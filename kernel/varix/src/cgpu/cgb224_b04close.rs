//! CGPU-F0224 · B04 组收口（B04 优先级与预算组 · 段位 0x07A0）
//!
//! B04 组（CGPU-F0209 ~ F0224，优先级与预算 16 项）的组级收口。
//! 收口不是仪式——是**可机检的账本**：十六项逐项自检入树、组级
//! 报告、优先级体系完整性验证、双签锁账、移交包断点一致性。
//!
//! 1. **十六项自检入树**（判据「16 项入树」）：F0209~F0224 逐项
//!    挂自检结果，缺项/未过即收口失败。
//! 2. **性能与确定性报告**（判据「报告」）：组级聚合——每项的
//!    操作计数与确定性标记折成一页报告（指纹锁定）。
//! 3. **优先级体系完整性**（判据「优先级体系完整性验证」）：优先
//!    级域 16 级无重叠、抢占链单调、时间片/快速通道两通道全覆盖
//!    ——三项机检。
//! 4. **组双签**（判据「双签」）：两个不同签名者 + 内容指纹 +
//!    时序单调（第二签时间 ≥ 第一签），同人双签/时序倒挂即拒。
//! 5. **台账与移交**（判据「台账」「移交」「断点」）：收口台账
//!    一条不落；移交包（B05 跨帧依赖组就绪声明：优先级传播就绪 +
//!    预算跨帧记账就绪）带断点指纹，B05 侧断点续接时逐位校验。
//!
//! no_std + alloc，全整数。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x07A0;

/// FNV-1a 64。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut fp: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        fp ^= b as u64;
        fp = fp.wrapping_mul(0x100000001b3);
    }
    fp
}

/// B04 组 16 项的单号表（F0209..=F0224）。
pub const B04_ITEMS: [u32; 16] = [
    209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224,
];

/// 单项自检记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemCheck {
    /// 单号（如 209 表示 CGPU-F0209）。
    pub item: u32,
    /// 自检绿。
    pub ok: bool,
    /// 操作计数（性能面）。
    pub ops: u64,
    /// 确定性标记。
    pub deterministic: bool,
}

/// 优先级体系完整性三项机检（判据「优先级体系完整性验证」）。
/// `levels`：16 级的权值表；`preempt_chain`：抢占链（级号序列，
/// 须严格单调）；`channels`：通道覆盖位图（bit0 时间片 / bit1 快速通道）。
pub fn verify_prio_integrity(levels: &[u16], preempt_chain: &[u8], channels: u8) -> Result<(), u16> {
    // 检 1：16 级无重叠（严格递增非零）。
    if levels.len() != 16 {
        return Err(SEG_BASE);
    }
    for w in levels.windows(2) {
        if w[0] == 0 || w[1] <= w[0] {
            return Err(SEG_BASE + 1); // 零权或重叠
        }
    }
    // 检 2：抢占链严格单调（抢占只许逐级向上）。
    if preempt_chain.len() < 2 {
        return Err(SEG_BASE + 2);
    }
    for w in preempt_chain.windows(2) {
        if w[1] <= w[0] {
            return Err(SEG_BASE + 3);
        }
    }
    // 检 3：两通道全覆盖。
    if channels & 0b11 != 0b11 {
        return Err(SEG_BASE + 4);
    }
    Ok(())
}

/// 双签记录（判据「双签」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSign {
    /// 第一签者。
    pub signer_a: u32,
    /// 第二签者。
    pub signer_b: u32,
    /// 内容指纹。
    pub fp: u64,
    /// 签名时序（逻辑时钟，单调）。
    pub t_a: u64,
    pub t_b: u64,
}

/// 双签校验：同人、指纹零、时序倒挂皆拒。
pub fn verify_dual_sign(s: &DualSign) -> Result<(), u16> {
    if s.signer_a == 0 || s.signer_b == 0 {
        return Err(SEG_BASE + 5);
    }
    if s.signer_a == s.signer_b {
        return Err(SEG_BASE + 6); // 同人双签
    }
    if s.fp == 0 {
        return Err(SEG_BASE + 7);
    }
    if s.t_b < s.t_a {
        return Err(SEG_BASE + 8); // 时序倒挂
    }
    Ok(())
}

/// B05 移交包（判据「移交」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handover {
    /// 优先级传播就绪（跨帧依赖的优先级传播基础齐备）。
    pub prio_ready: bool,
    /// 预算跨帧记账就绪。
    pub budget_ready: bool,
    /// 移交断点：收口台账总指纹。
    pub breakpoint_fp: u64,
    /// 组级操作计数。
    pub total_ops: u64,
}

/// 移交包校验（判据「断点」）：两项就绪 + 断点指纹非零。
pub fn verify_handover(h: &Handover) -> Result<(), u16> {
    if !h.prio_ready || !h.budget_ready {
        return Err(SEG_BASE + 9);
    }
    if h.breakpoint_fp == 0 {
        return Err(SEG_BASE + 10);
    }
    Ok(())
}

/// 收口状态机（判据「16 项入树」「报告」「台账」的载体）。
pub struct B04Close {
    checks: Vec<ItemCheck>,
    /// 报告指纹（frozen 后锁定）。
    report_fp: Option<u64>,
    pub sign: Option<DualSign>,
    pub handover: Option<Handover>,
}

impl B04Close {
    pub fn new() -> Self {
        B04Close { checks: Vec::new(), report_fp: None, sign: None, handover: None }
    }

    /// 挂一项自检（重复单号拒绝）。
    pub fn push_check(&mut self, c: ItemCheck) -> Result<(), u16> {
        if !B04_ITEMS.contains(&c.item) {
            return Err(SEG_BASE + 11); // 非本组单号
        }
        if self.checks.iter().any(|x| x.item == c.item) {
            return Err(SEG_BASE + 12); // 重复挂
        }
        if self.report_fp.is_some() {
            return Err(SEG_BASE + 13); // 报告冻结后不许再挂
        }
        self.checks.push(c);
        Ok(())
    }

    /// 十六项齐绿检查（判据「16 项入树」）。
    pub fn all_items_green(&self) -> bool {
        B04_ITEMS.iter().all(|&it| self.checks.iter().any(|c| c.item == it && c.ok))
    }

    /// 缺项清单（诊断面）。
    pub fn missing_items(&self) -> Vec<u32> {
        B04_ITEMS
            .iter()
            .filter(|it| !self.checks.iter().any(|c| c.item == **it))
            .copied()
            .collect()
    }

    /// 组级报告冻结（判据「报告」）：内容 = 逐项 (单号, 绿, ops,
    /// 确定标记) 折并指纹；任何项红或缺项即拒。
    pub fn freeze_report(&mut self) -> Result<u64, u16> {
        if self.report_fp.is_some() {
            return Err(SEG_BASE + 14); // 重复冻结
        }
        if self.checks.len() != 16 {
            return Err(SEG_BASE + 15);
        }
        if !self.all_items_green() {
            return Err(SEG_BASE + 16);
        }
        let any_nondet = self.checks.iter().any(|c| !c.deterministic);
        if any_nondet {
            return Err(SEG_BASE + 17); // 确定性面有红不许收口
        }
        let mut mix: u64 = 0xcbf29ce484222325;
        for c in &self.checks {
            mix ^= c.item as u64;
            mix = mix.wrapping_mul(0x100000001b3);
            mix ^= c.ops;
            mix = mix.wrapping_mul(0x100000001b3);
        }
        let fp = mix;
        self.report_fp = Some(fp);
        Ok(fp)
    }

    /// 双签（判据「双签」）：报告须先冻结，指纹取报告指纹。
    pub fn sign_off(&mut self, a: u32, b: u32, t_a: u64, t_b: u64) -> Result<(), u16> {
        let fp = self.report_fp.ok_or(SEG_BASE + 18)?;
        let s = DualSign { signer_a: a, signer_b: b, fp, t_a, t_b };
        verify_dual_sign(&s)?;
        self.sign = Some(s);
        Ok(())
    }

    /// 台账总指纹（判据「台账」）：全检查 + 报告指纹折并。
    pub fn ledger_fp(&self) -> Result<u64, u16> {
        let rf = self.report_fp.ok_or(SEG_BASE + 19)?;
        let mut mix: u64 = rf;
        for c in &self.checks {
            mix ^= fnv1a(&[c.item as u8, c.ok as u8, c.deterministic as u8]);
            mix = mix.wrapping_mul(0x100000001b3);
        }
        Ok(mix)
    }

    /// 组级操作计数（性能面聚合）。
    pub fn total_ops(&self) -> u64 {
        self.checks.iter().map(|c| c.ops).sum()
    }

    /// 生成移交包（判据「移交」「断点」）：双签 + 台账齐备才可移交。
    pub fn make_handover(&self) -> Result<Handover, u16> {
        if self.sign.is_none() {
            return Err(SEG_BASE + 20); // 未双签不许移交
        }
        let bp = self.ledger_fp()?;
        Ok(Handover { prio_ready: true, budget_ready: true, breakpoint_fp: bp, total_ops: self.total_ops() })
    }
}

impl Default for B04Close {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 夹具：十六项全绿自检。
    fn green_checks() -> Vec<ItemCheck> {
        B04_ITEMS
            .iter()
            .map(|&it| ItemCheck { item: it, ok: true, ops: 1000 + it as u64, deterministic: true })
            .collect()
    }

    #[test]
    fn sixteen_items_all_in_tree() {
        // 判据「16 项入树」：逐项挂齐、缺项有诊断。
        let mut c = B04Close::new();
        for it in green_checks() {
            c.push_check(it).unwrap();
        }
        assert!(c.all_items_green());
        assert!(c.missing_items().is_empty());
        // 缺一项。
        let mut c2 = B04Close::new();
        for it in green_checks().into_iter().skip(1) {
            c2.push_check(it).unwrap();
        }
        assert!(!c2.all_items_green());
        assert_eq!(c2.missing_items(), vec![209u32]);
    }

    #[test]
    fn push_check_rejects_out_of_group_and_dup() {
        let mut c = B04Close::new();
        assert_eq!(
            c.push_check(ItemCheck { item: 208, ok: true, ops: 1, deterministic: true }).unwrap_err(),
            SEG_BASE + 11
        );
        assert_eq!(
            c.push_check(ItemCheck { item: 225, ok: true, ops: 1, deterministic: true }).unwrap_err(),
            SEG_BASE + 11
        );
        c.push_check(ItemCheck { item: 209, ok: true, ops: 1, deterministic: true }).unwrap();
        assert_eq!(
            c.push_check(ItemCheck { item: 209, ok: true, ops: 2, deterministic: true }).unwrap_err(),
            SEG_BASE + 12
        );
    }

    #[test]
    fn report_freeze_requires_all_green_deterministic() {
        // 判据「报告」：缺项/有红/非确定皆拒，全绿才冻结。
        let mut c = B04Close::new();
        for it in green_checks().into_iter().take(15) {
            c.push_check(it).unwrap();
        }
        assert_eq!(c.freeze_report().unwrap_err(), SEG_BASE + 15); // 缺项
        let mut c2 = B04Close::new();
        for it in green_checks() {
            c2.push_check(it).unwrap();
        }
        let fp = c2.freeze_report().unwrap();
        assert_ne!(fp, 0);
        assert_eq!(c2.freeze_report().unwrap_err(), SEG_BASE + 14); // 重复冻结
        // 一项非确定。
        let mut c3 = B04Close::new();
        let mut checks = green_checks();
        checks[0].deterministic = false;
        for it in checks {
            c3.push_check(it).unwrap();
        }
        assert_eq!(c3.freeze_report().unwrap_err(), SEG_BASE + 17);
    }

    #[test]
    fn prio_integrity_three_checks() {
        // 判据「优先级体系完整性验证」：三检各自的面。
        let levels: [u16; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768];
        assert!(verify_prio_integrity(&levels, &[0, 3, 7, 15], 0b11).is_ok());
        // 级数错。
        assert_eq!(verify_prio_integrity(&levels[..15], &[0, 1], 0b11).unwrap_err(), SEG_BASE);
        // 权重叠。
        let mut bad = levels;
        bad[5] = bad[4];
        assert_eq!(verify_prio_integrity(&bad, &[0, 1], 0b11).unwrap_err(), SEG_BASE + 1);
        // 抢占链倒挂。
        assert_eq!(verify_prio_integrity(&levels, &[7, 3], 0b11).unwrap_err(), SEG_BASE + 3);
        // 通道缺覆盖。
        assert_eq!(verify_prio_integrity(&levels, &[0, 1], 0b01).unwrap_err(), SEG_BASE + 4);
    }

    #[test]
    fn dual_sign_rules() {
        // 判据「双签」：同人/零指纹/时序倒挂皆拒。
        let ok = DualSign { signer_a: 1022, signer_b: 1023, fp: 42, t_a: 10, t_b: 11 };
        assert!(verify_dual_sign(&ok).is_ok());
        assert_eq!(verify_dual_sign(&DualSign { signer_b: 1022, ..ok }).unwrap_err(), SEG_BASE + 6);
        assert_eq!(verify_dual_sign(&DualSign { fp: 0, ..ok }).unwrap_err(), SEG_BASE + 7);
        assert_eq!(verify_dual_sign(&DualSign { t_b: 5, ..ok }).unwrap_err(), SEG_BASE + 8);
        assert_eq!(verify_dual_sign(&DualSign { signer_a: 0, ..ok }).unwrap_err(), SEG_BASE + 5);
    }

    #[test]
    fn full_close_flow_to_handover() {
        // 全流程：入树 → 冻结报告 → 双签 → 台账 → 移交包。
        let mut c = B04Close::new();
        for it in green_checks() {
            c.push_check(it).unwrap();
        }
        // 未冻结不能签。
        assert_eq!(c.sign_off(1, 2, 1, 2).unwrap_err(), SEG_BASE + 18);
        c.freeze_report().unwrap();
        // 同人拒。
        assert_eq!(c.sign_off(7, 7, 1, 2).unwrap_err(), SEG_BASE + 6);
        c.sign_off(1022, 1023, 100, 105).unwrap();
        let bp = c.ledger_fp().unwrap();
        let h = c.make_handover().unwrap();
        assert!(verify_handover(&h).is_ok());
        assert_eq!(h.breakpoint_fp, bp);
        assert_eq!(h.total_ops, c.total_ops());
        // 未签不许移交。
        let mut c2 = B04Close::new();
        for it in green_checks() {
            c2.push_check(it).unwrap();
        }
        c2.freeze_report().unwrap();
        assert_eq!(c2.make_handover().unwrap_err(), SEG_BASE + 20);
    }

    #[test]
    fn handover_requires_readiness() {
        // 判据「断点」：未就绪/零断点皆拒。
        assert_eq!(verify_handover(&Handover { prio_ready: false, budget_ready: true, breakpoint_fp: 9, total_ops: 1 }).unwrap_err(), SEG_BASE + 9);
        assert_eq!(verify_handover(&Handover { prio_ready: true, budget_ready: false, breakpoint_fp: 9, total_ops: 1 }).unwrap_err(), SEG_BASE + 9);
        assert_eq!(verify_handover(&Handover { prio_ready: true, budget_ready: true, breakpoint_fp: 0, total_ops: 1 }).unwrap_err(), SEG_BASE + 10);
    }

    #[test]
    fn ledger_fp_changes_with_content() {
        // 台账指纹随内容变化（断点一致性基础）。
        let mut c1 = B04Close::new();
        for it in green_checks() {
            c1.push_check(it).unwrap();
        }
        c1.freeze_report().unwrap();
        let mut c2 = B04Close::new();
        let mut checks = green_checks();
        checks[7].ops += 1;
        for it in checks {
            c2.push_check(it).unwrap();
        }
        c2.freeze_report().unwrap();
        assert_ne!(c1.ledger_fp().unwrap(), c2.ledger_fp().unwrap());
    }
}
