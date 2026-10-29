//! unx4103_negstorm — UNX-F4103 负缓存防重复解析风暴回归（诊断段 0x8EE0）。
//!
//! 判据原文（提炼）：
//!   J1 同路径 10 万次连续不存在查找：开负缓存时 fs 穿透 == 1，
//!      关负缓存时穿透 == 100,000——对照实验账两组数据齐备。
//!   J2 风暴抑制比可计算：抑制比 = 关负缓存穿透数 / 开负缓存穿透数，
//!      本例恒等于负载次数（整数除法，除零拒收）。
//!   J3 风暴路径必须是"确认不存在"路径：实态存在的路径注入风暴实验
//!      被拒（E_PATH_EXIST），实验前提失守即拒跑。
//!   J4 两组实验账互不串账：开/关模式各自独立计数，终态可复核。
//!   J5 负载次数边界：零次负载拒跑（无实验意义），上限写死防失控。
//! 设计纪律：
//!   1. 对照实验双跑：同一负载在 NegOn / NegOff 两模式各跑一轮，
//!      模式由状态机锁定，单轮内不得切换（防串账）。
//!   2. 穿透单一计数面：穿透数只由 resolve 递增，命中账不触穿透。
//!   3. 负载次数写死：STORM_LOAD = 100_000（判据 J1 原文数），
//!      上限 STORM_CAP = 1_000_000 防失控注入。
//!   4. 抑制比整数化：不引入浮点，除法前查除数非零。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8EE0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8EE0;

/// 风暴负载次数：判据原文指定 10 万次（来源常量）。
pub const STORM_LOAD: u32 = 100_000;
/// 负载上限：防失控注入（工程防线，非判据）。
pub const STORM_CAP: u32 = 1_000_000;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_PATH_EXIST: u16 = SEG_BASE + 1; // 风暴路径实态存在（前提失守）
const E_LOAD_ZERO: u16 = SEG_BASE + 2;  // 零负载拒跑
const E_LOAD_OVER: u16 = SEG_BASE + 3;  // 负载超上限拒跑
const E_RUN_MISMATCH: u16 = SEG_BASE + 4; // 实测穿透数与模式预期不符
const E_DUP_RUN: u16 = SEG_BASE + 5;    // 同模式重复跑未清账（串账注入）
const E_RATIO_BAD: u16 = SEG_BASE + 6;  // 抑制比计算前置失守（除零）
const E_LEDGER_MIX: u16 = SEG_BASE + 7; // 两组实验账互串

/// 实验模式状态机：NegOn（负缓存开）/ NegOff（负缓存关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LabMode {
    NegOn,
    NegOff,
}

/// 单轮实验结果账：模式 + 负载 + 实测穿透 + 负命中。
pub struct StormRun {
    pub mode: LabMode,   // 本轮模式
    pub load: u32,       // 负载次数
    pub penetrations: u32, // 实测 fs 穿透数
    pub neg_hits: u32,   // 负命中数（仅 NegOn 可能非零）
}

/// 风暴实验室：双模式对照账 + 抑制比计算。
pub struct StormLab {
    pub fs_files: Vec<&'static str>, // fs 实态名集合（模拟）
    pub run_on: Option<StormRun>,    // 开负缓存轮结果
    pub run_off: Option<StormRun>,   // 关负缓存轮结果
    pub rejected: u32,               // 拒收注入计数
}

impl StormLab {
    pub fn new() -> StormLab {
        StormLab {
            fs_files: Vec::new(),
            run_on: None,
            run_off: None,
            rejected: 0,
        }
    }

    /// 供给 fs 实态（同名重复拒收）。
    pub fn fs_create(&mut self, name: &'static str) -> Result<(), u16> {
        for f in &self.fs_files {
            if *f == name {
                self.rejected += 1;
                return Err(E_PATH_EXIST);
            }
        }
        self.fs_files.push(name);
        Ok(())
    }

    /// fs 实态探测（模拟钩子）。
    fn probe(&self, name: &str) -> bool {
        for f in &self.fs_files {
            if *f == name {
                return true;
            }
        }
        false
    }

    /// 负载合法性门槛（判据 J3/J5）。
    fn load_ok(&mut self, path: &str, load: u32) -> Result<(), u16> {
        if load == 0 {
            return Err(E_LOAD_ZERO);
        }
        if load > STORM_CAP {
            self.rejected += 1;
            return Err(E_LOAD_OVER);
        }
        if self.probe(path) {
            self.rejected += 1;
            return Err(E_PATH_EXIST); // 实态存在的路径不得当风暴路径
        }
        Ok(())
    }

    /// 单轮实验（判据 J1）：模式内循环 load 次查找同路径。
    /// NegOn：首查穿透入负缓存，其后命中零穿透。
    /// NegOff：每查都穿透（负缓存关闭语义）。
    pub fn run(&mut self, mode: LabMode, path: &'static str, load: u32) -> Result<(), u16> {
        self.load_ok(path, load)?;
        // 串账防线：同模式重复跑须先重置该轮账
        let dup = match mode {
            LabMode::NegOn => self.run_on.is_some(),
            LabMode::NegOff => self.run_off.is_some(),
        };
        if dup {
            self.rejected += 1;
            return Err(E_DUP_RUN);
        }
        let mut penetrations = 0u32;
        let mut neg_hits = 0u32;
        let mut neg_cached = false; // 负缓存位（仅 NegOn 生效）
        let mut i = 0u32;
        while i < load {
            if mode == LabMode::NegOn && neg_cached {
                neg_hits += 1; // 负命中：零穿透
            } else {
                penetrations += 1; // 穿透恰好一次（NegOn 首查）或逐次（NegOff）
                if mode == LabMode::NegOn {
                    neg_cached = true; // 失败结果入负缓存
                }
            }
            i += 1;
        }
        let run = StormRun {
            mode,
            load,
            penetrations,
            neg_hits,
        };
        // 判据 J1 模式预期核验：账与语义必须一致
        match mode {
            LabMode::NegOn => {
                if run.penetrations != 1 {
                    return Err(E_RUN_MISMATCH);
                }
                self.run_on = Some(run);
            }
            LabMode::NegOff => {
                if run.penetrations != load {
                    return Err(E_RUN_MISMATCH);
                }
                if run.neg_hits != 0 {
                    return Err(E_RUN_MISMATCH);
                }
                self.run_off = Some(run);
            }
        }
        Ok(())
    }

    /// 风暴抑制比（判据 J2）：关/开穿透比，整数除法，除零拒收。
    pub fn suppression_ratio(&self) -> Result<u32, u16> {
        let on = match &self.run_on {
            Some(r) => r.penetrations,
            None => return Err(E_RATIO_BAD), // 未跑开负缓存轮
        };
        let off = match &self.run_off {
            Some(r) => r.penetrations,
            None => return Err(E_RATIO_BAD), // 未跑关负缓存轮
        };
        if on == 0 {
            return Err(E_RATIO_BAD); // 除零防线
        }
        Ok(off / on)
    }

    /// 两组账齐备且互不串账核验（判据 J4）。
    pub fn ledgers_conserved(&self) -> bool {
        let on = match &self.run_on {
            Some(r) => r,
            None => return false,
        };
        let off = match &self.run_off {
            Some(r) => r,
            None => return false,
        };
        on.mode == LabMode::NegOn
            && off.mode == LabMode::NegOff
            && on.load == off.load
            && on.penetrations == 1
            && off.penetrations == off.load
            && on.neg_hits == on.load - 1 // 首查穿透，余为负命中
            && off.neg_hits == 0
    }

    /// 拒收账读取（注入审计面）。
    pub fn rejections(&self) -> u32 {
        self.rejected
    }

    /// 穿透差账（判据 J1 对照差）：关负缓存穿透 - 开负缓存穿透，
    /// 两轮未齐时拒算（E_RATIO_BAD 复用账面）。
    pub fn penetration_gap(&self) -> Result<u32, u16> {
        let on = match &self.run_on {
            Some(r) => r.penetrations,
            None => return Err(E_RATIO_BAD),
        };
        let off = match &self.run_off {
            Some(r) => r.penetrations,
            None => return Err(E_RATIO_BAD),
        };
        if off < on {
            return Err(E_RUN_MISMATCH); // 开负缓存穿透反超：账不符
        }
        Ok(off - on)
    }

    /// 负命中覆盖率（NegOn 轮）：负命中 / (负命中+穿透)，
    /// 整数千分比；未跑轮拒算。本实验恒 (load-1)*1000/load。
    pub fn neg_coverage_permille(&self) -> Result<u32, u16> {
        let r = match &self.run_on {
            Some(r) => r,
            None => return Err(E_RATIO_BAD),
        };
        let denom = r.neg_hits + r.penetrations;
        if denom == 0 {
            return Err(E_RATIO_BAD); // 除零防线
        }
        Ok(r.neg_hits * 1000 / denom)
    }

    /// 清账重跑（对照实验复演面）：双轮结果清空，
    /// 拒收账保留（历史注入不灭失）。
    pub fn reset(&mut self) {
        self.run_on = None;
        self.run_off = None;
    }

    /// 单轮账只读访问（审计面）：按模式取轮账。
    pub fn run_of(&self, mode: LabMode) -> Result<(u32, u32, u32), u16> {
        let r = match mode {
            LabMode::NegOn => &self.run_on,
            LabMode::NegOff => &self.run_off,
        };
        match r {
            Some(x) => Ok((x.load, x.penetrations, x.neg_hits)),
            None => Err(E_LEDGER_MIX), // 该模式未跑
        }
    }
}

/// 自例程：R1 开负缓存风暴→R2 关负缓存风暴→R3 注入拦截→R4 抑制比对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 开负缓存：10 万次同路径不存在查找，穿透恰好 1。
    let mut lab = StormLab::new();
    lab.fs_create("real.txt")?;
    lab.run(LabMode::NegOn, "phantom", STORM_LOAD)?;
    let on = match &lab.run_on {
        Some(r) => r,
        None => return Err(E_LEDGER_MIX),
    };
    if on.penetrations != 1 || on.neg_hits != STORM_LOAD - 1 {
        return Err(E_RUN_MISMATCH); // 判据 J1 前半：穿透==1
    }
    hits += 1;

    // R2 关负缓存：同负载，穿透 == 100,000（判据 J1 后半）。
    lab.run(LabMode::NegOff, "phantom", STORM_LOAD)?;
    let off = match &lab.run_off {
        Some(r) => r,
        None => return Err(E_LEDGER_MIX),
    };
    if off.penetrations != STORM_LOAD {
        return Err(E_RUN_MISMATCH);
    }
    hits += 1;

    // R3 注入拦截：实态路径、零负载、超上限、同模式重跑全拒。
    if lab.run(LabMode::NegOn, "real.txt", 10).is_ok() {
        return Err(E_PATH_EXIST); // 实态存在路径不得跑风暴
    }
    if lab.run(LabMode::NegOn, "phantom2", 0).is_ok() {
        return Err(E_LOAD_ZERO); // 零负载应拒
    }
    if lab.run(LabMode::NegOn, "phantom2", STORM_CAP + 1).is_ok() {
        return Err(E_LOAD_OVER); // 超上限应拒
    }
    if lab.run(LabMode::NegOn, "phantom2", 10).is_ok() {
        return Err(E_DUP_RUN); // 同模式重复跑应拒
    }
    if lab.rejections() < 4 {
        return Err(E_LEDGER_MIX); // 拒收账必须如实记账
    }
    hits += 1;

    // R4 抑制比对账：比值 == 负载次数；两组账守恒成立。
    let ratio = lab.suppression_ratio()?;
    if ratio != STORM_LOAD {
        return Err(E_RATIO_BAD); // 判据 J2：1 次穿透 vs 10 万次
    }
    if !lab.ledgers_conserved() {
        return Err(E_LEDGER_MIX);
    }
    hits += 1;

    // R5 复演面：穿透差、负覆盖、清账重跑、单轮账读取。
    let gap = lab.penetration_gap()?;
    if gap != STORM_LOAD - 1 {
        return Err(E_RUN_MISMATCH); // 100000 - 1
    }
    // 负覆盖率 = 99999*1000/100000 = 999‰
    let cov = lab.neg_coverage_permille()?;
    if cov != 999 {
        return Err(E_RATIO_BAD);
    }
    let (ld, pen, neg) = lab.run_of(LabMode::NegOn)?;
    if ld != STORM_LOAD || pen != 1 || neg != STORM_LOAD - 1 {
        return Err(E_LEDGER_MIX);
    }
    if lab.run_of(LabMode::NegOff)?.1 != STORM_LOAD {
        return Err(E_LEDGER_MIX);
    }
    // 清账后未跑态拒绝取比值（防半账误报）。
    lab.reset();
    if lab.suppression_ratio().is_ok() {
        return Err(E_RATIO_BAD);
    }
    if lab.rejections() < 4 {
        return Err(E_LEDGER_MIX); // 拒收账跨清账保留
    }
    hits += 1;

    Ok(hits)
}
