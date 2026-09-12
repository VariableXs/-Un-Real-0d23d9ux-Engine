//! m700drv — VARIX-M700 AI-13 驱动框架域 (F301~F325)
//!
//! 驱动 trait 宪法/驱动自描述清单/驱动沙盒舱/驱动加载官/驱动错误律/
//! 驱动版本协商/驱动测试靶场/驱动性能仪/驱动日志礼仪/驱动回归矩阵/
//! 驱动依赖裁剪/驱动看门狗/驱动热备谱/驱动卸载安全律/驱动 DMA 合规/
//! 驱动功耗契约/驱动 fuzz 桩/驱动文档生成器/驱动金样本/驱动隔离验证/
//! 驱动事件流/驱动降级阶梯/驱动审计官/驱动生态索引/驱动域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F301 — 驱动 trait 宪法：必备入口一个不能少
// ===========================================================================

/// 驱动 vtable：probe/init/read/write/shutdown 五大必备入口，
/// 0 表示未提供（用 Option 风格的 0 哨兵，保持 const 友好）。
#[derive(Clone, Copy, Debug)]
pub struct DriverVtable {
    pub probe: u32, // 入口指纹，0 = 缺席
    pub init: u32,
    pub read: u32,
    pub write: u32,
    pub shutdown: u32,
}

pub const VTABLE_ENTRIES: usize = 5;

impl DriverVtable {
    /// 宪法：五大入口必须全部在场。
    pub fn constitutional(&self) -> bool {
        self.probe != 0 && self.init != 0 && self.read != 0 && self.write != 0 && self.shutdown != 0
    }

    /// 允许省略 write 的只读驱动变体。
    pub fn readonly_ok(&self) -> bool {
        self.probe != 0 && self.init != 0 && self.read != 0 && self.shutdown != 0
    }
}

// ===========================================================================
// F302 — 驱动自描述清单：名字/版本/许可/作者
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverManifest {
    pub name: &'static str,
    pub version: u32,       // semver 打包：maj<<16 | min<<8 | patch
    pub license: u8,        // 0=GPL 1=MIT 2=Apache 3=双许可 0xFF=未知
    pub author: &'static str,
}

pub const LICENSE_UNKNOWN: u8 = 0xFF;

impl DriverManifest {
    pub fn self_describing(&self) -> bool {
        !self.name.is_empty() && !self.author.is_empty() && self.version != 0 && self.license != LICENSE_UNKNOWN
    }

    pub fn major(&self) -> u8 {
        (self.version >> 16) as u8
    }
}

// ===========================================================================
// F303 — 驱动沙盒舱：能力位掩码越权即拒
// ===========================================================================

pub const CAP_IO: u32 = 1;
pub const CAP_IRQ: u32 = 2;
pub const CAP_DMA: u32 = 4;
pub const CAP_POWER: u32 = 8;

/// 越权检测：请求的能力位必须都已被授予。
pub fn sandbox_allows(granted: u32, requested: u32) -> bool {
    granted & requested == requested
}

/// 越权时返回被拒绝的能力位（=0 表示无越权）。
pub fn sandbox_violation(granted: u32, requested: u32) -> u32 {
    requested & !granted
}

// ===========================================================================
// F304 — 驱动加载官：签名 + 依赖齐备才放行
// ===========================================================================

pub const DRV_MAGIC: u32 = 0x56415258; // "VARX"

/// 加载闸门：魔数正确、已签名、依赖数不超过上限。
pub fn load_gate(magic: u32, signed: bool, deps_present: usize, deps_required: usize) -> bool {
    magic == DRV_MAGIC && signed && deps_present >= deps_required
}

// ===========================================================================
// F305 — 驱动错误律：可恢复/致命分类
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrvError {
    Timeout,
    NoDevice,
    HardwareFault,
    Busy,
}

pub fn drv_error_recoverable(e: DrvError) -> bool {
    matches!(e, DrvError::Timeout | DrvError::Busy)
}

/// 重试上限：可恢复错误最多 3 次。
pub const DRV_MAX_RETRIES: u8 = 3;

pub fn drv_should_retry(e: DrvError, attempt: u8) -> bool {
    drv_error_recoverable(e) && attempt < DRV_MAX_RETRIES
}

// ===========================================================================
// F306 — 驱动版本协商：主版本一致才兼容
// ===========================================================================

/// 协商结果：主版本必须相等，实际版本取两者较小者。
pub fn version_negotiate(dev_maj: u8, dev_min: u8, drv_maj: u8, drv_min: u8) -> Option<(u8, u8)> {
    if dev_maj != drv_maj {
        return None;
    }
    Some((dev_maj, dev_min.min(drv_min)))
}

// ===========================================================================
// F307 — 驱动测试靶场：场景清单通过率
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct RangeCase {
    pub name: &'static str,
    pub passed: bool,
}

/// 靶场通过率 permille。
pub fn range_pass_permille(cases: &[RangeCase]) -> u32 {
    if cases.is_empty() {
        return 0;
    }
    let p = cases.iter().filter(|c| c.passed).count();
    (p * 1000 / cases.len()) as u32
}

/// 毕业线：900‰。
pub const RANGE_GRADUATE_PERMILLE: u32 = 900;

pub fn range_graduated(cases: &[RangeCase]) -> bool {
    range_pass_permille(cases) >= RANGE_GRADUATE_PERMILLE
}

// ===========================================================================
// F308 — 驱动性能仪：操作级延迟预算
// ===========================================================================

pub const DRV_OP_BUDGET_US: [u32; 4] = [50, 200, 1000, 10_000]; // probe/read/write/ioctl

pub fn op_budget_us(op: usize) -> Option<u32> {
    if op < DRV_OP_BUDGET_US.len() {
        Some(DRV_OP_BUDGET_US[op])
    } else {
        None
    }
}

pub fn op_within_budget(op: usize, latency_us: u32) -> bool {
    match op_budget_us(op) {
        Some(b) => latency_us <= b,
        None => false,
    }
}

// ===========================================================================
// F309 — 驱动日志礼仪：限频日志
// ===========================================================================

pub const LOG_MAX_PER_WINDOW: u32 = 8;

#[derive(Clone, Copy, Debug)]
pub struct LogValve {
    pub sent_this_window: u32,
    pub suppressed: u32,
}

impl LogValve {
    pub const fn new() -> LogValve {
        LogValve { sent_this_window: 0, suppressed: 0 }
    }

    /// 窗口内限量放行，超出计数抑制。
    pub fn admit(&mut self) -> bool {
        if self.sent_this_window < LOG_MAX_PER_WINDOW {
            self.sent_this_window += 1;
            true
        } else {
            self.suppressed += 1;
            false
        }
    }

    /// 窗口翻转。
    pub fn new_window(&mut self) {
        self.sent_this_window = 0;
    }
}

// ===========================================================================
// F310 — 驱动回归矩阵：驱动 × 场景全绿
// ===========================================================================

pub const REGRESSION_ROWS: usize = 4;
pub const REGRESSION_COLS: usize = 4;

pub fn regression_matrix_all_green(matrix: &[[bool; REGRESSION_COLS]; REGRESSION_ROWS]) -> bool {
    matrix.iter().all(|row| row.iter().all(|&c| c))
}

/// 统计红格数量。
pub fn regression_red_count(matrix: &[[bool; REGRESSION_COLS]; REGRESSION_ROWS]) -> usize {
    matrix.iter().flat_map(|row| row.iter()).filter(|&&c| !c).count()
}

// ===========================================================================
// F311 — 驱动依赖裁剪：只留真正用到的
// ===========================================================================

/// 裁剪：保留 deps 中被 used 标记的依赖，返回裁剪后数量（原地去重压缩）。
pub fn prune_deps(deps: &mut [u32], used: &[bool]) -> usize {
    let mut w = 0usize;
    for r in 0..deps.len() {
        if used.get(r).copied().unwrap_or(false) {
            deps.swap(w, r);
            w += 1;
        }
    }
    w
}

// ===========================================================================
// F312 — 驱动看门狗：心跳过期计数
// ===========================================================================

pub const WATCHDOG_TIMEOUT_MS: u32 = 2000;

#[derive(Clone, Copy, Debug)]
pub struct Watchdog {
    pub last_heartbeat_ms: u32,
    pub misses: u32,
}

impl Watchdog {
    pub const fn new() -> Watchdog {
        Watchdog { last_heartbeat_ms: 0, misses: 0 }
    }

    pub fn heartbeat(&mut self, now_ms: u32) {
        self.last_heartbeat_ms = now_ms;
    }

    /// 巡检：超过超时窗口即记一次失约。
    pub fn patrol(&mut self, now_ms: u32) -> bool {
        let late = now_ms.wrapping_sub(self.last_heartbeat_ms) > WATCHDOG_TIMEOUT_MS;
        if late {
            self.misses += 1;
        }
        !late
    }
}

// ===========================================================================
// F313 — 驱动热备谱：主备切换
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandbyRole {
    Primary,
    Backup,
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub struct HotStandby {
    pub primary: StandbyRole,
    pub backup: StandbyRole,
    pub failovers: u32,
}

impl HotStandby {
    pub const fn new() -> HotStandby {
        HotStandby { primary: StandbyRole::Primary, backup: StandbyRole::Backup, failovers: 0 }
    }

    /// 主驱动失联：备胎顶上，次数记账。
    pub fn failover(&mut self, primary_dead: bool) -> bool {
        if primary_dead && self.primary == StandbyRole::Primary {
            self.primary = StandbyRole::Failed;
            self.backup = StandbyRole::Primary;
            self.failovers += 1;
            true
        } else {
            false
        }
    }

    pub fn served_by_backup(&self) -> bool {
        self.primary == StandbyRole::Failed && self.backup == StandbyRole::Primary
    }
}

// ===========================================================================
// F314 — 驱动卸载安全律：引用归零 + 无在途 IO
// ===========================================================================

pub fn unload_safe(refcount: u32, inflight_io: u32, suspended: bool) -> bool {
    refcount == 0 && inflight_io == 0 && suspended
}

// ===========================================================================
// F315 — 驱动 DMA 合规：对齐 + 生命周期
// ===========================================================================

pub const DRV_DMA_ALIGN: u32 = 32;

/// DMA 合规：基址对齐、长度非零、映射在卸载前解除。
pub fn dma_compliant(base: u32, len: u32, unmapped_before_unload: bool) -> bool {
    base % DRV_DMA_ALIGN == 0 && len > 0 && unmapped_before_unload
}

// ===========================================================================
// F316 — 驱动功耗契约：状态延迟声明
// ===========================================================================

/// 驱动声明进入 D3 并恢复的最坏延迟；系统据此选择休眠档。
pub const DRV_RESUME_LATENCY_MS: [u32; 4] = [0, 5, 20, 100]; // D0..D3

pub fn resume_latency_ok(state: usize, system_budget_ms: u32) -> bool {
    match DRV_RESUME_LATENCY_MS.get(state) {
        Some(&ms) => ms <= system_budget_ms,
        None => false,
    }
}

// ===========================================================================
// F317 — 驱动 fuzz 桩：畸形输入必须被拒绝
// ===========================================================================

/// fuzz 桩：输入的首字节必须等于 0xA5 且长度在 4..=64，否则拒绝。
pub fn fuzz_gate(input: &[u8]) -> bool {
    input.first() == Some(&0xA5) && input.len() >= 4 && input.len() <= 64
}

/// 确定性 fuzz 序列生成：以 seed 展开固定长度变异样本。
pub fn fuzz_sample(seed: u32, out: &mut [u8; 8]) {
    let mut x = seed | 1;
    for slot in out.iter_mut() {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *slot = (x >> 24) as u8;
    }
}

// ===========================================================================
// F318 — 驱动文档生成器：文档章节完备性
// ===========================================================================

pub const DRV_DOC_SECTIONS: [&str; 5] = ["overview", "io-map", "errors", "power", "examples"];

pub fn doc_complete(sections_filled: u32) -> bool {
    sections_filled >= DRV_DOC_SECTIONS.len() as u32
}

// ===========================================================================
// F319 — 驱动金样本：已知输入 → 已知输出
// ===========================================================================

/// 金样本：回显驱动样例——把输入字节的低 4 位打包成 BCD 风格半字节对。
pub fn golden_transform(input: &[u8], out: &mut [u8]) -> usize {
    let n = input.len().min(out.len() / 2);
    for i in 0..n {
        out[i * 2] = (input[i] >> 4) & 0x0F;
        out[i * 2 + 1] = input[i] & 0x0F;
    }
    n * 2
}

pub fn golden_matches(input: &[u8], produced: &[u8]) -> bool {
    let mut expect = [0u8; 16];
    let n = golden_transform(input, &mut expect);
    produced.len() >= n && produced[..n] == expect[..n]
}

// ===========================================================================
// F320 — 驱动隔离验证：一个驱动炸了不能带走别人
// ===========================================================================

/// 故障注入后，存活驱动集合与故障前除被注者外完全一致。
pub fn isolation_verified(alive_before: &[bool], alive_after: &[bool], faulted: usize) -> bool {
    if faulted >= alive_before.len() {
        return false;
    }
    alive_before
        .iter()
        .zip(alive_after.iter())
        .enumerate()
        .all(|(i, (a, b))| i == faulted || a == b)
}

// ===========================================================================
// F321 — 驱动事件流：定容事件环
// ===========================================================================

pub const DRV_EVENT_RING: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrvEventKind {
    Probe,
    Ready,
    Error,
    Detach,
}

#[derive(Clone, Copy, Debug)]
pub struct DrvEventRing {
    kinds: [DrvEventKind; DRV_EVENT_RING],
    seqs: [u64; DRV_EVENT_RING],
    head: usize,
    pub dropped: u64,
}

impl DrvEventRing {
    pub const fn new() -> DrvEventRing {
        DrvEventRing {
            kinds: [DrvEventKind::Probe; DRV_EVENT_RING],
            seqs: [0; DRV_EVENT_RING],
            head: 0,
            dropped: 0,
        }
    }

    pub fn push(&mut self, seq: u64, kind: DrvEventKind) {
        self.kinds[self.head] = kind;
        self.seqs[self.head] = seq;
        self.head = (self.head + 1) % DRV_EVENT_RING;
    }

    pub fn kind_at(&self, i: usize) -> DrvEventKind {
        self.kinds[i % DRV_EVENT_RING]
    }

    pub fn seq_at(&self, i: usize) -> u64 {
        self.seqs[i % DRV_EVENT_RING]
    }
}

// ===========================================================================
// F322 — 驱动降级阶梯：错误累积逐级降级
// ===========================================================================

pub const DEGRADE_STEPS: u8 = 4;
/// 每累积 4 次错误降一级。
pub const ERRORS_PER_STEP: u32 = 4;

/// 错误计数 → 降级档位（0=全速 3=最简）。
pub fn degrade_level(errors: u32) -> u8 {
    let level = errors / ERRORS_PER_STEP;
    (level as u8).min(DEGRADE_STEPS - 1)
}

/// 档位下允许的功能掩码：级别越高功能越少。
pub fn degrade_cap_mask(level: u8) -> u32 {
    match level {
        0 => 0b1111,
        1 => 0b0111,
        2 => 0b0011,
        _ => 0b0001,
    }
}

// ===========================================================================
// F323 — 驱动审计官：特权操作必须留痕
// ===========================================================================

pub const AUDIT_RING: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    pub driver_id: u16,
    pub op: u16,
    pub granted: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct AuditLog {
    recs: [AuditRecord; AUDIT_RING],
    head: usize,
    pub count: u64,
    pub dropped: u64,
}

impl AuditLog {
    pub const fn new() -> AuditLog {
        AuditLog {
            recs: [AuditRecord { driver_id: 0, op: 0, granted: false }; AUDIT_RING],
            head: 0,
            count: 0,
            dropped: 0,
        }
    }

    pub fn record(&mut self, r: AuditRecord) {
        self.recs[self.head] = r;
        self.head = (self.head + 1) % AUDIT_RING;
        self.count += 1;
    }

    pub fn rec_at(&self, i: usize) -> AuditRecord {
        self.recs[i % AUDIT_RING]
    }

    /// 特权操作（op 高位为 1）必须有记录且被裁决。
    pub fn privileged_covered(&self, driver_id: u16, op: u16) -> bool {
        if op & 0x8000 == 0 {
            return true;
        }
        let mut i = 0usize;
        while i < AUDIT_RING {
            let r = self.recs[i];
            if r.driver_id == driver_id && r.op == op {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ===========================================================================
// F324 — 驱动生态索引：兼容等级册
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatLevel {
    Certified,  // 官方认证
    Community,  // 社区可用
    Experimental, // 实验性
    Broken,
}

pub fn compat_rank(l: CompatLevel) -> u8 {
    match l {
        CompatLevel::Certified => 3,
        CompatLevel::Community => 2,
        CompatLevel::Experimental => 1,
        CompatLevel::Broken => 0,
    }
}

/// 索引准入：Broken 不得收录。
pub fn index_admissible(l: CompatLevel) -> bool {
    compat_rank(l) > 0
}

// ===========================================================================
// F325 — 驱动域年报
// ===========================================================================

pub const DRV_REPORT_SECTIONS: [&str; 5] = ["loads", "crashes", "sandbox", "perf", "fuzz"];

pub fn drv_report_complete(filled: u32) -> bool {
    filled >= DRV_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700drv_checks() -> CheckSet {
    let mut set = CheckSet::new("m700drv");

    // F301 驱动 trait 宪法
    let full = DriverVtable { probe: 1, init: 2, read: 3, write: 4, shutdown: 5 };
    let missing = DriverVtable { probe: 1, init: 2, read: 3, write: 0, shutdown: 5 };
    let readonly = DriverVtable { probe: 1, init: 2, read: 3, write: 0, shutdown: 5 };
    set.add(
        "F301 vtable full",
        full.constitutional() && !missing.constitutional(),
        "five entries",
    );
    set.add("F301 readonly variant", readonly.readonly_ok(), "write optional for read-only");

    // F302 驱动自描述清单
    let mf = DriverManifest { name: "nvme", version: 1 << 16 | 4 << 8 | 2, license: 2, author: "varix" };
    set.add(
        "F302 manifest",
        mf.self_describing() && mf.major() == 1,
        "name+ver+license+author",
    );
    set.add(
        "F302 manifest rejects blank",
        !DriverManifest { name: "", version: 1, license: 1, author: "x" }.self_describing()
            && !DriverManifest { name: "x", version: 1, license: LICENSE_UNKNOWN, author: "x" }.self_describing(),
        "unknown license refused",
    );

    // F303 驱动沙盒舱
    let granted = CAP_IO | CAP_DMA;
    set.add(
        "F303 sandbox grant",
        sandbox_allows(granted, CAP_IO) && !sandbox_allows(granted, CAP_IRQ),
        "least privilege",
    );
    set.add(
        "F303 violation bits",
        sandbox_violation(granted, CAP_IO | CAP_POWER | CAP_IRQ) == CAP_POWER | CAP_IRQ,
        "exact refusal",
    );

    // F304 驱动加载官
    set.add(
        "F304 load gate",
        load_gate(DRV_MAGIC, true, 2, 2) && !load_gate(0xDEAD, true, 2, 2) && !load_gate(DRV_MAGIC, false, 0, 1),
        "magic+sign+deps",
    );

    // F305 驱动错误律
    set.add(
        "F305 error classes",
        drv_error_recoverable(DrvError::Timeout) && drv_error_recoverable(DrvError::Busy)
            && !drv_error_recoverable(DrvError::HardwareFault),
        "recoverable subset",
    );
    set.add(
        "F305 retry policy",
        drv_should_retry(DrvError::Busy, 2) && !drv_should_retry(DrvError::Busy, 3)
            && !drv_should_retry(DrvError::NoDevice, 0),
        "3 strikes, fatal never",
    );

    // F306 驱动版本协商
    set.add(
        "F306 negotiate same major",
        version_negotiate(1, 6, 1, 2) == Some((1, 2)),
        "min minor wins",
    );
    set.add(
        "F306 negotiate major clash",
        version_negotiate(2, 0, 1, 9).is_none(),
        "major mismatch refused",
    );

    // F307 驱动测试靶场
    let cases = [
        RangeCase { name: "probe", passed: true },
        RangeCase { name: "rw", passed: true },
        RangeCase { name: "sleep", passed: true },
        RangeCase { name: "storm", passed: false },
    ];
    set.add("F307 range pass rate", range_pass_permille(&cases) == 750, "3/4");
    set.add(
        "F307 range graduation",
        !range_graduated(&cases) && !range_graduated(&[]),
        "750‰ below bar, empty fails",
    );

    // F308 驱动性能仪
    set.add(
        "F308 op budgets",
        op_within_budget(0, 40) && !op_within_budget(0, 60) && op_within_budget(3, 9000),
        "per-op table",
    );
    set.add("F308 unknown op", !op_within_budget(9, 0), "out of table");

    // F309 驱动日志礼仪
    let mut valve = LogValve::new();
    let mut admitted = 0;
    while valve.admit() {
        admitted += 1;
    }
    let suppressed_now = valve.suppressed;
    set.add(
        "F309 log rate limit",
        admitted == LOG_MAX_PER_WINDOW as usize && suppressed_now == 1,
        "8 then choke",
    );
    valve.admit();
    valve.new_window();
    set.add("F309 window reset", valve.sent_this_window == 0, "fresh window");

    // F310 驱动回归矩阵
    let mut matrix = [[true; REGRESSION_COLS]; REGRESSION_ROWS];
    set.add("F310 matrix green", regression_matrix_all_green(&matrix) && regression_red_count(&matrix) == 0, "all pass");
    matrix[2][1] = false;
    set.add("F310 matrix red cell", !regression_matrix_all_green(&matrix) && regression_red_count(&matrix) == 1, "one red");

    // F311 驱动依赖裁剪
    let mut deps = [7u32, 11u32, 13u32, 17u32];
    let used = [true, false, true, false];
    let kept = prune_deps(&mut deps, &used);
    set.add(
        "F311 prune keeps used",
        kept == 2 && deps[0] == 7 && deps[1] == 13,
        "compacted",
    );

    // F312 驱动看门狗
    let mut wd = Watchdog::new();
    wd.heartbeat(1000);
    let on_time = wd.patrol(2500);
    let misses_after_ok = wd.misses;
    let late = wd.patrol(3500);
    set.add(
        "F312 watchdog on time",
        on_time && misses_after_ok == 0,
        "within 2s",
    );
    set.add("F312 watchdog late", !late && wd.misses == 1, "miss recorded");

    // F313 驱动热备谱
    let mut hs = HotStandby::new();
    let switched = hs.failover(true);
    let failovers_after = hs.failovers;
    set.add(
        "F313 failover switch",
        switched && hs.served_by_backup() && failovers_after == 1,
        "backup promoted",
    );
    set.add(
        "F313 no double failover",
        !hs.failover(true) && hs.failovers == 1,
        "already failed",
    );

    // F314 驱动卸载安全律
    set.add(
        "F314 unload safe",
        unload_safe(0, 0, true) && !unload_safe(1, 0, true) && !unload_safe(0, 2, true) && !unload_safe(0, 0, false),
        "zero refs, zero io, suspended",
    );

    // F315 驱动 DMA 合规
    set.add(
        "F315 dma compliant",
        dma_compliant(64, 128, true) && !dma_compliant(33, 128, true) && !dma_compliant(64, 128, false),
        "align+map lifetime",
    );

    // F316 驱动功耗契约
    set.add(
        "F316 power contract",
        resume_latency_ok(0, 1) && resume_latency_ok(3, 100) && !resume_latency_ok(3, 50),
        "declared vs budget",
    );
    set.add("F316 bad state", !resume_latency_ok(9, 1000), "unknown state refused");

    // F317 驱动 fuzz 桩
    set.add(
        "F317 fuzz gate",
        fuzz_gate(&[0xA5, 1, 2, 3]) && !fuzz_gate(&[0x5A, 1, 2, 3]) && !fuzz_gate(&[0xA5, 1]),
        "magic+length",
    );
    let mut s1 = [0u8; 8];
    let mut s2 = [0u8; 8];
    fuzz_sample(42, &mut s1);
    fuzz_sample(42, &mut s2);
    let mut s3 = [0u8; 8];
    fuzz_sample(100, &mut s3);
    set.add(
        "F317 fuzz deterministic",
        s1 == s2 && s1 != s3,
        "same seed same stream",
    );

    // F318 驱动文档生成器
    set.add(
        "F318 doc sections",
        DRV_DOC_SECTIONS.len() == 5 && doc_complete(5) && !doc_complete(2),
        "sections complete",
    );

    // F319 驱动金样本
    let input = [0xABu8, 0xCD];
    let mut expect = [0u8; 16];
    let n = golden_transform(&input, &mut expect);
    set.add(
        "F319 golden transform",
        n == 4 && expect[0] == 0x0A && expect[1] == 0x0B && expect[2] == 0x0C && expect[3] == 0x0D,
        "nibble split",
    );
    set.add("F319 golden verify", golden_matches(&input, &expect[..4]) && !golden_matches(&input, &expect[..3]), "verify");

    // F320 驱动隔离验证
    let before = [true, true, true, true];
    let after = [true, false, true, true];
    set.add(
        "F320 isolation blast only",
        isolation_verified(&before, &after, 1),
        "only faulted dies",
    );
    set.add(
        "F320 isolation spread caught",
        !isolation_verified(&before, &[true, false, false, true], 1),
        "collateral damage",
    );

    // F321 驱动事件流
    let mut ring = DrvEventRing::new();
    let mut seq = 0u64;
    while seq < 11 {
        ring.push(seq, if seq == 10 { DrvEventKind::Detach } else { DrvEventKind::Ready });
        seq += 1;
    }
    let newest_kind = ring.kind_at(10);
    let newest_seq = ring.seq_at(10);
    let oldest_kind = ring.kind_at(0);
    set.add(
        "F321 event ring wrap",
        newest_kind == DrvEventKind::Detach && newest_seq == 10 && oldest_kind == DrvEventKind::Ready,
        "ring holds 8, newest wins",
    );
    set.add("F321 ring capacity", ring.kind_at(7) == DrvEventKind::Ready, "slot 7 live");

    // F322 驱动降级阶梯
    set.add(
        "F322 degrade levels",
        degrade_level(0) == 0 && degrade_level(4) == 1 && degrade_level(11) == 2 && degrade_level(99) == 3,
        "4 errors per step, capped",
    );
    set.add(
        "F322 degrade caps",
        degrade_cap_mask(0) == 0b1111 && degrade_cap_mask(2) == 0b0011 && degrade_cap_mask(3) == 0b0001,
        "features shed",
    );

    // F323 驱动审计官
    let mut audit = AuditLog::new();
    audit.record(AuditRecord { driver_id: 9, op: 0x8001, granted: true });
    let rec0 = audit.rec_at(0);
    set.add(
        "F323 audit records",
        rec0 == AuditRecord { driver_id: 9, op: 0x8001, granted: true } && audit.count == 1,
        "trail kept",
    );
    set.add(
        "F323 privileged covered",
        audit.privileged_covered(9, 0x8001) && !audit.privileged_covered(9, 0x8002)
            && audit.privileged_covered(9, 0x0001),
        "privileged needs trail",
    );

    // F324 驱动生态索引
    set.add(
        "F324 compat ranks",
        compat_rank(CompatLevel::Certified) == 3 && compat_rank(CompatLevel::Broken) == 0,
        "ordered levels",
    );
    set.add(
        "F324 index admission",
        index_admissible(CompatLevel::Experimental) && !index_admissible(CompatLevel::Broken),
        "broken not indexed",
    );

    // F325 驱动域年报
    set.add(
        "F325 drv report",
        DRV_REPORT_SECTIONS.len() == 5 && drv_report_complete(5) && !drv_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f305_retry_policy() {
        assert!(drv_should_retry(DrvError::Timeout, 0));
        assert!(!drv_should_retry(DrvError::HardwareFault, 0));
        assert!(!drv_should_retry(DrvError::Busy, DRV_MAX_RETRIES));
    }

    #[test]
    fn f306_version_negotiate() {
        assert_eq!(version_negotiate(3, 9, 3, 1), Some((3, 1)));
        assert_eq!(version_negotiate(1, 0, 2, 0), None);
    }

    #[test]
    fn f311_prune_compaction() {
        let mut deps = [1u32, 2, 3, 4, 5];
        let used = [false, true, false, true, true];
        let kept = prune_deps(&mut deps, &used);
        assert_eq!(kept, 3);
        assert_eq!(&deps[..3], &[2, 4, 5]);
    }

    #[test]
    fn f313_failover_semantics() {
        let mut hs = HotStandby::new();
        assert!(!hs.failover(false)); // 没死不切换
        assert!(hs.failover(true));
        assert_eq!(hs.primary, StandbyRole::Failed);
        assert_eq!(hs.backup, StandbyRole::Primary);
        assert!(hs.served_by_backup());
    }

    #[test]
    fn f319_golden_nibbles() {
        let input = [0xFF, 0x01];
        let mut out = [0u8; 16];
        let n = golden_transform(&input, &mut out);
        assert_eq!(n, 4);
        assert_eq!(&out[..4], &[0x0F, 0x0F, 0x00, 0x01]);
        assert!(golden_matches(&input, &out[..4]));
        assert!(!golden_matches(&input, &out[..2]));
    }

    #[test]
    fn f325_domain_selfcheck_all_pass() {
        let set = run_m700drv_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
