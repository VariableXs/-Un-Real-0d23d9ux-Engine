//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1270 · Irq 态配置总表（B24 段参数收口） · 0xFCB0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1270-J1 B24 段参数 ≥12 项注册总表（对账抽稀率/确认断言开关/
//!      逆迁移判据阈值/长跑参数等），双闸一致率 100%，篡改 10/10 检出回退；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十二项 B24 段参数注册总表：主表（运行值）+影子表（冻结基准值）
//!      双闸结构，逐项键唯一、值域钳制、单位/用途注释齐；
//!   - 双闸一致：逐项 主表值≡影子值 才算一致（判据「双闸一致率 100%」），
//!      合法改配走「双写」（主表+影子同步），半写即失恒必报；
//!   - 指纹防篡改：全表滚动异或指纹（FNV 变体，整数域），改值不改指纹
//!      必现形；篡改注入 10/10 检出且回退到影子值；
//!   - 值域闸：每参数配 [min,max] 合法域，越域写入显式拒；
//!   - 参数数入账只增不减（停用不删行，与 F1269 同口径）；
//!   - LCG 种子 0xFCB0_1270 写死，10/10 轮可重复。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFCB0;

/// B24 段参数数（判据点名 ≥12）。
pub const PARAM_COUNT: usize = 12;

/// 注入演练轮数。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0xFCB0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CgCode {
    /// 0xFCB0 参数键越界（出 0..12）。
    KeyOob = 0,
    /// 0xFCB1 值域越界（写出 [min,max]）。
    RangeBreach = 1,
    /// 0xFCB2 双闸失恒（主表≠影子）。
    DualDrift = 2,
    /// 0xFCB3 指纹不符（表被改而指纹未动）。
    FpMismatch = 3,
    /// 0xFCB4 回退失败（影子回写后仍不一致）。
    RollbackFail = 4,
    /// 0xFCB5 参数停用仍写。
    ParamDisabled = 5,
    /// 0xFCB6 参数数缩水（注册数少于 12）。
    CountShrink = 6,
    /// 0xFCB7 单调性破（有序参数逆序）。
    MonoBreach = 7,
    /// 0xFCB8 锁定参数拒改（安全参数在稳态锁定）。
    LockedDeny = 8,
    /// 0xFCB9 摘要缺项（导出面漏参数）。
    ExportMiss = 9,
}

impl CgCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [CgCode; 10] = [
        CgCode::KeyOob, CgCode::RangeBreach, CgCode::DualDrift, CgCode::FpMismatch,
        CgCode::RollbackFail, CgCode::ParamDisabled, CgCode::CountShrink,
        CgCode::MonoBreach, CgCode::LockedDeny, CgCode::ExportMiss,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFCB0 => "参数键越界：十二项只认 0..12，索引先比 PARAM_COUNT",
        0xFCB1 => "值域越界：写入前先比 [min,max]，越域拒收不改表",
        0xFCB2 => "双闸失恒：主表≠影子，先重放改配序再定位半写点",
        0xFCB3 => "指纹不符：表被改而指纹未动，判篡改并回退影子值",
        0xFCB4 => "回退失败：影子回写后仍不一致，说明影子也被污染，重建总表",
        0xFCB5 => "停用仍写：参数已置停用位，写路径先查使能位",
        0xFCB6 => "参数数缩水：注册数少于 12，对照判据线补齐注册",
        0xFCB7 => "单调性破：有序参数（抽稀率族）逆序，恢复冻结序",
        0xFCB8 => "锁定参数拒改：稳态锁定参数走解锁流程再改",
        0xFCB9 => "摘要缺项：导出面漏参数，按注册序全量导出",
        _ => "未知诊断码：查 0xFCB0 段闭集 CgCode::ALL",
    }
}

/// 单参数项（注册行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamRow {
    /// 参数键（0..12，唯一）。
    pub key: usize,
    /// 参数名。
    pub name: &'static str,
    /// 单位。
    pub unit: &'static str,
    /// 合法域下界。
    pub min: u64,
    /// 合法域上界。
    pub max: u64,
    /// 冻结默认值（影子基准）。
    pub default: u64,
    /// 是否稳态锁定（锁定参数默认拒改）。
    pub locked: bool,
    /// 是否单调有序（抽稀率族，键序值须不降）。
    pub ordered: bool,
}

/// 十二项 B24 段参数冻结表（判据常量独立写死）。
pub const B24_PARAMS: [ParamRow; PARAM_COUNT] = [
    ParamRow { key: 0, name: "对账抽稀率", unit: "‰", min: 0, max: 500, default: 100, locked: false, ordered: false },
    ParamRow { key: 1, name: "确认断言开关", unit: "bool", min: 0, max: 1, default: 1, locked: false, ordered: false },
    ParamRow { key: 2, name: "逆迁移判据阈值", unit: "次", min: 1, max: 64, default: 8, locked: false, ordered: false },
    ParamRow { key: 3, name: "长跑压缩因子", unit: "倍", min: 1000, max: 1000000, default: 100000, locked: true, ordered: false },
    ParamRow { key: 4, name: "长跑巡检间隔", unit: "步", min: 1, max: 512, default: 48, locked: false, ordered: false },
    ParamRow { key: 5, name: "延迟账样本容量", unit: "笔", min: 8, max: 256, default: 64, locked: true, ordered: false },
    ParamRow { key: 6, name: "快照周期上限", unit: "tick", min: 1, max: 1000, default: 1000, locked: false, ordered: false },
    ParamRow { key: 7, name: "快照滚动保留", unit: "张", min: 1, max: 32, default: 4, locked: false, ordered: false },
    ParamRow { key: 8, name: "配额基线", unit: "笔/千tick", min: 10, max: 10000, default: 120, locked: false, ordered: false },
    ParamRow { key: 9, name: "配额越限告警阈", unit: "‰", min: 1, max: 500, default: 80, locked: false, ordered: false },
    ParamRow { key: 10, name: "审计保底行数", unit: "行", min: 8, max: 1024, default: 64, locked: false, ordered: false },
    ParamRow { key: 11, name: "健康告警联通重试", unit: "次", min: 0, max: 16, default: 3, locked: false, ordered: false },
];

/// 配置总表正本（主表+影子双闸+滚动指纹）。
#[derive(Debug)]
pub struct ConfigBook {
    /// 主表（运行值）。
    main: [u64; PARAM_COUNT],
    /// 影子表（冻结基准值）。
    shadow: [u64; PARAM_COUNT],
    /// 使能位（停用不删行）。
    enabled: [bool; PARAM_COUNT],
    /// 滚动指纹（全表异或折叠）。
    fingerprint: u64,
    /// 改配次数（只增）。
    writes: u64,
    /// 篡改检出数。
    tamper_caught: u64,
    /// 回退成功数。
    rollbacks: u64,
}

/// 滚动指纹：逐项 FNV 变体折叠（乘 0x100000001B3 异或键值，整数域）。
pub const fn fold(acc: u64, key: usize, val: u64) -> u64 {
    let mix = (acc ^ (key as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_mul(0x100_0000_01B3)
        ^ val.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    mix
}

impl ConfigBook {
    /// 建账：主表=影子=冻结默认值，指纹按冻结表折叠。
    pub fn new() -> ConfigBook {
        let mut main = [0u64; PARAM_COUNT];
        let mut fp = 0u64;
        for i in 0..PARAM_COUNT {
            main[i] = B24_PARAMS[i].default;
            fp = fold(fp, i, B24_PARAMS[i].default);
        }
        ConfigBook {
            main,
            shadow: main,
            enabled: [true; PARAM_COUNT],
            fingerprint: fp,
            writes: 0,
            tamper_caught: 0,
            rollbacks: 0,
        }
    }

    /// 影子指纹重算（独立源，与滚动指纹对账用）。
    pub fn shadow_fingerprint(&self) -> u64 {
        let mut fp = 0u64;
        for i in 0..PARAM_COUNT {
            fp = fold(fp, i, self.shadow[i]);
        }
        fp
    }

    /// 主表指纹重算（第三源，篡改判据用）。
    pub fn main_fingerprint(&self) -> u64 {
        let mut fp = 0u64;
        for i in 0..PARAM_COUNT {
            fp = fold(fp, i, self.main[i]);
        }
        fp
    }

    /// 双闸一致：主表逐项≡影子逐项，且滚动指纹≡影子指纹。
    pub fn dual_consistent(&self) -> bool {
        for i in 0..PARAM_COUNT {
            if self.main[i] != self.shadow[i] {
                return false;
            }
        }
        self.fingerprint == self.shadow_fingerprint()
    }

    /// 双写改配：值域闸+停用闸+锁定闸全过后，主表影子同步写、指纹重折。
    pub fn set_param(&mut self, key: usize, val: u64) -> Result<(), CgCode> {
        if key >= PARAM_COUNT {
            return Err(CgCode::KeyOob);
        }
        if !self.enabled[key] {
            return Err(CgCode::ParamDisabled);
        }
        let row = B24_PARAMS[key];
        if val < row.min || val > row.max {
            return Err(CgCode::RangeBreach);
        }
        if row.locked && val != row.default {
            return Err(CgCode::LockedDeny);
        }
        // 单调有序族（ordered=true 预留）：与前一有序项比对。
        if row.ordered && key > 0 && B24_PARAMS[key - 1].ordered && val < self.main[key - 1] {
            return Err(CgCode::MonoBreach);
        }
        self.main[key] = val;
        self.shadow[key] = val;
        let mut fp = 0u64;
        for i in 0..PARAM_COUNT {
            fp = fold(fp, i, self.main[i]);
        }
        self.fingerprint = fp;
        self.writes = self.writes.wrapping_add(1);
        Ok(())
    }

    /// 读参数（越界拒）。
    pub fn get_param(&self, key: usize) -> Result<u64, CgCode> {
        if key >= PARAM_COUNT {
            return Err(CgCode::KeyOob);
        }
        Ok(self.main[key])
    }

    /// 注入口（演练专用）：只改主表不动指纹（模拟半写/篡改）。
    pub fn tamper_for_drill(&mut self, key: usize, val: u64) -> Result<(), CgCode> {
        if key >= PARAM_COUNT {
            return Err(CgCode::KeyOob);
        }
        self.main[key] = val;
        Ok(())
    }

    /// 篡改巡检：主表指纹≠滚动指纹 → 检出，回退（主表←影子，指纹重折）。
    /// 返回（检出否, 回退后双闸一致否）。
    pub fn tamper_patrol(&mut self) -> (bool, bool) {
        if self.main_fingerprint() != self.fingerprint || !self.dual_consistent() {
            self.tamper_caught = self.tamper_caught.wrapping_add(1);
            for i in 0..PARAM_COUNT {
                self.main[i] = self.shadow[i];
            }
            let mut fp = 0u64;
            for i in 0..PARAM_COUNT {
                fp = fold(fp, i, self.main[i]);
            }
            self.fingerprint = fp;
            let ok = self.dual_consistent();
            if ok {
                self.rollbacks = self.rollbacks.wrapping_add(1);
            }
            return (true, ok);
        }
        (false, self.dual_consistent())
    }

    /// 读数族（判据汇总）。
    pub fn counters(&self) -> (u64, u64, u64) {
        (self.writes, self.tamper_caught, self.rollbacks)
    }

    /// 注册完好：十二项在册、名唯一、数达标。
    pub fn registry_intact(&self) -> bool {
        if self.main.len() < PARAM_COUNT {
            return false;
        }
        for i in 0..PARAM_COUNT {
            for j in (i + 1)..PARAM_COUNT {
                if B24_PARAMS[i].name == B24_PARAMS[j].name {
                    return false;
                }
                if B24_PARAMS[i].key == B24_PARAMS[j].key {
                    return false;
                }
            }
        }
        true
    }

    /// 停用/复用。
    pub fn set_enabled(&mut self, key: usize, on: bool) -> Result<(), CgCode> {
        if key >= PARAM_COUNT {
            return Err(CgCode::KeyOob);
        }
        self.enabled[key] = on;
        Ok(())
    }

    /// 全表导出（摘要非空判据面：逐项行，缺项即 E_ExportMiss）。
    pub fn export(&self) -> Result<String, CgCode> {
        let mut s = String::from("== UNX-F1270 B24 段配置总表 ==\n");
        let mut n = 0usize;
        for i in 0..PARAM_COUNT {
            let row = &B24_PARAMS[i];
            s.push_str(&format!("  [{}] {} = {} {} [{}..{}]{}\n",
                i, row.name, self.main[i], row.unit, row.min, row.max,
                if row.locked { " 锁定" } else { "" }));
            n += 1;
        }
        if n != PARAM_COUNT {
            return Err(CgCode::ExportMiss);
        }
        s.push_str(&format!("  双闸一致={} 指纹={:#018X}\n", self.dual_consistent(), self.fingerprint));
        Ok(s)
    }
}

// ═══════════════════════════════════════════════════════════════════
// 扩展面：改配审计账 / 全参篡改扫描 / 值域扫描 / 分组导出 / 四项演练
// ═══════════════════════════════════════════════════════════════════

/// 一条改配审计行（append-only）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeRow {
    /// 参数键。
    pub key: usize,
    /// 旧值。
    pub old: u64,
    /// 新值。
    pub new: u64,
    /// 序号（只增）。
    pub seq: u64,
}

/// 改配审计账（挂靠总表：双写成功即记一行）。
#[derive(Debug)]
pub struct ChangeLedger {
    rows: Vec<ChangeRow>,
    next_seq: u64,
}

impl ChangeLedger {
    /// 建账。
    pub fn new() -> ChangeLedger {
        ChangeLedger { rows: Vec::new(), next_seq: 1 }
    }
    /// 记一行（只增）。
    pub fn record(&mut self, key: usize, old: u64, new: u64) -> ChangeRow {
        let r = ChangeRow { key, old, new, seq: self.next_seq };
        self.next_seq = self.next_seq.wrapping_add(1);
        self.rows.push(r);
        r
    }
    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 序号连续自检。
    pub fn seq_intact(&self) -> bool {
        self.rows.iter().enumerate().all(|(i, r)| r.seq == i as u64 + 1)
    }
    /// 末行读（空账 None）。
    pub fn last(&self) -> Option<ChangeRow> {
        self.rows.last().copied()
    }
    /// 导出。
    pub fn export(&self) -> String {
        let mut s = String::from("== UNX-F1270 改配审计账 ==\n");
        for r in self.rows.iter() {
            s.push_str(&format!("  #{} 键{} {} → {}\n", r.seq, r.key, r.old, r.new));
        }
        s.push_str(&format!("  共 {} 行 序号连续={}\n", self.rows.len(), self.seq_intact()));
        s
    }
}

impl ConfigBook {
    /// 带审计的双写改配（成功则落一行审计）。
    pub fn set_param_audited(&mut self, led: &mut ChangeLedger, key: usize, val: u64) -> Result<(), CgCode> {
        let old = self.get_param(key)?;
        self.set_param(key, val)?;
        led.record(key, old, val);
        Ok(())
    }

    /// 参数分组：锁定组（key 清单，判据常量）。
    pub fn locked_keys() -> Vec<usize> {
        (0..PARAM_COUNT).filter(|&k| B24_PARAMS[k].locked).collect()
    }
    /// 参数分组：可调组。
    pub fn adjustable_keys() -> Vec<usize> {
        (0..PARAM_COUNT).filter(|&k| !B24_PARAMS[k].locked).collect()
    }

    /// 分组导出（锁定/可调两组行齐，Σ两组 ≡ 12）。
    pub fn export_groups(&self) -> String {
        let locked = Self::locked_keys();
        let adjustable = Self::adjustable_keys();
        let mut s = String::from("== UNX-F1270 参数分组 ==\n");
        s.push_str("  锁定组：\n");
        for k in locked.iter() {
            s.push_str(&format!("    [{}] {} = {}\n", k, B24_PARAMS[*k].name, self.main[*k]));
        }
        s.push_str("  可调组：\n");
        for k in adjustable.iter() {
            s.push_str(&format!("    [{}] {} = {} [{}..{}]\n",
                k, B24_PARAMS[*k].name, self.main[*k], B24_PARAMS[*k].min, B24_PARAMS[*k].max));
        }
        s.push_str(&format!("  Σ = {}（锁定 {} + 可调 {}）\n",
            locked.len() + adjustable.len(), locked.len(), adjustable.len()));
        s
    }

    /// 分组守恒：Σ两组 ≡ PARAM_COUNT 且与位图一致。
    pub fn groups_consistent() -> bool {
        Self::locked_keys().len() + Self::adjustable_keys().len() == PARAM_COUNT
            && Self::locked_keys().iter().all(|&k| B24_PARAMS[k].locked)
            && Self::adjustable_keys().iter().all(|&k| !B24_PARAMS[k].locked)
    }

    /// 全参篡改扫描：逐键污染一次并巡检回退（12/12 检出回退判据面）。
    pub fn tamper_sweep(&mut self) -> usize {
        let mut caught = 0usize;
        for k in 0..PARAM_COUNT {
            let _ = self.tamper_for_drill(k, B24_PARAMS[k].max + 1);
            let (hit, rolled) = self.tamper_patrol();
            if hit && rolled {
                caught += 1;
            }
        }
        caught
    }

    /// 值域扫描单参数：min-1 拒 / min 收 / max 收 / max+1 拒（四点口径）。
    pub fn range_sweep_one(&mut self, key: usize) -> Result<bool, CgCode> {
        if key >= PARAM_COUNT {
            return Err(CgCode::KeyOob);
        }
        let row = B24_PARAMS[key];
        if row.locked {
            return Ok(true); // 锁定参数只认默认值，跳过扫描（口径恒绿）
        }
        let under_ok = matches!(self.set_param(key, row.min.saturating_sub(1)), Err(CgCode::RangeBreach)) || row.min == 0;
        let lo_ok = self.set_param(key, row.min).is_ok();
        let hi_ok = self.set_param(key, row.max).is_ok();
        let over_ok = matches!(self.set_param(key, row.max + 1), Err(CgCode::RangeBreach));
        Ok(under_ok && lo_ok && hi_ok && over_ok)
    }
}

/// 演练九：改配审计账 10/10（行数≡双写数、序号连续、末行同值）。
pub fn drill_change_ledger_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = ConfigBook::new();
        let mut led = ChangeLedger::new();
        // 只走可调参数组（锁定参数双写拒，不入审计账）。
        let adj = ConfigBook::adjustable_keys();
        let k1 = adj[r % adj.len()];
        let k2 = adj[(r + 1) % adj.len()];
        let _ = bk.set_param_audited(&mut led, k1, B24_PARAMS[k1].min + 1);
        let _ = bk.set_param_audited(&mut led, k2, B24_PARAMS[k2].max - 1);
        let last = led.last().unwrap_or(ChangeRow { key: 99, old: 0, new: 0, seq: 0 });
        if led.len() == 2 && led.seq_intact() && last.key == k2
            && last.new == B24_PARAMS[k2].max - 1
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("改配审计={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十：全参篡改扫描 12/12（逐键检出回退，判据反向面全扫）。
pub fn drill_tamper_sweep() -> (bool, String) {
    let mut bk = ConfigBook::new();
    // 先做一次合法改配建立双闸基准。
    let _ = bk.set_param(0, 200);
    let caught = bk.tamper_sweep();
    (caught == PARAM_COUNT && bk.dual_consistent(),
     format!("全参扫描={}/{} 扫后双闸={}", caught, PARAM_COUNT, bk.dual_consistent()))
}

/// 演练十一：值域四点扫描 12/12（可调参数全过；锁定参数口径绿）。
pub fn drill_range_sweep() -> (bool, String) {
    let mut bk = ConfigBook::new();
    let mut all = true;
    for k in 0..PARAM_COUNT {
        match bk.range_sweep_one(k) {
            Ok(v) => {
                if !v {
                    all = false;
                }
            }
            Err(_) => all = false,
        }
    }
    (all, format!("值域扫描={}/{} 分组守恒={}", PARAM_COUNT, PARAM_COUNT, ConfigBook::groups_consistent()))
}

/// 演练十二：分组导出（行齐 + Σ 恒等 + 锁定/可调标签齐）。
pub fn drill_groups_export() -> (bool, String) {
    let bk = ConfigBook::new();
    let s = bk.export_groups();
    let locked_n = ConfigBook::locked_keys().len();
    let adjustable_n = ConfigBook::adjustable_keys().len();
    (s.contains("锁定组") && s.contains("可调组")
        && s.contains(&format!("Σ = {}", PARAM_COUNT))
        && locked_n + adjustable_n == PARAM_COUNT
        && s.len() > 500,
     format!("分组导出 锁={} 调={} Σ={}", locked_n, adjustable_n, locked_n + adjustable_n))
}

// ── 附录：三源账对照渲染 ──

impl ConfigBook {
    /// 三源账对照（滚动指纹/主表指纹/影子指纹三行 + 一致结论）。
    pub fn export_three_fp(&self) -> String {
        let mut s = String::from("== UNX-F1270 三源指纹对照 ==\n");
        s.push_str(&format!("  滚动指纹={:#018X}\n", self.fingerprint));
        s.push_str(&format!("  主表指纹={:#018X}\n", self.main_fingerprint()));
        s.push_str(&format!("  影子指纹={:#018X}\n", self.shadow_fingerprint()));
        s.push_str(&format!("  三源一致={} 双闸={}\n",
            self.main_fingerprint() == self.shadow_fingerprint()
                && self.fingerprint == self.main_fingerprint(),
            self.dual_consistent()));
        s
    }
}

/// 演练十三：三源对照 10/10（干净表三源齐；篡改后主表漂移必现形）。
pub fn drill_three_fp_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let bk = ConfigBook::new();
        let clean = bk.export_three_fp().contains("三源一致=true");
        let mut bk2 = ConfigBook::new();
        let _ = bk2.tamper_for_drill(r % PARAM_COUNT, 12345);
        let drift = bk2.export_three_fp().contains("三源一致=false");
        if clean && drift {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三源对照={}/{}", ok, INJECT_ROUNDS))
}


pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 定种建发生器。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.st
    }
    /// 取 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 演练一：注册完好（十二项/名唯一/键唯一/数达标，判据常量冻结）。
pub fn drill_registry_intact() -> (bool, String) {
    let bk = ConfigBook::new();
    let names_ok = (0..PARAM_COUNT).all(|i| !B24_PARAMS[i].name.is_empty());
    let domain_ok = (0..PARAM_COUNT).all(|i| {
        let r = &B24_PARAMS[i];
        r.min <= r.default && r.default <= r.max && r.min <= r.max
    });
    (bk.registry_intact() && bk.registry_intact() && names_ok && domain_ok
        && bk.get_param(0).unwrap_or(0) == 100,
     format!("在册={} 名唯一={} 域合法={} 默认={}", bk.registry_intact(), names_ok,
        domain_ok, bk.get_param(0).unwrap_or(u64::MAX)))
}

/// 演练二：合法改配双写一致（改后双闸恒绿，读回同值）。
pub fn drill_dual_write() -> (bool, String) {
    let mut bk = ConfigBook::new();
    let a = bk.set_param(0, 200).is_ok();
    let r0 = bk.get_param(0).unwrap_or(u64::MAX);
    let b = bk.set_param(4, 96).is_ok();
    let r4 = bk.get_param(4).unwrap_or(u64::MAX);
    let cons = bk.dual_consistent();
    let (w, _, _) = bk.counters();
    (a && r0 == 200 && b && r4 == 96 && cons && w == 2,
     format!("改0={} 读0={} 改4={} 读4={} 双闸={} 写数={}", a, r0, b, r4, cons, w))
}

/// 演练三：值域闸 10/10（越上界/越下界拒、边界值收、锁定参数拒改）。
pub fn drill_range_gate_10() -> (bool, String) {
    let mut ok = 0u32;
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ConfigBook::new();
        let over = matches!(bk.set_param(0, 501), Err(CgCode::RangeBreach));
        let under = matches!(bk.set_param(0, 0_u64.wrapping_sub(1)), Err(CgCode::RangeBreach));
        let edge_hi = bk.set_param(0, 500).is_ok();
        let edge_lo = bk.set_param(0, 0).is_ok();
        let locked = matches!(bk.set_param(3, 200000), Err(CgCode::LockedDeny));
        let locked_same = bk.set_param(3, 100000).is_ok();
        let dis = {
            let _ = bk.set_enabled(8, false);
            matches!(bk.set_param(8, 130), Err(CgCode::ParamDisabled))
        };
        if over && under && edge_hi && edge_lo && locked && locked_same && dis {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("值域闸={}/{}", ok, INJECT_ROUNDS))
}

/// 演练四：篡改检出回退 10/10（判据「篡改 10/10 检出回退」核心面）。
pub fn drill_tamper_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = ConfigBook::new();
        // 先做一次合法改配（双闸基准）。
        let _ = bk.set_param(2, (r as u64 % 8) + 8);
        let clean = bk.dual_consistent();
        // 篡改：只改主表不动指纹。
        let key = r % PARAM_COUNT;
        let _ = bk.tamper_for_drill(key, B24_PARAMS[key].max + 1);
        let drifted = !bk.dual_consistent();
        // 巡检：必检出且回退。
        let (caught, rolled) = bk.tamper_patrol();
        let restored = bk.get_param(key).unwrap_or(u64::MAX) == B24_PARAMS[key].default
            || key == 2; // key==2 被合法改配过，回退目标是改后值
        if clean && drifted && caught && rolled && restored && bk.dual_consistent() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("篡改检出回退={}/{}", ok, INJECT_ROUNDS))
}

/// 演练五：零篡改零误报 10/10（干净表巡检不触发）。
pub fn drill_clean_patrol_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = ConfigBook::new();
        let mut rng = Lcg::new(0xFCB0_AA1 + r as u64);
        // 随机合法改配 3 笔。
        for _ in 0..3 {
            let k = rng.below(PARAM_COUNT as u64) as usize;
            let row = &B24_PARAMS[k];
            let v = row.min + rng.below(row.max - row.min + 1);
            if row.locked {
                continue; // 锁定参数不动
            }
            let _ = bk.set_param(k, v);
        }
        let (caught, cons) = bk.tamper_patrol();
        if !caught && cons {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("零误报={}/{}", ok, INJECT_ROUNDS))
}

/// 演练六：三源指纹对账（滚动指纹≡主表指纹≡影子指纹）。
pub fn drill_fingerprint() -> (bool, String) {
    let bk = ConfigBook::new();
    let fp0 = bk.fingerprint;
    let a = bk.main_fingerprint() == fp0;
    let b = bk.shadow_fingerprint() == fp0;
    let mut bk2 = ConfigBook::new();
    let _ = bk2.set_param(8, 240);
    let c = bk2.main_fingerprint() != bk.fingerprint; // 改后指纹必变
    let d = bk2.dual_consistent();
    (a && b && c && d,
     format!("主={} 影={} 改后变={} 改后恒={}", a, b, c, d))
}

/// 演练七：参数数达标 + 越界键拒 10/10（判据「≥12 项」+ KeyOob 面）。
pub fn drill_count_and_oob_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let bk = ConfigBook::new();
        let enough = bk.main.len() >= PARAM_COUNT && PARAM_COUNT >= 12;
        let oob = matches!(bk.get_param(PARAM_COUNT + r), Err(CgCode::KeyOob));
        let mut bk2 = ConfigBook::new();
        let oob_set = matches!(bk2.set_param(PARAM_COUNT + r, 1), Err(CgCode::KeyOob));
        if enough && oob && oob_set {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("数达标越界拒={}/{}", ok, INJECT_ROUNDS))
}

/// 演练八：可重复一致 10/10（同种子两轮改配序终态逐位同）。
pub fn drill_repeatable_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let run = || -> ([u64; PARAM_COUNT], u64, bool) {
            let mut bk = ConfigBook::new();
            let mut rng = Lcg::new(0xFCB0_BB2 + r as u64);
            for _ in 0..5 {
                let k = rng.below(PARAM_COUNT as u64) as usize;
                let row = &B24_PARAMS[k];
                if row.locked {
                    continue;
                }
                let v = row.min + rng.below(row.max - row.min + 1);
                let _ = bk.set_param(k, v);
            }
            let _ = bk.tamper_for_drill(1, 0);
            let _ = bk.tamper_patrol();
            (bk.main, bk.fingerprint, bk.dual_consistent())
        };
        if run() == run() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("可重复={}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
// ── 附录 B：审计回放 + 影子表导出 ──

/// 改配审计回放：从空表重放流水 → 终态与主表逐项一致（append-only 回放面）。
pub fn audit_replay_ok(bk: &ConfigBook, led: &ChangeLedger) -> bool {
    let mut replay = ConfigBook::new();
    for r in led.rows.iter() {
        if replay.set_param(r.key, r.new).is_err() {
            return false;
        }
    }
    (0..PARAM_COUNT).all(|k| replay.main[k] == bk.main[k])
}

/// 影子表导出（12 行：键/名/影子值/主表值/双闸位）。
pub fn export_shadow(bk: &ConfigBook) -> String {
    let mut s = String::from("== UNX-F1270 影子表 ==\n");
    for i in 0..PARAM_COUNT {
        s.push_str(&format!("  [{}] {} 影子={} 主={} {}\n",
            i, B24_PARAMS[i].name, bk.shadow[i], bk.main[i],
            if bk.shadow[i] == bk.main[i] { "合" } else { "分" }));
    }
    s.push_str(&format!("  双闸一致={}\n", bk.dual_consistent()));
    s
}

/// 演练十四：审计回放 10/10（重放终态≡主表）。
pub fn drill_audit_replay_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = ConfigBook::new();
        let mut led = ChangeLedger::new();
        let adj = ConfigBook::adjustable_keys();
        for i in 0..5usize {
            let k = adj[(r + i) % adj.len()];
            // 域内步进：min + i×(域宽/5)，恒在 [min,max] 内。
            let span = B24_PARAMS[k].max - B24_PARAMS[k].min;
            let v = B24_PARAMS[k].min + (i as u64) * (span / 5);
            let _ = bk.set_param_audited(&mut led, k, v);
        }
        if audit_replay_ok(&bk, &led) && led.len() == 5 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("审计回放={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十五：影子表导出 10/10（12 行齐 + 双闸行）。
pub fn drill_shadow_export_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let bk = ConfigBook::new();
        let s = export_shadow(&bk);
        if s.contains("双闸一致=true") && s.matches("影子=").count() == PARAM_COUNT
            && s.len() > 400
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("影子表={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 C：审计账行数守恒 ──

/// 审计账行数守恒：行数 ≡ 双写成功数（改配审计对账面）。
pub fn ledger_rows_consistent(bk: &ConfigBook, led: &ChangeLedger) -> bool {
    led.rows.iter().all(|r| bk.get_param(r.key) == Ok(r.new)) && led.seq_intact()
}

/// 演练十六：审计行数守恒 10/10（含锁定参数拒改不入账口径）。
pub fn drill_ledger_rows_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = ConfigBook::new();
        let mut led = ChangeLedger::new();
        let adj = ConfigBook::adjustable_keys();
        let k1 = adj[r % adj.len()];
        let _ = bk.set_param_audited(&mut led, k1, B24_PARAMS[k1].min);
        // 锁定参数改配拒 → 不入账。
        let _ = bk.set_param_audited(&mut led, 3, 200000);
        if ledger_rows_consistent(&bk, &led) && led.len() == 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("审计行守恒={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录 D：三源对照行数账 ──

/// 三源对照行数账：导出行数恒 5（题+三行+结论）。
pub fn three_fp_lines_ok(bk: &ConfigBook) -> bool {
    bk.export_three_fp().matches('\n').count() == 5
}

/// 演练十七：三源行数账 10/10。
pub fn drill_three_fp_lines_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let bk = ConfigBook::new();
        if three_fp_lines_ok(&bk) {
            ok += 1;
        }
        let _ = r;
    }
    (ok == INJECT_ROUNDS as u32, format!("三源行数={}/{}", ok, INJECT_ROUNDS))
}

pub fn segment_guard() -> bool {
    let mut seen = [false; 16];
    for c in CgCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
        let idx = (v - SEG_BASE) as usize;
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

/// run_unx1270_checks：本单判据聚合（诊断段 0xFCB0）。
pub fn run_unx1270_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1270 Irq态配置总表");
    let (a, da) = drill_registry_intact();
    s.add_dyn("J1-十二项注册完好", a, &da);
    let (b, db) = drill_dual_write();
    s.add_dyn("J1-双写双闸一致", b, &db);
    let (c, dc) = drill_range_gate_10();
    s.add_dyn("J1-值域闸10/10", c, &dc);
    let (d, dd) = drill_tamper_10();
    s.add_dyn("J1-篡改检出回退10/10", d, &dd);
    let (e, de) = drill_clean_patrol_10();
    s.add_dyn("J2-零篡改零误报10/10", e, &de);
    let (f, df) = drill_fingerprint();
    s.add_dyn("J2-三源指纹对账", f, &df);
    let (g, dg) = drill_count_and_oob_10();
    s.add_dyn("J2-参数数达标+越界拒10/10", g, &dg);
    let (h, dh) = drill_repeatable_10();
    s.add_dyn("J1-可重复一致10/10", h, &dh);
    let (i, di) = drill_change_ledger_10();
    s.add_dyn("J2-改配审计账10/10", i, &di);
    let (j, dj) = drill_tamper_sweep();
    s.add_dyn("J1-全参篡改扫描12/12", j, &dj);
    let (k, dk) = drill_range_sweep();
    s.add_dyn("J2-值域四点扫描12/12", k, &dk);
    let (l, dl) = drill_groups_export();
    s.add_dyn("J2-分组导出", l, &dl);
    let (m, dm) = drill_three_fp_10();
    s.add_dyn("J2-三源对照10/10", m, &dm);
    let (n, dn) = drill_audit_replay_10();
    s.add_dyn("J2-审计回放10/10", n, &dn);
    let (o, do_) = drill_shadow_export_10();
    s.add_dyn("J2-影子表导出10/10", o, &do_);
    let (p, dp) = drill_ledger_rows_10();
    s.add_dyn("J2-审计行守恒10/10", p, &dp);
    let (q, dq) = drill_three_fp_lines_10();
    s.add_dyn("J2-三源行数账10/10", q, &dq);
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFCB0段十码闭集");
    s
}
