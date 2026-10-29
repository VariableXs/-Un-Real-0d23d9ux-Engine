//! UNX-F11297 排疑案例库与诊断联动（0x1f6c0 段）
//!
//! 锚点：UNX-F11297-J1 八条排疑案例四要素齐备，诊断路径 100% 可复现，
//! 复现签名在重复运行下逐字节稳定。
//!
//! 纯功能模型：每条排疑案例由四要素构成——症状（现场原文）、诊断路径
//! （按序执行的探针步骤）、根因（可执行的结论）、判据 id（回指既有判据）。
//! 诊断器只接受「完整且按序」的路径：少一步、多一步、乱序、重复都会拒止。
//! 走完整路径即得该案例的稳定签名；同一案例跑两遍签名必须相同，这就是
//! 「可复现」的机器判据。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6c0;
/// 案例 id 重复
pub const CS_EDUP: u32 = SEG_BASE + 0x01;
/// 四要素存在空项
pub const CS_EEMPTY: u32 = SEG_BASE + 0x02;
/// 冻结后增删或运行
pub const CS_EFROZEN: u32 = SEG_BASE + 0x03;
/// 诊断路径缺步、越步、乱序或重复
pub const CS_ESTEP: u32 = SEG_BASE + 0x04;
/// 案例不存在
pub const CS_ENOENT: u32 = SEG_BASE + 0x05;
/// 判据 id 不合 "UNX-F…-J…" 形态
pub const CS_ECRIT: u32 = SEG_BASE + 0x06;
/// 两次复现签名漂移
pub const CS_EDRIFT: u32 = SEG_BASE + 0x07;
/// 案例 id 越界（0 或 >99）
pub const CS_EID: u32 = SEG_BASE + 0x08;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let mut k = 0usize;
    let bytes = s.as_bytes();
    while k < bytes.len() {
        if let Some(b) = bytes.get(k) {
            x = fold_byte(x, *b);
        }
        k = k.wrapping_add(1);
    }
    x
}

fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut s = 0u32;
    while s < 32 {
        x = fold_byte(x, ((v >> s) & 0xff) as u8);
        s = s.wrapping_add(8);
    }
    x
}

// ---------------------------------------------------------------- 案例
/// 八类装载期疑难，编号即诊断器内部的稳定分类码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    MissingLib,
    WrongSoname,
    SymbolVersion,
    RelocOverflow,
    InitCrash,
    AuditGap,
    SearchPathShadow,
    TlsMismatch,
}

impl CaseKind {
    pub fn code(&self) -> u32 {
        match self {
            CaseKind::MissingLib => 1,
            CaseKind::WrongSoname => 2,
            CaseKind::SymbolVersion => 3,
            CaseKind::RelocOverflow => 4,
            CaseKind::InitCrash => 5,
            CaseKind::AuditGap => 6,
            CaseKind::SearchPathShadow => 7,
            CaseKind::TlsMismatch => 8,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            CaseKind::MissingLib => "missing-lib",
            CaseKind::WrongSoname => "wrong-soname",
            CaseKind::SymbolVersion => "symbol-version",
            CaseKind::RelocOverflow => "reloc-overflow",
            CaseKind::InitCrash => "init-crash",
            CaseKind::AuditGap => "audit-gap",
            CaseKind::SearchPathShadow => "search-path-shadow",
            CaseKind::TlsMismatch => "tls-mismatch",
        }
    }
}

/// 案例四要素：症状 / 诊断路径 / 根因 / 判据 id。
#[derive(Clone, Copy, Debug)]
pub struct CaseEntry {
    pub id: u32,
    pub kind: CaseKind,
    pub symptom: &'static str,
    pub steps: &'static [&'static str],
    pub root: &'static str,
    pub criterion: &'static str,
}

impl CaseEntry {
    /// 四要素结构性自检（不查重复 id，重复由册判）。
    pub fn field_ok(&self) -> bool {
        !self.symptom.is_empty()
            && !self.steps.is_empty()
            && !self.root.is_empty()
            && !self.criterion.is_empty()
    }

    /// 复现签名：把四要素与步骤逐项折叠，路径不参与（路径由诊断器校验）。
    pub fn digest(&self) -> u64 {
        let mut h = fold_u32(FNV_OFF, self.id);
        h = fold_u32(h, self.kind.code());
        h = fold_str(h, self.symptom);
        let mut k = 0usize;
        while k < self.steps.len() {
            if let Some(step) = self.steps.get(k) {
                h = fold_str(h, step);
                h = fold_byte(h, b'\n');
            }
            k = k.wrapping_add(1);
        }
        h = fold_str(h, self.root);
        fold_str(h, self.criterion)
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

// ---------------------------------------------------------------- 案例册
/// 一次完整诊断的运行记录。
#[derive(Clone, Copy, Debug)]
pub struct RunRecord {
    pub case_id: u32,
    pub digest: u64,
    pub steps: u32,
}

/// 排疑案例册：登记 + 运行 + 覆盖率 + 冻结。
#[derive(Clone, Debug)]
pub struct CaseBook {
    cases: Vec<CaseEntry>,
    runs: Vec<RunRecord>,
    seal: Option<u64>,
    pub attempts: u32,
    pub rejects: u32,
    pub last: Option<u64>,
}

impl CaseBook {
    pub fn empty() -> Self {
        CaseBook {
            cases: Vec::new(),
            runs: Vec::new(),
            seal: None,
            attempts: 0,
            rejects: 0,
            last: None,
        }
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    pub fn is_frozen(&self) -> bool {
        self.seal.is_some()
    }

    pub fn cases(&self) -> &[CaseEntry] {
        self.cases.as_slice()
    }

    pub fn runs(&self) -> &[RunRecord] {
        self.runs.as_slice()
    }

    pub fn find(&self, id: u32) -> Option<&CaseEntry> {
        let mut k = 0usize;
        while k < self.cases.len() {
            if let Some(c) = self.cases.get(k) {
                if c.id == id {
                    return Some(c);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 登记一条案例：id 范围、四要素、判据形态、重复 id 逐关校验。
    pub fn add_case(&mut self, entry: CaseEntry) -> Result<(), u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CS_EFROZEN);
        }
        if entry.id == 0 || entry.id > 99 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CS_EID);
        }
        if !entry.field_ok() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CS_EEMPTY);
        }
        if !entry.criterion.starts_with("UNX-F") || !entry.criterion.contains("-J") {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CS_ECRIT);
        }
        if self.find(entry.id).is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CS_EDUP);
        }
        self.cases.push(entry);
        Ok(())
    }

    /// 某案例的完整按序路径（0..steps）。
    pub fn full_path(&self, id: u32) -> Option<Vec<u32>> {
        let n = match self.find(id) {
            Some(c) => c.steps.len(),
            None => return None,
        };
        let mut path = Vec::new();
        let mut k = 0usize;
        while k < n {
            path.push(k as u32);
            k = k.wrapping_add(1);
        }
        Some(path)
    }

    /// 按路径诊断：路径必须与步骤一一对应且严格升序；成功返回稳定签名。
    pub fn diagnose(&mut self, id: u32, path: &[u32]) -> Result<u64, u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CS_EFROZEN);
        }
        let (digest, len) = match self.find(id) {
            Some(c) => (c.digest(), c.steps.len()),
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(CS_ENOENT);
            }
        };
        if path.len() != len {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(CS_ESTEP);
        }
        let mut k = 0usize;
        while k < len {
            let want = k as u32;
            let got = match path.get(k) {
                Some(v) => *v,
                None => {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(CS_ESTEP);
                }
            };
            if got != want {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(CS_ESTEP);
            }
            k = k.wrapping_add(1);
        }
        self.attempts = self.attempts.wrapping_add(1);
        self.runs.push(RunRecord {
            case_id: id,
            digest,
            steps: len as u32,
        });
        self.last = Some(digest);
        Ok(digest)
    }

    /// 全库巡诊：每条案例走完整路径。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut done = 0u32;
        let mut k = 0usize;
        while k < self.cases.len() {
            let id = match self.cases.get(k) {
                Some(c) => c.id,
                None => break,
            };
            let path = match self.full_path(id) {
                Some(p) => p,
                None => {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(CS_ENOENT);
                }
            };
            self.diagnose(id, path.as_slice())?;
            done = done.wrapping_add(1);
            k = k.wrapping_add(1);
        }
        Ok(done)
    }

    /// 覆盖率（千分比）：已被完整诊断过的案例占比，全库巡诊后应为 1000。
    pub fn coverage_permille(&self) -> u32 {
        if self.cases.is_empty() {
            return 0;
        }
        let mut distinct = 0u32;
        let mut k = 0usize;
        while k < self.runs.len() {
            if let Some(r) = self.runs.get(k) {
                let mut seen = false;
                let mut j = 0usize;
                while j < k {
                    if let Some(prev) = self.runs.get(j) {
                        if prev.case_id == r.case_id {
                            seen = true;
                        }
                    }
                    j = j.wrapping_add(1);
                }
                if !seen {
                    distinct = distinct.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        distinct.saturating_mul(1000) / self.cases.len() as u32
    }

    /// 判据 id 的去重计数（案例库要求回指互不重复）。
    pub fn unique_criteria(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if let Some(c) = self.cases.get(k) {
                let mut seen = false;
                let mut j = 0usize;
                while j < k {
                    if let Some(prev) = self.cases.get(j) {
                        if prev.criterion == c.criterion {
                            seen = true;
                        }
                    }
                    j = j.wrapping_add(1);
                }
                if !seen {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.cases.len() {
            if let Some(c) = self.cases.get(k) {
                h = fold_u32(h, c.id);
                h = fold_u32(h, c.kind.code());
                h = fold_u32(h, c.digest() as u32);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.runs.len() {
            if let Some(r) = self.runs.get(j) {
                h = fold_u32(h, r.case_id);
                h = fold_u32(h, r.steps);
                h = fold_u32(h, r.digest as u32);
            }
            j = j.wrapping_add(1);
        }
        h = fold_u32(h, self.attempts);
        fold_u32(h, self.rejects)
    }

    /// 幂等冻结：案例库与运行账一并定格。
    pub fn freeze(&mut self) -> Option<u64> {
        if let Some(v) = self.seal {
            return Some(v);
        }
        let v = self.fold();
        self.seal = Some(v);
        Some(v)
    }
}

// ---------------------------------------------------------------- 标准案例库
static STEPS_MISSING: [&str; 3] = [
    "ldd ./probe",
    "LD_DEBUG=libs ./probe",
    "ld.so --list ./probe",
];
static STEPS_SONAME: [&str; 3] = [
    "readelf -d ./probe",
    "ldd ./probe",
    "LD_DEBUG=libs ./probe",
];
static STEPS_VERSION: [&str; 3] = [
    "ldd --version",
    "objdump -T libfoo.so",
    "LD_DEBUG=versions ./probe",
];
static STEPS_RELOC: [&str; 3] = [
    "readelf -r ./probe",
    "ld --verbose 对照链接脚本",
    "LD_DEBUG=reloc ./probe",
];
static STEPS_INIT: [&str; 3] = [
    "LD_DEBUG=init ./probe",
    "gdb --args ./probe",
    "ldd ./probe",
];
static STEPS_AUDIT: [&str; 3] = [
    "LD_AUDIT=./audit.so ./probe",
    "LD_DEBUG=audit ./probe",
    "ld.so --list-diagnostics",
];
static STEPS_SHADOW: [&str; 3] = [
    "LD_DEBUG=libs ./probe",
    "ld.so --list ./probe",
    "readelf -d ./probe",
];
static STEPS_TLS: [&str; 3] = [
    "LD_DEBUG=libs ./probe",
    "readelf -l libfoo.so",
    "ldd ./probe",
];

static STANDARD: [CaseEntry; 8] = [
    CaseEntry {
        id: 1,
        kind: CaseKind::MissingLib,
        symptom: "error while loading shared libraries: libssl.so.3: cannot open shared object file",
        steps: &STEPS_MISSING,
        root: "DT_NEEDED 指向的 soname 不在任何搜索目录，需补装或修正搜索路径",
        criterion: "UNX-F11290-J6",
    },
    CaseEntry {
        id: 2,
        kind: CaseKind::WrongSoname,
        symptom: "库文件就在磁盘上，ldd 却报 not found",
        steps: &STEPS_SONAME,
        root: "DT_NEEDED 记录的 soname 与磁盘文件的 SONAME 不一致",
        criterion: "UNX-F11291-J7",
    },
    CaseEntry {
        id: 3,
        kind: CaseKind::SymbolVersion,
        symptom: "version `GLIBC_2.38' not found (required by ./probe)",
        steps: &STEPS_VERSION,
        root: "目标库未导出所需版本节点，或链接时选中了旧版符号",
        criterion: "UNX-F11290-J11",
    },
    CaseEntry {
        id: 4,
        kind: CaseKind::RelocOverflow,
        symptom: "relocation truncated to fit: R_X86_64_PC32 against symbol",
        steps: &STEPS_RELOC,
        root: "非 PIC 目标被链接进高地址共享库，PC 相对位移越界",
        criterion: "UNX-F11293-J11",
    },
    CaseEntry {
        id: 5,
        kind: CaseKind::InitCrash,
        symptom: "Segmentation fault during DT_INIT before main",
        steps: &STEPS_INIT,
        root: "DT_INIT 依赖尚未完成重定位的符号，构造顺序有误",
        criterion: "UNX-F11294-J13",
    },
    CaseEntry {
        id: 6,
        kind: CaseKind::AuditGap,
        symptom: "audit module did not load; LD_AUDIT silently ignored",
        steps: &STEPS_AUDIT,
        root: "审计库 ABI 版本或搜索路径不匹配，装载器放弃加载",
        criterion: "UNX-F11295-J14",
    },
    CaseEntry {
        id: 7,
        kind: CaseKind::SearchPathShadow,
        symptom: "程序实际加载了旧版 libfoo，行为与安装版本不符",
        steps: &STEPS_SHADOW,
        root: "RPATH 优先于 RUNPATH，命中了旧副本",
        criterion: "UNX-F11296-J4",
    },
    CaseEntry {
        id: 8,
        kind: CaseKind::TlsMismatch,
        symptom: "cannot allocate memory in static TLS block",
        steps: &STEPS_TLS,
        root: "静态 TLS 余量不足且库要求过大对齐",
        criterion: "UNX-F11292-J10",
    },
];

/// 标准库：八条案例按 id 升序登记，尚未巡诊。
pub fn build_standard() -> CaseBook {
    let mut book = CaseBook::empty();
    let mut k = 0usize;
    while k < STANDARD.len() {
        if let Some(e) = STANDARD.get(k) {
            let _ = book.add_case(*e);
        }
        k = k.wrapping_add(1);
    }
    book
}

/// 四路对抗：重复 id、空步骤、坏判据、跳步路径。
pub fn inject_four() -> [bool; 4] {
    let mut hits = [false; 4];
    let first = match STANDARD.first() {
        Some(e) => *e,
        None => CaseEntry {
            id: 1,
            kind: CaseKind::MissingLib,
            symptom: "x",
            steps: &STEPS_MISSING,
            root: "x",
            criterion: "UNX-F11297-J1",
        },
    };

    let mut a = CaseBook::empty();
    let _ = a.add_case(first);
    hits[0] = a.add_case(first) == Err(CS_EDUP);

    let mut b = CaseBook::empty();
    let empty = CaseEntry {
        id: 2,
        kind: CaseKind::InitCrash,
        symptom: "x",
        steps: &[],
        root: "x",
        criterion: "UNX-F11297-J2",
    };
    hits[1] = b.add_case(empty) == Err(CS_EEMPTY);

    let mut c = CaseBook::empty();
    let bad = CaseEntry {
        id: 3,
        kind: CaseKind::AuditGap,
        symptom: "x",
        steps: &STEPS_AUDIT,
        root: "x",
        criterion: "F11297-J3",
    };
    hits[2] = c.add_case(bad) == Err(CS_ECRIT);

    let mut d = build_standard();
    hits[3] = d.diagnose(1, &[0u32, 2, 1]) == Err(CS_ESTEP);

    hits
}

// ---------------------------------------------------------------- 判据
pub fn run_unx11297_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11297");
    let mut base = build_standard();
    let ran = base.run_all();

    // J1 八条案例
    let first = base.cases().first();
    let last = base.cases().last();
    s.add(
        "J1-八条案例",
        base.len() == 8
            && first.map(|c| c.id) == Some(1)
            && last.map(|c| c.id) == Some(8)
            && first.map(|c| c.kind.name()) == Some("missing-lib")
            && last.map(|c| c.kind.name()) == Some("tls-mismatch"),
        "案例 1..8 升序，首末分类正确",
    );

    // J2 四要素齐备
    let field_ok = base
        .cases()
        .iter()
        .all(|c| c.field_ok() && c.step_count() >= 3);
    s.add(
        "J2-四要素齐备",
        field_ok,
        "每条症状/三步以上路径/根因/判据均非空",
    );

    // J3 判据去重
    s.add(
        "J3-判据去重",
        base.unique_criteria() == 8,
        "八条案例回指八个不同判据 id",
    );

    // J4 全库巡诊
    s.add(
        "J4-全库巡诊",
        ran == Ok(8)
            && base.attempts == 8
            && base.runs().len() == 8
            && base.coverage_permille() == 1000,
        "8 次诊断全部成功，覆盖率 1000‰",
    );

    // J5 复现签名稳定
    let mut rb = build_standard();
    let mut stable = true;
    let mut k = 0usize;
    while k < 8 {
        let id = (k as u32).wrapping_add(1);
        let path = match rb.full_path(id) {
            Some(p) => p,
            None => {
                stable = false;
                break;
            }
        };
        let one = rb.diagnose(id, path.as_slice());
        let two = rb.diagnose(id, path.as_slice());
        if one.is_err() || one != two {
            stable = false;
        }
        k = k.wrapping_add(1);
    }
    s.add(
        "J5-复现签名稳定",
        stable && rb.attempts == 16 && rb.runs().len() == 16,
        "每条案例跑两遍，十六次签名逐对相同",
    );

    // J6 乱序路径拒
    let mut a = build_standard();
    s.add(
        "J6-乱序路径拒",
        a.diagnose(1, &[1u32, 0, 2]) == Err(CS_ESTEP) && a.runs().is_empty(),
        "首步越到第二步即拒，不产生运行记录",
    );

    // J7 缺步路径拒
    let mut b = build_standard();
    s.add(
        "J7-缺步路径拒",
        b.diagnose(1, &[0u32, 1]) == Err(CS_ESTEP) && b.rejects == 1,
        "少一步即拒，拒绝计数加一",
    );

    // J8 多步路径拒
    let mut c = build_standard();
    s.add(
        "J8-多步路径拒",
        c.diagnose(1, &[0u32, 1, 2, 3]) == Err(CS_ESTEP),
        "路径长于步骤表即拒",
    );

    // J9 未知案例拒
    let mut d = build_standard();
    s.add(
        "J9-未知案例拒",
        d.diagnose(99, &[0u32]) == Err(CS_ENOENT) && d.attempts == 0,
        "未登记案例不进入诊断账",
    );

    // J10 重复 id 拒
    let mut e = CaseBook::empty();
    let dup = match STANDARD.first() {
        Some(c) => *c,
        None => CaseEntry {
            id: 1,
            kind: CaseKind::MissingLib,
            symptom: "x",
            steps: &STEPS_MISSING,
            root: "x",
            criterion: "UNX-F11297-J10",
        },
    };
    let _ = e.add_case(dup);
    s.add(
        "J10-重复id拒",
        e.add_case(dup) == Err(CS_EDUP) && e.len() == 1,
        "同 id 二次登记拒，册长不变",
    );

    // J11 空要素拒
    let mut f = CaseBook::empty();
    let no_steps = CaseEntry {
        id: 11,
        kind: CaseKind::MissingLib,
        symptom: "x",
        steps: &[],
        root: "x",
        criterion: "UNX-F11297-J11",
    };
    let no_symptom = CaseEntry {
        id: 12,
        kind: CaseKind::WrongSoname,
        symptom: "",
        steps: &STEPS_SONAME,
        root: "x",
        criterion: "UNX-F11297-J12",
    };
    s.add(
        "J11-空要素拒",
        f.add_case(no_steps) == Err(CS_EEMPTY)
            && f.add_case(no_symptom) == Err(CS_EEMPTY)
            && f.len() == 0,
        "空步骤/空症状均拒",
    );

    // J12 坏判据拒
    let mut g = CaseBook::empty();
    let bad_crit = CaseEntry {
        id: 13,
        kind: CaseKind::AuditGap,
        symptom: "x",
        steps: &STEPS_AUDIT,
        root: "x",
        criterion: "F11297-J13",
    };
    s.add(
        "J12-坏判据拒",
        g.add_case(bad_crit) == Err(CS_ECRIT) && g.len() == 0,
        "判据不以 UNX-F 开头即拒",
    );

    // J13 id 越界拒
    let mut h = CaseBook::empty();
    let zero = CaseEntry {
        id: 0,
        kind: CaseKind::RelocOverflow,
        symptom: "x",
        steps: &STEPS_RELOC,
        root: "x",
        criterion: "UNX-F11297-J13",
    };
    let big = CaseEntry {
        id: 100,
        kind: CaseKind::RelocOverflow,
        symptom: "x",
        steps: &STEPS_RELOC,
        root: "x",
        criterion: "UNX-F11297-J13",
    };
    s.add(
        "J13-id越界拒",
        h.add_case(zero) == Err(CS_EID) && h.add_case(big) == Err(CS_EID) && h.len() == 0,
        "id 0 与 id 100 均拒",
    );

    // J14 冻结拒变更
    let seal = base.freeze();
    let before = base.rejects;
    let late_case = CaseEntry {
        id: 20,
        kind: CaseKind::TlsMismatch,
        symptom: "x",
        steps: &STEPS_TLS,
        root: "x",
        criterion: "UNX-F11297-J14",
    };
    let late_add = base.add_case(late_case);
    let late_run = match base.full_path(1) {
        Some(p) => base.diagnose(1, p.as_slice()),
        None => Ok(0),
    };
    s.add(
        "J14-冻结拒变更",
        seal.is_some()
            && late_add == Err(CS_EFROZEN)
            && late_run == Err(CS_EFROZEN)
            && base.rejects == before.wrapping_add(2)
            && base.runs().len() == 8,
        "冻结后登记与诊断均拒，运行账不变",
    );

    // J15 冻结幂等
    let again = base.freeze();
    s.add(
        "J15-冻结幂等",
        again.is_some() && again == seal && base.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J16 注入四拒
    let hits = inject_four();
    s.add(
        "J16-注入四拒",
        hits.iter().all(|h| *h),
        "重复id/空步骤/坏判据/跳步四路全拒",
    );

    // J17 案例明细
    let last_digest = match base.last {
        Some(v) => v,
        None => 0,
    };
    s.add_dyn(
        "J17-案例明细",
        last_digest != 0 && base.coverage_permille() == 1000,
        &format!(
            "cases={} runs={} cov={}‰ att={} rej={} last={:x}",
            base.len(),
            base.runs().len(),
            base.coverage_permille(),
            base.attempts,
            base.rejects,
            last_digest
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11297_case_flow() {
        let mut book = build_standard();
        assert_eq!(book.len(), 8);
        assert_eq!(book.unique_criteria(), 8);
        assert_eq!(book.run_all(), Ok(8));
        assert_eq!(book.attempts, 8);
        assert_eq!(book.coverage_permille(), 1000);

        let first = match book.cases().first() {
            Some(c) => *c,
            None => return,
        };
        assert_eq!(first.id, 1);
        assert_eq!(first.kind, CaseKind::MissingLib);
        assert_eq!(first.step_count(), 3);
        assert!(first.field_ok());
        assert_ne!(first.digest(), 0);
        assert_eq!(first.digest(), first.digest());

        let path = match book.full_path(3) {
            Some(p) => p,
            None => return,
        };
        let one = book.diagnose(3, path.as_slice());
        let two = book.diagnose(3, path.as_slice());
        assert!(one.is_ok());
        assert_eq!(one, two);

        assert_eq!(book.diagnose(3, &[1u32, 0, 2]), Err(CS_ESTEP));
        assert_eq!(book.diagnose(42, &[0u32]), Err(CS_ENOENT));

        let seal = book.freeze();
        assert!(seal.is_some());
        assert_eq!(book.freeze(), seal);
        assert_eq!(book.run_all(), Err(CS_EFROZEN));

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11297_checks_green() {
        let s = run_unx11297_checks();
        assert!(s.all_passed(), "UNX-F11297 判据未全绿");
    }
}
