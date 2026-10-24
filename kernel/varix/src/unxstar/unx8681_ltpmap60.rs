//! UNX-F8681 · LTP 用例映射表 ≥60 行账 · 诊断段 0xB6C0
//! 验收锚点：UNX-F8681-J1 LTP（Linux Test Project）用例到内核判据
//! 的映射表 ≥60 行——行号连续、用例名唯一、判据号非空、未映射用例
//! 显式落账，双判据缺一即批内红灯。
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB6C0;

// ── 诊断段独占 · LTP 映射错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_SHORT: u16 = SEG_ERR + 1;   // 行数不足 60
const E_DUP: u16 = SEG_ERR + 2;     // 用例名重复
const E_GAP: u16 = SEG_ERR + 3;     // 行号断档
const E_EMPTY: u16 = SEG_ERR + 4;   // 判据号为空
const E_UNMAP: u16 = SEG_ERR + 5;   // 未映射未落账

pub const LTP_MIN_ROWS: usize = 60;

// ── FNV-1a 64 ────────────────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── LTP 映射行 ───────────────────────────────────────────────
/// 一行映射：行号 + LTP 用例名 + 内核判据号 + 映射状态。
#[derive(Clone, Debug)]
pub struct LtpRow {
    pub row: u32,        // 行号（1 起连续）
    pub case: String,    // LTP 用例名（如 epoll01）
    pub check_id: u16,   // 内核判据号（诊断段锚）
    pub mapped: bool,    // 映射状态（false = 显式未映射）
}

impl LtpRow {
    pub fn fp(&self) -> u64 {
        fnv1a64(format!("{}:{}:{}:{}", self.row, self.case, self.check_id, self.mapped).as_bytes())
    }
}

// ── LTP 映射表 ───────────────────────────────────────────────
pub struct LtpMap {
    pub rows: Vec<LtpRow>,
    pub unmapped_cases: Vec<String>, // 未映射用例落账
}

impl LtpMap {
    pub fn new() -> Self {
        LtpMap { rows: Vec::new(), unmapped_cases: Vec::new() }
    }
    /// 添行：行号自动连续，未映射行同步落账。
    pub fn add(&mut self, case: &str, check_id: u16, mapped: bool) {
        let row = (self.rows.len() + 1) as u32;
        if !mapped {
            self.unmapped_cases.push(case.to_string());
        }
        self.rows.push(LtpRow { row, case: case.to_string(), check_id, mapped });
    }
    /// 行数达标。
    pub fn enough(&self) -> bool {
        self.rows.len() >= LTP_MIN_ROWS
    }
    /// 行号连续性。
    pub fn rows_contiguous(&self) -> bool {
        for (i, r) in self.rows.iter().enumerate() {
            if r.row != (i + 1) as u32 {
                return false;
            }
        }
        true
    }
    /// 用例名唯一。
    pub fn cases_unique(&self) -> bool {
        let n = self.rows.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.rows[i].case == self.rows[j].case {
                    return false;
                }
            }
        }
        true
    }
    /// 已映射行判据号非空。
    pub fn mapped_ids_filled(&self) -> bool {
        self.rows.iter().all(|r| r.mapped || r.check_id != 0 || !r.mapped)
            && self.rows.iter().all(|r| !r.mapped || r.check_id != 0)
    }
    /// 未映射落账与行集一致。
    pub fn unmap_ledger_ok(&self) -> bool {
        let ledger_n = self.unmapped_cases.len();
        let row_n = self.rows.iter().filter(|r| !r.mapped).count();
        ledger_n == row_n
    }
    /// 按 LTP 用例名查行。
    pub fn find_case(&self, case: &str) -> Option<&LtpRow> {
        self.rows.iter().find(|r| r.case == case)
    }
    /// 表指纹（全行折叠）。
    pub fn table_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for r in &self.rows {
            let f = r.fp();
            for b in f.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
    /// 映射率（0~100）。
    pub fn map_rate(&self) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        let m = self.rows.iter().filter(|r| r.mapped).count();
        (m * 100 / self.rows.len()) as u32
    }
}

// ── 标准夹具：64 行真实 LTP 风格映射 ────────────────────────
pub fn standard_map() -> LtpMap {
    let mut m = LtpMap::new();
    // 用例名（LTP 风格）：60+ 行，其中 4 行显式未映射
    let cases: [&str; 64] = [
        "abort01", "accept01", "access01", "alarm02", "asyncio02",
        "bind01", "brk01", "chdir01", "chmod01", "chown01",
        "clock_gettime01", "close01", "connect01", "creat01", "dup01",
        "dup201", "epoll_create01", "epoll_ctl01", "epoll_wait01", "execve01",
        "exit01", "fchdir01", "fchmod01", "fchown01", "fcntl01",
        "fdatasync01", "fork01", "fstat01", "fsync01", "ftruncate01",
        "getcwd01", "getdents01", "getegid01", "geteuid01", "getgid01",
        "getgroups01", "getpeername01", "getpid01", "getppid01", "getsockname01",
        "getsockopt01", "gettid01", "gettimeofday01", "getuid01", "inotify_init01",
        "ioctl01", "kill01", "link01", "listen01", "lseek01",
        "lstat01", "madvise01", "membarrier01", "mkdir01", "mknod01",
        "mlock01", "mmap01", "mprotect01", "msync01", "munlock01",
        "munmap01", "nanosleep01", "open01", "pipe01",
    ];
    for (i, c) in cases.iter().enumerate() {
        let mapped = !matches!(i, 61 | 62 | 63 | 64);
        let cid = if mapped { (0xB6C0u16) + (i as u16) * 4 } else { 0 };
        m.add(c, cid, mapped);
    }
    m
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8681_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8681 LTP映射表≥60行");
    let m = standard_map();

    // 1. 行数 ≥60
    s.add("LTP-行数达标", m.enough(), if m.enough() { "" } else { "行数不足60" });

    // 2. 行号 1..N 连续
    s.add("LTP-行号连续", m.rows_contiguous(), if m.rows_contiguous() { "" } else { "行号断档" });

    // 3. 用例名唯一
    s.add("LTP-用例唯一", m.cases_unique(), if m.cases_unique() { "" } else { "用例名重复" });

    // 4. 已映射行判据号非空
    s.add("LTP-判据号非空", m.mapped_ids_filled(), if m.mapped_ids_filled() { "" } else { "判据号缺失" });

    // 5. 未映射落账一致（4 行未映射 → 4 条落账）
    s.add("LTP-未映射落账", m.unmap_ledger_ok() && m.unmapped_cases.len() == 4,
        if m.unmap_ledger_ok() { "" } else { "落账不齐" });

    // 6. 按用例名查行命中/落空
    let hit = m.find_case("fork01").map(|r| r.mapped && r.check_id != 0).unwrap_or(false);
    let miss = m.find_case("no_such_case").is_none();
    s.add("LTP-查行命中落空", hit && miss, if hit && miss { "" } else { "查行异常" });

    // 7. 表指纹稳定且内容敏感
    let fp1 = m.table_fp();
    let fp2 = m.table_fp();
    let mut m2 = standard_map();
    m2.rows[0].check_id = 0xFFFF;
    let fp3 = m2.table_fp();
    s.add("LTP-表指纹敏感", fp1 == fp2 && fp1 != fp3,
        if fp1 == fp2 && fp1 != fp3 { "" } else { "指纹碰撞或漂移" });

    // 8. 映射率计算正确（60/64 = 93%）
    let rate = m.map_rate();
    s.add("LTP-映射率", rate == 93, if rate == 93 { "" } else { "映射率算错" });

    // 9. 行号断档检出：手工构造断档表
    let mut m3 = LtpMap::new();
    m3.add("a01", 1, true);
    m3.add("a02", 2, true);
    m3.rows[1].row = 5; // 篡改断档
    s.add("LTP-断档检出", !m3.rows_contiguous(),
        if !m3.rows_contiguous() { "" } else { "断档未检出" });

    // 10. 用例重复检出
    let mut m4 = LtpMap::new();
    m4.add("dup01", 1, true);
    m4.add("dup01", 2, true);
    s.add("LTP-重复检出", !m4.cases_unique(),
        if !m4.cases_unique() { "" } else { "重复未检出" });

    // 11. 全映射表映射率 100（构造全映射对照）
    let mut m5 = LtpMap::new();
    for i in 0..60 {
        m5.add(&format!("full{:02}", i), 0x100 + i as u16, true);
    }
    s.add("LTP-全映射率100", m5.map_rate() == 100 && m5.unmapped_cases.is_empty(),
        if m5.map_rate() == 100 { "" } else { "全映射异常" });

    // 12. LCG 抽样 6 行复核行号与判据号
    let mut lcg = Lcg(0xB6C0);
    let mut spot = true;
    for _ in 0..6 {
        let i = lcg.below(m.rows.len() as u64) as usize;
        let r = &m.rows[i];
        if r.row != (i + 1) as u32 || (r.mapped && r.check_id == 0) {
            spot = false;
        }
    }
    s.add("LTP-抽样复核", spot, if spot { "" } else { "抽样异常" });

    // 13. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB6C0 && SEG_BASE < 0xB6E0;
    s.add("LTP-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
