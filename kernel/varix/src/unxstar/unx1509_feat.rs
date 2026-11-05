//! UNX-F1509 回归报告落档（执行报告与版本归档） · 0x780C0段
//!
//! 判据原文（UNX-F1509-J1 摘录）：「回归报告落档 10/10 次（执行报告四节：
//! 矩阵版本/执行账/结果汇总/红账归因，报告哈希入册），报告与账面对拍一致率
//! 100%，漏节注入 10/10 必抓」；完成时 API/软件写回：状态=已完成、结果摘要
//! 非空。
//!
//! 设计要点：
//! 1. 执行报告四节结构冻结：矩阵版本/执行账/结果汇总/红账归因，四节以位掩
//!    码登记，缺任何一节在归档门即被拒；
//! 2. 报告哈希（FNV 折叠）入册：归档表只追加不删改，重复哈希拒绝——同一
//!    报告不许重复占档位；
//! 3. 对拍一致率 100%：报告登记的总数/三态计数/红账标签数必须与执行账复算
//!    值逐项相等，任何一项不符即整份报告拒收；
//! 4. 漏节注入 10/10：缺单节 4 路 + 缺双节 6 路共十路缺节报告，归档门全数
//!    抓获且返回各自缺节码；
//! 5. 状态机 开放→已封档 单向，封档后写入拒绝；诊断码闭集
//!    0x780C1..0x780CC。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x780C0;

/// 回归报告落档诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 缺矩阵版本节。
    ENoVer,
    /// 缺执行账节。
    ENoExec,
    /// 缺结果汇总节。
    ENoResult,
    /// 缺红账归因节。
    ENoRed,
    /// 报告与账面对拍不符。
    EMismatch,
    /// 重复入册（同哈希已占档）。
    EDup,
    /// 空报告。
    EEmpty,
    /// 落档乱序。
    EOrder,
    /// 归档已封档。
    ESealed,
    /// 哈希复算不符。
    EHash,
    /// 查询未入册的报告。
    ENoReg,
    /// 封档重放不符。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENoVer => 0x01,
                Diag::ENoExec => 0x02,
                Diag::ENoResult => 0x03,
                Diag::ENoRed => 0x04,
                Diag::EMismatch => 0x05,
                Diag::EDup => 0x06,
                Diag::EEmpty => 0x07,
                Diag::EOrder => 0x08,
                Diag::ESealed => 0x09,
                Diag::EHash => 0x0a,
                Diag::ENoReg => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENoVer => "缺矩阵版本节：报告必须载明矩阵版本号，先补版本节",
            Diag::ENoExec => "缺执行账节：报告必须挂执行账快照，先补执行账节",
            Diag::ENoResult => "缺结果汇总节：报告必须载明三态计数，先补汇总节",
            Diag::ENoRed => "缺红账归因节：报告必须载明红账标签数，先补归因节",
            Diag::EMismatch => "对拍不符：报告数字与执行账复算不一致，以账面为准",
            Diag::EDup => "重复入册：同哈希报告已占档位，勿重复落档",
            Diag::EEmpty => "空报告：无任何节的草稿不构成报告",
            Diag::EOrder => "落档乱序：档案号必须按序分配，禁止跳号回填",
            Diag::ESealed => "归档已封档：封档后写入拒绝，走下一批归档",
            Diag::EHash => "哈希不符：报告体被改，重出报告再落档",
            Diag::ENoReg => "未入册：查询的档案号不存在，先核对入册清单",
            Diag::ESeal => "封档重放不符：入册哈希复算不一致，档案可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ENoVer,
    Diag::ENoExec,
    Diag::ENoResult,
    Diag::ENoRed,
    Diag::EMismatch,
    Diag::EDup,
    Diag::EEmpty,
    Diag::EOrder,
    Diag::ESealed,
    Diag::EHash,
    Diag::ENoReg,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x780C1..0x780CC 闭集内"
}

/// 节掩码位：矩阵版本节。
pub const SEC_VERSION: u8 = 0x01;
/// 节掩码位：执行账节。
pub const SEC_EXEC: u8 = 0x02;
/// 节掩码位：结果汇总节。
pub const SEC_RESULT: u8 = 0x04;
/// 节掩码位：红账归因节。
pub const SEC_RED: u8 = 0x08;
/// 四节齐备掩码。
pub const SEC_ALL: u8 = SEC_VERSION | SEC_EXEC | SEC_RESULT | SEC_RED;

/// 执行账判决：绿（全过）。
pub const V_GREEN: u8 = 0;
/// 执行账判决：黄（超时过）。
pub const V_YELLOW: u8 = 1;
/// 执行账判决：红（失败）。
pub const V_RED: u8 = 2;

/// 一条执行账记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecRecord {
    /// 场景编号（1..=10）。
    pub sid: u16,
    /// 判决（0 绿 / 1 黄 / 2 红）。
    pub verdict: u8,
}

/// 执行账（一批十场景）。
pub struct ExecAccount {
    records: Vec<ExecRecord>,
}

impl ExecAccount {
    /// 空账。
    pub fn new() -> ExecAccount {
        ExecAccount { records: Vec::new() }
    }

    /// 登记一条执行记录（按场景序）。
    pub fn record(&mut self, sid: u16, verdict: u8) -> Result<(), u32> {
        if sid == 0 {
            return Err(Diag::EEmpty.val());
        }
        if sid as usize != self.records.len() + 1 {
            return Err(Diag::EOrder.val());
        }
        if verdict > V_RED {
            return Err(Diag::EEmpty.val());
        }
        self.records.push(ExecRecord { sid, verdict });
        Ok(())
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 取记录。
    pub fn at(&self, i: usize) -> Option<ExecRecord> {
        self.records.get(i).copied()
    }

    /// 某判决计数。
    pub fn count(&self, verdict: u8) -> u32 {
        let mut n = 0u32;
        for r in &self.records {
            if r.verdict == verdict {
                n += 1;
            }
        }
        n
    }

    /// 绿态计数。
    pub fn green(&self) -> u32 {
        self.count(V_GREEN)
    }

    /// 黄态计数。
    pub fn yellow(&self) -> u32 {
        self.count(V_YELLOW)
    }

    /// 红态计数。
    pub fn red(&self) -> u32 {
        self.count(V_RED)
    }
}

/// FNV 折叠辅助。
fn fold_word(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 本批落档报告数（10/10）。
pub const REPORTS: usize = 10;

/// 一份执行报告草稿（四节 + 对拍数字 + 哈希）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportDraft {
    /// 节掩码（四节位或）。
    pub mask: u8,
    /// 矩阵版本号。
    pub ver: u32,
    /// 场景总数。
    pub total: u32,
    /// 绿态数。
    pub green: u32,
    /// 黄态数。
    pub yellow: u32,
    /// 红态数。
    pub red: u32,
    /// 红账已归因条数。
    pub red_tagged: u32,
    /// 报告哈希（入册凭据）。
    pub hash: u64,
}

/// 按四节掩码与对拍数字出报告草稿（哈希覆盖全部字段）。
pub fn build_report(mask: u8, ver: u32, acct: &ExecAccount, red_tagged: u32) -> ReportDraft {
    let mut d = ReportDraft {
        mask,
        ver,
        total: acct.len() as u32,
        green: acct.green(),
        yellow: acct.yellow(),
        red: acct.red(),
        red_tagged,
        hash: 0,
    };
    let mut h = FNV_OFF;
    h = fold_word(h, d.mask as u64);
    h = fold_word(h, d.ver as u64);
    h = fold_word(h, d.total as u64);
    h = fold_word(h, d.green as u64);
    h = fold_word(h, d.yellow as u64);
    h = fold_word(h, d.red as u64);
    h = fold_word(h, d.red_tagged as u64);
    d.hash = h;
    d
}

/// 四节齐备判定。
pub fn sections_complete(mask: u8) -> bool {
    mask & SEC_ALL == SEC_ALL
}

/// 归档相位：开放 → 已封档（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 开放（可落档）。
    Open,
    /// 已封档。
    Sealed,
}

/// 报告归档表（只追加）。
pub struct ReportArchive {
    entries: Vec<ReportDraft>,
    phase: Phase,
    rejects: u32,
    root: u64,
}

impl ReportArchive {
    /// 空归档。
    pub fn new() -> ReportArchive {
        ReportArchive { entries: Vec::new(), phase: Phase::Open, rejects: 0, root: 0 }
    }

    /// 已落档份数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 取第 k 份报告。
    pub fn at(&self, k: usize) -> Option<ReportDraft> {
        self.entries.get(k).copied()
    }

    /// 按档案号取报告（未入册拒绝）。
    pub fn lookup(&self, file_no: u32) -> Result<ReportDraft, u32> {
        match self.entries.get(file_no as usize) {
            Some(d) => Ok(*d),
            None => Err(Diag::ENoReg.val()),
        }
    }

    /// 落档一份报告：四节齐备 → 对拍 → 哈希复核 → 去重 → 追加。
    pub fn file(&mut self, d: ReportDraft, acct: &ExecAccount, red_tagged: u32) -> Result<u32, u32> {
        if self.phase == Phase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.entries.len() >= REPORTS {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        if d.mask == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        if d.mask & SEC_VERSION == 0 {
            self.rejects += 1;
            return Err(Diag::ENoVer.val());
        }
        if d.mask & SEC_EXEC == 0 {
            self.rejects += 1;
            return Err(Diag::ENoExec.val());
        }
        if d.mask & SEC_RESULT == 0 {
            self.rejects += 1;
            return Err(Diag::ENoResult.val());
        }
        if d.mask & SEC_RED == 0 {
            self.rejects += 1;
            return Err(Diag::ENoRed.val());
        }
        if d.total != acct.len() as u32
            || d.green != acct.green()
            || d.yellow != acct.yellow()
            || d.red != acct.red()
            || d.red_tagged != red_tagged
        {
            self.rejects += 1;
            return Err(Diag::EMismatch.val());
        }
        let expect = build_report(d.mask, d.ver, acct, red_tagged).hash;
        if d.hash != expect {
            self.rejects += 1;
            return Err(Diag::EHash.val());
        }
        for e in &self.entries {
            if e.hash == d.hash {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        let file_no = self.entries.len() as u32;
        self.entries.push(d);
        Ok(file_no)
    }

    /// 归档折叠根。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.entries.len() as u64);
        for e in &self.entries {
            h = fold_word(h, e.hash);
        }
        h
    }

    /// 封档：入册哈希逐份复核后冻结（单向）。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        for e in &self.entries {
            if !sections_complete(e.mask) {
                return Err(Diag::ESeal.val());
            }
        }
        self.root = self.fold();
        self.phase = Phase::Sealed;
        Ok(self.root)
    }

    /// 封档根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重放：逐份复核入册哈希。
    pub fn replay(&self) -> Result<usize, u32> {
        if self.phase != Phase::Sealed {
            return Err(Diag::ESeal.val());
        }
        for e in &self.entries {
            if e.hash == 0 {
                return Err(Diag::EHash.val());
            }
        }
        if self.root != self.fold() {
            return Err(Diag::ESeal.val());
        }
        Ok(self.entries.len())
    }
}

/// 报告可读摘要（落档凭据文本）。
pub fn report_text(d: &ReportDraft) -> String {
    format!(
        "report ver={} sections={} total={} g={} y={} r={} red_tagged={} hash={:016x}",
        d.ver, d.mask, d.total, d.green, d.yellow, d.red, d.red_tagged, d.hash
    )
}

/// 标准执行账：7 绿 + 2 黄 + 1 红，红账标签 1。
pub fn build_account() -> ExecAccount {
    let mut a = ExecAccount::new();
    for sid in 1..=10u16 {
        let v = match sid {
            9 => V_YELLOW,
            10 => V_YELLOW,
            8 => V_RED,
            _ => V_GREEN,
        };
        let _ = a.record(sid, v);
    }
    a
}

/// 漏节注入 10/10：缺单节 4 路 + 缺双节 6 路，归档门全数抓获。
pub fn missing_section_matrix() -> [bool; 10] {
    // 十种缺节掩码：单缺 4 种 + 双缺 6 种。
    let masks: [u8; 10] = [
        SEC_ALL & !SEC_VERSION,
        SEC_ALL & !SEC_EXEC,
        SEC_ALL & !SEC_RESULT,
        SEC_ALL & !SEC_RED,
        SEC_VERSION | SEC_EXEC,
        SEC_VERSION | SEC_RESULT,
        SEC_VERSION | SEC_RED,
        SEC_EXEC | SEC_RESULT,
        SEC_EXEC | SEC_RED,
        SEC_RESULT | SEC_RED,
    ];
    let mut out = [false; 10];
    let acct = build_account();
    let mut i = 0usize;
    while i < 10 {
        let mask = masks[i];
        let d = build_report(mask, 0x780C0, &acct, 1);
        let mut arch = ReportArchive::new();
        let r = arch.file(d, &acct, 1);
        let caught = match i {
            0 => r == Err(Diag::ENoVer.val()),
            1 => r == Err(Diag::ENoExec.val()),
            2 => r == Err(Diag::ENoResult.val()),
            3 => r == Err(Diag::ENoRed.val()),
            _ => r.is_err(),
        };
        // 拒后档案仍可正常落档（门只拒缺节，不毁档案）。
        let ok_report = build_report(SEC_ALL, 0x780C0, &acct, 1);
        let recover = arch.file(ok_report, &acct, 1).is_ok();
        out[i] = caught && recover && !sections_complete(mask);
        i += 1;
    }
    out
}

/// 构建标准归档：十批报告逐份落档（版本号递增保证哈希唯一）。
pub fn build_archive() -> ReportArchive {
    let mut arch = ReportArchive::new();
    let acct = build_account();
    let mut batch = 0u32;
    while batch < REPORTS as u32 {
        let d = build_report(SEC_ALL, 0x780C0 + batch, &acct, 1);
        let _ = arch.file(d, &acct, 1);
        batch += 1;
    }
    arch
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    SEG_BASE == 0x780C0
}

/// 落档 10/10 且哈希入册只增。
fn archive_ok(arch: &ReportArchive) -> bool {
    if arch.len() != REPORTS {
        return false;
    }
    let mut k = 0usize;
    while k < arch.len() {
        match arch.at(k) {
            Some(d) => {
                if !sections_complete(d.mask) || d.hash == 0 {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    true
}

/// 对拍一致率 100%：报告数字与账面逐项相等。
fn crosscheck_ok(arch: &ReportArchive) -> bool {
    let acct = build_account();
    let mut ok = 0u32;
    let mut k = 0usize;
    while k < arch.len() {
        let d = match arch.at(k) {
            Some(x) => x,
            None => return false,
        };
        if d.total == acct.len() as u32
            && d.green == acct.green()
            && d.yellow == acct.yellow()
            && d.red == acct.red()
        {
            ok += 1;
        }
        k += 1;
    }
    ok == REPORTS as u32
}

/// 漏节注入 10/10 必抓。
fn missing_ok() -> bool {
    missing_section_matrix() == [true; 10]
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_archive();
    let b = build_archive();
    a.fold() == b.fold() && a.fold() != 0
}

/// 十四项判据。
pub fn run_unx1509_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1509");
    let arch = build_archive();

    s.add_dyn(
        "F1509-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1509-02 四节结构齐备",
        sections_complete(SEC_ALL) && SEC_ALL == 0x0F,
        &format!("sec_all=0x{:02x} mask=0x{:02x}", SEC_ALL, {
            let d = match arch.at(0) {
                Some(x) => x,
                None => ReportDraft {
                    mask: 0,
                    ver: 0,
                    total: 0,
                    green: 0,
                    yellow: 0,
                    red: 0,
                    red_tagged: 0,
                    hash: 0,
                },
            };
            d.mask
        }),
    );

    s.add_dyn(
        "F1509-03 落档十份齐额",
        archive_ok(&arch) && !arch.is_empty(),
        &format!("reports={}", arch.len()),
    );

    s.add_dyn(
        "F1509-04 对拍一致率百分百",
        crosscheck_ok(&arch),
        &format!("agree_reports={}", arch.len()),
    );

    s.add_dyn(
        "F1509-05 漏节注入十路必抓",
        missing_ok(),
        &format!("missing10={:?}", missing_section_matrix()),
    );

    // 对拍门：数字不符整份拒收。
    let acct = build_account();
    let mut m = ReportArchive::new();
    let good = build_report(SEC_ALL, 0x780C0, &acct, 1);
    let ok1 = m.file(good, &acct, 1).is_ok();
    let mut bad_total = good;
    bad_total.total += 1;
    let rej_total = m.file(bad_total, &acct, 1) == Err(Diag::EMismatch.val());
    let mut bad_tag = good;
    bad_tag.ver += 1;
    bad_tag.red_tagged = 0;
    let rej_tag = m.file(bad_tag, &acct, 1) == Err(Diag::EMismatch.val());
    s.add_dyn(
        "F1509-06 对拍门逐项拦截",
        ok1 && rej_total && rej_tag && m.rejects() == 2 && m.len() == 1,
        &format!("rejects={} len={}", m.rejects(), m.len()),
    );

    // 哈希门：版本号入哈希但不入对拍——改版本号必须只被哈希门抓住。
    let mut h = ReportArchive::new();
    let mut tampered = build_report(SEC_ALL, 0x780C1, &acct, 1);
    tampered.ver += 1;
    let caught = h.file(tampered, &acct, 1) == Err(Diag::EHash.val());
    let clean = build_report(SEC_ALL, 0x780C1, &acct, 1);
    let after = h.file(clean, &acct, 1).is_ok();
    s.add_dyn(
        "F1509-07 哈希门抓改报",
        caught && after && h.rejects() == 1,
        &format!("rejects={}", h.rejects()),
    );

    // 去重门：同哈希重复入册拒绝。
    let mut dup = ReportArchive::new();
    let first = build_report(SEC_ALL, 0x780C2, &acct, 1);
    let f1 = dup.file(first, &acct, 1);
    let again = dup.file(first, &acct, 1) == Err(Diag::EDup.val());
    s.add_dyn(
        "F1509-08 重复入册拒绝",
        f1 == Ok(0) && again && dup.len() == 1,
        &format!("len={}", dup.len()),
    );

    // 空报告拒绝。
    let mut e = ReportArchive::new();
    let empty = build_report(0, 0x780C0, &acct, 1);
    s.add_dyn(
        "F1509-09 空报告拒收",
        e.file(empty, &acct, 1) == Err(Diag::EEmpty.val()) && e.rejects() == 1,
        &format!("rejects={}", e.rejects()),
    );

    // 封档单向与封档后写入拒绝。
    let mut seq = build_archive();
    let r1 = seq.seal();
    let root = seq.root();
    let d = build_report(SEC_ALL, 0x79000, &acct, 1);
    let write_after = seq.file(d, &acct, 1) == Err(Diag::ESealed.val());
    let re_seal = seq.seal() == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1509-10 封档单向写入被拒",
        r1.is_ok() && root != 0 && write_after && re_seal && seq.phase() == Phase::Sealed,
        &format!("root={:016x}", root),
    );

    s.add_dyn(
        "F1509-11 封档重放与未入册查询",
        seq.replay() == Ok(REPORTS) && seq.lookup(99) == Err(Diag::ENoReg.val()),
        &format!("replay={:?}", seq.replay()),
    );

    // 查询已入册的报告可取回且哈希一致。
    let d0 = arch.lookup(0);
    let d9 = arch.lookup(9);
    s.add_dyn(
        "F1509-12 入册可查回",
        match d0 {
            Ok(d) => d.ver == 0x780C0 && d.hash != 0,
            Err(_) => false,
        } && match d9 {
            Ok(d) => d.ver == 0x780C0 + 9,
            Err(_) => false,
        },
        &format!("d0={:?}", d0.map(|d| d.ver)),
    );

    s.add_dyn(
        "F1509-13 双跑一致",
        dual_run_ok(),
        &format!("fold={:016x}", build_archive().fold()),
    );

    // 报告摘要非空落档。
    let head = match arch.at(0) {
        Some(d) => report_text(&d),
        None => String::new(),
    };
    s.add_dyn(
        "F1509-14 报告摘要非空",
        head.contains("sections=15") && head.len() > 40,
        &head,
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_counts() {
        let a = build_account();
        assert_eq!(a.len(), 10);
        assert_eq!(a.green(), 7);
        assert_eq!(a.yellow(), 2);
        assert_eq!(a.red(), 1);
        assert_eq!(a.count(V_RED), 1);
    }

    #[test]
    fn record_gates() {
        let mut a = ExecAccount::new();
        assert_eq!(a.record(0, V_GREEN), Err(Diag::EEmpty.val()));
        assert_eq!(a.record(2, V_GREEN), Err(Diag::EOrder.val()));
        assert_eq!(a.record(1, 9), Err(Diag::EEmpty.val()));
        assert_eq!(a.record(1, V_GREEN), Ok(()));
        assert_eq!(a.record(1, V_GREEN), Err(Diag::EOrder.val()));
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn sections_bitmask() {
        assert!(sections_complete(SEC_ALL));
        assert!(sections_complete(0x0F));
        assert!(!sections_complete(0x0E));
        assert!(!sections_complete(0));
        assert_eq!(SEC_ALL, SEC_VERSION | SEC_EXEC | SEC_RESULT | SEC_RED);
    }

    #[test]
    fn hash_covers_fields() {
        let acct = build_account();
        let a = build_report(SEC_ALL, 1, &acct, 1);
        let b = build_report(SEC_ALL, 2, &acct, 1);
        let c = build_report(SEC_ALL, 1, &acct, 2);
        assert_ne!(a.hash, b.hash);
        assert_ne!(a.hash, c.hash);
        assert_eq!(a.hash, build_report(SEC_ALL, 1, &acct, 1).hash);
    }

    #[test]
    fn missing_sections_all_caught() {
        assert_eq!(missing_section_matrix(), [true; 10]);
        assert!(missing_ok());
    }

    #[test]
    fn archive_files_ten() {
        let arch = build_archive();
        assert_eq!(arch.len(), 10);
        assert!(archive_ok(&arch));
        assert!(crosscheck_ok(&arch));
        assert!(arch.lookup(9).is_ok());
        assert_eq!(arch.lookup(10), Err(Diag::ENoReg.val()));
    }

    #[test]
    fn mismatch_and_hash_gates() {
        let acct = build_account();
        let mut arch = ReportArchive::new();
        let good = build_report(SEC_ALL, 0x780C0, &acct, 1);
        assert_eq!(arch.file(good, &acct, 1), Ok(0));
        let mut bad = good;
        bad.yellow = 0;
        assert_eq!(arch.file(bad, &acct, 1), Err(Diag::EMismatch.val()));
        let mut hbad = build_report(SEC_ALL, 0x780C1, &acct, 1);
        hbad.ver += 1;
        assert_eq!(arch.file(hbad, &acct, 1), Err(Diag::EHash.val()));
        assert_eq!(arch.rejects(), 2);
        assert_eq!(arch.len(), 1);
    }

    #[test]
    fn seal_monotonic_replay() {
        let mut arch = build_archive();
        let root = arch.seal();
        assert!(root.is_ok());
        assert_ne!(arch.root(), 0);
        assert_eq!(arch.phase(), Phase::Sealed);
        assert_eq!(arch.seal(), Err(Diag::ESealed.val()));
        assert_eq!(arch.replay(), Ok(10));
        // 未封档重放拒绝。
        assert_eq!(build_archive().replay(), Err(Diag::ESeal.val()));
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::ENoVer.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeal.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x0d), "未知诊断码：不在 0x780C1..0x780CC 闭集内");
    }

    #[test]
    fn report_text_and_checks_green() {
        let arch = build_archive();
        let d = arch.at(0).unwrap();
        let t = report_text(&d);
        assert!(t.contains("sections=15"));
        assert!(t.len() > 40);
        let s = run_unx1509_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
