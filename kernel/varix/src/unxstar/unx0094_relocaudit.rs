//! UNX-F0094 · 重定位审计报告 · 诊断段 0x22A0
//! 验收锚点：UNX-F0094-J1 修复器产出（applied/rejected 两账）汇总
//! 成审计报告，红项率门槛与指纹固化判据通过；
//! 纯功能代码 ≥220 行。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x22A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAL: u16 = SEG_BASE + 1;     // 汇总账守恒不平
const E_RDONLY: u16 = SEG_BASE + 2;  // 报告已定稿
const E_EMPTY: u16 = SEG_BASE + 3;   // 零样本出报告
const E_REDRATE: u16 = SEG_BASE + 4; // 红项率超门槛
const E_FINGER: u16 = SEG_BASE + 5;  // 指纹漂移（账面被改）
const E_ORDER: u16 = SEG_BASE + 6;   // 定稿前修改

/// 红项率门槛：千分之五十（5%）。
pub const RED_RATE_FLOOR_PERMILLE: u64 = 50;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_BAL => "汇总账守恒：applied + rejected = 提交总数",
        E_RDONLY => "报告已定稿：定稿后只读",
        E_EMPTY => "零样本出报告：先喂修复账",
        E_REDRATE => "红项率超门槛：修复器系统性异常，禁止放行",
        E_FINGER => "指纹漂移：定稿后账面被改",
        E_ORDER => "定稿前修改被拒：先定稿再复核",
        _ => "未知诊断码",
    }
}

/// 单诊断码聚合项。
#[derive(Clone, Debug, PartialEq)]
pub struct CodeAgg {
    pub code: u16,
    pub count: u64,
}

// ── 审计账 ───────────────────────────────────────────────────
#[derive(Clone)]
pub struct RelocAudit {
    pub applied_total: u64,
    pub rejected_total: u64,
    pub by_code: Vec<CodeAgg>,
    pub sealed_fp: u64,
    pub sealed: bool,
}

impl RelocAudit {
    pub fn new() -> RelocAudit {
        RelocAudit {
            applied_total: 0,
            rejected_total: 0,
            by_code: Vec::new(),
            sealed_fp: 0,
            sealed: false,
        }
    }

    /// 吸收修复账（applied 数 + rejected (偏移,码) 列表）。
    pub fn absorb(&mut self, applied: u64, rejected: &[(u64, u16)]) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        self.applied_total += applied;
        for (_, c) in rejected {
            self.rejected_total += 1;
            match self.by_code.iter_mut().find(|a| a.code == *c) {
                Some(a) => a.count += 1,
                None => self.by_code.push(CodeAgg {
                    code: *c,
                    count: 1,
                }),
            }
        }
        Ok(())
    }

    /// 红项率（千分比）。
    pub fn red_rate_permille(&self) -> u64 {
        let total = self.applied_total + self.rejected_total;
        if total == 0 {
            return 0;
        }
        self.rejected_total * 1000 / total
    }

    /// 定稿：守恒 + 红项率门槛 + 指纹固化。
    pub fn seal(&mut self) -> Result<String, u16> {
        if self.applied_total + self.rejected_total == 0 {
            return Err(E_EMPTY);
        }
        if self.red_rate_permille() > RED_RATE_FLOOR_PERMILLE {
            return Err(E_REDRATE);
        }
        self.sealed = true;
        self.sealed_fp = self.compute_fp();
        Ok(self.render()?)
    }

    fn compute_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.applied_total.to_le_bytes());
        buf.extend_from_slice(&self.rejected_total.to_le_bytes());
        for a in &self.by_code {
            buf.extend_from_slice(&a.code.to_le_bytes());
            buf.extend_from_slice(&a.count.to_le_bytes());
        }
        fnv1a64(&buf)
    }

    /// 定稿后指纹复核。
    pub fn recheck(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_ORDER);
        }
        if self.compute_fp() != self.sealed_fp {
            return Err(E_FINGER);
        }
        Ok(())
    }

    /// 报告文本（N/M 绿 + 诊断码分布）。
    pub fn render(&self) -> Result<String, u16> {
        let total = self.applied_total + self.rejected_total;
        if total == 0 {
            return Err(E_EMPTY);
        }
        let mut s = format!(
            "重定位审计：{}/{} 修复成功，红项率 {}‰",
            self.applied_total,
            total,
            self.red_rate_permille()
        );
        let mut aggs = self.by_code.clone();
        aggs.sort_by_key(|a| core::cmp::Reverse(a.count));
        for a in aggs.iter().take(8) {
            s.push_str(&format!("，码{:04X}×{}", a.code, a.count));
        }
        Ok(s)
    }

    pub fn reconcile(&self, submitted: u64) -> Result<(), u16> {
        if self.applied_total + self.rejected_total != submitted {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        self.compute_fp()
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：低红项率汇总 → 定稿放行 + 报告含分布。
    let mut a = RelocAudit::new();
    a.absorb(97, &[(0, 0x2205), (1, 0x2205), (2, 0x220A), (3, 0x2202)])?;
    a.reconcile(101)?;
    let rep = a.seal()?;
    if !rep.contains("97/101") || !rep.contains("2205×2") {
        return Err(E_BAL);
    }
    a.recheck()?;
    hits += 1;

    // R2：指纹复核——篡改账面 → E_FINGER。
    let mut a2 = a.clone();
    a2.rejected_total += 1;
    if a2.recheck() != Err(E_FINGER) {
        return Err(E_FINGER);
    }
    hits += 1;

    // R3：零红项汇总 → 红项率 0。
    let mut a3 = RelocAudit::new();
    a3.absorb(64, &[])?;
    if a3.red_rate_permille() != 0 {
        return Err(E_BAL);
    }
    a3.seal()?;
    hits += 1;

    // I1：红项率超门槛 → E_REDRATE。
    let mut i1 = RelocAudit::new();
    let rejects: Vec<(u64, u16)> = (0..60u64).map(|i| (i, 0x2205)).collect();
    i1.absorb(40, &rejects)?;
    if i1.seal() != Err(E_REDRATE) {
        return Err(E_REDRATE);
    }
    hits += 1;

    // I2：零样本 → E_EMPTY。
    let mut i2 = RelocAudit::new();
    if i2.seal() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I3：守恒不平 → E_BAL。
    let mut i3 = RelocAudit::new();
    i3.absorb(10, &[(0, 0x2205)])?;
    if i3.reconcile(12) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I4：定稿后再吸收 → E_RDONLY。
    if a.absorb(1, &[]) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：未定稿复核 → E_ORDER。
    let i5 = RelocAudit::new();
    if i5.recheck() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    Ok(hits)
}
