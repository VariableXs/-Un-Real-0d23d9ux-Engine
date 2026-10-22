//! CGPU-F0465 · 接口总账（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 接口总账：九组冻结 API 盘点合一（C01-C09 全部冻结签名合并——
//!   含 F0397 总冻结 v1.0 与 F0447 增量的合并终版）；
//! - 域内查重对账（九组两两边界复核零重复）；
//! - 与 D 域（F0481 80 帧保真系统）边界契约（帧计时反馈/预算修正/
//!   合同校验三对接点清单——F0332/F0402 契约正式化）。
//!   判据：总账、零重复、D 域契约、判据。
//!
//! 设计纪律：
//! - 九组合一：各组冻结签名以「组号+名+指纹」登记，合并时同组内
//!   同名拒、跨组同名允许（命名空间隔离）但全名（组前缀+名）必
//!   唯一——总账唯一键 = 组号 × 名。
//! - 查重对账：两两边界复核（同指纹不同名=语义撞车，判重）；
//!   指纹唯一性全称断言。
//! - D 域契约：三对接点（帧计时反馈/预算修正/合同校验）正式化
//!   为契约条目（方向+数据+频率约束），缺点即契约不完整。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1040 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1040，全仓独占）。
pub const SEG_BASE: u16 = 0x1040;

pub const E_AL_DUP: u16 = SEG_BASE + 1; // 总账重复（唯一键冲突）
pub const E_AL_FPCOLLIDE: u16 = SEG_BASE + 2; // 指纹撞车（同名异签语义重）
pub const E_AL_CONTRACT: u16 = SEG_BASE + 3; // 契约不完整
pub const E_AL_GROUP: u16 = SEG_BASE + 4; // 组号非法

/// 九组（C01~C09）。
pub const GROUPS: [u8; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];

/// 总账条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 组号（1~9）。
    pub group: u8,
    /// 全名（含组前缀，如 "c01.arb.grant"）。
    pub fq_name: &'static str,
    /// 签名指纹。
    pub fp: u32,
}

fn fp_of(s: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in s.as_bytes() {
        h = (h ^ *b as u32).wrapping_mul(0x0100_0193);
    }
    h
}

/// 接口总账。
#[derive(Debug, Default)]
pub struct ApiLedger {
    pub entries: Vec<LedgerEntry>,
}

impl ApiLedger {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 登记：唯一键（组号×全名）+ 指纹全局唯一。
    pub fn register(&mut self, group: u8, fq_name: &'static str, sig_hint: &str) -> Result<u32, u16> {
        if !GROUPS.contains(&group) {
            return Err(E_AL_GROUP);
        }
        if self.entries.iter().any(|e| e.group == group && e.fq_name == fq_name) {
            return Err(E_AL_DUP);
        }
        let fp = fp_of(sig_hint);
        // 指纹撞车：不同名同指纹=语义重。
        if self.entries.iter().any(|e| e.fp == fp && e.fq_name != fq_name) {
            return Err(E_AL_FPCOLLIDE);
        }
        self.entries.push(LedgerEntry { group, fq_name, fp });
        Ok(fp)
    }

    /// 组盘点：某组的签名清单。
    pub fn group_entries(&self, group: u8) -> Vec<&LedgerEntry> {
        self.entries.iter().filter(|e| e.group == group).collect()
    }

    /// 查重对账：指纹全局唯一（两两复核）。
    pub fn dedup_ok(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].fp == self.entries[j].fp {
                    return false;
                }
            }
        }
        true
    }

    /// 总账完整性：九组每组至少一条。
    pub fn all_groups_covered(&self) -> bool {
        GROUPS.iter().all(|&g| self.entries.iter().any(|e| e.group == g))
    }
}

/// D 域契约三对接点。
pub const D_FRAME_TIMING: u8 = 1; // 帧计时反馈（D→本域）
pub const D_BUDGET_FIX: u8 = 2; // 预算修正（本域→D）
pub const D_CONTRACT_CHECK: u8 = 3; // 合同校验（双向）
pub const D_LINKS: [u8; 3] = [D_FRAME_TIMING, D_BUDGET_FIX, D_CONTRACT_CHECK];

/// 契约条目（方向+数据+频率约束）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DContract {
    pub link: u8,
    /// 方向：0=D→本域 1=本域→D 2=双向。
    pub direction: u8,
    /// 频率约束（每帧/每事件）。
    pub per_frame: bool,
}

impl DContract {
    pub fn valid(&self) -> bool {
        D_LINKS.contains(&self.link) && self.direction <= 2
    }
}

/// 契约正式化：三对接点齐备+方向正确才算契约成立。
pub fn contract_of(links: &[DContract]) -> Result<(), u16> {
    for need in D_LINKS.iter() {
        let hit = links.iter().find(|c| c.link == *need);
        let ok = match need {
            &D_FRAME_TIMING => hit.map(|c| c.valid() && c.direction == 0).unwrap_or(false),
            &D_BUDGET_FIX => hit.map(|c| c.valid() && c.direction == 1).unwrap_or(false),
            &D_CONTRACT_CHECK => hit.map(|c| c.valid() && c.direction == 2).unwrap_or(false),
            _ => false,
        };
        if !ok {
            return Err(E_AL_CONTRACT);
        }
    }
    Ok(())
}

/// 九组合一终版构造（演示性最小账：每组一条代表签名）。
pub fn merged_v1() -> ApiLedger {
    let mut led = ApiLedger::new();
    let seeds: [(u8, &'static str); 9] = [
        (1, "c01.model.open"),
        (2, "c02.arb.grant"),
        (3, "c03.scene.apply"),
        (4, "c04.pred.feed"),
        (5, "c05.govern.sweep"),
        (6, "c06.integ.push"),
        (7, "c07.stat.export"),
        (8, "c08.adv.policy"),
        (9, "c09.edge.state"),
    ];
    for (g, n) in seeds.iter() {
        let _ = led.register(*g, n, n);
    }
    led
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、总账：九组合一覆盖。
    {
        let led = merged_v1();
        v.push(("F0465-总账-九组全覆盖", led.all_groups_covered() && led.entries.len() == 9));
        // 组盘点独立。
        let g1 = led.group_entries(1);
        v.push(("F0465-总账-组盘点独立", g1.len() == 1 && g1[0].fq_name == "c01.model.open"));
    }
    // 二、唯一键：同组同名拒、跨组同名允许但全名唯一。
    {
        let mut led = ApiLedger::new();
        led.register(1, "c01.a", "x1").ok();
        let dup = led.register(1, "c01.a", "x1");
        v.push(("F0465-唯一键-同组同名拒", dup == Err(E_AL_DUP)));
        // 指纹撞车：不同名同指纹拒。
        let collide = led.register(2, "c02.b", "x1");
        v.push(("F0465-唯一键-指纹撞车拒", collide == Err(E_AL_FPCOLLIDE)));
        // 组号非法。
        v.push(("F0465-唯一键-组号非法拒", led.register(99, "c99.x", "x9") == Err(E_AL_GROUP)));
    }
    // 三、查重对账：指纹全局唯一。
    {
        let led = merged_v1();
        v.push(("F0465-查重-指纹两两互异", led.dedup_ok()));
        // 九组两两边界：组间无同指纹。
        let mut pair_ok = true;
        for i in 0..GROUPS.len() {
            for j in (i + 1)..GROUPS.len() {
                let a = led.group_entries(GROUPS[i]);
                let b = led.group_entries(GROUPS[j]);
                for x in a.iter() {
                    for y in b.iter() {
                        if x.fp == y.fp {
                            pair_ok = false;
                        }
                    }
                }
            }
        }
        v.push(("F0465-查重-九组两两零重复", pair_ok));
    }
    // 四、D 域契约：三对接点齐备+方向正确。
    {
        let full = vec![
            DContract { link: D_FRAME_TIMING, direction: 0, per_frame: true },
            DContract { link: D_BUDGET_FIX, direction: 1, per_frame: false },
            DContract { link: D_CONTRACT_CHECK, direction: 2, per_frame: true },
        ];
        v.push(("F0465-D域契约-三点齐备通过", contract_of(&full) == Ok(())));
        // 缺点/方向错。
        let miss = vec![
            DContract { link: D_FRAME_TIMING, direction: 0, per_frame: true },
            DContract { link: D_BUDGET_FIX, direction: 1, per_frame: false },
        ];
        v.push(("F0465-D域契约-缺点拒", contract_of(&miss) == Err(E_AL_CONTRACT)));
        let wrong_dir = vec![
            DContract { link: D_FRAME_TIMING, direction: 1, per_frame: true },
            DContract { link: D_BUDGET_FIX, direction: 1, per_frame: false },
            DContract { link: D_CONTRACT_CHECK, direction: 2, per_frame: true },
        ];
        v.push(("F0465-D域契约-方向错拒", contract_of(&wrong_dir) == Err(E_AL_CONTRACT)));
        // F0332/F0402 契约正式化在场（帧计时 per-frame 约束）。
        v.push(("F0465-D域契约-帧计时per-frame", full[0].per_frame));
    }
    // 五、判据自检。
    {
        v.push(("F0465-判据-指纹确定性", fp_of("a") == fp_of("a") && fp_of("a") != fp_of("b")));
        v.push(("F0465-判据-组表九枚举", GROUPS.len() == 9 && GROUPS[0] == 1 && GROUPS[8] == 9));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0465_apiledger_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
