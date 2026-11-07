//! UNX-F51534 B17 错误码扩展（带宽族 ×10 并入域内三表） · 0xC0880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51535-J1 持久化 ×10 恢复一致率 100%；损坏注入 10/10 走默认安全档
//!
//! 设计要点：
//!   ① 带宽族 ×10 错误码并入域内三表：码值表、严重度档表、建议文案表，
//!      三表同索引同序，任一索引处 (码值, 严重度档, 文案) 三元组一致可取。
//!   ② 冻结基表 + 扩展段：域内基表 8 条冻结不可改写，带宽族 10 条依序追加，
//!      注册表总容量恰为 18 条，超容显性拒（容量耗尽边界）。
//!   ③ 单调状态机：Init → BaseReady → BandwidthMerged → Sealed，只进不退，
//!      回退或跳段显性拒。
//!   ④ 持久化指纹：三表内容折叠为 64 位滚动指纹，落盘→恢复 ×10 轮指纹
//!      逐轮相等，恢复一致率 100%。
//!   ⑤ 损坏注入走默认安全档：严重度越档 / 码值越族 / 指纹损坏三类注入，
//!      条目级损坏逐条检出并回落 (SAFE_CODE, SAFE_SEV) 默认安全档，
//!      指纹级损坏整体拒恢复保旧表；×10 轮 10/10 全抓，逃逸恒 0。
//!   ⑥ 判定次序：相位单调 → 容量 → 条目合法性（负值/越族/重复/越档/文案）
//!      → 落表记账；注入账守恒 注入 = 检出 + 逃逸，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：0xC0880 + 0x40 × (51534 − 51534)。
pub const SEG_BASE: u32 = 0xC0880;

/// 域内基表条数（冻结）。
pub const BASE_CNT: usize = 8;
/// 带宽族扩展条数（冻结）。
pub const BANDWIDTH_CNT: usize = 10;
/// 注册表总容量：基表 + 带宽族恰好填满。
pub const MAX_ENTRIES: usize = BASE_CNT + BANDWIDTH_CNT;
/// 严重度档上限（含）。
pub const SEV_MAX: u8 = 3;
/// 默认安全档严重度。
pub const SAFE_SEV: u8 = 1;
/// 带宽族基码。
pub const BANDWIDTH_BASE: u32 = 0xB170;
/// 默认安全档码（带宽族首位）。
pub const SAFE_CODE: u32 = BANDWIDTH_BASE;
/// 默认安全档文案。
pub const SAFE_TEXT: &str = "默认安全档：带宽错误码回落告警";

/// 域内基表冻结码（0xB100..0xB107）。
pub const BASE_CODES: [u32; BASE_CNT] = [
    0xB100, 0xB101, 0xB102, 0xB103, 0xB104, 0xB105, 0xB106, 0xB107,
];

/// 带宽族 ×10 冻结码表。
pub const BANDWIDTH_CODES: [u32; BANDWIDTH_CNT] = [
    0xB170, 0xB171, 0xB172, 0xB173, 0xB174, 0xB175, 0xB176, 0xB177, 0xB178, 0xB179,
];

/// 冻结文案表：按码值取文案；越族码取不到文案即 TextMissing。
pub fn frozen_text(code: u32) -> Option<&'static str> {
    match code {
        0xB100 => Some("基表 0：总线级错误"),
        0xB101 => Some("基表 1：描述符非法"),
        0xB102 => Some("基表 2：容量越界"),
        0xB103 => Some("基表 3：队列满"),
        0xB104 => Some("基表 4：超时"),
        0xB105 => Some("基表 5：校验失败"),
        0xB106 => Some("基表 6：状态机拒止"),
        0xB107 => Some("基表 7：资源耗尽"),
        0xB170 => Some("带宽族 0：带宽仲裁拒绝"),
        0xB171 => Some("带宽族 1：QoS 档位越界"),
        0xB172 => Some("带宽族 2：令牌桶枯竭"),
        0xB173 => Some("带宽族 3：预算截断"),
        0xB174 => Some("带宽族 4：突发超限"),
        0xB175 => Some("带宽族 5：延迟上限触顶"),
        0xB176 => Some("带宽族 6：带宽账不平"),
        0xB177 => Some("带宽族 7：拓扑锚失配"),
        0xB178 => Some("带宽族 8：公平性越界"),
        0xB179 => Some("带宽族 9：令牌超发"),
        _ => None,
    }
}

/// 诊断码闭集（错误码扩展域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrExtCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 相位回退或跳段（状态机拒止）
    PhaseRegress,
    /// 码值越族（负值 / 未知码）
    CodeAlien,
    /// 码值重复并入
    CodeDup,
    /// 严重度越档
    SevOutOfRange,
    /// 文案缺失
    TextMissing,
    /// 注册表容量耗尽
    CapacityFull,
    /// 索引越出三表范围
    IdxOutOfRange,
    /// 持久化指纹不一致
    FpMismatch,
    /// 恢复载荷三表失配
    RestoreLenMismatch,
    /// 注入账守恒失败
    LedgerMismatch,
}

impl ErrExtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ErrExtCode::Ok => "成功",
            ErrExtCode::SegBad => "段位越界：核对 SEG_BASE 对齐与取值范围",
            ErrExtCode::PhaseRegress => "相位回退：注册表状态机只进不退",
            ErrExtCode::CodeAlien => "码值越族：只接受基表与带宽族冻结码",
            ErrExtCode::CodeDup => "码重复：同一错误码不得并入两次",
            ErrExtCode::SevOutOfRange => "严重度越档：档位必须不超上限",
            ErrExtCode::TextMissing => "文案缺失：码值必须映射到冻结文案表",
            ErrExtCode::CapacityFull => "容量耗尽：注册表 18 条已满",
            ErrExtCode::IdxOutOfRange => "索引越界：读取位置必须落在三表范围内",
            ErrExtCode::FpMismatch => "指纹不一致：持久化载荷被损坏，拒恢复保旧表",
            ErrExtCode::RestoreLenMismatch => "载荷失配：码值表与严重度表长度必须一致",
            ErrExtCode::LedgerMismatch => "注入账不平：注入=检出+逃逸必须守恒",
        }
    }
}

/// 注册表相位（单调状态机，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegPhase {
    /// 空注册表
    Init = 0,
    /// 基表 8 条落地
    BaseReady = 1,
    /// 带宽族 10 条并入完成
    BandwidthMerged = 2,
    /// 封版：三表冻结
    Sealed = 3,
}

/// 持久化载荷：码值表 + 严重度档表 + 64 位滚动指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistBlob {
    pub codes: Vec<u32>,
    pub sevs: Vec<u8>,
    pub fp: u64,
}

/// 恢复报告：恢复条目数与回落默认安全档条目数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestoreReport {
    pub entries: usize,
    pub safe_fallback: usize,
}

/// 滚动指纹折叠：码值与档位依序折入 64 位状态。
fn fold_entries(codes: &[u32], sevs: &[u8]) -> u64 {
    let mut fp: u64 = 0x243F_6A88_85A3_08D3;
    for i in 0..codes.len() {
        let c = match codes.get(i) {
            Some(v) => *v as u64,
            None => 0,
        };
        let s = match sevs.get(i) {
            Some(v) => *v as u64,
            None => 0,
        };
        fp = fp.rotate_left(7) ^ c.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (s << 52);
    }
    fp
}

/// 错误码注册表：三表并行 + 相位机 + 注入账。
pub struct ErrRegistry {
    codes: Vec<u32>,
    sevs: Vec<u8>,
    texts: Vec<&'static str>,
    phase: RegPhase,
    injected: u64,
    detected: u64,
    escaped: u64,
    restore_rounds: u32,
    restore_ok_rounds: u32,
}

impl ErrRegistry {
    /// 新注册表：空表、Init 相位。
    pub fn new() -> ErrRegistry {
        ErrRegistry {
            codes: Vec::new(),
            sevs: Vec::new(),
            texts: Vec::new(),
            phase: RegPhase::Init,
            injected: 0,
            detected: 0,
            escaped: 0,
            restore_rounds: 0,
            restore_ok_rounds: 0,
        }
    }

    /// 码值是否属于已知闭集（基表 ∪ 带宽族）。
    pub fn known(code: u32) -> bool {
        BASE_CODES.contains(&code) || BANDWIDTH_CODES.contains(&code)
    }

    /// 单调推进相位：只允许逐级 +1，回退/跳段显性拒。
    fn step_phase(&mut self, target: RegPhase) -> Result<RegPhase, ErrExtCode> {
        let cur = self.phase as u32;
        let tgt = target as u32;
        if tgt <= cur || tgt > cur + 1 {
            return Err(ErrExtCode::PhaseRegress);
        }
        self.phase = target;
        Ok(self.phase)
    }

    /// 外部相位推进入口（状态机拒止测试面）。
    pub fn advance_phase(&mut self, target: RegPhase) -> Result<RegPhase, ErrExtCode> {
        self.step_phase(target)
    }

    fn append_entry(&mut self, code: u32, sev: u8) {
        let text = match frozen_text(code) {
            Some(t) => t,
            None => SAFE_TEXT,
        };
        self.codes.push(code);
        self.sevs.push(sev);
        self.texts.push(text);
    }

    /// 条目合法性判定（判序：负值/越族 → 重复 → 越档 → 文案）。
    fn validate_entry(&self, staged: &[(u32, u8)], code_raw: i64, sev: u8) -> Result<u32, ErrExtCode> {
        if code_raw < 0 {
            return Err(ErrExtCode::CodeAlien);
        }
        let code = code_raw as u32;
        if !Self::known(code) {
            return Err(ErrExtCode::CodeAlien);
        }
        if self.codes.contains(&code) || staged.iter().any(|s| s.0 == code) {
            return Err(ErrExtCode::CodeDup);
        }
        if sev > SEV_MAX {
            return Err(ErrExtCode::SevOutOfRange);
        }
        match frozen_text(code) {
            Some(_) => Ok(code),
            None => Err(ErrExtCode::TextMissing),
        }
    }

    /// 基表落地（仅 Init 相位一次）。
    pub fn load_base(&mut self) -> Result<usize, ErrExtCode> {
        if self.phase != RegPhase::Init {
            return Err(ErrExtCode::PhaseRegress);
        }
        for i in 0..BASE_CNT {
            let code = match BASE_CODES.get(i) {
                Some(c) => *c,
                None => return Err(ErrExtCode::CodeAlien),
            };
            self.append_entry(code, 2);
        }
        self.step_phase(RegPhase::BaseReady)?;
        Ok(BASE_CNT)
    }

    /// 带宽族批量并入：全批合法才落表（禁部分并入），落表后相位推进。
    pub fn merge_bandwidth(&mut self, entries: &[(i64, u8)]) -> Result<usize, ErrExtCode> {
        if self.phase != RegPhase::BaseReady {
            return Err(ErrExtCode::PhaseRegress);
        }
        let total = match self.codes.len().checked_add(entries.len()) {
            Some(n) => n,
            None => return Err(ErrExtCode::CapacityFull),
        };
        if total > MAX_ENTRIES {
            return Err(ErrExtCode::CapacityFull);
        }
        let mut staged: Vec<(u32, u8)> = Vec::new();
        for e in entries {
            let code = self.validate_entry(&staged, e.0, e.1)?;
            staged.push((code, e.1));
        }
        for s in staged.iter() {
            self.append_entry(s.0, s.1);
        }
        self.step_phase(RegPhase::BandwidthMerged)?;
        Ok(staged.len())
    }

    /// 封版：三表冻结，此后任何并入/落地都被相位机拒止。
    pub fn seal(&mut self) -> Result<RegPhase, ErrExtCode> {
        if self.phase != RegPhase::BandwidthMerged {
            return Err(ErrExtCode::PhaseRegress);
        }
        if self.codes.len() != MAX_ENTRIES {
            return Err(ErrExtCode::CapacityFull);
        }
        self.step_phase(RegPhase::Sealed)
    }

    /// 三表同索引读取：任一表缺失即 IdxOutOfRange。
    pub fn entry_at(&self, idx: usize) -> Result<(u32, u8, &'static str), ErrExtCode> {
        let c = match self.codes.get(idx) {
            Some(v) => *v,
            None => return Err(ErrExtCode::IdxOutOfRange),
        };
        let s = match self.sevs.get(idx) {
            Some(v) => *v,
            None => return Err(ErrExtCode::IdxOutOfRange),
        };
        let t = match self.texts.get(idx) {
            Some(v) => *v,
            None => return Err(ErrExtCode::IdxOutOfRange),
        };
        Ok((c, s, t))
    }

    /// 当前指纹。
    pub fn fingerprint(&self) -> u64 {
        fold_entries(&self.codes, &self.sevs)
    }

    /// 落盘：导出持久化载荷。
    pub fn persist(&self) -> PersistBlob {
        PersistBlob {
            codes: self.codes.clone(),
            sevs: self.sevs.clone(),
            fp: self.fingerprint(),
        }
    }

    /// 恢复：指纹先行，条目级损坏回落默认安全档，指纹级损坏整体拒。
    pub fn restore(&mut self, blob: &PersistBlob) -> Result<RestoreReport, ErrExtCode> {
        if blob.codes.len() != blob.sevs.len() {
            return Err(ErrExtCode::RestoreLenMismatch);
        }
        if fold_entries(&blob.codes, &blob.sevs) != blob.fp {
            return Err(ErrExtCode::FpMismatch);
        }
        let mut codes: Vec<u32> = Vec::new();
        let mut sevs: Vec<u8> = Vec::new();
        let mut texts: Vec<&'static str> = Vec::new();
        let mut fallback = 0usize;
        for i in 0..blob.codes.len() {
            let c = match blob.codes.get(i) {
                Some(v) => *v,
                None => SAFE_CODE,
            };
            let s = match blob.sevs.get(i) {
                Some(v) => *v,
                None => SAFE_SEV,
            };
            if Self::known(c) && s <= SEV_MAX && frozen_text(c).is_some() {
                let t = match frozen_text(c) {
                    Some(t) => t,
                    None => SAFE_TEXT,
                };
                codes.push(c);
                sevs.push(s);
                texts.push(t);
            } else {
                // 损坏条目：整体回落默认安全档（码、档、文案三表同步回落）。
                codes.push(SAFE_CODE);
                sevs.push(SAFE_SEV);
                texts.push(SAFE_TEXT);
                fallback += 1;
            }
        }
        self.codes = codes;
        self.sevs = sevs;
        self.texts = texts;
        self.restore_rounds += 1;
        if fallback == 0 {
            self.restore_ok_rounds += 1;
        }
        Ok(RestoreReport {
            entries: self.codes.len(),
            safe_fallback: fallback,
        })
    }

    pub fn phase(&self) -> RegPhase {
        self.phase
    }

    pub fn len(&self) -> usize {
        self.codes.len()
    }

    pub fn restore_rounds(&self) -> u32 {
        self.restore_rounds
    }

    pub fn restore_ok_rounds(&self) -> u32 {
        self.restore_ok_rounds
    }

    /// 注入记账：注入一笔。
    pub fn note_injected(&mut self) {
        self.injected += 1;
    }

    /// 注入记账：检出（默认安全档接住）。
    pub fn note_detected(&mut self) {
        self.detected += 1;
    }

    /// 注入记账：逃逸（判据要求恒 0）。
    pub fn note_escaped(&mut self) {
        self.escaped += 1;
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 注入账守恒：注入 = 检出 + 逃逸，且逃逸恒 0、检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 损坏注入器 ①：严重度越档（写入 SEV_MAX+1）。
pub fn inject_sev_corrupt(blob: &mut PersistBlob, idx: usize) -> bool {
    match blob.sevs.get_mut(idx) {
        Some(s) => {
            *s = SEV_MAX + 1;
            // 条目级损坏：载荷指纹同步重算，使恢复判序落到条目校验（回落安全档）。
            blob.fp = fold_entries(&blob.codes, &blob.sevs);
            true
        }
        None => false,
    }
}

/// 损坏注入器 ②：码值越族（写入未知码，文案随之失联）。
pub fn inject_code_corrupt(blob: &mut PersistBlob, idx: usize) -> bool {
    match blob.codes.get_mut(idx) {
        Some(c) => {
            *c = 0xDEAD_BEEF;
            // 同上：条目级损坏保留有效指纹，恢复时逐条目回落。
            blob.fp = fold_entries(&blob.codes, &blob.sevs);
            true
        }
        None => false,
    }
}

/// 损坏注入器 ③：指纹损坏（整体载荷被改写）。
pub fn inject_fp_corrupt(blob: &mut PersistBlob) -> bool {
    blob.fp ^= 0xFFFF_FFFF;
    true
}

/// 便捷构造：基表落地 + 带宽族全量并入 + 封版。
pub fn fresh_sealed() -> ErrRegistry {
    let mut e = ErrRegistry::new();
    let _ = e.load_base();
    let batch = bandwidth_batch(SEV_MAX);
    let _ = e.merge_bandwidth(&batch);
    let _ = e.seal();
    e
}

/// 带宽族全量批次（给定档位）。
pub fn bandwidth_batch(sev: u8) -> Vec<(i64, u8)> {
    let mut v: Vec<(i64, u8)> = Vec::new();
    for i in 0..BANDWIDTH_CNT {
        let c = match BANDWIDTH_CODES.get(i) {
            Some(c) => *c,
            None => 0,
        };
        v.push((c as i64, sev));
    }
    v
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一构造序列两次建表，载荷与指纹逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (PersistBlob, u64) {
        let e = fresh_sealed();
        let blob = e.persist();
        let fp = e.fingerprint();
        (blob, fp)
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51534_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51534");
    // 1 正常流：基表 + 带宽族 ×10 并入 + 封版，三表同长且三元组一致。
    {
        let e = fresh_sealed();
        let mut aligned = e.len() == MAX_ENTRIES && e.phase() == RegPhase::Sealed;
        for i in 0..MAX_ENTRIES {
            if let Ok((c, _sev, t)) = e.entry_at(i) {
                match frozen_text(c) {
                    Some(ft) => {
                        if ft != t {
                            aligned = false;
                        }
                    }
                    None => aligned = false,
                }
            } else {
                aligned = false;
            }
        }
        s.add_dyn("正常流三表对齐", aligned, "18 条三元组逐条与冻结文案一致");
    }
    // 2 边界-零：空扩展批次合法放行；索引 0 可读。
    {
        let mut e = ErrRegistry::new();
        let _ = e.load_base();
        let zero_batch = e.merge_bandwidth(&[]).is_ok();
        let idx0 = e.entry_at(0).is_ok();
        s.add_dyn("边界零", zero_batch && idx0, "空批次合法、索引 0 可读");
    }
    // 3 边界-负：负值码显性拒。
    {
        let mut e = ErrRegistry::new();
        let _ = e.load_base();
        let r = e.merge_bandwidth(&[(-1i64, 1u8)]);
        s.add_dyn("边界负值拒", r == Err(ErrExtCode::CodeAlien), "负值码必须显性拒");
    }
    // 4 边界-上限：严重度取 SEV_MAX 全批并入成功。
    {
        let e = fresh_sealed();
        s.add_dyn("边界上限放行", e.len() == MAX_ENTRIES, "上限档位并入成功");
    }
    // 5 边界-上限+1：档位越档拒；读索引越界拒。
    {
        let mut e = ErrRegistry::new();
        let _ = e.load_base();
        let r = e.merge_bandwidth(&[(0xB170i64, SEV_MAX + 1)]);
        let idx_bad = e.entry_at(MAX_ENTRIES).is_err();
        s.add_dyn(
            "边界上限+1 拒",
            r == Err(ErrExtCode::SevOutOfRange) && idx_bad,
            "越档与越界索引都必须拒绝",
        );
    }
    // 6 注入必抓 ①：严重度越档 ×10 走默认安全档。
    {
        let mut e = fresh_sealed();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut blob = e.persist();
            let idx = BASE_CNT + (i as usize) % BANDWIDTH_CNT;
            let _ = inject_sev_corrupt(&mut blob, idx);
            e.note_injected();
            match e.restore(&blob) {
                Ok(rep) => {
                    if rep.safe_fallback == 1 {
                        caught += 1;
                        e.note_detected();
                    } else {
                        e.note_escaped();
                    }
                }
                Err(_) => e.note_escaped(),
            }
        }
        s.add_dyn("越档注入 10/10 安全档", caught == 10, "损坏条目必须逐条回落安全档");
    }
    // 7 注入必抓 ②：码值越族 ×10 走默认安全档。
    {
        let mut e = fresh_sealed();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut blob = e.persist();
            let idx = BASE_CNT + (i as usize) % BANDWIDTH_CNT;
            let _ = inject_code_corrupt(&mut blob, idx);
            e.note_injected();
            match e.restore(&blob) {
                Ok(rep) => {
                    if rep.safe_fallback == 1 {
                        caught += 1;
                        e.note_detected();
                    } else {
                        e.note_escaped();
                    }
                }
                Err(_) => e.note_escaped(),
            }
        }
        s.add_dyn("越族注入 10/10 安全档", caught == 10, "越族码必须回落安全档");
    }
    // 8 注入必抓 ③：指纹损坏 ×10 整体拒恢复保旧表。
    {
        let mut e = fresh_sealed();
        let fp_before = e.fingerprint();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut blob = e.persist();
            let _ = inject_fp_corrupt(&mut blob);
            e.note_injected();
            match e.restore(&blob) {
                Err(ErrExtCode::FpMismatch) => {
                    caught += 1;
                    e.note_detected();
                }
                _ => e.note_escaped(),
            }
        }
        s.add_dyn(
            "指纹注入 10/10 拒恢复",
            caught == 10 && e.fingerprint() == fp_before,
            "指纹损坏必须整体拒且旧表不变",
        );
    }
    // 9 判定次序：容量判据先于条目合法性。
    {
        let mut e = ErrRegistry::new();
        let _ = e.load_base();
        let mut batch: Vec<(i64, u8)> = Vec::new();
        batch.push((-5i64, 1u8));
        for i in 0..BANDWIDTH_CNT {
            let c = match BANDWIDTH_CODES.get(i) {
                Some(c) => *c,
                None => 0,
            };
            batch.push((c as i64, 2));
        }
        batch.push((0xB170i64, 2));
        let r = e.merge_bandwidth(&batch);
        s.add_dyn(
            "判序容量先于条目",
            r == Err(ErrExtCode::CapacityFull),
            "8+11>18：容量判据先于负值判据",
        );
    }
    // 10 状态机拒止：跳段 / 回退 / 重复落地全部显性拒。
    {
        let mut e = ErrRegistry::new();
        let r1 = e.advance_phase(RegPhase::Sealed);
        let _ = e.load_base();
        let r2 = e.advance_phase(RegPhase::Init);
        let r3 = e.load_base();
        s.add_dyn(
            "状态机拒止",
            r1.is_err() && r2 == Err(ErrExtCode::PhaseRegress) && r3.is_err(),
            "跳段/回退/重复推进必须全部拒绝",
        );
    }
    // 11 容量耗尽：封版后再并入被相位拒；条数不足封版被容量拒。
    {
        let mut e = fresh_sealed();
        let r1 = e.merge_bandwidth(&[(0xB170i64, 2)]);
        let mut e2 = ErrRegistry::new();
        let _ = e2.load_base();
        let r2 = e2.seal();
        s.add_dyn(
            "容量耗尽拒",
            r1 == Err(ErrExtCode::PhaseRegress) && r2 == Err(ErrExtCode::CapacityFull),
            "封版拒并入、欠条拒封版",
        );
    }
    // 12 持久化 ×10 恢复一致率 100%。
    {
        let mut e = fresh_sealed();
        for _ in 0..10u32 {
            let blob = e.persist();
            let _ = e.restore(&blob);
        }
        s.add_dyn(
            "恢复一致率 100%",
            e.restore_rounds() == 10 && e.restore_ok_rounds() == 10,
            "十轮落盘恢复指纹逐轮相等",
        );
    }
    // 13 对账守恒：注入 = 检出 + 逃逸，逃逸 0。
    {
        let mut e = fresh_sealed();
        for _ in 0..10u32 {
            let mut blob = e.persist();
            let _ = inject_fp_corrupt(&mut blob);
            e.note_injected();
            match e.restore(&blob) {
                Err(ErrExtCode::FpMismatch) => e.note_detected(),
                _ => e.note_escaped(),
            }
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 14 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一构造序列两次建表全等");
    // 15 文案闭集非空（诊断码 + 三表冻结文案）。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        for i in 0..BASE_CNT {
            let ok = match BASE_CODES.get(i) {
                Some(c) => frozen_text(*c).is_some(),
                None => false,
            };
            if !ok {
                closed = false;
            }
        }
        for i in 0..BANDWIDTH_CNT {
            let ok = match BANDWIDTH_CODES.get(i) {
                Some(c) => frozen_text(*c).is_some(),
                None => false,
            };
            if !ok {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "诊断码与冻结文案全部非空");
    }
    // 16 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [ErrExtCode; 12] = [
    ErrExtCode::Ok,
    ErrExtCode::SegBad,
    ErrExtCode::PhaseRegress,
    ErrExtCode::CodeAlien,
    ErrExtCode::CodeDup,
    ErrExtCode::SevOutOfRange,
    ErrExtCode::TextMissing,
    ErrExtCode::CapacityFull,
    ErrExtCode::IdxOutOfRange,
    ErrExtCode::FpMismatch,
    ErrExtCode::RestoreLenMismatch,
    ErrExtCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_seal_tables_aligned() {
        let e = fresh_sealed();
        assert_eq!(e.len(), MAX_ENTRIES);
        assert_eq!(e.phase(), RegPhase::Sealed);
        let triple = e.entry_at(BASE_CNT);
        assert_eq!(triple, Ok((0xB170, SEV_MAX, "带宽族 0：带宽仲裁拒绝")));
    }

    #[test]
    fn negative_and_alien_codes_rejected() {
        let mut e = ErrRegistry::new();
        let _ = e.load_base();
        assert_eq!(e.merge_bandwidth(&[(-7i64, 1u8)]), Err(ErrExtCode::CodeAlien));
        assert_eq!(e.merge_bandwidth(&[(0xB999i64, 1u8)]), Err(ErrExtCode::CodeAlien));
        assert_eq!(e.merge_bandwidth(&[(0xB100i64, 1u8)]), Err(ErrExtCode::CodeDup));
        assert_eq!(e.merge_bandwidth(&[(0xB170i64, SEV_MAX + 1)]), Err(ErrExtCode::SevOutOfRange));
    }

    #[test]
    fn sev_bounds_max_ok_plus_one_rejected() {
        let e = fresh_sealed();
        assert_eq!(e.len(), MAX_ENTRIES);
        let mut e2 = ErrRegistry::new();
        let _ = e2.load_base();
        assert_eq!(
            e2.merge_bandwidth(&[(0xB177i64, SEV_MAX + 1)]),
            Err(ErrExtCode::SevOutOfRange)
        );
    }

    #[test]
    fn injection_sev_caught_ten_of_ten() {
        let mut e = fresh_sealed();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut blob = e.persist();
            let idx = BASE_CNT + (i as usize) % BANDWIDTH_CNT;
            assert!(inject_sev_corrupt(&mut blob, idx));
            e.note_injected();
            match e.restore(&blob) {
                Ok(rep) => {
                    if rep.safe_fallback == 1 {
                        caught += 1;
                        e.note_detected();
                    } else {
                        e.note_escaped();
                    }
                }
                Err(_) => e.note_escaped(),
            }
        }
        assert_eq!(caught, 10);
        assert!(e.conserved());
    }

    #[test]
    fn injection_fp_caught_keeps_old_table() {
        let mut e = fresh_sealed();
        let fp = e.fingerprint();
        let mut blob = e.persist();
        assert!(inject_fp_corrupt(&mut blob));
        e.note_injected();
        assert_eq!(e.restore(&blob), Err(ErrExtCode::FpMismatch));
        e.note_detected();
        assert_eq!(e.fingerprint(), fp);
        assert!(e.conserved());
    }

    #[test]
    fn judge_order_capacity_before_entry() {
        let mut e = ErrRegistry::new();
        let _ = e.load_base();
        let mut batch: Vec<(i64, u8)> = Vec::new();
        batch.push((-5i64, 1u8));
        for i in 0..BANDWIDTH_CNT {
            let c = match BANDWIDTH_CODES.get(i) {
                Some(c) => *c,
                None => 0,
            };
            batch.push((c as i64, 2));
        }
        batch.push((0xB170i64, 2));
        assert_eq!(e.merge_bandwidth(&batch), Err(ErrExtCode::CapacityFull));
    }

    #[test]
    fn phase_machine_rejects_regress_and_skip() {
        let mut e = ErrRegistry::new();
        assert!(e.advance_phase(RegPhase::Sealed).is_err());
        let _ = e.load_base();
        assert_eq!(e.advance_phase(RegPhase::Init), Err(ErrExtCode::PhaseRegress));
        assert!(e.load_base().is_err());
    }

    #[test]
    fn restore_ten_rounds_identical() {
        let mut e = fresh_sealed();
        for _ in 0..10u32 {
            let blob = e.persist();
            let rep = match e.restore(&blob) {
                Ok(r) => r,
                Err(_) => {
                    assert!(false, "恢复不应失败");
                    return;
                }
            };
            assert_eq!(rep.safe_fallback, 0);
            assert_eq!(e.fingerprint(), blob.fp);
        }
        assert_eq!(e.restore_rounds(), 10);
        assert_eq!(e.restore_ok_rounds(), 10);
    }

    #[test]
    fn capacity_full_and_index_out() {
        let mut e = fresh_sealed();
        assert_eq!(e.merge_bandwidth(&[(0xB170i64, 2)]), Err(ErrExtCode::PhaseRegress));
        assert!(e.entry_at(MAX_ENTRIES).is_err());
        let mut e2 = ErrRegistry::new();
        let _ = e2.load_base();
        // 满表上限：并入超容量条目 → CapacityFull（判序：相位闸先于容量闸）。
        let overflow: Vec<(i64, u8)> = (0..MAX_ENTRIES).map(|_| (0xB170i64, 2u8)).collect();
        assert_eq!(e2.merge_bandwidth(&overflow), Err(ErrExtCode::CapacityFull));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
        assert!(frozen_text(SAFE_CODE).is_some());
        assert!(frozen_text(0xDEAD_BEEF).is_none());
    }
}
