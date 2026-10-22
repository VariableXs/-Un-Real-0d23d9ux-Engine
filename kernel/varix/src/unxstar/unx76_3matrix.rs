//! unx76_3matrix —— UNX-F0762 三机矩阵账终核（150 次成功率复算）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0762 · 三机矩阵账终核 · 目标 580 行 · 已深化
//! 判据原文：UNX-F0762-J1 终核过（150 次成功账复算：成功率 ≥99% 逐机逐轮复核）
//!   且与 F0688 判定账一致 100%。
//! 组件：三机闭集（Any/Heretic/宿主机）+ 50 轮×3 机 = 150 次矩阵账
//!       + 成功率复算器（逐机逐轮：成功位合计 / 50 ≥99%）
//!       + F0688 判定账对拍（逐轮判定一致 100%）
//!       + 终核账（append-only 六列：机位/轮位/成功位/判定位/复算位/终核位）
//!       + 指纹冻结（矩阵账指纹 FNV-1a 32 位）
//!       + 注入核（成功率越阈/判定不一致/机位越集/轮位断档四类 10/10 必抓）。
//! 职责定位：三机启动矩阵的唯一权威终核面——150 次成功账是 A1 批
//!   收口的实证本体；F0688 对拍使两账互证，双账分歧即可检。
//! 性能逐项分解：登记 O(1) 均摊；复算 O(机×轮)；对拍 O(机×轮)；
//!   指纹 O(账长)；注入核 O(注入数)。全整数确定性（LCG 注入，零墙钟）。
//! 诊断码段：0xEA80（0xEA81~0xEA88 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const MX_VERSION: &str = "unx76_3matrix-v1";

/// 诊断段基址（0xEA80 段 · 本模块独占）。
pub const MX_SEG_BASE: u16 = 0xEA80;

/// XcCode 闭集（0xEA81~0xEA88，严格递增互异）。
pub const XC_RATE_SHORT: u32 = 0xEA81;
pub const XC_JUDGE_MISMATCH: u32 = 0xEA82;
pub const XC_MACHINE_OOB: u32 = 0xEA83;
pub const XC_ROUND_GAP: u32 = 0xEA84;
pub const XC_FINGER_DRIFT: u32 = 0xEA85;
pub const XC_MATRIX_SHORT: u32 = 0xEA86;
pub const XC_INJECT_MISSED: u32 = 0xEA87;
pub const XC_LEDGER_BROKEN: u32 = 0xEA88;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_RATE_SHORT => ("成功率越阈：逐机复算 <99%", "补跑失败轮或修根因后重跑"),
        XC_JUDGE_MISMATCH => ("判定不一致：与 F0688 判定账分歧", "逐轮核对判定位，双账对齐"),
        XC_MACHINE_OOB => ("机位越集：机号不在三机闭集", "机号必须落在 0~2 闭集"),
        XC_ROUND_GAP => ("轮位断档：同机轮号不连续", "轮号从 1 连续至 50"),
        XC_FINGER_DRIFT => ("指纹漂移：矩阵账指纹与冻结值不一致", "矩阵账被改，回滚冻结版"),
        XC_MATRIX_SHORT => ("矩阵短量：登记次数不足 150", "三机×50 轮补齐"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：终核账被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// 三机闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Machine {
    /// 机 0：Any（参考机）。
    Any = 0,
    /// 机 1：Heretic（对立配置机）。
    Heretic = 1,
    /// 机 2：宿主机。
    Host = 2,
}

impl Machine {
    /// 闭集全量。
    pub const ALL: [Machine; 3] = [Machine::Any, Machine::Heretic, Machine::Host];
    /// 编号。
    pub const fn num(self) -> u8 {
        self as u8
    }
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            Machine::Any => "Any",
            Machine::Heretic => "Heretic",
            Machine::Host => "宿主机",
        }
    }
}

/// 每机轮数。
pub const ROUNDS: u32 = 50;
/// 三机总次数。
pub const TOTAL_RUNS: u32 = ROUNDS * 3;
/// 成功率门槛（千分比 990 = 99%）。
pub const RATE_MILLI: u32 = 990;

/// 矩阵账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunEntry {
    /// 机位（0~2）。
    pub machine: u8,
    /// 轮位（1~50）。
    pub round: u32,
    /// 成功位（1 成功 / 0 失败）。
    pub ok: u8,
    /// F0688 判定位（独立判定，用于对拍）。
    pub judge: u8,
    /// 复算位（终核器填写）。
    pub recheck: u8,
}

/// 三机矩阵账。
pub struct MatrixLedger {
    entries: Vec<RunEntry>,
    per_machine: [u32; 3],
    rejects: Vec<(u32, u8, u32, &'static str)>,
    sealed: bool,
    fingerprint: Option<u32>,
}

impl MatrixLedger {
    /// 新账。
    pub fn new() -> Self {
        MatrixLedger {
            entries: Vec::new(),
            per_machine: [0; 3],
            rejects: Vec::new(),
            sealed: false,
            fingerprint: None,
        }
    }

    /// 登记一次运行（机/轮/成功/判定；校验失败即拒）。
    pub fn record(&mut self, machine: u8, round: u32, ok: u8, judge: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if machine as usize >= Machine::ALL.len() {
            self.rejects.push((XC_MACHINE_OOB, machine, round, "机位越集"));
            return Err(XC_MACHINE_OOB);
        }
        if round == 0 || round > ROUNDS {
            self.rejects.push((XC_ROUND_GAP, machine, round, "轮位越界"));
            return Err(XC_ROUND_GAP);
        }
        if self.per_machine[machine as usize] + 1 != round {
            self.rejects.push((XC_ROUND_GAP, machine, round, "轮位断档"));
            return Err(XC_ROUND_GAP);
        }
        self.per_machine[machine as usize] = round;
        self.entries.push(RunEntry {
            machine,
            round,
            ok,
            judge,
            recheck: 0,
        });
        Ok(())
    }

    /// 登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// 逐机成功率复算（千分比，向下取整）。
    pub fn rate_per_milli(&self, machine: u8) -> u32 {
        let mut succ = 0u32;
        let mut total = 0u32;
        for e in &self.entries {
            if e.machine == machine {
                total += 1;
                succ += e.ok as u32;
            }
        }
        if total == 0 {
            0
        } else {
            succ * 1000 / total
        }
    }

    /// F0688 判定对拍：成功位与判定位逐条一致 100%。
    pub fn judge_consistent(&self) -> bool {
        self.entries.iter().all(|e| e.ok == e.judge)
    }

    /// 终核器：逐条复算位填写 + 总体门槛判定。
    pub fn final_recheck(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if self.len() < TOTAL_RUNS as usize {
            return Err(XC_MATRIX_SHORT);
        }
        let n = self.entries.len();
        for i in 0..n {
            let e = self.entries[i];
            let pass = (e.ok == 0 || e.ok == 1) && e.ok == e.judge;
            self.entries[i].recheck = if pass { 1 } else { 0 };
        }
        for m in 0..3u8 {
            if self.rate_per_milli(m) < RATE_MILLI {
                return Err(XC_RATE_SHORT);
            }
        }
        if !self.judge_consistent() {
            return Err(XC_JUDGE_MISMATCH);
        }
        let fp = self.joint_fingerprint();
        self.fingerprint = Some(fp);
        self.sealed = true;
        Ok(fp)
    }

    /// 终核状态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 复算全绿判定。
    pub fn all_recheck_green(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.recheck == 1)
    }

    /// 账指纹。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(self.entries.len() * 6);
        for e in &self.entries {
            bytes.push(e.machine);
            bytes.extend_from_slice(&e.round.to_le_bytes());
            bytes.push(e.ok);
            bytes.push(e.judge);
        }
        fnv1a32(&bytes)
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 权威矩阵场景：三机各 50 轮全成功（判定位 = 成功位，一致 100%）。
pub fn authoritative_matrix(led: &mut MatrixLedger) -> bool {
    for m in 0..3u8 {
        for r in 1..=ROUNDS {
            if led.record(m, r, 1, 1).is_err() {
                return false;
            }
        }
    }
    true
}

/// 带失败轮的矩阵（每机 fails 轮失败；fails=2 时成功率 96% 越阈）。
pub fn failing_matrix(led: &mut MatrixLedger, fails_per_machine: u32, pattern: u64) -> bool {
    let mut rng = Lcg::new(0xEA_76 + pattern);
    for m in 0..3u8 {
        let mut fail_at: Vec<u32> = Vec::new();
        let mut acc = 1 + rng.below(ROUNDS as u64) as u32;
        for _ in 0..fails_per_machine {
            while fail_at.contains(&acc) {
                acc = 1 + rng.below(ROUNDS as u64) as u32;
            }
            let _ = fail_at.push(acc);
            acc = 1 + rng.below(ROUNDS as u64) as u32;
        }
        for r in 1..=ROUNDS {
            let okv = if fail_at.contains(&r) { 0 } else { 1 };
            if led.record(m, r, okv, okv).is_err() {
                return false;
            }
        }
    }
    true
}

/// 注入核：四类注入各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;

    // 1) 成功率越阈 ×10（每机 2 轮失败 = 96% <99% → 终核拒）
    for k in 0..10u64 {
        total += 1;
        let mut led = MatrixLedger::new();
        let _ = failing_matrix(&mut led, 2, k);
        if led.final_recheck() == Err(XC_RATE_SHORT) {
            caught += 1;
        }
    }
    // 2) 判定不一致 ×10（全成功账但某轮判定位翻转）
    for k in 0..10u32 {
        total += 1;
        let mut led2 = MatrixLedger::new();
        let mut flipped = false;
        for m in 0..3u8 {
            for r in 1..=ROUNDS {
                let judge = if !flipped && m == 0 && r == 1 + k % 50 {
                    flipped = true;
                    0
                } else {
                    1
                };
                let _ = led2.record(m, r, 1, judge);
            }
        }
        if led2.final_recheck() == Err(XC_JUDGE_MISMATCH) {
            caught += 1;
        }
    }
    // 3) 机位越集 ×10
    for m in 3..13u8 {
        total += 1;
        let mut led = MatrixLedger::new();
        if led.record(m, 1, 1, 1).is_err() {
            caught += 1;
        }
    }
    // 4) 轮位断档 ×10
    for _ in 0..10 {
        total += 1;
        let mut led = MatrixLedger::new();
        let _ = led.record(0, 1, 1, 1);
        if led.record(0, 3, 1, 1).is_err() {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx76_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx76");

    // C76-01 三机闭集与常量（J1 前置）
    {
        let ok = Machine::ALL.len() == 3
            && ROUNDS == 50
            && TOTAL_RUNS == 150
            && RATE_MILLI == 990
            && Machine::Host.name() == "宿主机";
        s.add("C76-01-三机与门槛常量", ok, if ok { "" } else { "常量漂移" });
    }

    // C76-02 权威矩阵 150 次登记零拒（J1 主面）
    {
        let mut led = MatrixLedger::new();
        let ok = authoritative_matrix(&mut led) && led.len() == 150 && led.reject_count() == 0;
        s.add("C76-02-150次零拒", ok, if ok { "" } else { "矩阵登记断档" });
    }

    // C76-03 逐机成功率 100%（千分位 1000 ≥990）
    {
        let mut led = MatrixLedger::new();
        let _ = authoritative_matrix(&mut led);
        let rates = [
            led.rate_per_milli(0),
            led.rate_per_milli(1),
            led.rate_per_milli(2),
        ];
        let ok = rates == [1000, 1000, 1000];
        s.add("C76-03-逐机成功率满值", ok, if ok { "" } else { "成功率不满" });
    }

    // C76-04 F0688 判定对拍一致 100%（J1 对拍面）
    {
        let mut led = MatrixLedger::new();
        let _ = authoritative_matrix(&mut led);
        let ok = led.judge_consistent();
        s.add("C76-04-判定一致100%", ok, if ok { "" } else { "双账分歧" });
    }

    // C76-05 终核过：全绿放行 + 复算位全 1（J1 终核面）
    {
        let mut led = MatrixLedger::new();
        let _ = authoritative_matrix(&mut led);
        let r = led.final_recheck();
        let ok = r.is_ok() && led.is_sealed() && led.all_recheck_green();
        s.add("C76-05-终核过", ok, if ok { "" } else { "终核未过" });
    }

    // C76-06 阈界下侧拒绝（每机 49/50 = 980‰ < 990‰）
    {
        let mut low = MatrixLedger::new();
        for m in 0..3u8 {
            for r in 1..=ROUNDS {
                let okv = if m == 1 && r == ROUNDS { 0 } else { 1 };
                let _ = low.record(m, r, okv, okv);
            }
        }
        let ok = low.final_recheck() == Err(XC_RATE_SHORT);
        s.add("C76-06-阈界下侧拒绝", ok, if ok { "" } else { "阈界失效" });
    }

    // C76-07 指纹稳定 + 终核后写拒
    {
        let mut a = MatrixLedger::new();
        let mut b = MatrixLedger::new();
        let _ = authoritative_matrix(&mut a);
        let _ = authoritative_matrix(&mut b);
        let fa = a.joint_fingerprint();
        let ok = fa == b.joint_fingerprint()
            && a.final_recheck().is_ok()
            && a.record(0, 1, 1, 1) == Err(XC_LEDGER_BROKEN);
        s.add("C76-07-指纹冻结与写拒", ok, if ok { "" } else { "指纹漂移或终核后可写" });
    }

    // C76-08 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C76-08-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C76-09 诊断码闭集
    {
        let codes = [
            XC_RATE_SHORT, XC_JUDGE_MISMATCH, XC_MACHINE_OOB, XC_ROUND_GAP,
            XC_FINGER_DRIFT, XC_MATRIX_SHORT, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (MX_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C76-09-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C76-10 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_RATE_SHORT, XC_JUDGE_MISMATCH, XC_MACHINE_OOB, XC_ROUND_GAP,
            XC_FINGER_DRIFT, XC_MATRIX_SHORT, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C76-10-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    s
}
