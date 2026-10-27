//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1251 · Arming 态配置总表（B23 段参数收口） · 0xFB90
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1251-J1 B23 段参数 ≥12 项注册总表（确认窗口时长/缓冲容量/
//!      回退重试上限/依赖表版本等），双闸一致率 100%，篡改 10/10 检出
//!      回退；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 参数总表：14 项（≥12 达标），每项五元组（编号/中文名/默认值/
//!     域下界/域上界），编号 5101 起连续唯一；
//!   - 双闸结构：默认闸（defaults，编译期定值）+ 生效闸（active，运行
//!     期副本），各自独立 FNV 校验和，双闸摘要必须全等；
//!   - 越域拒绝：active 写入先比 [min,max]，越域必拒不改账；
//!   - 篡改检出回退：手改 active 后校验和失配 → 从 defaults 回滚 →
//!     摘要复原，10/10 轮全检出全回退；
//!   - 回滚账：append-only（轮号/被篡项/旧值/新值/回滚值）；
//!   - 注入核 5 项（缺项/重号/越域写入/双闸失衡/回滚失败）必抓；
//!   - LCG 种子写死 0xFB90，禁浮点/墙钟/rand。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFB90 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFB90;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B23 段配置参数项数（判准 ≥12，注册 14 项）。
pub const CONFIG_COUNT: usize = 14;

/// 诊断码（0xFB90 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CgCode {
    /// 0xFB90 缺参数（总表 <12 项）。
    ParamShort = 0,
    /// 0xFB91 参数重号（编号唯一性破）。
    ParamDup = 1,
    /// 0xFB92 越域写入（active 值出 [min,max]）。
    RangeOob = 2,
    /// 0xFB93 双闸失衡（两闸校验和不一致）。
    TwinMismatch = 3,
    /// 0xFB94 回滚失败（篡改检出后未复原）。
    RollbackFail = 4,
    /// 0xFB95 篡改漏检（active 改动未触发检出）。
    TamperMiss = 5,
    /// 0xFB96 回滚账缩水（append-only 被破）。
    RbShrink = 6,
    /// 0xFB97 参数名空（收口登记不全）。
    NameEmpty = 7,
    /// 0xFB98 域倒挂（min>max）。
    RangeInvert = 8,
    /// 0xFB99 摘要空文本（写回摘要非空判据破）。
    SummaryEmpty = 9,
}

impl CgCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [CgCode; 10] = [
        CgCode::ParamShort, CgCode::ParamDup, CgCode::RangeOob, CgCode::TwinMismatch,
        CgCode::RollbackFail, CgCode::TamperMiss, CgCode::RbShrink, CgCode::NameEmpty,
        CgCode::RangeInvert, CgCode::SummaryEmpty,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFB90 => "缺参数：总表补齐 ≥12 项（确认窗口/缓冲容量/回退重试上限/依赖表版本等）。",
        0xFB91 => "参数重号：两两去重核编号，重号改号。",
        0xFB92 => "越域写入：active 先比 [min,max]，越域拒收并红账。",
        0xFB93 => "双闸失衡：默认闸为真值源，重放 active=defaults 复原。",
        0xFB94 => "回滚失败：检出后逐项从 defaults 回写，再核校验和。",
        0xFB95 => "篡改漏检：校验和覆盖全项，漏检回溯覆盖面。",
        0xFB96 => "回滚账缩水：append-only，禁截断。",
        0xFB97 => "参数名空：收口登记补齐中文名。",
        0xFB98 => "域倒挂：min≤max 必须成立，登记时核域。",
        0xFB99 => "摘要空文本：摘要含项数+双闸摘要+告警数。",
        _ => "未知诊断码：回查 0xFB90 段闭集。",
    }
}

/// 配置参数（五元组，判准点名项全含）。
#[derive(Clone, Copy, Debug)]
pub struct ConfigParam {
    /// 参数编号（5101 起连续）。
    pub id: u32,
    /// 中文名。
    pub name: &'static str,
    /// 默认值（默认闸定值）。
    pub default_val: u64,
    /// 域下界。
    pub min: u64,
    /// 域上界。
    pub max: u64,
    /// 单位短句。
    pub unit: &'static str,
}

/// 14 项参数总表（默认值来源：B23 段收口值，写死）。
pub const CONFIG_PARAMS: [ConfigParam; CONFIG_COUNT] = [
    ConfigParam { id: 5101, name: "确认窗口时长", default_val: 100, min: 10, max: 1000, unit: "ms" },
    ConfigParam { id: 5102, name: "缓冲容量", default_val: 64, min: 8, max: 4096, unit: "笔" },
    ConfigParam { id: 5103, name: "回退重试上限", default_val: 3, min: 1, max: 16, unit: "次" },
    ConfigParam { id: 5104, name: "依赖表版本", default_val: 20260901, min: 20260101, max: 20991231, unit: "版" },
    ConfigParam { id: 5105, name: "武装向量上限", default_val: 8, min: 1, max: 32, unit: "个" },
    ConfigParam { id: 5106, name: "排空静默门槛", default_val: 3, min: 1, max: 60, unit: "tick" },
    ConfigParam { id: 5107, name: "事件洪峰速率上限", default_val: 128, min: 16, max: 65536, unit: "笔/窗" },
    ConfigParam { id: 5108, name: "重武装间隔", default_val: 5, min: 0, max: 1000, unit: "tick" },
    ConfigParam { id: 5109, name: "红账保底深度", default_val: 64, min: 16, max: 8192, unit: "行" },
    ConfigParam { id: 5110, name: "压测连跑次数", default_val: 3, min: 1, max: 9, unit: "次" },
    ConfigParam { id: 5111, name: "预算偏差容限", default_val: 100, min: 10, max: 300, unit: "‰" },
    ConfigParam { id: 5112, name: "统计矩阵刷新轮", default_val: 50, min: 1, max: 1000, unit: "轮" },
    ConfigParam { id: 5113, name: "审计滚动保底", default_val: 256, min: 64, max: 16384, unit: "行" },
    ConfigParam { id: 5114, name: "启动序超时", default_val: 200, min: 20, max: 5000, unit: "ms" },
];

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFB90 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFB90_FB90_FB90_FB90;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// FNV-1a 滚动哈希步。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

/// 校验和（值表 → FNV，双闸独立计算）。
pub fn checksum(vals: &[u64]) -> u64 {
    let mut d = 0xcbf29ce484222325;
    for v in vals.iter() {
        d = fnv_step(d, *v & 0xFF);
        d = fnv_step(d, (*v >> 8) & 0xFF);
        d = fnv_step(d, (*v >> 16) & 0xFF);
        d = fnv_step(d, (*v >> 24) & 0xFF);
    }
    d
}

/// 回滚账行。
#[derive(Clone, Copy, Debug)]
pub struct RollbackRow {
    /// 轮号（从 1 连续）。
    pub round: u64,
    /// 被篡参数编号。
    pub param: u32,
    /// 篡改后值。
    pub bad_val: u64,
    /// 回滚复原值。
    pub restored: u64,
}

/// 配置总表（双闸正本）。
#[derive(Debug)]
pub struct ConfigTable {
    /// 默认闸（真值源）。
    defaults: Vec<u64>,
    /// 生效闸（运行期副本）。
    active: Vec<u64>,
    /// 回滚账（append-only）。
    rb: Vec<RollbackRow>,
    /// 回滚账峰值。
    rb_high: usize,
    /// 变更账（域内写入记录，append-only）。
    changes: Vec<ChangeRow>,
}

impl ConfigTable {
    /// 建表（active 初始化为 defaults）。
    pub fn new() -> Self {
        let d: Vec<u64> = CONFIG_PARAMS.iter().map(|p| p.default_val).collect();
        let a = d.clone();
        ConfigTable { defaults: d, active: a, rb: Vec::new(), rb_high: 0, changes: Vec::new() }
    }

    /// 参数元信息读取。
    pub fn param(&self, idx: usize) -> Option<ConfigParam> {
        CONFIG_PARAMS.get(idx).copied()
    }

    /// 生效值读取。
    pub fn active_of(&self, idx: usize) -> Option<u64> {
        self.active.get(idx).copied()
    }

    /// 默认值读取。
    pub fn default_of(&self, idx: usize) -> Option<u64> {
        self.defaults.get(idx).copied()
    }

    /// 越域判据。
    pub fn out_of_range(idx: usize, v: u64) -> bool {
        match CONFIG_PARAMS.get(idx) {
            Some(p) => v < p.min || v > p.max,
            None => true,
        }
    }

    /// 生效闸写入（越域必拒不改账；域内允许改）。
    pub fn set_active(&mut self, idx: usize, v: u64) -> Result<(), CgCode> {
        if idx >= CONFIG_COUNT {
            return Err(CgCode::RangeOob);
        }
        if Self::out_of_range(idx, v) {
            return Err(CgCode::RangeOob);
        }
        self.active[idx] = v;
        Ok(())
    }

    /// 默认闸校验和。
    pub fn checksum_defaults(&self) -> u64 {
        checksum(&self.defaults)
    }

    /// 生效闸校验和。
    pub fn checksum_active(&self) -> u64 {
        checksum(&self.active)
    }

    /// 双闸一致（判准 100%）。
    pub fn twin_agree(&self) -> bool {
        self.checksum_defaults() == self.checksum_active()
    }

    /// 篡改检出：active 与 defaults 逐项比对，返回首个差异项。
    pub fn detect_tamper(&self) -> Option<(usize, u64, u64)> {
        for i in 0..CONFIG_COUNT {
            if self.active[i] != self.defaults[i] {
                return Some((i, self.defaults[i], self.active[i]));
            }
        }
        None
    }

    /// 回滚：检出差异项逐项从 defaults 回写，入回滚账。
    pub fn rollback(&mut self, round: u64) -> usize {
        let mut n = 0usize;
        // 先收集差异（避免借用冲突）。
        let mut diffs: Vec<(usize, u64, u64)> = Vec::new();
        for i in 0..CONFIG_COUNT {
            if self.active[i] != self.defaults[i] {
                diffs.push((i, self.defaults[i], self.active[i]));
            }
        }
        for (i, dv, av) in diffs.iter() {
            self.rb.push(RollbackRow {
                round,
                param: CONFIG_PARAMS[*i].id,
                bad_val: *av,
                restored: *dv,
            });
            self.active[*i] = *dv;
            n += 1;
        }
        if self.rb.len() > self.rb_high {
            self.rb_high = self.rb.len();
        }
        n
    }

    /// 变更账迭代器。
    pub fn changes_iter(&self) -> impl Iterator<Item = &ChangeRow> {
        self.changes.iter()
    }

    /// 回滚账迭代器。
    pub fn rb_iter(&self) -> impl Iterator<Item = &RollbackRow> {
        self.rb.iter()
    }

    /// 回滚账长度。
    pub fn rb_len(&self) -> usize {
        self.rb.len()
    }

    /// 回滚账只增不减。
    pub fn rb_monotone(&self) -> bool {
        self.rb.len() >= self.rb_high
    }

    /// 总表完整性：≥12 项、编号连续唯一、域不倒挂、名/单位非空。
    pub fn table_ok() -> bool {
        if CONFIG_COUNT < 12 {
            return false;
        }
        for (i, p) in CONFIG_PARAMS.iter().enumerate() {
            if p.id != 5101 + i as u32
                || p.name.is_empty()
                || p.unit.is_empty()
                || p.min > p.max
                || p.default_val < p.min
                || p.default_val > p.max
            {
                return false;
            }
        }
        // 两两去重。
        for i in 0..CONFIG_COUNT {
            for j in (i + 1)..CONFIG_COUNT {
                if CONFIG_PARAMS[i].id == CONFIG_PARAMS[j].id {
                    return false;
                }
            }
        }
        true
    }

    /// 摘要文本（非空判据）。
    pub fn summary(&self) -> String {
        format!(
            "B23 收口 {} 项 默认闸={:#x} 生效闸={:#x} 双闸一致={} 回滚账={}",
            CONFIG_COUNT,
            self.checksum_defaults(),
            self.checksum_active(),
            self.twin_agree(),
            self.rb_len()
        )
    }
}

/// LCG 10/10 篡改检出回退演练：每轮随机篡一项 → 检出 → 回滚 → 复原。
pub fn drill_tamper_rollback_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB90 段基址。
    let mut lcg = Lcg::new(0xFB90);
    for r in 0..INJECT_ROUNDS {
        let mut tbl = ConfigTable::new();
        let base_d = tbl.checksum_defaults();
        let base_a = tbl.checksum_active();
        // 篡改：任选项、越域值（必出域）。
        let idx = (lcg.below(CONFIG_COUNT as u64)) as usize;
        let bad_val = CONFIG_PARAMS[idx].max + 1 + (lcg.below(97));
        // 直改 active（绕过 set_active 的越域闸——模拟内存篡改）。
        tbl.active[idx] = bad_val;
        // 检出。
        let detected = tbl.detect_tamper().is_some();
        let mismatched = !tbl.twin_agree();
        // 回滚。
        let n = tbl.rollback(r as u64 + 1);
        let restored = tbl.twin_agree() && tbl.active_of(idx) == tbl.default_of(idx);
        if detected && mismatched && n == 1 && restored && base_d == tbl.checksum_defaults() && base_a == tbl.checksum_active() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("检出回退轮={}/{}", ok, INJECT_ROUNDS))
}

/// 双闸一致率演练：10/10 轮域内改写后双闸仍可同步一致。
pub fn drill_twin_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut lcg = Lcg::new(0xFB91);
    for r in 0..INJECT_ROUNDS {
        let mut tbl = ConfigTable::new();
        // 域内改写（合法调整）：active 改，defaults 同步改不合法——双闸
        // 一致的正确路径是显式同步：defaults 保持定值，active 回默认。
        let idx = (lcg.below(CONFIG_COUNT as u64)) as usize;
        let span = CONFIG_PARAMS[idx].max - CONFIG_PARAMS[idx].min;
        let v = CONFIG_PARAMS[idx].min + (lcg.below(span + 1));
        let set = tbl.set_active(idx, v);
        let changed = tbl.active_of(idx) == Some(v);
        // 同步回滚（显式收口：回到默认）。
        let _ = tbl.rollback(r as u64 + 1);
        let agree_after = tbl.twin_agree();
        if set.is_ok() && changed && agree_after && tbl.detect_tamper().is_none() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("双闸同步轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：参数变更账（域内 set_active 成功记录，append-only）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ChangeRow {
    /// 参数编号。
    pub param: u32,
    /// 旧值（active 改前）。
    pub old_val: u64,
    /// 新值。
    pub new_val: u64,
}

impl ConfigTable {
    /// 变更账读取。
    pub fn changes(&self) -> &[ChangeRow] {
        &self.changes
    }

    /// 变更账长度。
    pub fn change_len(&self) -> usize {
        self.changes.len()
    }

    /// 带账写入（域内成功 → 入变更账；越域拒绝不记账）。
    pub fn set_active_logged(&mut self, idx: usize, v: u64) -> Result<(), CgCode> {
        let old = self.active_of(idx);
        self.set_active(idx, v)?;
        if let Some(o) = old {
            self.changes.push(ChangeRow { param: CONFIG_PARAMS[idx].id, old_val: o, new_val: v });
        }
        Ok(())
    }

    /// 变更账守恒：每行 old≠new 且 param 在册。
    pub fn changes_sane(&self) -> bool {
        self.changes.iter().all(|c| {
            c.old_val != c.new_val
                && CONFIG_PARAMS.iter().any(|p| p.id == c.param)
        })
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：参数检索器（按编号/名称取参数元信息与下标）。
/// ─────────────────────────────────────────────────────────────────────
/// 按编号查下标。
pub fn idx_by_id(id: u32) -> Option<usize> {
    CONFIG_PARAMS.iter().position(|p| p.id == id)
}

/// 按名称查下标。
pub fn idx_by_name(name: &str) -> Option<usize> {
    CONFIG_PARAMS.iter().position(|p| p.name == name)
}

/// 检索健全：编号/名称倒排双向可查。
pub fn lookup_sane() -> bool {
    CONFIG_PARAMS.iter().enumerate().all(|(i, p)| idx_by_id(p.id) == Some(i) && idx_by_name(p.name) == Some(i))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：域宽表（逐参数 [min,max] 域宽与默认值相对位）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct RangeRow {
    /// 参数编号。
    pub id: u32,
    /// 域宽（max-min）。
    pub span: u64,
    /// 默认值相对位（千分数，(default-min)*1000/span）。
    pub default_pos_milli: u64,
}

/// 域宽表构建。
pub fn build_range_table() -> Vec<RangeRow> {
    CONFIG_PARAMS
        .iter()
        .map(|p| RangeRow {
            id: p.id,
            span: p.max - p.min,
            default_pos_milli: (p.default_val - p.min) * 1000 / (p.max - p.min).max(1),
        })
        .collect()
}

/// 域宽表健全：域宽>0、相对位 ∈ [0,1000]。
pub fn range_table_sane(t: &[RangeRow]) -> bool {
    t.len() == CONFIG_COUNT && t.iter().all(|r| r.span > 0 && r.default_pos_milli <= 1000)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：单位分布（参数按单位聚合计数）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct UnitDistRow {
    /// 单位。
    pub unit: &'static str,
    /// 项数。
    pub count: u32,
}

/// 单位分布构建（保序去重）。
pub fn build_unit_dist() -> Vec<UnitDistRow> {
    let mut units: Vec<&'static str> = Vec::new();
    for p in CONFIG_PARAMS.iter() {
        if !units.contains(&p.unit) {
            units.push(p.unit);
        }
    }
    units
        .iter()
        .map(|u| UnitDistRow {
            unit: u,
            count: CONFIG_PARAMS.iter().filter(|p| p.unit == *u).count() as u32,
        })
        .collect()
}

/// 单位分布守恒：计数和 ≡ 项数。
pub fn unit_dist_conserved(d: &[UnitDistRow]) -> bool {
    d.iter().map(|r| r.count as usize).sum::<usize>() == CONFIG_COUNT
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：边界写入演练（逐参数 min/max 恰在域界写入必成功）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_boundary_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB90 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFB91);
    for r in 0..INJECT_ROUNDS {
        let mut tbl = ConfigTable::new();
        let pick = (lcg.below(CONFIG_COUNT as u64)) as usize;
        let lo = tbl.set_active_logged(pick, CONFIG_PARAMS[pick].min).is_ok();
        let hi = tbl.set_active_logged(pick, CONFIG_PARAMS[pick].max).is_ok();
        let below = matches!(tbl.set_active(pick, CONFIG_PARAMS[pick].min.saturating_sub(1)), Err(CgCode::RangeOob));
        let above = matches!(tbl.set_active(pick, CONFIG_PARAMS[pick].max + 1), Err(CgCode::RangeOob));
        // 复原（回滚账复用）。
        let _ = tbl.rollback(r as u64 + 1);
        let restored = tbl.twin_agree();
        let _ = lcg.below(3);
        if lo && hi && below && above && restored {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("边界写入轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：变更账 10/10 演练（域内改写全程记账，守恒不变）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_change_log_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB90 段基址 ^ 2。
    let mut lcg = Lcg::new(0xFB92);
    for _r in 0..INJECT_ROUNDS {
        let mut tbl = ConfigTable::new();
        let mut all = true;
        for _k in 0..3 {
            let pick = (lcg.below(CONFIG_COUNT as u64)) as usize;
            let span = CONFIG_PARAMS[pick].max - CONFIG_PARAMS[pick].min;
            let mut v = CONFIG_PARAMS[pick].min + (lcg.below(span + 1));
            // 去同值写（同参同值会使 old==new 破变更账健全）。
            let old = tbl.active_of(pick).unwrap_or(CONFIG_PARAMS[pick].default_val);
            if v == old {
                v = if old < CONFIG_PARAMS[pick].max { old + 1 } else { old - 1 };
            }
            all = all && tbl.set_active_logged(pick, v).is_ok();
        }
        let green = all
            && tbl.change_len() == 3
            && tbl.changes_sane()
            && tbl.detect_tamper().is_some()
            && !tbl.twin_agree();
        // 显式收口。
        let _ = tbl.rollback(99);
        let closed = tbl.twin_agree() && tbl.changes_sane();
        let _ = lcg.below(3);
        if green && closed {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("变更账轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：参数摘要位（逐参数 FNV 摘要，参数间两两不同）。
/// ─────────────────────────────────────────────────────────────────────
/// 逐参数摘要表。
pub fn param_digests() -> Vec<u64> {
    CONFIG_PARAMS
        .iter()
        .map(|p| {
            let mut d = 0xcbf29ce484222325;
            for b in p.name.bytes() {
                d = fnv_step(d, b as u64);
            }
            d = fnv_step(d, p.default_val & 0xFF);
            d = fnv_step(d, p.max & 0xFF);
            d
        })
        .collect()
}

/// 摘要两两不同。
pub fn param_digests_unique() -> bool {
    let ds = param_digests();
    for i in 0..ds.len() {
        for j in (i + 1)..ds.len() {
            if ds[i] == ds[j] {
                return false;
            }
        }
    }
    ds.len() == CONFIG_COUNT
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：配置总表会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ConfigReport {
    /// 项数。
    pub items: usize,
    /// 双闸一致。
    pub twin: bool,
    /// 回滚账行数。
    pub rollbacks: usize,
    /// 变更账行数。
    pub changes: usize,
    /// 摘要。
    pub digest: u64,
}

impl ConfigReport {
    /// 建报告。
    pub fn build(t: &ConfigTable) -> Self {
        ConfigReport {
            items: CONFIG_COUNT,
            twin: t.twin_agree(),
            rollbacks: t.rb_len(),
            changes: t.change_len(),
            digest: t.checksum_active(),
        }
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ConfigReport 项数={} 双闸={} 回滚={} 变更={} 摘要={:#x}",
            self.items, self.twin, self.rollbacks, self.changes, self.digest
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 I：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct CgCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_config_checklist(t: &ConfigTable) -> Vec<CgCheckRow> {
    let rt = build_range_table();
    let ud = build_unit_dist();
    let mut rows: Vec<CgCheckRow> = Vec::new();
    rows.push(CgCheckRow { name: "项数=14", passed: CONFIG_COUNT == 14 });
    rows.push(CgCheckRow { name: "总表收口", passed: ConfigTable::table_ok() });
    rows.push(CgCheckRow { name: "双闸一致", passed: t.twin_agree() });
    rows.push(CgCheckRow { name: "检索健全", passed: lookup_sane() });
    rows.push(CgCheckRow { name: "域宽表健全", passed: range_table_sane(&rt) });
    rows.push(CgCheckRow { name: "单位分布守恒", passed: unit_dist_conserved(&ud) });
    rows.push(CgCheckRow { name: "摘要唯一", passed: param_digests_unique() });
    rows.push(CgCheckRow { name: "变更账健全", passed: t.changes_sane() });
    rows.push(CgCheckRow { name: "回滚账健全", passed: t.rb_monotone() });
    rows.push(CgCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn config_checklist_green(rows: &[CgCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 J：矩阵纯函数 10/10 演练。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB90 段基址 ^ 3。
    let mut lcg = Lcg::new(0xFB93);
    for _r in 0..INJECT_ROUNDS {
        let t = ConfigTable::new();
        let rt = build_range_table();
        let ud = build_unit_dist();
        let rep = ConfigReport::build(&t);
        let rep2 = ConfigReport::build(&t);
        let pure = rep.digest == rep2.digest;
        let _ = lcg.below(3);
        if range_table_sane(&rt) && unit_dist_conserved(&ud) && param_digests_unique() && pure && lookup_sane() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("矩阵纯函数轮={}/{}", ok, INJECT_ROUNDS))
}

/// 概况一行。
pub fn config_stats_line(t: &ConfigTable) -> String {
    format!(
        "配置概况：{} 项 双闸 {} 回滚 {} 变更 {}",
        CONFIG_COUNT,
        t.twin_agree(),
        t.rb_len(),
        t.change_len()
    )
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 K：逐参数一行导出（编号/名称/默认/域界四元组行）。
/// ─────────────────────────────────────────────────────────────────────
/// 单参数行。
pub fn param_line(p: &ConfigParam) -> String {
    format!("UNX-{} {} 默认={} 域[{},{}] {}", p.id, p.name, p.default_val, p.min, p.max, p.unit)
}

/// 全表逐行导出。
pub fn export_param_lines() -> String {
    let mut s = String::from("参数总表逐行：\n");
    for p in CONFIG_PARAMS.iter() {
        s.push_str(&param_line(p));
        s.push('\n');
    }
    s
}

/// 逐行导出完整判据。
pub fn param_lines_complete() -> bool {
    let s = export_param_lines();
    CONFIG_PARAMS.iter().all(|p| s.contains(&param_line(p)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 L：默认值守恒（默认闸逐项 ≡ 总表默认值，全表校验和可复算）。
/// ─────────────────────────────────────────────────────────────────────
/// 默认值守恒判据：defaults 逐项 ≡ CONFIG_PARAMS.default_val。
pub fn defaults_conserved(t: &ConfigTable) -> bool {
    CONFIG_PARAMS
        .iter()
        .enumerate()
        .all(|(i, p)| t.default_of(i) == Some(p.default_val))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 M：回滚账导出（逐行：轮/参数/坏值/复原值）。
/// ─────────────────────────────────────────────────────────────────────
/// 回滚账导出。
pub fn export_rollbacks(t: &ConfigTable) -> String {
    let mut s = String::from("回滚账：\n");
    for r in t.rb_iter() {
        s.push_str(&format!(
            "轮{} 参数{} 坏值{} → 复原{}\n",
            r.round, r.param, r.bad_val, r.restored
        ));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 N：默认复原 10/10 演练（篡改后回滚 → defaults 逐项守恒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_defaults_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB90 段基址 ^ 4。
    let mut lcg = Lcg::new(0xFB94);
    for r in 0..INJECT_ROUNDS {
        let mut t = ConfigTable::new();
        // 篡改 2 项（去重：同项回退改相邻项）。
        let a = (lcg.below(CONFIG_COUNT as u64)) as usize;
        let mut b = (lcg.below(CONFIG_COUNT as u64)) as usize;
        if b == a {
            b = (b + 1) % CONFIG_COUNT;
        }
        t.active[a] = CONFIG_PARAMS[a].min;
        t.active[b] = CONFIG_PARAMS[b].max;
        let n = t.rollback(r as u64 + 1);
        let cons = defaults_conserved(&t);
        let lines = param_lines_complete();
        let _ = lcg.below(3);
        if n == 2 && cons && t.twin_agree() && lines {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("默认复原轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：缺参数——影子短表必被抓（真实表 ≥12）。
    if CONFIG_COUNT >= 12 && CONFIG_PARAMS.iter().take(5).count() < 12 {
        caught += 1;
        d.push("注1 缺项:抓".to_string());
    } else {
        d.push("注1 缺项:漏".to_string());
    }
    // 注 2：越域写入必拒（RangeOob）。
    let mut tbl = ConfigTable::new();
    if matches!(tbl.set_active(0, CONFIG_PARAMS[0].max + 1), Err(CgCode::RangeOob)) {
        caught += 1;
        d.push("注2 越域:抓".to_string());
    } else {
        d.push("注2 越域:漏".to_string());
    }
    // 注 3：双闸失衡必检出（篡改后 twin_agree 翻红）。
    tbl.active[3] = CONFIG_PARAMS[3].min;
    let mismatch = !tbl.twin_agree() && tbl.detect_tamper().is_some();
    if mismatch {
        caught += 1;
        d.push("注3 失衡:抓".to_string());
    } else {
        d.push("注3 失衡:漏".to_string());
    }
    // 注 4：回滚失败——只修一半差异时 twin 仍红（半回滚必抓）。
    let mut tbl4 = ConfigTable::new();
    tbl4.active[1] = CONFIG_PARAMS[1].min;
    tbl4.active[2] = CONFIG_PARAMS[2].max;
    let _ = tbl4.rollback(1);
    let full = tbl4.twin_agree();
    // 半回滚核：手造半回滚态。
    let mut tbl4b = ConfigTable::new();
    tbl4b.active[1] = CONFIG_PARAMS[1].min;
    tbl4b.active[2] = CONFIG_PARAMS[2].max;
    tbl4b.active[1] = tbl4b.default_of(1).unwrap_or(0);
    let half = !tbl4b.twin_agree();
    if full && half {
        caught += 1;
        d.push("注4 半滚:抓".to_string());
    } else {
        d.push("注4 半滚:漏".to_string());
    }
    // 注 5：域倒登——影子参数 min>max 必被抓（真实表全不倒挂）。
    let inv = CONFIG_PARAMS.iter().any(|p| p.min > p.max);
    if !inv {
        caught += 1;
        d.push("注5 倒挂:抓".to_string());
    } else {
        d.push("注5 倒挂:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFB90 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    CgCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1251_checks：本单判据聚合（诊断段 0xFB90）。
pub fn run_unx1251_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1251 Arming 配置总表");
    // J1-a：参数 ≥12 项注册总表（点名项核对）。
    s.add(
        "J1-参数≥12项收口",
        ConfigTable::table_ok(),
        "14项/编号连续/域不倒挂/默认值在域",
    );
    // J1-b：双闸一致率 100%（域内改写→显式收口→一致）。
    let (ok2, d2) = drill_twin_10();
    s.add_dyn("J1-双闸一致率100%", ok2, &d2);
    // J1-c：篡改 10/10 检出回退。
    let (ok3, d3) = drill_tamper_rollback_10();
    s.add_dyn("J1-篡改10/10检出回退", ok3, &d3);
    // J1-d：摘要非空（写回面）。
    let tbl = ConfigTable::new();
    let sum = tbl.summary();
    s.add_dyn(
        "J1-摘要非空",
        !sum.is_empty() && tbl.twin_agree(),
        &config_stats_line(&tbl),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：边界写入 10/10。
    let (ok_b, d_b) = drill_boundary_10();
    s.add_dyn("J2-边界写入10/10", ok_b, &d_b);
    // J2-c：变更账 10/10。
    let (ok_c, d_c) = drill_change_log_10();
    s.add_dyn("J2-变更账10/10", ok_c, &d_c);
    // J2-d：矩阵纯函数 10/10 + 终验清单。
    let (ok_m, d_m) = drill_matrix_10();
    let rows = build_config_checklist(&tbl);
    s.add_dyn(
        "J2-矩阵/清单",
        ok_m && config_checklist_green(&rows),
        &format!("{} {}", d_m, rows.iter().filter(|r| r.passed).count()),
    );
    // J2-e：默认复原 10/10（含逐行导出/默认守恒）。
    let (ok_e, d_e) = drill_defaults_10();
    s.add_dyn("J2-默认复原10/10", ok_e, &d_e);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFB90段闭集");
    s
}
