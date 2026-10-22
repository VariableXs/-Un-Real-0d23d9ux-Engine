//! UNX-F21593 · 证据三件套归档（脚本/报告/哈希）
//!
//! 诊断段 0x91061~0x91068（F2-B40 域收官段 13/100）。
//!
//! 判据原文：三件套（校验器 exit=0 / 聚合报告 / 文件哈希）归档路径落账。
//!
//! 设计要点（为什么缺件要拆成三个码，而不是一个「不完整」）：
//! 「不完整」对施工员没有信息量——他需要知道该去补哪一件。三件套的补齐动作
//! 完全不同（重跑校验器 / 重生成报告 / 重算哈希），故缺脚本、缺报告、缺哈希
//! 各占一码，且校验顺序固定为「脚本 → 报告 → 哈希」：脚本是生成另外两件的
//! 源头，没有脚本时讨论报告条目数没有意义。
//!
//! 第二个取舍是**交叉约束**：哈希覆盖文件数与报告条目数必须相等**且为正**。
//! 单看任何一件都自洽，只有交叉才对得上——这正是三件套存在的意义：三件里
//! 任意一件都可以是「事后补写」的，但两件独立生成的产物在数量上对不上时，
//! 至少有一件是假的。零条目同样判死：一份 0 条的报告配 0 覆盖的哈希，数量上
//! 相等却什么也没证明。
//!
//! 指纹只覆盖内容、不覆盖归档路径：同一份证据换个目录归档仍是同一份证据，
//! 把它算进指纹会让「移动目录」被误判成「证据被篡改」。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 三件套件数（脚本 / 报告 / 哈希）。
pub const TRIO_PARTS: u8 = 3;

/// 归档诊断码闭集（0x91061~0x91068 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrioCode {
    /// 0x91061 三件齐全且自洽。
    TrioOk = 0x91061,
    /// 0x91062 缺校验脚本。
    MissingScript = 0x91062,
    /// 0x91063 缺聚合报告。
    MissingReport = 0x91063,
    /// 0x91064 缺文件哈希（含摘要或覆盖为零）。
    MissingHash = 0x91064,
    /// 0x91065 脚本退出码非 0。
    ExitNonZero = 0x91065,
    /// 0x91066 报告条目数与哈希覆盖数不一致或非正。
    CountMismatch = 0x91066,
    /// 0x91067 归档路径已被占用。
    PathTaken = 0x91067,
    /// 0x91068 证据链指纹对不上。
    ChainBroken = 0x91068,
}

impl TrioCode {
    /// 诊断三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            TrioCode::TrioOk => "证据三件套齐全自洽",
            TrioCode::MissingScript => "缺校验脚本",
            TrioCode::MissingReport => "缺聚合报告",
            TrioCode::MissingHash => "缺文件哈希或摘要/覆盖为零",
            TrioCode::ExitNonZero => "校验脚本退出码非 0",
            TrioCode::CountMismatch => "报告条目数与哈希覆盖数不一致或非正",
            TrioCode::PathTaken => "归档路径已被占用",
            TrioCode::ChainBroken => "证据链指纹不匹配",
        }
    }
    /// 诊断三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            TrioCode::TrioOk => "三件齐、退出码 0、条目数与覆盖数相等且为正",
            TrioCode::MissingScript => "校验器未运行，或脚本本体未随证据归档",
            TrioCode::MissingReport => "报告未聚合，或聚合失败后只剩原始日志",
            TrioCode::MissingHash => "哈希未计算，或计算失败后留了全零摘要",
            TrioCode::ExitNonZero => "校验器跑了但判失败，结论不可采信",
            TrioCode::CountMismatch => "报告与哈希不是同一次运行产出（或两者都为空）",
            TrioCode::PathTaken => "同一路径被两份证据声明占用",
            TrioCode::ChainBroken => "归档后内容被改动，或指纹计算口径不一致",
        }
    }
    /// 诊断三要素之三：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TrioCode::TrioOk => "落账归档并导出清单",
            TrioCode::MissingScript => "重跑校验器并把脚本本体一起归档",
            TrioCode::MissingReport => "重跑聚合器，确认报告落盘后再归档",
            TrioCode::MissingHash => "对报告所列文件重算哈希",
            TrioCode::ExitNonZero => "先修校验失败项，再重新出证据",
            TrioCode::CountMismatch => "以同一次运行的报告与哈希重出证据",
            TrioCode::PathTaken => "换归档路径，或先撤掉旧证据",
            TrioCode::ChainBroken => "以归档件重算指纹，核对是否被改动",
        }
    }
    /// 数值形式（u32：五位数码段）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 归档故障。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrioFault {
    /// 诊断码。
    pub code: u32,
    /// 定位（归档序号，0 = 与具体位置无关）。
    pub at: u16,
}

impl TrioFault {
    const fn new(code: TrioCode, at: u16) -> TrioFault {
        TrioFault { code: code.code(), at }
    }
    /// 码是否落在本单码段内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91061 && self.code <= 0x91068
    }
}

/// 报告结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 不结论（数据不足）。
    Inconclusive,
}

impl Verdict {
    const fn rank(self) -> u8 {
        match self { Verdict::Pass => 0, Verdict::Fail => 1, Verdict::Inconclusive => 2 }
    }
    /// 清单用短名。
    pub const fn label(self) -> &'static str {
        match self { Verdict::Pass => "pass", Verdict::Fail => "fail", Verdict::Inconclusive => "inconclusive" }
    }
}

/// 哈希算法（本域只用这两种：内核内 FNV 与外部 sha256 截断）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HashAlgo {
    /// FNV-1a 64。
    Fnv1a64,
    /// SHA-256 取前 8 字节。
    Sha256Trunc,
}

impl HashAlgo {
    const fn rank(self) -> u8 {
        match self { HashAlgo::Fnv1a64 => 0, HashAlgo::Sha256Trunc => 1 }
    }
    /// 清单用短名。
    pub const fn label(self) -> &'static str {
        match self { HashAlgo::Fnv1a64 => "fnv1a64", HashAlgo::Sha256Trunc => "sha256t" }
    }
}

/// 第一件：校验脚本（路径 + 退出码 + 参数指纹）。
///
/// 参数指纹由实参逐个喂 FNV 得到，而不是手填：手填的值一旦与真实命令行脱钩，
/// 「这份证据是跑什么得到的」就无法复现，而三件套的全部价值正是可复现。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScriptPart {
    /// 脚本归档路径。
    pub path: String,
    /// 退出码（0 = 通过）。
    pub exit_code: i32,
    /// 参数指纹。
    pub arg_fingerprint: u64,
}

impl ScriptPart {
    /// 构造（参数按序喂 FNV-1a，含分隔符 0x1F 以防 `["ab","c"]` 与 `["a","bc"]` 同指）。
    pub fn new(path: &str, exit_code: i32, args: &[&str]) -> ScriptPart {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for a in args.iter() {
            for b in a.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h ^= 0x1F;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        ScriptPart { path: path.to_string(), exit_code, arg_fingerprint: h }
    }
    /// 是否通过（退出码 0）。
    pub const fn passed(&self) -> bool {
        self.exit_code == 0
    }
}

/// 第二件：聚合报告（路径 + 条目数 + 结论）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReportPart {
    /// 报告归档路径。
    pub path: String,
    /// 条目数（报告里逐条列出的检查行数）。
    pub entries: u32,
    /// 结论。
    pub verdict: Verdict,
}

impl ReportPart {
    /// 构造（不在构造期校验条目数：零条目要能构造出来，才能被判据抓到）。
    pub fn new(path: &str, entries: u32, verdict: Verdict) -> ReportPart {
        ReportPart { path: path.to_string(), entries, verdict }
    }
}

/// 第三件：文件哈希（算法 + 摘要 + 覆盖文件数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HashPart {
    /// 算法。
    pub algo: HashAlgo,
    /// 摘要（0 = 未计算）。
    pub digest: u64,
    /// 覆盖文件数。
    pub covered_files: u32,
}

impl HashPart {
    /// 构造。
    pub const fn new(algo: HashAlgo, digest: u64, covered_files: u32) -> HashPart {
        HashPart { algo, digest, covered_files }
    }
    /// 摘要是否有效（全零＝没算）。
    pub const fn computed(&self) -> bool {
        self.digest != 0 && self.covered_files > 0
    }
}

/// 一份证据（三件套，缺件用 None 表示，缺件本身要被判据抓到）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvidenceTrio {
    /// 归档路径（唯一键）。
    pub archive_path: String,
    /// 第一件。
    pub script: Option<ScriptPart>,
    /// 第二件。
    pub report: Option<ReportPart>,
    /// 第三件。
    pub hash: Option<HashPart>,
}

impl EvidenceTrio {
    /// 从归档路径起建（三件后续挂载）。
    pub fn new(archive_path: &str) -> EvidenceTrio {
        EvidenceTrio { archive_path: archive_path.to_string(), script: None, report: None, hash: None }
    }
    /// 挂脚本。
    pub fn with_script(mut self, s: ScriptPart) -> EvidenceTrio {
        self.script = Some(s);
        self
    }
    /// 挂报告。
    pub fn with_report(mut self, r: ReportPart) -> EvidenceTrio {
        self.report = Some(r);
        self
    }
    /// 挂哈希。
    pub fn with_hash(mut self, h: HashPart) -> EvidenceTrio {
        self.hash = Some(h);
        self
    }

    /// 齐备性 + 语义 + 交叉约束，一次走完。
    ///
    /// 顺序：缺件（结构）→ 退出码（语义）→ 摘要有效（语义）→ 数量交叉。
    /// 结构永远先于语义：缺脚本时「退出码是多少」这个问题本身不成立。
    pub fn validate(&self) -> Result<(), TrioFault> {
        let s = match self.script.as_ref() {
            Some(s) => s,
            None => return Err(TrioFault::new(TrioCode::MissingScript, 0)),
        };
        let r = match self.report.as_ref() {
            Some(r) => r,
            None => return Err(TrioFault::new(TrioCode::MissingReport, 0)),
        };
        let h = match self.hash.as_ref() {
            Some(h) => h,
            None => return Err(TrioFault::new(TrioCode::MissingHash, 0)),
        };
        if !s.passed() {
            return Err(TrioFault::new(TrioCode::ExitNonZero, 0));
        }
        if !h.computed() {
            return Err(TrioFault::new(TrioCode::MissingHash, 0));
        }
        // 交叉约束：两件独立产物在数量上必须一致。零条目的情形由「摘要/覆盖
        // 为零」先判死（见 `HashPart::computed`）：0 条配 0 覆盖时数量虽然相等，
        // 但那是「没算」，报缺哈希比报数量不符更接近事实。
        if r.entries != h.covered_files {
            return Err(TrioFault::new(TrioCode::CountMismatch, 0));
        }
        Ok(())
    }

    /// 证据链指纹：三件的内容摘要串成一条 FNV 链（不含归档路径）。
    ///
    /// 缺件时对应字段跳过而不是补零：补零会让「缺件」与「字段恰为 0」撞成
    /// 同一个指纹，而这两件事的处置完全不同。
    pub fn chain(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        if let Some(s) = self.script.as_ref() {
            h = fnv_u64(h, s.arg_fingerprint);
            h = fnv_u64(h, s.exit_code as u32 as u64);
        }
        if let Some(r) = self.report.as_ref() {
            h = fnv_u64(h, r.entries as u64);
            h = fnv_u64(h, r.verdict.rank() as u64);
        }
        if let Some(x) = self.hash.as_ref() {
            h = fnv_u64(h, x.algo.rank() as u64);
            h = fnv_u64(h, x.digest);
            h = fnv_u64(h, x.covered_files as u64);
        }
        h
    }

    /// 按记录下来的指纹复核（归档后自证）。
    pub fn verify_chain(&self, expected: u64) -> Result<(), TrioFault> {
        if self.chain() != expected {
            return Err(TrioFault::new(TrioCode::ChainBroken, 0));
        }
        Ok(())
    }
}

/// 归档库。
pub struct EvidenceArchive {
    trios: Vec<EvidenceTrio>,
    /// 每次归档时记下的链值（复核时逐条比对）。
    chains: Vec<u64>,
    rejected: u16,
}

impl EvidenceArchive {
    /// 新建空档。
    pub const fn new() -> EvidenceArchive {
        EvidenceArchive { trios: Vec::new(), chains: Vec::new(), rejected: 0 }
    }

    /// 归档一份证据，返回它的链值。
    ///
    /// 路径唯一性先于内容校验：路径是归档的键，同键二次声明属于身份冲突，
    /// 与内容对不对无关；先报内容错会让运维去修内容，然后第二次仍撞路径。
    pub fn archive(&mut self, trio: EvidenceTrio) -> Result<u64, TrioFault> {
        let verdict = self.try_archive(&trio);
        match verdict {
            Ok(c) => {
                self.chains.push(c);
                self.trios.push(trio);
                Ok(c)
            }
            Err(f) => {
                self.rejected = self.rejected.saturating_add(1);
                Err(f)
            }
        }
    }

    fn try_archive(&self, trio: &EvidenceTrio) -> Result<u64, TrioFault> {
        if self.trios.iter().any(|t| t.archive_path == trio.archive_path) {
            return Err(TrioFault::new(TrioCode::PathTaken, 0));
        }
        trio.validate()?;
        Ok(trio.chain())
    }

    /// 已归档份数。
    pub fn len(&self) -> usize {
        self.trios.len()
    }

    /// 被拒次数。
    pub const fn rejected(&self) -> u16 {
        self.rejected
    }

    /// 路径是否已被占用。
    pub fn contains(&self, path: &str) -> bool {
        self.trios.iter().any(|t| t.archive_path == path)
    }

    /// 归档库总链：逐份链值再咬合一遍。空库返回 0（哨兵：无证据即无链）。
    pub fn chain(&self) -> u64 {
        if self.chains.is_empty() {
            return 0;
        }
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for c in self.chains.iter() {
            h = fnv_u64(h, *c);
        }
        h
    }

    /// 全库复核：逐份重算链值并校验，返回复核份数。
    pub fn audit(&self) -> Result<u16, TrioFault> {
        for (i, t) in self.trios.iter().enumerate() {
            t.validate()?;
            if t.chain() != self.chains[i] {
                return Err(TrioFault::new(TrioCode::ChainBroken, (i + 1) as u16));
            }
        }
        Ok(self.trios.len() as u16)
    }

    /// 导出归档清单（每份一行，字段顺序固定便于 diff）。
    pub fn manifest(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("evidence count=");
        out.push_str(&self.trios.len().to_string());
        out.push_str(" parts=");
        out.push_str(&TRIO_PARTS.to_string());
        out.push_str(" chain=");
        out.push_str(&hex64(self.chain()));
        for (i, t) in self.trios.iter().enumerate() {
            out.push('\n');
            out.push_str(&(i + 1).to_string());
            out.push(' ');
            out.push_str(&t.archive_path);
            out.push_str(" chain=");
            out.push_str(&hex64(self.chains[i]));
            if let Some(r) = t.report.as_ref() {
                out.push_str(" entries=");
                out.push_str(&r.entries.to_string());
                out.push_str(" verdict=");
                out.push_str(r.verdict.label());
            }
            if let Some(h) = t.hash.as_ref() {
                out.push_str(" algo=");
                out.push_str(h.algo.label());
                out.push_str(" covered=");
                out.push_str(&h.covered_files.to_string());
            }
        }
        out
    }
}

impl Default for EvidenceArchive {
    fn default() -> Self {
        Self::new()
    }
}

/// FNV-1a 64 单步（质数与偏移基写死，来源：FNV 规范 1a 变体）。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制（no_std 手写，避免格式化器的分配）。
pub fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEX[nib] as char);
        shift -= 4;
    }
    s
}

/// 标准三件套（判据夹具）：校验器 exit=0、报告 40 条、哈希覆盖 40 文件。
///
/// 返回 Result 而非裸值：夹具自带矛盾（比如条目数与覆盖数写错）时应当在
/// 第一条判据就当场暴露，而不是让后面十几条判据逐个失败去猜是谁的问题。
pub fn standard_trio() -> Result<EvidenceTrio, TrioFault> {
    let t = EvidenceTrio::new("/ev/f2b40/run-01")
        .with_script(ScriptPart::new("/ev/f2b40/verify.sh", 0, &["--suite", "f2b40"]))
        .with_report(ReportPart::new("/ev/f2b40/report.txt", 40, Verdict::Pass))
        .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0x5eed_0f2b_40aa_0001, 40));
    t.validate()?;
    Ok(t)
}

/// 夹具失败时统一返回 false（夹具由第一条判据单独证明）。
fn on_trio<F: FnOnce(EvidenceTrio) -> bool>(f: F) -> bool {
    match standard_trio() {
        Ok(t) => f(t),
        Err(_) => false,
    }
}

/// 判据入口（UNX-F21593-J1 及配套十五条）。
pub fn run_unx21593_evidencetrio_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21593");

    // 1) 夹具可构造且通过校验（夹具自带矛盾会在这里当场暴露）。
    cs.add("f21593.fixture_buildable", standard_trio().is_ok(), "标准三件套构造后未通过校验");

    // 2) 归档接受（正向）：链值等于用字面零件重算的链，库链再咬合一层。
    let archived = on_trio(|t| {
        let fp = match t.script.as_ref() { Some(s) => s.arg_fingerprint, None => 0 };
        let digest = match t.hash.as_ref() { Some(h) => h.digest, None => 0 };
        // 独立重算：从零件字面量按「脚本→报告→哈希」的固定顺序咬合一遍。
        let mut want = 0xcbf2_9ce4_8422_2325u64;
        want = fnv_u64(want, fp);
        want = fnv_u64(want, 0); // exit_code = 0
        want = fnv_u64(want, 40); // entries
        want = fnv_u64(want, 0); // verdict = pass
        want = fnv_u64(want, 0); // algo = fnv1a64
        want = fnv_u64(want, digest);
        want = fnv_u64(want, 40); // covered_files
        let mut a = EvidenceArchive::new();
        match a.archive(t) {
            Ok(c) => c == want && a.len() == 1 && a.chain() == fnv_u64(0xcbf2_9ce4_8422_2325, c),
            Err(_) => false,
        }
    });
    cs.add("f21593.archive_accept", archived, "归档被拒，或链值与零件重算不符");

    // 3) 缺脚本（反向，断言具体码）。
    let no_script = EvidenceTrio::new("/ev/x")
        .with_report(ReportPart::new("/ev/r.txt", 40, Verdict::Pass))
        .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0x1234, 40));
    cs.add(
        "f21593.missing_script",
        no_script.validate().err().map(|f| f.code) == Some(0x91062),
        "缺脚本未被拒或码不符",
    );

    // 4) 缺报告（反向）。
    let no_report = EvidenceTrio::new("/ev/x")
        .with_script(ScriptPart::new("/ev/v.sh", 0, &["--suite"]))
        .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0x1234, 40));
    cs.add(
        "f21593.missing_report",
        no_report.validate().err().map(|f| f.code) == Some(0x91063),
        "缺报告未被拒或码不符",
    );

    // 5) 缺哈希（反向）。
    let no_hash = EvidenceTrio::new("/ev/x")
        .with_script(ScriptPart::new("/ev/v.sh", 0, &["--suite"]))
        .with_report(ReportPart::new("/ev/r.txt", 40, Verdict::Pass));
    cs.add(
        "f21593.missing_hash",
        no_hash.validate().err().map(|f| f.code) == Some(0x91064),
        "缺哈希未被拒或码不符",
    );

    // 6) 退出码非 0 即证据无效（反向）。
    let bad_exit = EvidenceTrio::new("/ev/x")
        .with_script(ScriptPart::new("/ev/v.sh", 1, &["--suite"]))
        .with_report(ReportPart::new("/ev/r.txt", 40, Verdict::Pass))
        .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0x1234, 40));
    cs.add(
        "f21593.exit_nonzero",
        bad_exit.validate().err().map(|f| f.code) == Some(0x91065),
        "exit=1 未被拒或码不符",
    );

    // 7) 数量交叉约束（反向）：条目 40、覆盖 39。
    let mismatch = EvidenceTrio::new("/ev/x")
        .with_script(ScriptPart::new("/ev/v.sh", 0, &["--suite"]))
        .with_report(ReportPart::new("/ev/r.txt", 40, Verdict::Pass))
        .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0x1234, 39));
    cs.add(
        "f21593.count_mismatch",
        mismatch.validate().err().map(|f| f.code) == Some(0x91066),
        "40/39 未被拒或码不符",
    );

    // 8) 零条目边界：0 条报告配 0 覆盖，数量相等但那是「没算」，按缺哈希判死。
    let zero = EvidenceTrio::new("/ev/x")
        .with_script(ScriptPart::new("/ev/v.sh", 0, &["--suite"]))
        .with_report(ReportPart::new("/ev/r.txt", 0, Verdict::Pass))
        .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0x1234, 0));
    cs.add(
        "f21593.zero_entries",
        zero.validate().err().map(|f| f.code) == Some(0x91064),
        "零条目零覆盖未被拒或码不符（不应报成数量不符）",
    );

    // 9) 摘要为零（反向边界）：等价于没算哈希。
    let zero_digest = EvidenceTrio::new("/ev/x")
        .with_script(ScriptPart::new("/ev/v.sh", 0, &["--suite"]))
        .with_report(ReportPart::new("/ev/r.txt", 40, Verdict::Pass))
        .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0, 40));
    cs.add(
        "f21593.digest_zero",
        zero_digest.validate().err().map(|f| f.code) == Some(0x91064),
        "零摘要未被拒或码不符",
    );

    // 10) 路径唯一性（反向）：同路径二次归档被拒，库内仍一份。
    let path_taken = on_trio(|t| {
        let mut a = EvidenceArchive::new();
        let first = a.archive(t.clone()).is_ok();
        let second = a.archive(t).err();
        first
            && second.map(|f| f.code) == Some(0x91067)
            && a.len() == 1
            && a.rejected() == 1
            && a.contains("/ev/f2b40/run-01")
    });
    cs.add("f21593.path_taken", path_taken, "同路径二次归档未被拒，或库内份数不符");

    // 11) 链值篡改必被复核发现（反向）。
    let broken = on_trio(|t| {
        let recorded = t.chain();
        let mut tampered = t;
        if let Some(h) = tampered.hash.as_mut() {
            h.digest = 0x5eed_0f2b_40aa_0002;
        }
        let code = tampered.verify_chain(recorded).err().map(|f| f.code);
        let self_ok = tampered.verify_chain(tampered.chain()).is_ok();
        code == Some(0x91068) && self_ok && tampered.chain() != recorded
    });
    cs.add("f21593.chain_broken", broken, "篡改后复核未报链断，或自洽复核被误报");

    // 12) 内容寻址：同内容换路径链不变，改参数链必变。
    let content_addressed = on_trio(|a| {
        let moved = EvidenceTrio::new("/ev/other/dir")
            .with_script(ScriptPart::new("/ev/f2b40/verify.sh", 0, &["--suite", "f2b40"]))
            .with_report(ReportPart::new("/ev/f2b40/report.txt", 40, Verdict::Pass))
            .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0x5eed_0f2b_40aa_0001, 40));
        let changed = EvidenceTrio::new("/ev/f2b40/run-01")
            .with_script(ScriptPart::new("/ev/f2b40/verify.sh", 0, &["--suite", "f2b41"]))
            .with_report(ReportPart::new("/ev/f2b40/report.txt", 40, Verdict::Pass))
            .with_hash(HashPart::new(HashAlgo::Fnv1a64, 0x5eed_0f2b_40aa_0001, 40));
        a.chain() == moved.chain() && a.chain() != changed.chain()
    });
    cs.add(
        "f21593.content_addressed",
        content_addressed,
        "换路径后链值变了（不应含路径），或换参数后链值没变",
    );

    // 13) 参数指纹：同参同指、异参异指、拆分方式不同也不同指。
    let args_ok = {
        let a = ScriptPart::new("/v.sh", 0, &["--suite", "f2b40"]);
        let b = ScriptPart::new("/v.sh", 0, &["--suite", "f2b40"]);
        let c = ScriptPart::new("/v.sh", 0, &["--suite", "f2b41"]);
        let d = ScriptPart::new("/v.sh", 0, &["--suitef2b40"]);
        a.arg_fingerprint == b.arg_fingerprint
            && a.arg_fingerprint != c.arg_fingerprint
            && a.arg_fingerprint != d.arg_fingerprint
            && a.arg_fingerprint != 0
    };
    cs.add("f21593.args_fingerprint", args_ok, "参数指纹不稳定，或未区分参数拆分");

    // 14) 空库与单份边界：空库链为 0、复核 0 份；单份复核 1 份。
    let empty_one = on_trio(|t| {
        let mut a = EvidenceArchive::new();
        let empty_ok = a.chain() == 0 && a.audit() == Ok(0) && a.len() == 0;
        let one_ok = a.archive(t).is_ok() && a.audit() == Ok(1);
        empty_ok && one_ok
    });
    cs.add("f21593.empty_and_one", empty_one, "空库链/复核不符，或单份复核份数不为 1");

    // 15) 清单可归档：计数、链值、条目数与结论成对出现。
    let manifest_ok = on_trio(|t| {
        let mut a = EvidenceArchive::new();
        let ok = a.archive(t).is_ok();
        let m = a.manifest();
        ok && m.contains("count=1")
            && m.contains("parts=3")
            && m.contains("/ev/f2b40/run-01")
            && m.contains("entries=40")
            && m.contains("verdict=pass")
            && m.contains("algo=fnv1a64")
            && m.contains("covered=40")
            && m.contains("chain=")
    });
    cs.add("f21593.manifest_fields", manifest_ok, "清单缺字段或字段值不符");

    // 16) 诊断码闭集：八码连续、三要素非空、未知码被识别。
    let codes_ok = {
        let codes = [
            TrioCode::TrioOk, TrioCode::MissingScript, TrioCode::MissingReport, TrioCode::MissingHash,
            TrioCode::ExitNonZero, TrioCode::CountMismatch, TrioCode::PathTaken, TrioCode::ChainBroken,
        ];
        let mut ok = true;
        for (i, c) in codes.iter().enumerate() {
            if c.what().is_empty() || c.why().is_empty() || c.advice().is_empty() {
                ok = false;
            }
            if c.code() != 0x91061u32 + i as u32 {
                ok = false;
            }
        }
        let unknown = TrioFault { code: 0x91060, at: 0 };
        ok && !unknown.is_known() && TrioFault::new(TrioCode::TrioOk, 0).is_known()
    };
    cs.add("f21593.code_closed_set", codes_ok, "码值不连续、三要素为空，或未知码被当成已知码");

    cs
}
