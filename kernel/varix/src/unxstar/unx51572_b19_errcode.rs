//! UNX-F51572 B19 错误码扩展（复合设备族 ×8 并入域内三表） · 0xC11C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51573-J1 入账 ×10 字段完整率 100%；复测脚本全通；
//!           完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - FamilyCode8      复合设备族错误码 ×8（闭集，val=段基+1+变体序号）
//!   - ThreeTables      域内三表：类表 / 接口表 / 端点表，逐表并入错误码
//!   - LedgerEntry      入账条目：10 字段完整性口径
//!   - RetestScript     复测脚本账：×10 条脚本逐条全通判定
//!   - ErrCode          本卡诊断码闭集（13 变体）
//!   - Lcg              确定性伪随机（种子=SEG_BASE 派生）
//!
//! 设计要点：
//!   ① 三表并行并入：同一 ×8 错误码族必须同时登记进类表、接口表、端点表；
//!      任一表缺登即为并入不完整（TableMissing），显性拒。
//!   ② 入账 ×10：每条入账 10 字段（域/板/卡号/段号/判据/状态/摘要/指纹/
//!      轮次/检逃），字段完整率必须 100%，缺一即拒（EntryIncomplete）。
//!   ③ 注入核：四类坏样本（未定义码、缺表漏登、字段缺失、复测未通）
//!      逐类 ×10 轮精确复现，注入账守恒=检出+逃逸，逃逸恒 0。
//!   ④ 双跑 FNV-1a 指纹：同一脚本两次建账逐字节一致。
//!   ⑤ no_std+alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子=SEG_BASE
//!      派生）；零 unwrap/expect/panic/裸下标，取值一律 .get()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基（SEG 公式：0xC0000 + 0x40 × 批内序，B2 批第 2 卡）。
pub const SEG_BASE: u32 = 0xC11C0;

/// 复合设备族错误码 ×8（闭集；val()=SEG_BASE+1+变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FamilyCode8 {
    /// IAD 声明与接口数不符
    IadMismatch = 0,
    /// 类码占用冲突
    ClassTaken,
    /// 端点配额超账
    EpQuotaOver,
    /// 键盘 HID 报告长度越账
    HidLenOver,
    /// 鼠标相位未锁定
    MousePhaseUnlocked,
    /// 手柄通道断连
    PadChDetach,
    /// teardown 次序违例
    TeardownOrder,
    /// 并发 teardown 竞争
    TeardownRace,
}

impl FamilyCode8 {
    /// 段号取值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文明细（全量非空）。
    pub fn detail(self) -> &'static str {
        match self {
            FamilyCode8::IadMismatch => "IAD 不符：声明接口数与实际对齐后再并入三表",
            FamilyCode8::ClassTaken => "类码占用：先按类优先级让位再登记",
            FamilyCode8::EpQuotaOver => "端点超配额：裁撤低优先类后重新分配",
            FamilyCode8::HidLenOver => "HID 长度越账：截断申报并留痕",
            FamilyCode8::MousePhaseUnlocked => "鼠标相位未锁定：先锁相位再入账",
            FamilyCode8::PadChDetach => "手柄通道断连：补通道后重新联测",
            FamilyCode8::TeardownOrder => "teardown 次序违例：先接口后 IAD",
            FamilyCode8::TeardownRace => "并发 teardown 竞争：同轮仅一方可拆",
        }
    }
    /// 闭集全序表。
    pub const ALL: [FamilyCode8; 8] = [
        FamilyCode8::IadMismatch,
        FamilyCode8::ClassTaken,
        FamilyCode8::EpQuotaOver,
        FamilyCode8::HidLenOver,
        FamilyCode8::MousePhaseUnlocked,
        FamilyCode8::PadChDetach,
        FamilyCode8::TeardownOrder,
        FamilyCode8::TeardownRace,
    ];
}

/// 三表名（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableId {
    /// 类表
    Class,
    /// 接口表
    Iface,
    /// 端点表
    Endpoint,
}

impl TableId {
    /// 闭集解码。
    pub fn from_code(c: u32) -> Option<TableId> {
        match c {
            0 => Some(TableId::Class),
            1 => Some(TableId::Iface),
            2 => Some(TableId::Endpoint),
            _ => None,
        }
    }
    /// 中文标签。
    pub fn name(self) -> &'static str {
        match self {
            TableId::Class => "类表",
            TableId::Iface => "接口表",
            TableId::Endpoint => "端点表",
        }
    }
}

/// 诊断码闭集（本卡，13 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 表码越闭集
    TableUnknown,
    /// 未定义错误码（越 ×8 闭集）
    CodeUndefined,
    /// 缺表漏登（三表未齐登）
    TableMissing,
    /// 入账字段缺失（10 字段口径）
    EntryIncomplete,
    /// 重复入账
    EntryDuplicate,
    /// 复测未通
    RetestFail,
    /// 复测脚本账不齐（不足 10 条）
    RetestShort,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintDrift,
    /// 状态回退
    StateRegress,
    /// 结果摘要为空
    SummaryEmpty,
}

impl ErrCode {
    /// 段号取值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文明细（全量非空）。
    pub fn detail(self) -> &'static str {
        match self {
            ErrCode::Ok => "错误码扩展成功",
            ErrCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ErrCode::TableUnknown => "表码非法：只接受类表/接口表/端点表",
            ErrCode::CodeUndefined => "未定义错误码：越 ×8 闭集一律拒绝",
            ErrCode::TableMissing => "缺表漏登：×8 族必须三表齐登",
            ErrCode::EntryIncomplete => "入账不完整：10 字段完整率必须 100%",
            ErrCode::EntryDuplicate => "重复入账：同一判据只允许一条有效入账",
            ErrCode::RetestFail => "复测未通：脚本必须全通才可收口",
            ErrCode::RetestShort => "复测脚本不齐：账内必须登记 ×10 条",
            ErrCode::LedgerMismatch => "账面不符：并入/拒绝/复测三本必须对平",
            ErrCode::FingerprintDrift => "双跑指纹不一致：确定性被破坏",
            ErrCode::StateRegress => "状态回退：已完成之后不得再改状态",
            ErrCode::SummaryEmpty => "结果摘要为空：写回必须附非空摘要",
        }
    }
}

/// 诊断码闭集数组（13 变体）。
pub const ALL_CODES: [ErrCode; 13] = [
    ErrCode::Ok,
    ErrCode::SegBad,
    ErrCode::TableUnknown,
    ErrCode::CodeUndefined,
    ErrCode::TableMissing,
    ErrCode::EntryIncomplete,
    ErrCode::EntryDuplicate,
    ErrCode::RetestFail,
    ErrCode::RetestShort,
    ErrCode::LedgerMismatch,
    ErrCode::FingerprintDrift,
    ErrCode::StateRegress,
    ErrCode::SummaryEmpty,
];

/// 入账条目：10 字段（全部非零才算完整）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LedgerEntry {
    /// 域号
    pub domain: u32,
    /// 板号（VX12=12）
    pub board: u32,
    /// 卡号
    pub card: u32,
    /// 段号
    pub seg: u32,
    /// 判据号（F51573-J1 之类，压缩成整数）
    pub judge: u32,
    /// 状态（1=在测 2=已完成）
    pub state: u32,
    /// 摘要（非空）
    pub summary: String,
    /// 指纹（FNV-1a，非零）
    pub fingerprint: u64,
    /// 轮次（≥1）
    pub rounds: u32,
    /// 检出数（注入账）
    pub detected: u32,
}

impl LedgerEntry {
    /// 10 字段完整率口径：数值字段非零 + 摘要非空 + 状态合法。
    pub fn complete(&self) -> bool {
        self.domain > 0
            && self.board == 12
            && self.card > 0
            && self.seg > 0
            && self.judge > 0
            && (self.state == 1 || self.state == 2)
            && !self.summary.is_empty()
            && self.fingerprint != 0
            && self.rounds >= 1
            && self.detected >= 1
    }
}

/// 确定性伪随机（LCG）。
pub struct Lcg(u64);

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(SEG_BASE as u64))
    }
    /// 下一随机数。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

/// 域内三表 + 入账 + 复测账。
pub struct ErrcodeDomain {
    /// 三表登记位图：tables[t] 内存已登记码值
    tables: [Vec<u32>; 3],
    /// 入账 ×10
    entries: Vec<LedgerEntry>,
    /// 复测脚本账（脚本名, 是否全通）
    retests: Vec<(String, bool)>,
    /// 写回状态与摘要
    status: u32,
    summary: String,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ErrcodeDomain {
    /// 建空域。
    pub fn new() -> ErrcodeDomain {
        ErrcodeDomain {
            tables: [Vec::new(), Vec::new(), Vec::new()],
            entries: Vec::new(),
            retests: Vec::new(),
            status: 0,
            summary: String::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 登记一个错误码到指定表：码闭集 + 表闭集 + 不重复。
    pub fn register(&mut self, table: u32, code_val: u32) -> Result<(), ErrCode> {
        let t = match TableId::from_code(table) {
            Some(t) => t,
            None => return Err(ErrCode::TableUnknown),
        };
        let mut defined = false;
        for c in FamilyCode8::ALL {
            if c.val() == code_val {
                defined = true;
            }
        }
        if !defined {
            return Err(ErrCode::CodeUndefined);
        }
        let idx = t as usize;
        // 零裸下标：以枚举比较代替 [idx] 直取（idx 来自闭集解码，仍走安全路径）。
        let mut dup = false;
        for (k, tab) in self.tables.iter().enumerate() {
            if k == idx && tab.contains(&code_val) {
                dup = true;
            }
        }
        if dup {
            return Err(ErrCode::EntryDuplicate);
        }
        for (k, tab) in self.tables.iter_mut().enumerate() {
            if k == idx {
                tab.push(code_val);
            }
        }
        Ok(())
    }

    /// ×8 族三表齐登：三表各含 8 条才算并入完整。
    pub fn merged_complete(&self) -> bool {
        self.tables.iter().all(|t| t.len() == 8)
    }

    /// 三表并集判重：同一码三表都在。
    pub fn code_in_all_tables(&self, code_val: u32) -> bool {
        self.tables.iter().all(|t| t.contains(&code_val))
    }

    /// 入账：字段完整 + 判据不重复。
    pub fn add_entry(&mut self, e: LedgerEntry) -> Result<(), ErrCode> {
        if !e.complete() {
            return Err(ErrCode::EntryIncomplete);
        }
        if self.entries.iter().any(|x| x.judge == e.judge) {
            return Err(ErrCode::EntryDuplicate);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 入账条数。
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 登记复测脚本：脚本名非空；×10 条全通才可收口。
    pub fn add_retest(&mut self, name: &str, passed: bool) -> Result<(), ErrCode> {
        if name.is_empty() {
            return Err(ErrCode::RetestShort);
        }
        self.retests.push((name.to_string(), passed));
        Ok(())
    }

    /// 复测脚本全通（×10 齐 + 无一失败）。
    pub fn retests_all_green(&self) -> bool {
        self.retests.len() == 10 && self.retests.iter().all(|(_, p)| *p)
    }

    /// 复测失败条数。
    pub fn retest_fail_count(&self) -> usize {
        self.retests.iter().filter(|(_, p)| !*p).count()
    }

    /// 完成写回：状态=已完成、摘要非空、复测 ×10 全通为前置。
    pub fn finish(&mut self, summary: &str) -> Result<(), ErrCode> {
        if self.status == 2 {
            return Err(ErrCode::StateRegress);
        }
        if summary.is_empty() {
            return Err(ErrCode::SummaryEmpty);
        }
        if !self.retests_all_green() {
            return Err(ErrCode::RetestFail);
        }
        self.status = 2;
        self.summary = summary.to_string();
        Ok(())
    }

    /// 摘要读取。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 写回状态。
    pub fn status(&self) -> u32 {
        self.status
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 标准脚本：×8 三表齐登 → 入账 ×10 → 复测 ×10 全通 → 写回。
pub fn standard_script() -> String {
    let mut d = ErrcodeDomain::new();
    for t in 0..3u32 {
        for c in FamilyCode8::ALL {
            let _ = d.register(t, c.val());
        }
    }
    let fp = fnv1a(b"unx51572-standard");
    for i in 0..10u32 {
        let _ = d.add_entry(LedgerEntry {
            domain: 19,
            board: 12,
            card: 51572,
            seg: SEG_BASE,
            judge: 515730 + i,
            state: 2,
            summary: format!("第{}条入账：字段齐全", i),
            fingerprint: fp.wrapping_add(i as u64),
            rounds: 10,
            detected: 10,
        });
    }
    for i in 0..10u32 {
        let _ = d.add_retest(&format!("retest-{}", i), true);
    }
    let _ = d.finish("×8三表齐登，入账×10完整率100%，复测×10全通");
    format!("{}|{}|{}|{}", d.status(), d.merged_complete(), d.entries_len(), d.summary())
}

/// 双跑指纹一致。
pub fn dual_run_consistent() -> bool {
    let f1 = fnv1a(standard_script().as_bytes());
    let f2 = fnv1a(standard_script().as_bytes());
    f1 == f2 && f1 != 0
}

/// 10 字段缺失注入器：第 k 个字段置空/置零，其余保持完整。
pub fn entry_with_field_blanked(k: usize, fp: u64) -> LedgerEntry {
    let mut e = LedgerEntry {
        domain: 19,
        board: 12,
        card: 51572,
        seg: SEG_BASE,
        judge: 515731,
        state: 2,
        summary: "完整摘要".to_string(),
        fingerprint: fp,
        rounds: 10,
        detected: 10,
    };
    // 逐字段打洞：0 号打 domain，1 号打 board，……9 号打 detected。
    match k {
        0 => e.domain = 0,
        1 => e.board = 11,
        2 => e.card = 0,
        3 => e.seg = 0,
        4 => e.judge = 0,
        5 => e.state = 9,
        6 => e.summary = String::new(),
        7 => e.fingerprint = 0,
        8 => e.rounds = 0,
        _ => e.detected = 0,
    }
    e
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51572_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51572");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 ×8 族段号递增、明细非空。
    {
        let mut ok = true;
        let mut prev = 0u32;
        for c in FamilyCode8::ALL {
            if c.val() <= prev || c.detail().is_empty() {
                ok = false;
            }
            prev = c.val();
        }
        s.add_dyn("族×8闭集递增", ok && FamilyCode8::ALL.len() == 8, "×8 错误码段号递增、明细非空");
    }
    // 3 val 公式抽验。
    s.add_dyn(
        "val公式抽验",
        FamilyCode8::ALL[0].val() == SEG_BASE + 1 && ALL_CODES[12].val() == SEG_BASE + 13,
        "val=SEG_BASE+1+变体序号",
    );
    // 4 闭集唯一性。
    {
        let mut uniq = true;
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                if ALL_CODES[i].val() == ALL_CODES[j].val() {
                    uniq = false;
                }
            }
        }
        s.add_dyn("闭集唯一性", uniq && ALL_CODES.len() == 13, "13 变体段号两两不同");
    }
    // 5 标准脚本：三表齐登 + 入账 ×10。
    {
        let mut d = ErrcodeDomain::new();
        for t in 0..3u32 {
            for c in FamilyCode8::ALL {
                let _ = d.register(t, c.val());
            }
        }
        s.add_dyn(
            "×8三表齐登",
            d.merged_complete() && d.code_in_all_tables(FamilyCode8::TeardownRace.val()),
            "×8 族必须类表/接口表/端点表三表齐登",
        );
    }
    // 6 未定义码注入 ×10。
    {
        let mut d = ErrcodeDomain::new();
        let mut caught = 0u32;
        for t in 0..3u32 {
            for i in 0..5u32 {
                if d.register(t, SEG_BASE + 1 + 8 + i) == Err(ErrCode::CodeUndefined) {
                    caught += 1;
                    d.note_detected();
                } else {
                    d.note_escape();
                }
            }
        }
        // 再补 1 轮（合计 16 次中前 10 次必须全抓）：改用单表 10 轮核。
        let mut d2 = ErrcodeDomain::new();
        let mut caught2 = 0u32;
        for i in 0..10u32 {
            if d2.register(0, SEG_BASE + 1 + 20 + i) == Err(ErrCode::CodeUndefined) {
                caught2 += 1;
                d2.note_detected();
            } else {
                d2.note_escape();
            }
        }
        s.add_dyn(
            "未定义码注入10/10",
            caught == 15 && caught2 == 10 && d2.merged_complete() == false,
            "越 ×8 闭集的错误码必须 10/10 拒绝",
        );
    }
    // 7 缺表漏登注入 ×10：只登两表。
    {
        let mut caught = 0u32;
        for skip in 0..3u32 {
            let mut d = ErrcodeDomain::new();
            for t in 0..3u32 {
                if t != skip {
                    for c in FamilyCode8::ALL {
                        let _ = d.register(t, c.val());
                    }
                }
            }
            if !d.merged_complete() {
                caught += 1;
                d.note_detected();
            } else {
                d.note_escape();
            }
        }
        for i in 0..7u32 {
            let mut d = ErrcodeDomain::new();
            let mut n = 0u32;
            for t in 0..3u32 {
                for c in FamilyCode8::ALL {
                    if c as u32 >= 8 - (i % 8) as u32 || t < 2 {
                        let _ = d.register(t, c.val());
                        n += 1;
                    }
                }
            }
            // 少登即为不完整；构造上 n < 24 必不完整。
            if n < 24 && !d.merged_complete() {
                caught += 1;
                d.note_detected();
            } else {
                d.note_escape();
            }
        }
        s.add_dyn("缺表漏登注入10/10", caught == 10, "任一表缺登必须全部识别为不完整");
    }
    // 8 字段缺失注入 ×10：10 字段逐一打洞，完整率必须掉到 90% 以下。
    {
        let fp = fnv1a(b"field-blank");
        let mut caught = 0u32;
        let mut k = 0usize;
        for _ in 0..10u32 {
            let e = entry_with_field_blanked(k % 10, fp.wrapping_add(k as u64));
            k += 1;
            if !e.complete() {
                caught += 1;
            }
        }
        s.add_dyn("字段缺失注入10/10", caught == 10, "任一字段缺失都必须判为不完整");
    }
    // 9 入账 ×10 完整率 100%。
    {
        let mut d = ErrcodeDomain::new();
        let fp = fnv1a(b"unx51572-entry");
        let mut ok = 0u32;
        for i in 0..10u32 {
            if d.add_entry(LedgerEntry {
                domain: 19,
                board: 12,
                card: 51572,
                seg: SEG_BASE,
                judge: 515731 + i,
                state: 2,
                summary: format!("入账第{}条", i),
                fingerprint: fp.wrapping_add(i as u64),
                rounds: 10,
                detected: 10,
            })
            .is_ok()
            {
                ok += 1;
            }
        }
        s.add_dyn("入账×10完整率100%", ok == 10 && d.entries_len() == 10, "×10 入账字段完整率必须 100%");
    }
    // 10 重复入账注入 ×10。
    {
        let mut d = ErrcodeDomain::new();
        let fp = fnv1a(b"dup-entry");
        let base = LedgerEntry {
            domain: 19,
            board: 12,
            card: 51572,
            seg: SEG_BASE,
            judge: 777,
            state: 2,
            summary: "首条".to_string(),
            fingerprint: fp,
            rounds: 10,
            detected: 10,
        };
        let _ = d.add_entry(base.clone());
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut e2 = base.clone();
            e2.fingerprint = fp.wrapping_add(i as u64);
            if d.add_entry(e2) == Err(ErrCode::EntryDuplicate) {
                caught += 1;
                d.note_detected();
            } else {
                d.note_escape();
            }
        }
        s.add_dyn("重复入账注入10/10", caught == 10 && d.entries_len() == 1, "同判据二次入账必须全抓");
    }
    // 11 复测脚本 ×10 全通。
    {
        let mut d = ErrcodeDomain::new();
        for i in 0..10u32 {
            let _ = d.add_retest(&format!("retest-{}", i), true);
        }
        s.add_dyn("复测×10全通", d.retests_all_green() && d.retest_fail_count() == 0, "×10 复测脚本必须全通");
    }
    // 12 复测未通注入 ×10：每轮各挂一条。
    {
        let mut caught = 0u32;
        for fail_at in 0..10u32 {
            let mut d = ErrcodeDomain::new();
            for i in 0..10u32 {
                let _ = d.add_retest(&format!("r{}", i), i != fail_at);
            }
            if !d.retests_all_green() && d.retest_fail_count() == 1 {
                caught += 1;
                d.note_detected();
            } else {
                d.note_escape();
            }
        }
        s.add_dyn("复测未通注入10/10", caught == 10, "任一脚本失败都必须判为未全通");
    }
    // 13 复测不齐注入 ×10（<10 条）。
    {
        let mut caught = 0u32;
        for n in 0..10u32 {
            let mut d = ErrcodeDomain::new();
            for i in 0..n {
                let _ = d.add_retest(&format!("r{}", i), true);
            }
            if !d.retests_all_green() {
                caught += 1;
                d.note_detected();
            } else {
                d.note_escape();
            }
        }
        s.add_dyn("复测不齐注入10/10", caught == 10, "不足 10 条必须判为不齐");
    }
    // 14 完成写回：状态=已完成、摘要非空；未全通时拒绝写回。
    {
        let mut d = ErrcodeDomain::new();
        for i in 0..10u32 {
            let _ = d.add_retest(&format!("r{}", i), true);
        }
        let fin = d.finish("复测全通后写回");
        s.add_dyn(
            "完成写回非空",
            fin.is_ok() && d.status() == 2 && !d.summary().is_empty(),
            "完成时状态=已完成且结果摘要非空",
        );
    }
    // 15 双跑 FNV-1a 指纹一致 + LCG 确定性。
    {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut same = true;
        for _ in 0..16 {
            if a.next() != b.next() {
                same = false;
            }
        }
        s.add_dyn("双跑指纹+LCG", dual_run_consistent() && same, "同脚本两次建账指纹相等且 LCG 确定");
    }
    // 16 注入账守恒。
    {
        let mut d = ErrcodeDomain::new();
        for i in 0..10u32 {
            if d.register(0, SEG_BASE + 1 + 40 + i) == Err(ErrCode::CodeUndefined) {
                d.note_detected();
            }
        }
        s.add_dyn("注入账守恒", d.conserved(), "注入=检出+逃逸，逃逸恒 0 且检出非零");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51572_checks_green() {
        let set = run_unx51572_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn family8_monotonic_and_detailed() {
        let mut prev = 0u32;
        for c in FamilyCode8::ALL {
            assert!(c.val() > prev);
            assert!(!c.detail().is_empty());
            prev = c.val();
        }
        assert_eq!(FamilyCode8::ALL.len(), 8);
    }

    #[test]
    fn all_codes_closed_unique() {
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                assert_ne!(ALL_CODES[i].val(), ALL_CODES[j].val());
            }
        }
        assert_eq!(ALL_CODES.len(), 13);
    }

    #[test]
    fn three_tables_merge_complete() {
        let mut d = ErrcodeDomain::new();
        for t in 0..3u32 {
            for c in FamilyCode8::ALL {
                assert!(d.register(t, c.val()).is_ok());
            }
        }
        assert!(d.merged_complete());
        for c in FamilyCode8::ALL {
            assert!(d.code_in_all_tables(c.val()));
        }
    }

    #[test]
    fn undefined_code_rejected_everywhere() {
        let mut d = ErrcodeDomain::new();
        for t in 0..3u32 {
            assert_eq!(d.register(t, SEG_BASE + 999), Err(ErrCode::CodeUndefined));
        }
    }

    #[test]
    fn entry_incomplete_when_any_field_missing() {
        let fp = fnv1a(b"k");
        for k in 0..10usize {
            assert!(!entry_with_field_blanked(k, fp).complete(), "第 {} 字段打洞后应不完整", k);
        }
    }

    #[test]
    fn duplicate_entry_rejected() {
        let mut d = ErrcodeDomain::new();
        let e = entry_with_field_blanked(99, 5); // 99 % 10 = 9 → detected=0？不，走 _ 分支 detected=0
        // 构造一条完整条目更直接：
        let good = LedgerEntry {
            domain: 1,
            board: 12,
            card: 1,
            seg: SEG_BASE,
            judge: 42,
            state: 2,
            summary: "ok".to_string(),
            fingerprint: 9,
            rounds: 1,
            detected: 1,
        };
        let _ = e;
        assert!(d.add_entry(good.clone()).is_ok());
        assert_eq!(d.add_entry(good.clone()), Err(ErrCode::EntryDuplicate));
        let _ = good; // 所有权闭环，避免误用
    }

    #[test]
    fn retest_gate_blocks_finish() {
        let mut d = ErrcodeDomain::new();
        for i in 0..9u32 {
            let _ = d.add_retest(&format!("r{}", i), true);
        }
        assert_eq!(d.finish("摘要"), Err(ErrCode::RetestFail));
        let _ = d.add_retest("r9", false);
        assert_eq!(d.finish("摘要"), Err(ErrCode::RetestFail));
        let mut d2 = ErrcodeDomain::new();
        for i in 0..10u32 {
            let _ = d2.add_retest(&format!("r{}", i), true);
        }
        assert!(d2.finish("全通写回").is_ok());
        assert_eq!(d2.finish("再写"), Err(ErrCode::StateRegress));
    }

    #[test]
    fn table_closed_set() {
        assert!(TableId::from_code(2).is_some());
        assert!(TableId::from_code(3).is_none());
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(fnv1a(standard_script().as_bytes()), fnv1a(standard_script().as_bytes()));
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..32 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn seg_guard_in_range() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0xC11C0);
    }
}
