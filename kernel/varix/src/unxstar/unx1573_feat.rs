//! UNX-F1573 移交包安全审查（无敏感信息/无越权面） · 0x78E40段
//!
//! 判据原文（UNX-F1573-J1 摘录）：「包安全审查通过（五件逐件扫敏感信息与
//! 越权面：密钥/口令/未授权写路径三项零命中），误报 0，审查报告落档」；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 五件逐件扫：移交包五件（总表快照/联签件/终验票/移交清单/导览页）各自
//!    独立过扫描器，缺件即拒，任一件命中即整包不可移交；
//! 2. 三类敏感面闭集：密钥（key=/AKIA/BEGIN PRIVATE）、口令（pass=/pwd=/
//!    口令:）、未授权写路径（写: 前缀不在白名单 out/ 内），规则表冻结；
//! 3. 误报 0：白名单语料（含形近但无害的字样如 keymap、写:out/）必须零命中
//!    ——扫描器不是「见词就抓」，规则边界用白语料钉死；
//! 4. 注入 10/10：十路敏感样本（三类各路）必须全数命中，防止扫描器退化成
//!    恒绿；每类至少两路真阳性锁住规则活性；
//! 5. 审查报告落档：逐件一行 + 命中清单 + 折叠根，有命中禁止封档；状态机
//!    扫描中→已审查→已封档 单向，倒退显性拒；诊断码闭集 0x78E41..0x78E4C。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B07 批窗口 0x78E00..=0x78FC0）。
pub const SEG_BASE: u32 = 0x78E40;

/// 移交包件数（五件）。
pub const ARTIFACT_COUNT: usize = 5;

/// 注入样本路数（10/10 必抓）。
pub const INJECT_ROUTES: usize = 10;

/// 授权写路径白名单前缀（越权面判定的唯一基准）。
pub const WRITE_ALLOW_PREFIX: &str = "写:out/";

/// 移交包安全审查诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 件号越界或重复。
    EAid,
    /// 缺件（五件未齐）。
    ELack,
    /// 敏感命中：密钥。
    EKey,
    /// 敏感命中：口令。
    EPass,
    /// 敏感命中：未授权写路径。
    EWrite,
    /// 有命中禁止封档。
    ERedBlock,
    /// 包已封档。
    ESealed,
    /// 重复发起审查。
    EDupScan,
    /// 空包样本。
    EEmpty,
    /// 审查报告缺失。
    ENoReport,
    /// 报告重放不符。
    EReplay,
    /// 相位倒退。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EAid => 0x01,
                Diag::ELack => 0x02,
                Diag::EKey => 0x03,
                Diag::EPass => 0x04,
                Diag::EWrite => 0x05,
                Diag::ERedBlock => 0x06,
                Diag::ESealed => 0x07,
                Diag::EDupScan => 0x08,
                Diag::EEmpty => 0x09,
                Diag::ENoReport => 0x0a,
                Diag::EReplay => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EAid => "件号越界或重复：五件按 1..5 逐件登记，每件只登一次",
            Diag::ELack => "缺件：五件未齐先补件，再发起安全审查",
            Diag::EKey => "密钥命中：删除或替换为占位符，密钥不入移交包",
            Diag::EPass => "口令命中：删除或改用引用名，口令不入移交包",
            Diag::EWrite => "未授权写路径命中：改写到 out/ 白名单路径内",
            Diag::ERedBlock => "有命中禁止封档：先清敏感命中再封档移交",
            Diag::ESealed => "包已封档：封档后写入拒绝，改动走下一批移交包",
            Diag::EDupScan => "审查已发起：重复发起拒绝，先取回上一轮报告",
            Diag::EEmpty => "空包样本：无件不构成审查，先补齐五件",
            Diag::ENoReport => "审查报告缺失：封档前必须生成报告行",
            Diag::EReplay => "报告重放不符：封档后报告被改动，须复核",
            Diag::EPhase => "相位倒退：扫描中→已审查→已封档 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EAid,
    Diag::ELack,
    Diag::EKey,
    Diag::EPass,
    Diag::EWrite,
    Diag::ERedBlock,
    Diag::ESealed,
    Diag::EDupScan,
    Diag::EEmpty,
    Diag::ENoReport,
    Diag::EReplay,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78E41..0x78E4C 闭集内"
}

/// 敏感面三类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitClass {
    /// 密钥面。
    Key,
    /// 口令面。
    Pass,
    /// 未授权写路径面。
    Write,
}

impl HitClass {
    /// 类名（报告行用）。
    pub fn name(self) -> &'static str {
        match self {
            HitClass::Key => "密钥",
            HitClass::Pass => "口令",
            HitClass::Write => "未授权写路径",
        }
    }

    /// 对应诊断码。
    pub fn diag(self) -> Diag {
        match self {
            HitClass::Key => Diag::EKey,
            HitClass::Pass => Diag::EPass,
            HitClass::Write => Diag::EWrite,
        }
    }
}

/// 三类闭集数组。
pub const HIT_ALL: &[HitClass] = &[HitClass::Key, HitClass::Pass, HitClass::Write];

/// 单条命中：件号 + 行号 + 类别 + 行摘录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 件号（1..=5）。
    pub aid: u16,
    /// 行号（1 起）。
    pub line_no: u16,
    /// 敏感类别。
    pub class: HitClass,
    /// 行摘录（截前 24 字节）。
    pub excerpt: String,
}

/// 行级判定（扫描器核心，规则表冻结）：
/// 返回 Some(类别) 表示命中敏感面。
pub fn classify_line(line: &str) -> Option<HitClass> {
    // 密钥面：三种冻结模式。
    if line.contains("key=") || line.contains("AKIA") || line.contains("BEGIN PRIVATE") {
        return Some(HitClass::Key);
    }
    // 口令面：三种冻结模式。
    if line.contains("pass=") || line.contains("pwd=") || line.contains("口令:") {
        return Some(HitClass::Pass);
    }
    // 未授权写路径面：出现「写:」且不在白名单前缀内。
    if line.contains("写:") && !line.contains(WRITE_ALLOW_PREFIX) {
        return Some(HitClass::Write);
    }
    None
}

/// 逐行扫描一件正文（行号从 1 起，行按 \n 切）。
pub fn scan_body(aid: u16, body: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut no = 1u16;
    for line in body.split('\n') {
        if let Some(class) = classify_line(line) {
            let mut ex = String::new();
            let mut k = 0usize;
            for ch in line.chars() {
                if k >= 24 {
                    break;
                }
                ex.push(ch);
                k += 1;
            }
            hits.push(Hit { aid, line_no: no, class, excerpt: ex });
        }
        no += 1;
    }
    hits
}

/// 移交包件。
#[derive(Clone, Debug)]
pub struct Artifact {
    /// 件号（1..=5）。
    pub aid: u16,
    /// 件名。
    pub name: String,
    /// 件正文（\n 分行）。
    pub body: String,
}

/// 审查相位（扫描中→已审查→已封档 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 扫描中：五件入包中。
    Scanning,
    /// 已审查：扫描已发起且报告已生成。
    Audited,
    /// 已封档：报告冻结、写入拒绝。
    Sealed,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Scanning => 0,
            Phase::Audited => 1,
            Phase::Sealed => 2,
        }
    }
}

/// 审查报告：逐件行 + 命中清单 + 折叠根。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditReport {
    /// 逐件报告行。
    lines: Vec<String>,
    /// 命中清单。
    hits: Vec<Hit>,
    /// 折叠根（FNV-1a）。
    root: u64,
}

impl AuditReport {
    /// 报告行数。
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// 取第 k 行。
    pub fn line_at(&self, k: usize) -> Option<&str> {
        self.lines.get(k).map(|s| s.as_str())
    }

    /// 命中总数。
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// 第 k 条命中。
    pub fn hit_at(&self, k: usize) -> Option<&Hit> {
        self.hits.get(k)
    }

    /// 折叠根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 全文落档面。
    pub fn text(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            out.push_str(l);
            out.push('\n');
        }
        out
    }
}

/// FNV-1a 折叠（整数哈希）。
fn fold_word(mut h: u64, w: u64) -> u64 {
    let mut k = w;
    let mut i = 0;
    while i < 8 {
        let b = (k & 0xFF) as u8;
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001B3);
        k >>= 8;
        i += 1;
    }
    h
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x100000001B3);
    }
    acc
}

/// 移交包安全审查账本：五件入包 + 扫描 + 报告封档。
#[derive(Clone, Debug)]
pub struct PackAuditor {
    phase: Phase,
    /// 五件（按件号槽位）。
    arts: Vec<Option<Artifact>>,
    /// 审查报告。
    report: Option<AuditReport>,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl PackAuditor {
    /// 新审查包。
    pub fn new() -> PackAuditor {
        PackAuditor {
            phase: Phase::Scanning,
            arts: (0..ARTIFACT_COUNT).map(|_| None).collect(),
            report: None,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已入包件数。
    pub fn packed(&self) -> usize {
        self.arts.iter().filter(|x| x.is_some()).count()
    }

    /// 入一件（件号 1..=5，件名与正文非空）。
    pub fn put(&mut self, art: Artifact) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if art.aid == 0 || (art.aid as usize) > ARTIFACT_COUNT {
            self.rejects += 1;
            return Err(Diag::EAid.val());
        }
        if art.name.is_empty() || art.body.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        let idx = (art.aid - 1) as usize;
        match self.arts.get_mut(idx) {
            Some(slot) => {
                *slot = Some(art);
                Ok(())
            }
            None => Err(Diag::EAid.val()),
        }
    }

    /// 取第 aid 件。
    pub fn artifact_at(&self, aid: u16) -> Option<&Artifact> {
        if aid == 0 || (aid as usize) > ARTIFACT_COUNT {
            return None;
        }
        match self.arts.get((aid - 1) as usize) {
            Some(Some(a)) => Some(a),
            _ => None,
        }
    }

    /// 发起审查：五件必须齐，逐件扫描生成报告。
    pub fn audit(&mut self) -> Result<AuditReport, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.phase == Phase::Audited {
            return Err(Diag::EDupScan.val());
        }
        if self.packed() < ARTIFACT_COUNT {
            return Err(Diag::ELack.val());
        }
        let mut lines = Vec::new();
        let mut hits = Vec::new();
        let mut h: u64 = 0xCBF29CE484222325;
        let mut aid = 1u16;
        while aid <= ARTIFACT_COUNT as u16 {
            let a = match self.artifact_at(aid) {
                Some(x) => x,
                None => return Err(Diag::ELack.val()),
            };
            let body_hits = scan_body(aid, &a.body);
            let line = if body_hits.is_empty() {
                format!("件{} {} 零命中", aid, a.name)
            } else {
                format!("件{} {} 命中{}条", aid, a.name, body_hits.len())
            };
            for x in &body_hits {
                h = fold_word(h, x.aid as u64);
                h = fold_word(h, x.line_no as u64);
                h = fold_word(h, x.class.name().len() as u64);
                h = fold_str(h, &x.excerpt);
            }
            h = fold_str(h, &line);
            hits.extend(body_hits);
            lines.push(line);
            aid += 1;
        }
        let rep = AuditReport { lines, hits, root: h };
        self.report = Some(rep.clone());
        self.phase = Phase::Audited;
        Ok(rep)
    }

    /// 已生成报告的只读视图。
    pub fn report(&self) -> Option<&AuditReport> {
        self.report.as_ref()
    }

    /// 封档：零命中才放行，报告必须存在且根非零。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.phase != Phase::Audited {
            return Err(Diag::EPhase.val());
        }
        let rep = match &self.report {
            Some(r) => r,
            None => return Err(Diag::ENoReport.val()),
        };
        // 三类敏感面任一命中即封档拒绝（判据：三项零命中）。
        for x in &rep.hits {
            return Err(x.class.diag().val());
        }
        if rep.hit_count() == 0 && rep.root() == 0 {
            return Err(Diag::ENoReport.val());
        }
        self.phase = Phase::Sealed;
        Ok(rep.root())
    }

    /// 封档后重放：重扫五件复算命中与报告行根，逐字一致才算通过。
    pub fn replay(&self) -> Result<u32, u32> {
        if self.phase != Phase::Sealed {
            return Err(Diag::EPhase.val());
        }
        let rep = match &self.report {
            Some(r) => r,
            None => return Err(Diag::ENoReport.val()),
        };
        let mut h: u64 = 0xCBF29CE484222325;
        let mut aid = 1u16;
        while aid <= ARTIFACT_COUNT as u16 {
            let a = match self.artifact_at(aid) {
                Some(x) => x,
                None => return Err(Diag::ELack.val()),
            };
            let body_hits = scan_body(aid, &a.body);
            for x in &body_hits {
                h = fold_word(h, x.aid as u64);
                h = fold_word(h, x.line_no as u64);
                h = fold_word(h, x.class.name().len() as u64);
                h = fold_str(h, &x.excerpt);
            }
            // 报告行与 audit 同序折叠（命中后折行）。
            let line = if body_hits.is_empty() {
                format!("件{} {} 零命中", aid, a.name)
            } else {
                format!("件{} {} 命中{}条", aid, a.name, body_hits.len())
            };
            h = fold_str(h, &line);
            aid += 1;
        }
        if h != rep.root() {
            return Err(Diag::EReplay.val());
        }
        Ok(rep.line_count() as u32)
    }
}

/// 白名单五件（零命中的标准包：正文全部为无害语料）。
pub fn build_clean_pack() -> PackAuditor {
    let mut p = PackAuditor::new();
    let bodies: [&str; ARTIFACT_COUNT] = [
        "总表快照\n十域状态齐\n无异常行",
        "联签件\n四方签字已齐\n结论通过",
        "终验票\n逐项验收通过\n票根在册",
        "移交清单\n五件齐\n落档完成",
        "导览页\n五件导读齐\n随包冻结",
    ];
    let names: [&str; ARTIFACT_COUNT] = ["总表快照", "联签件", "终验票", "移交清单", "导览页"];
    let mut aid = 1u16;
    while aid <= ARTIFACT_COUNT as u16 {
        let idx = (aid - 1) as usize;
        let name = match names.get(idx) {
            Some(x) => (*x).to_string(),
            None => break,
        };
        let body = match bodies.get(idx) {
            Some(x) => (*x).to_string(),
            None => break,
        };
        let _ = p.put(Artifact { aid, name, body });
        aid += 1;
    }
    p
}

/// 注入样本 10/10：十路敏感样本（密钥 4 路/口令 3 路/越权写 3 路），
/// 每路必须命中且类别正确。返回十布尔（true=该路被抓且类别正确）。
pub fn inject_matrix() -> [bool; INJECT_ROUTES] {
    // (期望类别, 注入行)。
    let routes: [(HitClass, &str); INJECT_ROUTES] = [
        (HitClass::Key, "api key=AKIA12345678"),
        (HitClass::Key, "-----BEGIN PRIVATE KEY-----"),
        (HitClass::Key, "sign key=deadbeef"),
        (HitClass::Key, "AKIAIOSFODNN7 in text"),
        (HitClass::Pass, "db pass=hunter2"),
        (HitClass::Pass, "root pwd=letmein"),
        (HitClass::Pass, "口令:变更需审批"),
        (HitClass::Write, "写:/etc/kernel/mod.rs 追加"),
        (HitClass::Write, "写:taskboard.md 直接改"),
        (HitClass::Write, "写:../outside 越界"),
    ];
    let mut out = [false; INJECT_ROUTES];
    let mut i = 0usize;
    while i < INJECT_ROUTES {
        let (want, line) = match routes.get(i) {
            Some(x) => *x,
            None => break,
        };
        // 判定一：行级扫描必须命中且类别正确。
        let caught = classify_line(line) == Some(want);
        // 判定二：注入进白包后审查报告必须点到该类（整包联动）。
        let mut p = build_clean_pack();
        let _ = p.put(Artifact {
            aid: 1,
            name: "总表快照".to_string(),
            body: format!("总表快照\n{}\n尾行", line),
        });
        let rep = p.audit();
        let pack_caught = match &rep {
            Ok(r) => r.hit_count() > 0 && r.hit_at(0).map(|x| x.class) == Some(want),
            Err(_) => false,
        };
        // 判定三：有命中封档必须被拒。
        let seal_blocked = match p.seal() {
            Err(c) => c == want.diag().val(),
            Ok(_) => false,
        };
        let cell = match out.get_mut(i) {
            Some(x) => x,
            None => break,
        };
        *cell = caught && pack_caught && seal_blocked;
        i += 1;
    }
    out
}

/// 白名单语料（形近无害样）——误报 0 的活性钉：这些行必须零命中。
pub const BENIGN_LINES: [&str; 6] = [
    "keymap 布局已冻结",
    "密码学模块仅引用名",
    "写:out/移交包/五件落档",
    "写:out/对账报告落档",
    "口令规范引用见手册",
    "键盘输入路径已裁剪",
];

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B07 派工卡 0x78E00..=0x78FC0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x78E00 || SEG_BASE > 0x78FC0 {
        return false;
    }
    SEG_BASE == 0x78E40
}

/// 三类敏感面闭集。
fn hit_closed_ok() -> bool {
    if HIT_ALL.len() != 3 {
        return false;
    }
    let a = match HIT_ALL.get(0) {
        Some(x) => *x,
        None => return false,
    };
    let b = match HIT_ALL.get(1) {
        Some(x) => *x,
        None => return false,
    };
    let c = match HIT_ALL.get(2) {
        Some(x) => *x,
        None => return false,
    };
    a != b && b != c && a != c
}

/// 白包五件齐备且零命中。
fn clean_pack_ok(p: &mut PackAuditor) -> bool {
    if p.packed() != ARTIFACT_COUNT {
        return false;
    }
    let rep = p.audit();
    match rep {
        Ok(r) => r.hit_count() == 0 && r.line_count() == ARTIFACT_COUNT,
        Err(_) => false,
    }
}

/// 误报 0：白名单语料逐行零命中。
fn false_positive_ok() -> bool {
    for line in BENIGN_LINES {
        if classify_line(line).is_some() {
            return false;
        }
    }
    true
}

/// 注入 10/10 必抓。
fn inject_ok() -> bool {
    inject_matrix() == [true; INJECT_ROUTES]
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_clean_pack();
    let mut b = build_clean_pack();
    let ra = match a.clone().audit() {
        Ok(r) => r.root(),
        Err(_) => return false,
    };
    let rb = match b.audit() {
        Ok(r) => r.root(),
        Err(_) => return false,
    };
    ra == rb && ra != 0
}

/// 十三项判据：UNX-F1573 移交包安全审查。
pub fn run_unx1573_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1573");
    let mut p = build_clean_pack();

    s.add_dyn(
        "F1573-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1573-02 敏感面三类闭集",
        hit_closed_ok(),
        &format!(
            "n={} classes={}/{}/{}",
            HIT_ALL.len(),
            HitClass::Key.name(),
            HitClass::Pass.name(),
            HitClass::Write.name()
        ),
    );

    s.add_dyn(
        "F1573-03 白包五件齐备",
        p.packed() == ARTIFACT_COUNT,
        &format!("packed={}", p.packed()),
    );

    // 结构拒绝：坏件号、空件、缺件审查、空包审查。
    let mut r = PackAuditor::new();
    let bad_aid = r.put(Artifact { aid: 0, name: "x".to_string(), body: "y".to_string() }) == Err(Diag::EAid.val());
    let over_aid = r.put(Artifact { aid: 6, name: "x".to_string(), body: "y".to_string() }) == Err(Diag::EAid.val());
    let empty_art = r.put(Artifact { aid: 1, name: String::new(), body: "y".to_string() }) == Err(Diag::EEmpty.val());
    let lack = r.audit() == Err(Diag::ELack.val());
    s.add_dyn(
        "F1573-04 结构拒绝四门",
        bad_aid && over_aid && empty_art && lack,
        &format!("rejects={}", r.rejects()),
    );

    // 白名单语料误报 0。
    s.add_dyn(
        "F1573-05 白名单语料误报零",
        false_positive_ok(),
        &format!("benign_n={}", BENIGN_LINES.len()),
    );

    // 白包审查：零命中报告生成、重复发起拒绝。
    let rep = p.audit();
    let rep_ok = match &rep {
        Ok(x) => x.hit_count() == 0 && x.line_count() == ARTIFACT_COUNT && x.root() != 0,
        Err(_) => false,
    };
    let dup = p.audit() == Err(Diag::EDupScan.val());
    s.add_dyn(
        "F1573-06 白包审查零命中单向",
        rep_ok && dup && p.phase() == Phase::Audited,
        &format!("phase_ord={}", p.phase().ord()),
    );

    // 注入 10/10。
    s.add_dyn(
        "F1573-07 注入十路全检出",
        inject_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 命中报告行点名件号与类别。
    let mut q = build_clean_pack();
    let _ = q.put(Artifact {
        aid: 3,
        name: "终验票".to_string(),
        body: "终验票\napi key=AKIA999\n票根在册".to_string(),
    });
    let qrep = q.audit();
    let q_ok = match &qrep {
        Ok(x) => {
            x.hit_count() == 1
                && x.hit_at(0).map(|h| h.aid == 3 && h.line_no == 2 && h.class == HitClass::Key).unwrap_or(false)
                && x.line_at(2).map(|t| t.contains("命中1条")).unwrap_or(false)
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F1573-08 命中报告点名定位",
        q_ok,
        &match &qrep {
            Ok(x) => x.line_at(2).map(|t| t.to_string()).unwrap_or_default(),
            Err(c) => detail(*c).to_string(),
        },
    );

    // 封档闸门：有命中拒绝（按类点名）；零命中放行；封档后写入拒绝。
    let q_seal = q.seal();
    let q_blocked = match q_seal {
        Err(c) => c == Diag::EKey.val(),
        Ok(_) => false,
    };
    let seal = p.seal();
    let seal_ok = match seal {
        Ok(root) => root != 0,
        Err(_) => false,
    };
    let write_after = p.put(Artifact { aid: 1, name: "x".to_string(), body: "y".to_string() }) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1573-09 封档闸门三向",
        q_blocked && seal_ok && write_after && p.phase() == Phase::Sealed,
        &format!("q_blocked={} seal_ok={}", q_blocked, seal_ok),
    );

    // 封档重放逐字一致。
    s.add_dyn(
        "F1573-10 封档重放逐字一致",
        p.replay() == Ok(ARTIFACT_COUNT as u32),
        &format!("replay={:?}", p.replay()),
    );

    // 白包报告落档面：全文非空含逐件行。
    let text = match p.report() {
        Some(x) => x.text(),
        None => String::new(),
    };
    s.add_dyn(
        "F1573-11 审查报告落档非空",
        text.contains("件1") && text.contains("件5") && text.contains("零命中"),
        &format!("bytes={}", text.len()),
    );

    // 相位倒退显性拒：已封档后不可再发起审查。
    let back = p.audit() == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1573-12 相位倒退显性拒",
        back && p.phase().ord() == 2,
        &format!("phase_ord={}", p.phase().ord()),
    );

    s.add_dyn(
        "F1573-13 双跑一致与白名单前缀",
        dual_run_ok() && WRITE_ALLOW_PREFIX == "写:out/" && ALL.len() == 12,
        &format!("root={:016x} allow={}", match build_clean_pack().audit() { Ok(r) => r.root(), Err(_) => 0 }, WRITE_ALLOW_PREFIX),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_class_closed() {
        assert_eq!(HIT_ALL.len(), 3);
        assert_ne!(HitClass::Key, HitClass::Pass);
        assert_ne!(HitClass::Pass, HitClass::Write);
        assert_eq!(HitClass::Key.diag(), Diag::EKey);
    }

    #[test]
    fn scanner_catches_three_classes() {
        assert_eq!(classify_line("api key=xx"), Some(HitClass::Key));
        assert_eq!(classify_line("db pass=yy"), Some(HitClass::Pass));
        assert_eq!(classify_line("写:/etc/passwd"), Some(HitClass::Write));
        assert_eq!(classify_line("干净行"), None);
    }

    #[test]
    fn allowed_write_path_not_flagged() {
        // 白名单前缀内的写路径不是越权面。
        assert_eq!(classify_line("写:out/移交包/五件落档"), None);
        assert_eq!(classify_line(WRITE_ALLOW_PREFIX), None);
    }

    #[test]
    fn benign_zero_false_positive() {
        for line in BENIGN_LINES {
            assert_eq!(classify_line(line), None, "误报：{}", line);
        }
        assert!(false_positive_ok());
    }

    #[test]
    fn scan_body_reports_line_numbers() {
        let hits = scan_body(2, "干净\npass=x\n干净\n写:/tmp/x");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits.get(0).map(|h| (h.line_no, h.class)), Some((2, HitClass::Pass)));
        assert_eq!(hits.get(1).map(|h| (h.line_no, h.class)), Some((4, HitClass::Write)));
    }

    #[test]
    fn pack_gates() {
        let mut p = PackAuditor::new();
        assert_eq!(p.audit(), Err(Diag::ELack.val()));
        assert_eq!(p.put(Artifact { aid: 9, name: "x".to_string(), body: "y".to_string() }), Err(Diag::EAid.val()));
        assert_eq!(p.put(Artifact { aid: 1, name: String::new(), body: "y".to_string() }), Err(Diag::EEmpty.val()));
        assert_eq!(p.rejects(), 2);
    }

    #[test]
    fn clean_pack_audits_and_seals() {
        let mut p = build_clean_pack();
        assert_eq!(p.packed(), ARTIFACT_COUNT);
        let rep = p.audit().expect("白包审查通过");
        assert_eq!(rep.hit_count(), 0);
        assert_eq!(rep.line_count(), ARTIFACT_COUNT);
        assert_eq!(p.audit(), Err(Diag::EDupScan.val()));
        let root = p.seal().expect("零命中封档");
        assert_ne!(root, 0);
        assert_eq!(p.phase(), Phase::Sealed);
        assert_eq!(p.replay(), Ok(ARTIFACT_COUNT as u32));
    }

    #[test]
    fn injected_pack_blocked_and_named() {
        let mut p = build_clean_pack();
        let _ = p.put(Artifact {
            aid: 3,
            name: "终验票".to_string(),
            body: "终验票\npwd=root\n尾行".to_string(),
        });
        let rep = p.audit().expect("审查可发起");
        assert_eq!(rep.hit_count(), 1);
        assert_eq!(rep.hit_at(0).map(|h| h.class), Some(HitClass::Pass));
        assert_eq!(p.seal(), Err(Diag::EPass.val()));
        assert_eq!(p.phase(), Phase::Audited);
    }

    #[test]
    fn inject_ten_of_ten() {
        let m = inject_matrix();
        assert_eq!(m, [true; INJECT_ROUTES]);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EAid.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x30), "未知诊断码：不在 0x78E41..0x78E4C 闭集内");
    }

    #[test]
    fn checks_all_green_thirteen() {
        let s = run_unx1573_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
