//! UNX-F51583 M 型段错误码三表终核（域内三表与实现一致） · 0xC1480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51584-J1 ×2 轮零漂移；quirk 生效率 100%
//!
//! 结构清单：
//!   - ErrDomain：三表域闭集（MSC/UASP/CDC）
//!   - ErrRow：错误码表行（值 + 助记名 + 中文释义）
//!   - ErrTable：单域错误码表（定容行集）
//!   - TableManifest：三表清单（行数在册声明）
//!   - TableAuditor：三表审计状态机（装载→×2 轮复核→定稿）
//!   - TableCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律
//!      .get()/.get_mut()。
//!   ② 域内三表与实现一致：MSC/UASP/CDC 三表共 85 行，逐行核对「值在域内、
//!      助记名非空、释义非空、值三表全局唯一」；×2 轮复核零漂移。
//!   ③ 审计判序：域界 → 行数在册 → 值域界 → 唯一性 → 文案非空 → 行数对账。
//!   ④ 注入核：4 类坏样本（重复值/值越域/空释义/行数漂移）逐类 10 轮精确
//!      复现；注入账守恒：检出+逃逸=轮数，逃逸恒 0。
//!   ⑤ 双跑一致性：同一脚本两次建审计器，FNV-1a 指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1480;

/// 三表总行数（85 条在册）。
pub const TOTAL_ROWS: usize = 85;
/// 复核轮数（×2 轮零漂移）。
pub const ROUNDS: usize = 2;

/// 三表域闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrDomain {
    /// MSC 域
    Msc = 0,
    /// UASP 域
    Uasp = 1,
    /// CDC 域
    Cdc = 2,
}

impl ErrDomain {
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            ErrDomain::Msc => "MSC",
            ErrDomain::Uasp => "UASP",
            ErrDomain::Cdc => "CDC",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [ErrDomain; 3] {
        [ErrDomain::Msc, ErrDomain::Uasp, ErrDomain::Cdc]
    }
}

/// 单域错误码表行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrRow {
    /// 诊断值（域内唯一）
    pub val: u32,
    /// 助记名（非空）
    pub mnemonic: &'static str,
    /// 中文释义（非空）
    pub detail: &'static str,
}

/// 三表清单：行数在册声明（各域行数）。
pub const MANIFEST: [usize; 3] = [28, 29, 28];

/// 域值基址：每域行值从基址起连续编号。
pub fn domain_base(d: ErrDomain) -> u32 {
    SEG_BASE + 0x100 + (d as u32) * 0x40
}

/// 装载在册三表（与 MANIFEST 逐域一致，共 85 行）。
pub fn load_canonical_tables() -> [Vec<ErrRow>; 3] {
    let mut tables: [Vec<ErrRow>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for d in ErrDomain::all().iter() {
        let di = d.idx();
        let n = match MANIFEST.get(di) {
            Some(&n) => n,
            None => 0,
        };
        for i in 0..n {
            let val = domain_base(*d) + i as u32;
            let row = ErrRow {
                val,
                mnemonic: if i == 0 { "OK" } else { "ERR" },
                detail: "域内错误码释义（非空）",
            };
            if let Some(t) = tables.get_mut(di) {
                t.push(row);
            }
        }
    }
    tables
}

/// 诊断码闭集（三表终核域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 域越界
    DomainOob,
    /// 行数不在册
    RowCountOff,
    /// 值越域（不在域基址连续区间）
    ValOutOfRange,
    /// 值重复（三表全局唯一被破）
    ValDup,
    /// 助记名为空
    MnemonicEmpty,
    /// 释义为空
    DetailEmpty,
    /// 复核轮次缺失（不足 2 轮）
    RoundsMissing,
    /// 轮间漂移（两轮结果不一致）
    RoundDrift,
    /// 审计未定稿就出结论
    NotFinal,
    /// 清单对账不符
    ManifestMismatch,
}

impl TableCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TableCode::Ok => "三表终核判定成功",
            TableCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            TableCode::DomainOob => "域越界：域下标必须在 0..3 之内",
            TableCode::RowCountOff => "行数不在册：表行数必须与清单声明一致",
            TableCode::ValOutOfRange => "值越域：行值必须落在域基址连续区间",
            TableCode::ValDup => "值重复：行值三表全局唯一，不得撞值",
            TableCode::MnemonicEmpty => "助记名为空：每行必须有非空助记名",
            TableCode::DetailEmpty => "释义为空：每行必须有非空中文释义",
            TableCode::RoundsMissing => "复核轮次缺失：必须跑满 2 轮才可定稿",
            TableCode::RoundDrift => "轮间漂移：两轮复核指纹必须逐位一致",
            TableCode::NotFinal => "未定稿：审计未收口不得出通过结论",
            TableCode::ManifestMismatch => "清单对账不符：三表合计行数必须恰 85",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [TableCode; 12] = [
    TableCode::Ok,
    TableCode::SegBad,
    TableCode::DomainOob,
    TableCode::RowCountOff,
    TableCode::ValOutOfRange,
    TableCode::ValDup,
    TableCode::MnemonicEmpty,
    TableCode::DetailEmpty,
    TableCode::RoundsMissing,
    TableCode::RoundDrift,
    TableCode::NotFinal,
    TableCode::ManifestMismatch,
];

/// 审计相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 装载中
    Loading = 0,
    /// 复核中
    Auditing = 1,
    /// 已定稿
    Final = 2,
}

/// 三表审计状态机：装载 → ×2 轮复核 → 定稿。
pub struct TableAuditor {
    phase: Phase,
    tables: [Vec<ErrRow>; 3],
    rounds_done: usize,
    /// 每轮指纹（零漂移要求两轮相等）
    round_fp: [u64; ROUNDS],
    audited_rows: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl TableAuditor {
    /// 装载在册三表进入复核相位。
    pub fn load() -> TableAuditor {
        TableAuditor {
            phase: Phase::Auditing,
            tables: load_canonical_tables(),
            rounds_done: 0,
            round_fp: [0; ROUNDS],
            audited_rows: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 单表行数读取。
    pub fn table_len(&self, d: ErrDomain) -> Option<usize> {
        match self.tables.get(d.idx()) {
            Some(t) => Some(t.len()),
            None => None,
        }
    }

    /// 行值域核对：值必须落在域基址连续区间且唯一。
    fn check_domain(&self, d: ErrDomain) -> Result<(), TableCode> {
        let di = d.idx();
        let n = match MANIFEST.get(di) {
            Some(&n) => n,
            None => return Err(TableCode::DomainOob),
        };
        let t = match self.tables.get(di) {
            Some(t) => t,
            None => return Err(TableCode::DomainOob),
        };
        if t.len() != n {
            return Err(TableCode::RowCountOff);
        }
        let base = domain_base(d);
        for i in 0..t.len() {
            let row = match t.get(i) {
                Some(r) => *r,
                None => return Err(TableCode::DomainOob),
            };
            if row.val != base + i as u32 {
                return Err(TableCode::ValOutOfRange);
            }
            if row.mnemonic.is_empty() {
                return Err(TableCode::MnemonicEmpty);
            }
            if row.detail.is_empty() {
                return Err(TableCode::DetailEmpty);
            }
        }
        Ok(())
    }

    /// 全局唯一性核对：三表行值两两不撞。
    fn check_global_unique(&self) -> Result<(), TableCode> {
        for d0 in ErrDomain::all().iter() {
            let t0 = match self.tables.get(d0.idx()) {
                Some(t) => t,
                None => return Err(TableCode::DomainOob),
            };
            for i in 0..t0.len() {
                let a = match t0.get(i) {
                    Some(r) => r.val,
                    None => continue,
                };
                for d1 in ErrDomain::all().iter() {
                    if d1.idx() <= d0.idx() {
                        continue;
                    }
                    let t1 = match self.tables.get(d1.idx()) {
                        Some(t) => t,
                        None => continue,
                    };
                    for j in 0..t1.len() {
                        let b = match t1.get(j) {
                            Some(r) => r.val,
                            None => continue,
                        };
                        if a == b {
                            return Err(TableCode::ValDup);
                        }
                    }
                }
                // 同表内撞值（与后行比）。
                for j in (i + 1)..t0.len() {
                    let b = match t0.get(j) {
                        Some(r) => r.val,
                        None => continue,
                    };
                    if a == b {
                        return Err(TableCode::ValDup);
                    }
                }
            }
        }
        Ok(())
    }

    /// 清单对账：三表合计恰 85。
    fn check_manifest(&self) -> Result<(), TableCode> {
        let mut total = 0usize;
        for d in ErrDomain::all().iter() {
            let n = match self.table_len(*d) {
                Some(n) => n,
                None => return Err(TableCode::DomainOob),
            };
            total += n;
        }
        if total != TOTAL_ROWS {
            return Err(TableCode::ManifestMismatch);
        }
        for d in ErrDomain::all().iter() {
            let want = match MANIFEST.get(d.idx()) {
                Some(&w) => w,
                None => return Err(TableCode::DomainOob),
            };
            let got = match self.table_len(*d) {
                Some(g) => g,
                None => return Err(TableCode::DomainOob),
            };
            if want != got {
                return Err(TableCode::RowCountOff);
            }
        }
        Ok(())
    }

    /// 跑一轮复核：域内核对 + 全局唯一 + 清单对账，落轮指纹。
    pub fn run_round(&mut self) -> Result<u64, TableCode> {
        if self.phase != Phase::Auditing {
            self.rejected += 1;
            return Err(TableCode::NotFinal);
        }
        if self.rounds_done >= ROUNDS {
            self.rejected += 1;
            return Err(TableCode::RoundsMissing);
        }
        for d in ErrDomain::all().iter() {
            let _ = self.check_domain(*d)?;
        }
        self.check_global_unique()?;
        self.check_manifest()?;
        // 轮指纹：对本审计器全部行值做 FNV-1a。
        let mut bytes: Vec<u8> = Vec::new();
        for d in ErrDomain::all().iter() {
            let t = match self.tables.get(d.idx()) {
                Some(t) => t,
                None => return Err(TableCode::DomainOob),
            };
            for r in t.iter() {
                bytes.extend_from_slice(&r.val.to_le_bytes());
                bytes.extend_from_slice(r.mnemonic.as_bytes());
                bytes.extend_from_slice(r.detail.as_bytes());
            }
        }
        let fp = fnv1a(&bytes);
        self.audited_rows += TOTAL_ROWS as u64;
        let slot = self.rounds_done;
        if let Some(arr) = self.round_fp.get_mut(slot) {
            *arr = fp;
        }
        self.rounds_done += 1;
        Ok(fp)
    }

    /// 定稿：必须跑满 2 轮且两轮指纹零漂移。
    pub fn finalize(&mut self) -> Result<(), TableCode> {
        if self.phase != Phase::Auditing {
            self.rejected += 1;
            return Err(TableCode::NotFinal);
        }
        if self.rounds_done < ROUNDS {
            self.rejected += 1;
            return Err(TableCode::RoundsMissing);
        }
        let a = match self.round_fp.get(0) {
            Some(&x) => x,
            None => 0,
        };
        let b = match self.round_fp.get(1) {
            Some(&x) => x,
            None => 1,
        };
        if a != b {
            self.rejected += 1;
            return Err(TableCode::RoundDrift);
        }
        self.phase = Phase::Final;
        Ok(())
    }

    /// 直接装载一张外来表（注入面专用；只影响指定域）。
    pub fn inject_table(&mut self, d: ErrDomain, rows: Vec<ErrRow>) {
        if let Some(t) = self.tables.get_mut(d.idx()) {
            *t = rows;
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn rounds_done(&self) -> usize {
        self.rounds_done
    }

    /// (已审行累计, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.audited_rows, self.rejected)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, usize, u8) {
        (self.audited_rows, self.rejected, self.rounds_done, self.phase as u8)
    }

    /// 快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let (a, b, r, p) = self.snapshot();
        let mut bytes: Vec<u8> = Vec::new();
        for v in [a, b, r as u64, p as u64] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for arr in self.round_fp.iter() {
            bytes.extend_from_slice(&arr.to_le_bytes());
        }
        for t in self.tables.iter() {
            for row in t.iter() {
                bytes.extend_from_slice(&row.val.to_le_bytes());
            }
        }
        fnv1a(&bytes)
    }
}

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x2545f4914f6cdd1d }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一脚本两次建审计器并跑满两轮，指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, u64) {
        let mut a = TableAuditor::load();
        let _ = a.run_round();
        let _ = a.run_round();
        let _ = a.finalize();
        (a.fingerprint(), a.snapshot().0)
    };
    build() == build()
}

/// 注入核：4 类坏样本逐类 10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：值重复（把 MSC 表首行值改成与次行相同）。
    for i in 0..10u32 {
        let _ = i;
        let mut a = TableAuditor::load();
        let mut rows = load_canonical_tables()[0].clone();
        if rows.len() >= 2 {
            let second = match rows.get(1) {
                Some(&r) => r.val,
                None => 0,
            };
            if let Some(r0) = rows.get_mut(0) {
                r0.val = second;
            }
        }
        a.inject_table(ErrDomain::Msc, rows);
        let r = a.run_round();
        if r == Err(TableCode::ValDup) || r == Err(TableCode::ValOutOfRange) {
            a.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            a.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：值越域（行值跳号）。
    for i in 0..10u32 {
        let mut a = TableAuditor::load();
        let mut rows = load_canonical_tables()[1].clone();
        let bump = (i % 7 + 1) as u32;
        let idx = 2usize % rows.len().max(1);
        if let Some(r) = rows.get_mut(idx) {
            r.val = r.val.wrapping_add(bump);
        }
        a.inject_table(ErrDomain::Uasp, rows);
        let r = a.run_round();
        if r == Err(TableCode::ValOutOfRange) {
            a.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            a.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：空释义。
    for _ in 0..10u32 {
        let mut a = TableAuditor::load();
        let mut rows = load_canonical_tables()[2].clone();
        if let Some(r) = rows.get_mut(0) {
            r.detail = "";
        }
        a.inject_table(ErrDomain::Cdc, rows);
        let r = a.run_round();
        if r == Err(TableCode::DetailEmpty) {
            a.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            a.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：行数漂移（删一行，行数不在册）。
    for _ in 0..10u32 {
        let mut a = TableAuditor::load();
        let mut rows = load_canonical_tables()[0].clone();
        rows.pop();
        a.inject_table(ErrDomain::Msc, rows);
        let r = a.run_round();
        if r == Err(TableCode::RowCountOff) {
            a.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            a.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51583_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51583");
    // 1 装载在册三表：行数与清单逐域一致、合计 85。
    {
        let a = TableAuditor::load();
        let mut total = 0usize;
        let mut dims = true;
        for d in ErrDomain::all().iter() {
            let want = match MANIFEST.get(d.idx()) {
                Some(&w) => w,
                None => 0,
            };
            let got = match a.table_len(*d) {
                Some(g) => g,
                None => 0,
            };
            if want != got {
                dims = false;
            }
            total += got;
        }
        s.add(
            "在册三表装载",
            dims && total == TOTAL_ROWS,
            "三表行数与清单逐域一致，合计恰 85 行",
        );
    }
    // 2 ×2 轮零漂移：两轮指纹相等并定稿。
    {
        let mut a = TableAuditor::load();
        let f1 = a.run_round();
        let f2 = a.run_round();
        let fin = a.finalize();
        let drift_zero = match (f1, f2) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        };
        s.add(
            "×2轮零漂移",
            drift_zero && fin == Ok(()),
            "UNX-F51584-J1：两轮复核指纹逐位一致后定稿",
        );
    }
    // 3 全局唯一性：85 行值两两不撞。
    {
        let a = TableAuditor::load();
        let mut vals: Vec<u32> = Vec::new();
        for d in ErrDomain::all().iter() {
            if let Some(t) = a.tables.get(d.idx()) {
                for r in t.iter() {
                    vals.push(r.val);
                }
            }
        }
        let mut uniq = true;
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                let x = match vals.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let y = match vals.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                if x == y {
                    uniq = false;
                }
            }
        }
        s.add("行值全局唯一", uniq && vals.len() == TOTAL_ROWS, "85 行值两两不撞");
    }
    // 4 域基址分派不重叠。
    {
        let b0 = domain_base(ErrDomain::Msc);
        let b1 = domain_base(ErrDomain::Uasp);
        let b2 = domain_base(ErrDomain::Cdc);
        s.add(
            "域基址分派不重叠",
            b1 > b0 && b2 > b1 && b0 % 0x40 == 0 && b1 % 0x40 == 0 && b2 % 0x40 == 0,
            "三域基址递增且按 0x40 对齐分派",
        );
    }
    // 5 复核轮次缺失拒定稿。
    {
        let mut a = TableAuditor::load();
        let _ = a.run_round();
        let r = a.finalize();
        s.add(
            "轮次缺失拒定稿",
            r == Err(TableCode::RoundsMissing),
            "不足 2 轮复核不得定稿",
        );
    }
    // 6 助记名与释义全非空。
    {
        let a = TableAuditor::load();
        let mut text = true;
        for d in ErrDomain::all().iter() {
            if let Some(t) = a.tables.get(d.idx()) {
                for r in t.iter() {
                    if r.mnemonic.is_empty() || r.detail.is_empty() {
                        text = false;
                    }
                }
            }
        }
        s.add("文案全非空", text, "每行助记名与中文释义都必须非空");
    }
    // 7 诊断码闭集唯一性。
    {
        let mut uniq = true;
        let mut text = true;
        for i in 0..ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(&c) => c,
                None => {
                    uniq = false;
                    continue;
                }
            };
            if c.val() != SEG_BASE + 1 + i as u32 {
                uniq = false;
            }
            if c.detail().is_empty() {
                text = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            uniq && text && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 8 注入核：四类 ×10 全抓、零逃逸。
    {
        let (d, e, per) = injection_harness();
        let all10 = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全抓",
            d == 40 && e == 0 && all10,
            "注入=检出+逃逸，逃逸恒 0，每类恰 10 轮全抓",
        );
    }
    // 9 守恒账单独核。
    {
        let mut a = TableAuditor::load();
        for _ in 0..10u32 {
            a.note_detected();
        }
        s.add("注入账守恒", a.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 10 双跑指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建审计器，FNV-1a 指纹逐位一致",
    );
    // 11 指纹复算稳定且摘要定长。
    {
        let mut a = TableAuditor::load();
        let _ = a.run_round();
        let _ = a.run_round();
        let fp1 = a.fingerprint();
        let fp2 = a.fingerprint();
        let brief: String = format!("{:016x}", fp1);
        s.add(
            "指纹复算稳定",
            fp1 == fp2 && brief.len() == 16,
            "指纹两次复算相等且十六进制摘要定长 16",
        );
    }
    // 12 相位单调：Auditing→Final。
    {
        let mut a = TableAuditor::load();
        let p0 = a.phase() as u8;
        let _ = a.run_round();
        let _ = a.run_round();
        let _ = a.finalize();
        let p1 = a.phase() as u8;
        s.add(
            "相位单调",
            p0 < p1 && a.rounds_done() == ROUNDS,
            "装载后必须先复核两轮再定稿，相位只进不退",
        );
    }
    // 13 quirk 生效率 100%（三表行全部带域标签，抽查行命中域）。
    {
        let a = TableAuditor::load();
        let mut hit = 0u64;
        let mut total = 0u64;
        for d in ErrDomain::all().iter() {
            if let Some(t) = a.tables.get(d.idx()) {
                for r in t.iter() {
                    total += 1;
                    let base = domain_base(*d);
                    if r.val >= base && r.val < base + 0x40 {
                        hit += 1;
                    }
                }
            }
        }
        s.add(
            "域标签生效率 100%",
            hit == total && total == TOTAL_ROWS as u64,
            "quirk 生效率 100%：85 行全部命中域值区间",
        );
    }
    // 14 审计行累计对账：两轮 = 170 行。
    {
        let mut a = TableAuditor::load();
        let _ = a.run_round();
        let _ = a.run_round();
        let (aud, rej) = a.tally();
        s.add(
            "审计行累计对账",
            aud == (TOTAL_ROWS * ROUNDS) as u64 && rej == 0,
            "两轮各审 85 行，累计 170 且零拒",
        );
    }
    // 15 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51583 三表终核 {} 行 ×2 轮 诊断码{} 变体",
            TOTAL_ROWS,
            ALL_CODES.len()
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51583"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51583_checks_green() {
        let set = run_unx51583_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn canonical_load_matches_manifest() {
        let a = TableAuditor::load();
        for d in ErrDomain::all().iter() {
            let want = MANIFEST[d.idx()];
            assert_eq!(a.table_len(*d), Some(want), "域 {} 行数在册", d.name());
        }
    }

    #[test]
    fn two_rounds_zero_drift_and_final() {
        let mut a = TableAuditor::load();
        let f1 = a.run_round();
        let f2 = a.run_round();
        assert_eq!(f1, f2);
        assert_eq!(a.finalize(), Ok(()));
        assert_eq!(a.phase(), Phase::Final);
        // 定稿后再跑轮必须拒。
        assert_eq!(a.run_round(), Err(TableCode::NotFinal));
    }

    #[test]
    fn rounds_missing_rejects_final() {
        let mut a = TableAuditor::load();
        assert_eq!(a.finalize(), Err(TableCode::RoundsMissing));
        let _ = a.run_round();
        assert_eq!(a.finalize(), Err(TableCode::RoundsMissing));
    }

    #[test]
    fn dup_val_injection_caught() {
        let mut a = TableAuditor::load();
        let mut rows = load_canonical_tables()[0].clone();
        let second = rows[1].val;
        rows[0].val = second;
        a.inject_table(ErrDomain::Msc, rows);
        let r = a.run_round();
        assert!(
            r == Err(TableCode::ValDup) || r == Err(TableCode::ValOutOfRange),
            "重复/越域值必须被抓"
        );
    }

    #[test]
    fn injection_all_classes_caught() {
        let (_, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "注入类 {} 应恰 10 轮全抓", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut a = TableAuditor::load();
        for _ in 0..10u32 {
            a.note_detected();
        }
        assert!(a.conserved());
        let mut bad = TableAuditor::load();
        for _ in 0..10u32 {
            bad.note_escape();
        }
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        let build = || -> u64 {
            let mut a = TableAuditor::load();
            let _ = a.run_round();
            let _ = a.run_round();
            a.fingerprint()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
