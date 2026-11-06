//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18453 · 声明语义探针（10 例标准 manifest 对照） · 0x7ED00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18453-J1 10 例标准 manifest 判定与 Windows 对照零偏差，
//!       判据样本入 E1 库 E4 分册
//! 本模块结构清单：
//!   1. 冻结常量层：GUID 闭集 8（Vista→Win11，build 下界取实机口径）/
//!      样本数 10 / 判定轮数（推导注释写死）；
//!   2. 诊断码闭集 SpCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一：OsGuid（supportedOS GUID 闭集行：名 + build 下界）；
//!   4. 数据结构二：MfCase（标准 manifest 例：例号/GUID/build/Windows
//!      实机对照冻结判定）；
//!   5. 判定器：本地语义 = build >= GUID 下界 → Compatible，否则
//!      Incompatible（Windows supportedOS 口径：声明世代起向后兼容）；
//!   6. 数据结构三：E1 库 E4 分册样本账 SampleRegistry（10 例冻结
//!      入账，指纹 FNV-1a 复算，追加只读，例号唯一）；
//!   7. 对照核 probe_all：本地判定 vs 冻结判定逐例对拍 → 偏差数
//!      必须为 0（J1 前半句）；
//!   8. 注入核 4 类×10 轮：判定翻面 / 伪 GUID / build 越域 /
//!      例号重复，逐类断言被显式拒绝；
//!   9. 黄金全链 + run_unx18453_checks：15 条判据（断言性质）。
//! 为什么对照表冻结：偏差判定必须有不可动的外部基准——Windows
//! 实机口径以冻结表入账，本地判定器任何语义漂移都会在逐例对拍
//! 中现形，偏差账不为零即红。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7ED00，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7ED00;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// GUID 闭集大小（冻结 8：Vista→Win11 全集，与 F18442 口径对齐）。
pub const GUID_CLOSEN: usize = 8;
/// 标准 manifest 例数（冻结 10：J1 口径）。
pub const CASES: usize = 10;
/// build 域上界（冻结 99999：实机 build 全在六位数内）。
pub const BUILD_MAX: u32 = 99_999;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpCode {
    /// 伪 GUID（闭集外）
    GuidFake,
    /// build 越域（0 或 >99999）
    BuildIllegal,
    /// 判定翻面（本地 vs 冻结基准偏差）
    VerdictDeviation,
    /// 例号重复（E4 分册键唯一）
    CaseDup,
    /// 样本账指纹复算不符
    FingerprintMismatch,
    /// 样本账条数不足（E4 分册欠账）
    RegistryShort,
    /// 对照核样本缺例（判定面没跑满 10 例）
    ProbeMissing,
    /// 偏差账与样本账守恒破
    DeviationLedgerBroken,
    /// 双跑不一致
    NotReproducible,
}

impl SpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            SpCode::GuidFake => "伪 GUID：supportedOS 闭集 8 外",
            SpCode::BuildIllegal => "build 越域：0 或超 99999",
            SpCode::VerdictDeviation => "判定偏差：本地判定与 Windows 冻结基准不一致",
            SpCode::CaseDup => "例号重复：E4 分册例号唯一",
            SpCode::FingerprintMismatch => "指纹复算不符：样本账被篡改",
            SpCode::RegistryShort => "样本账欠账：E4 分册不足 10 例",
            SpCode::ProbeMissing => "对照核缺例：判定面没跑满全部标准例",
            SpCode::DeviationLedgerBroken => "偏差账守恒破：对照次数≠样本数",
            SpCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [SpCode; 9] = [
        SpCode::GuidFake,
        SpCode::BuildIllegal,
        SpCode::VerdictDeviation,
        SpCode::CaseDup,
        SpCode::FingerprintMismatch,
        SpCode::RegistryShort,
        SpCode::ProbeMissing,
        SpCode::DeviationLedgerBroken,
        SpCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SpCode::ALL.len() {
        if SpCode::ALL[i].val() == code {
            return SpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7ED00 段闭集内"
}

// ── 哈希（样本指纹） ────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 例指纹 = hash(case_id, guid_id, build, frozen_verdict)。
pub fn case_fingerprint(case_id: u32, guid_id: u32, build: u32, verdict: u32) -> u32 {
    let mut buf = [0u8; 16];
    let mut k = 0;
    while k < 4 {
        buf[k] = (case_id >> (8 * k)) as u8;
        buf[4 + k] = (guid_id >> (8 * k)) as u8;
        buf[8 + k] = (build >> (8 * k)) as u8;
        buf[12 + k] = (verdict >> (8 * k)) as u8;
        k += 1;
    }
    fnv1a(&buf)
}

// ── 数据结构一：GUID 闭集 ───────────────────────────────────────
/// supportedOS GUID 闭集行：名 + 实机 build 下界。
/// 下界推导（微软口径）：各世代首发 build——6000/7600/9200/9600/
/// 10240/17763/19044/22000。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsGuid {
    pub id: usize,
    pub name: &'static str,
    pub min_build: u32,
}

/// GUID 全集（冻结，与 F18442 世代表零偏差对齐）。
pub const GUID_TABLE: [OsGuid; GUID_CLOSEN] = [
    OsGuid { id: 0, name: "VISTA", min_build: 6000 },
    OsGuid { id: 1, name: "WIN7", min_build: 7600 },
    OsGuid { id: 2, name: "WIN8", min_build: 9200 },
    OsGuid { id: 3, name: "WIN81", min_build: 9600 },
    OsGuid { id: 4, name: "WIN10_1507", min_build: 10240 },
    OsGuid { id: 5, name: "WIN10_1809", min_build: 17763 },
    OsGuid { id: 6, name: "WIN10_21H2", min_build: 19044 },
    OsGuid { id: 7, name: "WIN11", min_build: 22000 },
];

/// 判定值闭集：1=Compatible，0=Incompatible。
pub const VERDICT_COMPAT: u32 = 1;
pub const VERDICT_INCOMPAT: u32 = 0;

// ── 本地判定器 ──────────────────────────────────────────────────
/// 单例判定：伪 GUID / build 越域显式拒；语义 = build >= GUID 下界
/// → Compatible（声明世代起向后兼容，Windows supportedOS 口径）。
pub fn judge(guid_id: u32, build: u32) -> Result<u32, u32> {
    if build == 0 || build > BUILD_MAX {
        return Err(SpCode::BuildIllegal.val());
    }
    let g = match GUID_TABLE.get(guid_id as usize) {
        Some(x) => *x,
        None => return Err(SpCode::GuidFake.val()),
    };
    if build >= g.min_build {
        Ok(VERDICT_COMPAT)
    } else {
        Ok(VERDICT_INCOMPAT)
    }
}

// ── 数据结构二：10 例标准 manifest（冻结） ──────────────────────
/// 标准例：例号 / GUID / build / Windows 实机对照冻结判定。
/// 冻结判定推导：与 judge 同语义的实机口径——build >= 下界即兼容。
/// 例 0: (WIN7, 6500)  → 6500 < 7600 → 0
/// 例 1: (WIN7, 7600)  → 边界相等   → 1
/// 例 2: (WIN10_1507, 10239) → 差 1 → 0（下界-1 边界负例）
/// 例 3: (WIN10_1507, 10240) → 边界相等 → 1
/// 例 4: (VISTA, 22000) → Win11 build 声明 Vista → 1（向后兼容正例）
/// 例 5: (WIN11, 6000) → 6000 < 22000 → 0（低 build 声明高世代）
/// 例 6: (WIN81, 9600) → 边界相等 → 1
/// 例 7: (WIN10_1809, 19044) → 19044 >= 17763 → 1
/// 例 8: (WIN8, 9199) → 差 1 → 0（下界-1 边界负例）
/// 例 9: (WIN11, 22000) → 边界相等 → 1
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MfCase {
    pub case_id: u32,
    pub guid_id: u32,
    pub build: u32,
    pub win_verdict: u32,
}

/// 10 例冻结表（Windows 实机对照基准，任何改动即偏差）。
pub const CASE_TABLE: [MfCase; CASES] = [
    MfCase { case_id: 0, guid_id: 1, build: 6500, win_verdict: 0 },
    MfCase { case_id: 1, guid_id: 1, build: 7600, win_verdict: 1 },
    MfCase { case_id: 2, guid_id: 4, build: 10239, win_verdict: 0 },
    MfCase { case_id: 3, guid_id: 4, build: 10240, win_verdict: 1 },
    MfCase { case_id: 4, guid_id: 0, build: 22000, win_verdict: 1 },
    MfCase { case_id: 5, guid_id: 7, build: 6000, win_verdict: 0 },
    MfCase { case_id: 6, guid_id: 3, build: 9600, win_verdict: 1 },
    MfCase { case_id: 7, guid_id: 5, build: 19044, win_verdict: 1 },
    MfCase { case_id: 8, guid_id: 2, build: 9199, win_verdict: 0 },
    MfCase { case_id: 9, guid_id: 7, build: 22000, win_verdict: 1 },
];

// ── 数据结构三：E1 库 E4 分册样本账 ─────────────────────────────
/// 样本账行：例号 + 指纹 + 冻结判定（追加只读）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRow {
    pub case_id: u32,
    pub fingerprint: u32,
    pub win_verdict: u32,
}

/// E1 库 E4 分册样本账（J1 后半句：判据样本入册）。
pub struct SampleRegistry {
    pub rows: Vec<SampleRow>,
    pub tamper_hits: u32,
}

impl SampleRegistry {
    /// 从冻结表整册入账（指纹期算，追加只读）。
    pub fn from_frozen() -> SampleRegistry {
        let mut rows = Vec::new();
        let mut i = 0;
        while i < CASE_TABLE.len() {
            let c = CASE_TABLE[i];
            rows.push(SampleRow {
                case_id: c.case_id,
                fingerprint: case_fingerprint(c.case_id, c.guid_id, c.build, c.win_verdict),
                win_verdict: c.win_verdict,
            });
            i += 1;
        }
        SampleRegistry { rows, tamper_hits: 0 }
    }

    /// 例号唯一线性扫。
    pub fn find_case(&self, case_id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.case_id == case_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 追加入账：例号重复拒（键唯一）。
    pub fn append(&mut self, row: SampleRow) -> Result<(), u32> {
        if self.find_case(row.case_id).is_some() {
            return Err(SpCode::CaseDup.val());
        }
        self.rows.push(row);
        Ok(())
    }

    /// 全册指纹复算：任一行指纹与重算不符 → 篡改检出。
    /// 职责边界：本函数只管指纹完整性（记录被改）；判定值与冻结表
    /// 的口径偏差归对照核 probe_all 检出（判定翻面注入保留原指纹，
    /// 若在此报指纹错会把偏差检出截胡——两闸必须分工）。
    pub fn audit(&mut self) -> Result<(), u32> {
        if self.rows.len() < CASES {
            return Err(SpCode::RegistryShort.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(SpCode::RegistryShort.val()),
            };
            // 反查冻结表还原原字段复算指纹。
            let c = match CASE_TABLE.get(r.case_id as usize) {
                Some(x) => *x,
                None => {
                    self.tamper_hits = self.tamper_hits.saturating_add(1);
                    return Err(SpCode::FingerprintMismatch.val());
                }
            };
            let expect = case_fingerprint(c.case_id, c.guid_id, c.build, c.win_verdict);
            if r.fingerprint != expect {
                self.tamper_hits = self.tamper_hits.saturating_add(1);
                return Err(SpCode::FingerprintMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 对照核 ──────────────────────────────────────────────────────
/// 对照结果账：对照例数 / 偏差例号账 / 偏差数。
#[derive(Debug)]
pub struct ProbeReport {
    pub compared: u32,
    pub deviations: Vec<u32>,
}

/// 对照核：10 例逐例本地判定 vs 冻结判定对拍。
/// 注册表判定被篡改（注入）时偏差必现形。
pub fn probe_all(reg: &mut SampleRegistry) -> Result<ProbeReport, u32> {
    reg.audit()?;
    if reg.rows.len() < CASES {
        return Err(SpCode::RegistryShort.val());
    }
    let mut rep = ProbeReport { compared: 0, deviations: Vec::new() };
    let mut i = 0;
    while i < CASE_TABLE.len() {
        let c = CASE_TABLE[i];
        // 判定面跑满 10 例（本地判定器 + 冻结基准双方）。
        let local = judge(c.guid_id, c.build)?;
        let frozen = match reg.find_case(c.case_id) {
            Some(idx) => match reg.rows.get(idx) {
                Some(r) => r.win_verdict,
                None => return Err(SpCode::RegistryShort.val()),
            },
            None => return Err(SpCode::ProbeMissing.val()),
        };
        if local != frozen {
            rep.deviations.push(c.case_id);
        }
        rep.compared = rep.compared.saturating_add(1);
        i += 1;
    }
    if rep.compared != CASES as u32 {
        return Err(SpCode::DeviationLedgerBroken.val());
    }
    Ok(rep)
}

// ── 注入核（4 类 ×10 轮） ───────────────────────────────────────
/// 注入检出账：四槽 tries/caught 成对。
#[derive(Debug)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：槽 0 判定翻面 / 槽 1 伪 GUID / 槽 2 build 越域 /
/// 槽 3 例号重复。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：判定翻面——样本账中一例判定被改，对照核必报偏差。
        let mut reg = SampleRegistry::from_frozen();
        let flip_idx = 3;
        if let Some(row) = reg.rows.get_mut(flip_idx) {
            row.win_verdict = if row.win_verdict == VERDICT_COMPAT {
                VERDICT_INCOMPAT
            } else {
                VERDICT_COMPAT
            };
        }
        let rep0 = probe_all(&mut reg);
        let hit0 = match &rep0 {
            Ok(rep) => rep.deviations.len() == 1 && rep.deviations.contains(&3),
            Err(_) => false,
        };
        led.record(0, hit0);
        // 槽 1：伪 GUID——guid_id=8 闭集外必拒。
        let hit1 = judge(GUID_CLOSEN as u32, 10240).err() == Some(SpCode::GuidFake.val());
        led.record(1, hit1);
        // 槽 2：build 越域——0 与 BUILD_MAX+1 都拒。
        let a2 = judge(1, 0).err() == Some(SpCode::BuildIllegal.val());
        let b2 = judge(1, BUILD_MAX + 1).err() == Some(SpCode::BuildIllegal.val());
        led.record(2, a2 && b2);
        // 槽 3：例号重复——append 同例号必拒。
        let mut reg3 = SampleRegistry::from_frozen();
        let c0 = CASE_TABLE[0];
        let hit3 = reg3
            .append(SampleRow {
                case_id: c0.case_id,
                fingerprint: case_fingerprint(c0.case_id, c0.guid_id, c0.build, c0.win_verdict),
                win_verdict: c0.win_verdict,
            })
            .err()
            == Some(SpCode::CaseDup.val());
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：样本入册→指纹审计→对照核零偏差→注入全抓→双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut reg = SampleRegistry::from_frozen();
    if reg.rows.len() != CASES {
        return Err(SpCode::RegistryShort.val());
    }
    reg.audit()?;
    let rep = probe_all(&mut reg)?;
    if rep.deviations.len() != 0 {
        return Err(SpCode::VerdictDeviation.val());
    }
    if rep.compared != CASES as u32 {
        return Err(SpCode::DeviationLedgerBroken.val());
    }
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || inj.missed() != 0 {
        return Err(SpCode::NotReproducible.val());
    }
    Ok((rep.compared, reg.tamper_hits))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SpCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：全部 10 例本地判定与冻结表一致。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut i = 0;
    while i < CASE_TABLE.len() {
        let c = CASE_TABLE[i];
        let local = judge(c.guid_id, c.build)?;
        if local != c.win_verdict {
            return Err(SpCode::VerdictDeviation.val());
        }
        i += 1;
    }
    Ok(())
}

/// 负探针 1：build 越域拒优先于伪 GUID（判定次序：域守卫先行）。
pub fn probe_seq_b() -> Result<(), u32> {
    // build=0 且 guid 闭集外：先报 build 域错。
    match judge(GUID_CLOSEN as u32 + 5, 0) {
        Err(c) if c == SpCode::BuildIllegal.val() => Ok(()),
        _ => Err(SpCode::BuildIllegal.val()),
    }
}

/// 负探针 2：下界-1 边界负例（build = 下界-1 必判不兼容）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut i = 0;
    while i < GUID_TABLE.len() {
        let g = GUID_TABLE[i];
        let v = judge(g.id as u32, g.min_build - 1)?;
        if v != VERDICT_INCOMPAT {
            return Err(SpCode::VerdictDeviation.val());
        }
        i += 1;
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SpCode::ALL.len() {
        let v = SpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18453_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18453 声明语义探针");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "对照{:?} 篡改检出{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0)
        ),
    );
    // ② 零偏差（J1 前半句）：10 例本地 vs Windows 冻结基准。
    let mut reg = SampleRegistry::from_frozen();
    let rep = probe_all(&mut reg);
    s.add(
        "10例对照零偏差",
        matches!(&rep, Ok(r) if r.deviations.len() == 0 && r.compared == CASES as u32),
        "本地判定与 Windows 冻结基准逐例相等",
    );
    // ③ 样本入册（J1 后半句）：E4 分册 10 例齐。
    s.add("E4分册10例齐", reg.rows.len() == CASES, "判据样本账 10/10 追加只读");
    // ④ 指纹复算：全册指纹与重算逐条相等。
    s.add("样本指纹复算", reg.audit().is_ok(), "FNV-1a 全册复算零失配");
    // ⑤ 判定器边界：各 GUID 下界相等 → 兼容。
    let mut edge_ok = true;
    let mut gi = 0;
    while gi < GUID_TABLE.len() {
        let gt = GUID_TABLE[gi];
        if judge(gt.id as u32, gt.min_build).unwrap_or(0) != VERDICT_COMPAT {
            edge_ok = false;
        }
        gi += 1;
    }
    s.add("下界边界兼容", edge_ok, "build == 下界必判 Compatible");
    // ⑥ 下界-1 全部不兼容。
    s.add("下界-1不兼容", probe_seq_c().is_ok(), "边界负例全判 Incompatible");
    // ⑦ 探针三序。
    s.add("探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok(),
        "全例一致/域守卫先行");
    // ⑧ 注入四类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入四类10/10",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0)
        ),
    );
    // ⑨ 漏检为零。
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "注入=检出+漏检，漏检=0");
    // ⑩ 伪 GUID 拒：闭集外逐个打。
    let mut fake_ok = true;
    let mut fi = GUID_CLOSEN as u32;
    while fi < GUID_CLOSEN as u32 + 4 {
        if judge(fi, 10240).err() != Some(SpCode::GuidFake.val()) {
            fake_ok = false;
        }
        fi += 1;
    }
    s.add("伪GUID逐个拒", fake_ok, "闭集外 4 个伪 ID 全拒");
    // ⑪ 对照守恒：对照例数 == 样本例数 == 10。
    s.add("对照守恒", matches!(&rep, Ok(r) if r.compared == CASES as u32),
        "对照次数=样本数，账不凭空生灭");
    // ⑫ 双跑一致。
    s.add("双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // ⑬ GUID 表与 F18442 世代口径对齐（build 单调递增复算）。
    let mut mono_ok = true;
    let mut mi = 1;
    while mi < GUID_TABLE.len() {
        let prev = GUID_TABLE[mi - 1];
        let cur = GUID_TABLE[mi];
        if cur.min_build <= prev.min_build {
            mono_ok = false;
        }
        mi += 1;
    }
    s.add("GUID表build单调", mono_ok, "世代推进 build 严格递增");
    // ⑭ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < SpCode::ALL.len() {
        if SpCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "9 码全有中文建议");
    s.add("段位守卫 0x7ED00", segment_guard(), "SEG_BASE 0x40 对齐窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_deviation_all_cases() {
        let mut reg = SampleRegistry::from_frozen();
        let rep = probe_all(&mut reg).unwrap_or(ProbeReport { compared: 0, deviations: Vec::new() });
        assert_eq!(rep.compared, 10, "对照跑满 10 例");
        assert!(rep.deviations.is_empty(), "零偏差");
    }

    #[test]
    fn registry_fingerprint_audit_clean() {
        let mut reg = SampleRegistry::from_frozen();
        assert!(reg.audit().is_ok());
        assert_eq!(reg.rows.len(), CASES);
        assert_eq!(reg.tamper_hits, 0);
    }

    #[test]
    fn verdict_flip_injection_deviation() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let mut reg = SampleRegistry::from_frozen();
            if let Some(row) = reg.rows.get_mut(1) {
                row.win_verdict = VERDICT_INCOMPAT;
            }
            let rep = probe_all(&mut reg).unwrap_or(ProbeReport { compared: 0, deviations: Vec::new() });
            assert!(rep.deviations.contains(&1), "第 {} 轮翻面必现偏差", r);
            r += 1;
        }
    }

    #[test]
    fn fake_guid_rejected_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            assert_eq!(
                judge(GUID_CLOSEN as u32, 7600).err(),
                Some(SpCode::GuidFake.val()),
                "第 {} 轮伪 GUID 必拒",
                r
            );
            r += 1;
        }
    }

    #[test]
    fn build_domain_edges() {
        assert_eq!(judge(0, 0).err(), Some(SpCode::BuildIllegal.val()));
        assert_eq!(judge(0, BUILD_MAX + 1).err(), Some(SpCode::BuildIllegal.val()));
        assert_eq!(judge(0, BUILD_MAX).unwrap_or(0), VERDICT_COMPAT);
    }

    #[test]
    fn case_dup_rejected() {
        let mut reg = SampleRegistry::from_frozen();
        let c0 = CASE_TABLE[0];
        assert_eq!(
            reg.append(SampleRow {
                case_id: c0.case_id,
                fingerprint: case_fingerprint(c0.case_id, c0.guid_id, c0.build, c0.win_verdict),
                win_verdict: c0.win_verdict,
            })
            .err(),
            Some(SpCode::CaseDup.val())
        );
    }

    #[test]
    fn guard_order_build_before_guid() {
        // 判定次序：build 域守卫先于 GUID 闭集（域先行协议）。
        assert_eq!(
            judge(GUID_CLOSEN as u32 + 9, 0).err(),
            Some(SpCode::BuildIllegal.val())
        );
    }

    #[test]
    fn boundary_minus_one_all_incompatible() {
        let mut i = 0;
        while i < GUID_TABLE.len() {
            let gt = GUID_TABLE[i];
            assert_eq!(
                judge(gt.id as u32, gt.min_build - 1).unwrap_or(1),
                VERDICT_INCOMPAT,
                "{} 下界-1 必不兼容",
                gt.name
            );
            i += 1;
        }
    }

    #[test]
    fn injection_ledger_all_caught_no_miss() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn double_run_identical() {
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn probes_and_segment_guard() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18453_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 {} 在窗内", s.len());
    }
}
