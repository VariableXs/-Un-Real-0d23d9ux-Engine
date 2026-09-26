
// ---------------------------------------------------------------------------
// F010 · 深化批次三：逃逸防御记账面（穿越重判 + 镜像写拒绝计数）+ 跨沙盒
// 共享区写过审计环
//
// 主册依据（G-A-10【状态与异常】）：「程序试图写系统镜像区 → 拒绝 + 归因日志
// （不给『写成功』假象）；路径穿越（..\）逃逸尝试 → 规范化后重判，穿越失败
// 如实报错」；【设计细节】「SharedData（显式声明应用才可写，写操作过审计）」。
// normalize/classify_path/SharedWhite 既有面（一处一事实），本段只补记账与审计。
// ---------------------------------------------------------------------------

/// 逃逸防御台账（归因日志的计数面——诊断页可查）。
#[derive(Clone, Copy, Debug)]
pub struct EscapeLedger {
    /// 规范化判死（越出根）的穿越尝试。
    pub traversal_refused: u32,
    /// 规范化成功后重判放行的路径（曾含 ..\ 但收敛进根）。
    pub rejudged_after_norm: u32,
    /// 系统镜像区写拒绝（不给「写成功」假象——每次如实计数）。
    pub mirror_writes_refused: u32,
}

impl EscapeLedger {
    pub const fn new() -> EscapeLedger {
        EscapeLedger { traversal_refused: 0, rejudged_after_norm: 0, mirror_writes_refused: 0 }
    }

    /// 穿越判定：normalize 既有语义（None = 越出根 = 拒绝；Some = 规范化后
    /// 重判放行——重判不等于放任，系统镜像区检查仍在其后）。
    pub fn judge_traversal(&mut self, path: &str) -> Result<String, &'static str> {
        match normalize(path) {
            None => {
                self.traversal_refused += 1;
                Err("path traversal refused (escaped root)")
            }
            Some(norm) => {
                self.rejudged_after_norm += 1;
                Ok(norm)
            }
        }
    }

    /// 系统镜像区写尝试：恒拒绝（白名单纪律——内置盘与系统区默认只读）。
    pub fn judge_mirror_write(&mut self, path: &str) -> Result<&'static str, &'static str> {
        const MIRROR_PREFIXES: [&str; 3] =
            ["c:\\windows", "c:\\program files", "c:\\program files (x86)"];
        let p = path.to_ascii_lowercase();
        if MIRROR_PREFIXES.iter().any(|m| p.starts_with(*m)) {
            self.mirror_writes_refused += 1;
            return Err("system image area is read-only; write refused and logged");
        }
        Ok("redirected to sandbox")
    }
}

/// 共享区写过审计环（SharedData 写操作过审计——应用 + 内容指纹，环形 32 条）。
pub struct SharedWriteAudit {
    entries: [(u32, u64); 32],
    n: usize,
    /// 无共享声明却被放行前的拒绝计数（SharedWhite 既有判定之上的审计面）。
    pub unauthorized_refused: u32,
}

impl SharedWriteAudit {
    pub const fn new() -> SharedWriteAudit {
        SharedWriteAudit { entries: [(0, 0); 32], n: 0, unauthorized_refused: 0 }
    }

    /// 记一条共享区写审计（app + 内容指纹）。环满 → 覆盖最旧（容量纪律）。
    pub fn record(&mut self, app: u32, content_hash: u64) {
        self.entries[self.n % 32] = (app, content_hash);
        self.n += 1;
    }

    pub fn note_unauthorized(&mut self) {
        self.unauthorized_refused += 1;
    }

    pub fn len(&self) -> usize {
        self.n.min(32)
    }

    /// 第 idx 条（按写入序；环覆盖后从最旧开始）。
    pub fn at(&self, idx: usize) -> Option<(u32, u64)> {
        if idx >= self.len() {
            return None;
        }
        let start = self.n.saturating_sub(32);
        self.entries.get((start + idx) % 32).copied()
    }
}

/// F010 深化批次三自检。
pub fn run_fsredir_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep2");
    // 1) 穿越三态：「a\..\b」收敛 → 重判放行；「..\」越根 → 拒绝；「a\.\b」
    //    消点号 → 放行（normalize 既有语义 + 台账计数对账）。
    let mut led = EscapeLedger::new();
    let ok1 = led.judge_traversal("c:\\app\\sub\\..\\data.txt");
    let ok2 = led.judge_traversal("c:\\app\\.\\data.txt");
    let bad = led.judge_traversal("..\\..\\etc");
    cs.add(
        "traversal_rejudge_and_refuse",
        matches!(&ok1, Ok(p) if p == "c:\\app\\data.txt")
            && matches!(&ok2, Ok(p) if p == "c:\\app\\data.txt")
            && bad.is_err()
            && led.rejudged_after_norm == 2
            && led.traversal_refused == 1,
        "",
    );
    // 2) 镜像区写恒拒绝 + 计数（Windows / Program Files / x86 三前缀）。
    let m1 = led.judge_mirror_write("C:\\Windows\\system32\\evil.dll");
    let m2 = led.judge_mirror_write("c:\\Program Files\\app\\x.ini");
    let m3 = led.judge_mirror_write("C:\\Program Files (x86)\\legacy\\y.dll");
    let safe = led.judge_mirror_write("C:\\Users\\doc\\report.txt");
    cs.add(
        "mirror_write_refused_logged",
        m1.is_err() && m2.is_err() && m3.is_err() && safe.is_ok() && led.mirror_writes_refused == 3,
        "",
    );
    // 3) 共享区审计环：35 条写入环形覆盖后剩最后 32 条（最旧 3 条被覆盖）；
    //    未授权拒绝计数独立。
    let mut audit = SharedWriteAudit::new();
    for i in 0..35u64 {
        audit.record((i % 3) as u32, i * 0x9E37_79B9_7F4A_7C15);
    }
    audit.note_unauthorized();
    let first = audit.at(0);
    let last = audit.at(31);
    cs.add(
        "shared_write_audit_ring",
        audit.len() == 32
            && matches!(first, Some((0, 3 * 0x9E37_79B9_7F4A_7C15)))
            && matches!(last, Some((1, 34 * 0x9E37_79B9_7F4A_7C15)))
            && audit.unauthorized_refused == 1,
        "",
    );
    cs
}
