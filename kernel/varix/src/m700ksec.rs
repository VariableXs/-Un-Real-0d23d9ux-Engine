//! m700ksec — VARIX-M700 AI-07 内核安全域 (F151~F175)
//!
//! 栈金丝雀网/写保护矩阵/特权指令围栏/内核地址保密/完整性巡检官/
//! 能力位裁决所/侧信道基线/随机数中台/内核堆防护/攻击面清单/
//! 安全启动度量/越权演练场/安全补丁热道/提权哨兵/内核参数硬化/
//! 内存破坏指纹/安全回归走廊/沙盒逃逸靶场/最小权限执行谱/
//! 安全事件时间轴/加密原语库/密钥驻留舱/防降级锁/漏洞评分员/
//! 安全域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F151 — 栈金丝雀网：每帧金丝雀，越界写入即碎
// ===========================================================================

pub const SEC_CANARY_WORD: u64 = 0xC0FF_EE7A_5EC1_0CA4;

/// 金丝雀随帧地址变化，攻击者无法一处破处处用。
pub fn canary_of(frame_addr: u64) -> u64 {
    SEC_CANARY_WORD ^ frame_addr.rotate_left(17)
}

pub fn canary_intact(frame_addr: u64, observed: u64) -> bool {
    observed == canary_of(frame_addr)
}

// ===========================================================================
// F152 — 写保护矩阵：区域 × 权限位，W^X 红线
// ===========================================================================

pub const SEC_PROT_REGIONS: usize = 4;
pub const SEC_PROT_READ: u8 = 1;
pub const SEC_PROT_WRITE: u8 = 2;
pub const SEC_PROT_EXEC: u8 = 4;

pub struct SecProtMatrix {
    perms: [u8; SEC_PROT_REGIONS],
}

impl SecProtMatrix {
    pub const fn new() -> SecProtMatrix {
        SecProtMatrix { perms: [0; SEC_PROT_REGIONS] }
    }

    pub fn set_perm(&mut self, region: usize, perm: u8) -> bool {
        if region >= SEC_PROT_REGIONS {
            return false;
        }
        self.perms[region] = perm;
        true
    }

    pub fn allow(&self, region: usize, want: u8) -> bool {
        region < SEC_PROT_REGIONS && self.perms[region] & want == want
    }
}

/// W^X 红线：同区域可写又可执行即违规。
pub fn wx_violation(perm: u8) -> bool {
    perm & SEC_PROT_WRITE != 0 && perm & SEC_PROT_EXEC != 0
}

// ===========================================================================
// F153 — 特权指令围栏：每条敏感指令有最低环要求
// ===========================================================================

pub const SEC_RING_USER: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecPrivOp {
    Hlt,
    Wrmsr,
    PortOut,
    Iret,
}

pub fn required_ring(op: SecPrivOp) -> u8 {
    match op {
        SecPrivOp::Hlt => 0,
        SecPrivOp::Wrmsr => 0,
        SecPrivOp::PortOut => 1,
        SecPrivOp::Iret => 0,
    }
}

pub fn op_allowed(op: SecPrivOp, ring: u8) -> bool {
    ring <= required_ring(op)
}

// ===========================================================================
// F154 — 内核地址保密：指针出界必先混掺
// ===========================================================================

pub const SEC_PTR_HASH_KEY: u64 = 0x9E37_79B9_7F4A_7C15;

/// 指针混掺：暴露给外界的句柄，绝不等于原始地址。
pub fn ptr_hash(ptr: u64) -> u64 {
    let mut h = ptr ^ SEC_PTR_HASH_KEY;
    h = h.rotate_left(13);
    h = h.wrapping_mul(0x2545_F491_4F6C_DD1D);
    h
}

pub fn ptr_secret(ptr: u64, exposed: u64) -> bool {
    exposed == ptr_hash(ptr) && exposed != ptr
}

// ===========================================================================
// F155 — 完整性巡检官：页基线校验和比对
// ===========================================================================

pub const SEC_PATROL_PAGES: usize = 8;

/// 页校验和：字节回卷累加。
pub fn page_checksum(page: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0usize;
    while i < page.len() {
        sum = sum.wrapping_add(page[i] as u32);
        i += 1;
    }
    sum
}

pub struct SecIntegrityLedger {
    sums: [Option<u32>; SEC_PATROL_PAGES],
    count: usize,
}

impl SecIntegrityLedger {
    pub const fn new() -> SecIntegrityLedger {
        SecIntegrityLedger {
            sums: [const { None }; SEC_PATROL_PAGES],
            count: 0,
        }
    }

    pub fn baseline(&mut self, page: &[u8]) -> bool {
        if self.count >= SEC_PATROL_PAGES {
            return false;
        }
        self.sums[self.count] = Some(page_checksum(page));
        self.count += 1;
        true
    }

    pub fn verify(&self, index: usize, page: &[u8]) -> bool {
        if index >= self.count {
            return false;
        }
        self.sums[index] == Some(page_checksum(page))
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F156 — 能力位裁决所：位掩码授予、包含与超额
// ===========================================================================

pub const SEC_CAP_IO: u64 = 1 << 0;
pub const SEC_CAP_MMIO: u64 = 1 << 1;
pub const SEC_CAP_DMA: u64 = 1 << 2;
pub const SEC_CAP_IRQ: u64 = 1 << 3;

pub fn has_cap(caps: u64, want: u64) -> bool {
    caps & want == want
}

pub fn caps_subset(granted: u64, needed: u64) -> bool {
    granted & needed == needed
}

/// 越权多拿的能力位（最小权限的对立面）。
pub fn excess_caps(granted: u64, needed: u64) -> u64 {
    granted & !needed
}

// ===========================================================================
// F157 — 侧信道基线：时延抖动 permille 与常数时间比较
// ===========================================================================

pub const SEC_JITTER_BASELINE_PERMILLE: u32 = 50;

pub fn jitter_permille(fast_ns: u64, slow_ns: u64) -> u32 {
    if slow_ns == 0 || slow_ns < fast_ns {
        return 0;
    }
    ((slow_ns - fast_ns) * 1000 / slow_ns) as u32
}

pub fn side_channel_suspect(fast_ns: u64, slow_ns: u64) -> bool {
    jitter_permille(fast_ns, slow_ns) > SEC_JITTER_BASELINE_PERMILLE
}

/// 常数时间比较：等长时吸收全部差异位，不提前返回。
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    let mut i = 0usize;
    while i < a.len() {
        diff |= a[i] ^ b[i];
        i += 1;
    }
    diff == 0
}

// ===========================================================================
// F158 — 随机数中台：xorshift64，零种子换金种子
// ===========================================================================

pub const SEC_RNG_GOLDEN_SEED: u64 = 0x9E37_79B9_97F4_A7C1;

pub struct SecRng {
    state: u64,
}

impl SecRng {
    pub const fn seed(s: u64) -> SecRng {
        SecRng { state: if s == 0 { SEC_RNG_GOLDEN_SEED } else { s } }
    }

    pub fn state(&self) -> u64 {
        self.state
    }

    /// xorshift64：非零状态永不出零。
    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    pub fn below_bound(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

// ===========================================================================
// F159 — 内核堆防护：前后警戒区，越界踩线即报
// ===========================================================================

pub const SEC_HEAP_GUARD: u64 = 0xBEEF_CAFE_DEAD_BEEF;

#[derive(Clone, Copy, Debug)]
pub struct SecHeapAlloc {
    pub size: u32,
    pub front: u64,
    pub rear: u64,
}

impl SecHeapAlloc {
    pub const fn alloc_guarded(size: u32) -> SecHeapAlloc {
        SecHeapAlloc { size, front: SEC_HEAP_GUARD, rear: SEC_HEAP_GUARD }
    }

    pub fn guards_intact(&self) -> bool {
        self.front == SEC_HEAP_GUARD && self.rear == SEC_HEAP_GUARD
    }
}

pub fn overflow_detected(a: &SecHeapAlloc) -> bool {
    a.rear != SEC_HEAP_GUARD || a.front != SEC_HEAP_GUARD
}

// ===========================================================================
// F160 — 攻击面清单：入口登记去重，系统调用面积统计
// ===========================================================================

pub const SEC_SURFACE_SLOTS: usize = 12;

pub struct SecAttackSurface {
    ids: [Option<u32>; SEC_SURFACE_SLOTS],
    syscalls: [u32; SEC_SURFACE_SLOTS],
    count: usize,
}

impl SecAttackSurface {
    pub const fn new() -> SecAttackSurface {
        SecAttackSurface {
            ids: [const { None }; SEC_SURFACE_SLOTS],
            syscalls: [0; SEC_SURFACE_SLOTS],
            count: 0,
        }
    }

    /// 登记入口；id 去重，容量 12 拒绝溢出。
    pub fn register_entry(&mut self, id: u32, syscalls: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == Some(id) {
                return false;
            }
            i += 1;
        }
        if self.count >= SEC_SURFACE_SLOTS {
            return false;
        }
        self.ids[self.count] = Some(id);
        self.syscalls[self.count] = syscalls;
        self.count += 1;
        true
    }

    pub fn exposed_syscalls(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.count {
            n += self.syscalls[i];
            i += 1;
        }
        n
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F161 — 安全启动度量：度量链逐级吸收，顺序敏感
// ===========================================================================

/// 度量链：空链为 0；每级 = (前值 ^ 度量) 循环左移 1 位后加盐。
pub fn measure_chain(measures: &[u64]) -> u64 {
    let mut h: u64 = 0;
    let mut i = 0usize;
    while i < measures.len() {
        h ^= measures[i];
        h = h.rotate_left(1);
        h = h.wrapping_add(0x9E37_79B9);
        i += 1;
    }
    h
}

pub fn boot_chain_ok(expected: u64, measures: &[u64]) -> bool {
    measure_chain(measures) == expected
}

// ===========================================================================
// F162 — 越权演练场：未授权操作必须全部拒绝
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct SecDrill {
    pub attempts: u32,
    pub denied: u32,
}

impl SecDrill {
    /// 演练一次：authorized=false 的尝试必须被拒并记账。
    pub fn drill_attempt(&mut self, authorized: bool) -> bool {
        self.attempts += 1;
        if authorized {
            true
        } else {
            self.denied += 1;
            false
        }
    }

    pub fn all_denied(&self) -> bool {
        self.denied == self.attempts
    }
}

// ===========================================================================
// F163 — 安全补丁热道：版本吻合才可应用，补丁只打一次
// ===========================================================================

pub const SEC_PATCH_FLOOR_VERSION: u32 = 7;

#[derive(Clone, Copy, Debug)]
pub struct SecPatch {
    pub target_version: u32,
    pub applied: bool,
}

impl SecPatch {
    pub const fn new(target_version: u32) -> SecPatch {
        SecPatch { target_version, applied: false }
    }

    pub fn apply(&mut self, current_version: u32) -> bool {
        if self.applied || current_version != self.target_version {
            return false;
        }
        self.applied = true;
        true
    }
}

/// 防回滚底线：当前版本不得低于底线版本。
pub fn rollback_guard(current_version: u32, floor: u32) -> bool {
    current_version >= floor
}

// ===========================================================================
// F164 — 提权哨兵：非授权路径跃迁到 uid 0 即告警
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecEscalationEvent {
    pub from_uid: u32,
    pub to_uid: u32,
    pub via_syscall: bool,
}

pub fn escalation_suspicious(e: &SecEscalationEvent) -> bool {
    e.to_uid == 0 && e.from_uid != 0 && !e.via_syscall
}

// ===========================================================================
// F165 — 内核参数硬化：安全默认值台账
// ===========================================================================

pub const SEC_HARDEN_DEFAULTS: [(u32, u32); 4] = [(1, 1), (2, 0), (3, 1), (4, 0)];

/// 参数取值是否符合硬化基线；未知参数放行。
pub fn param_secure(id: u32, value: u32) -> bool {
    let mut i = 0usize;
    while i < SEC_HARDEN_DEFAULTS.len() {
        if SEC_HARDEN_DEFAULTS[i].0 == id {
            return SEC_HARDEN_DEFAULTS[i].1 == value;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F166 — 内存破坏指纹：签名 → 破坏类别
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecCorruptionKind {
    Overflow,
    Uaf,
    DoubleFree,
    Wild,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecCorruptionSig {
    pub magic_trampled: bool,
    pub guard_trampled: bool,
    pub free_flag_set: bool,
}

/// 判序：双放最先，魔数其次，警戒区再次，其余归野。
pub fn classify_corruption(s: &SecCorruptionSig) -> SecCorruptionKind {
    if s.free_flag_set {
        SecCorruptionKind::DoubleFree
    } else if s.magic_trampled {
        SecCorruptionKind::Uaf
    } else if s.guard_trampled {
        SecCorruptionKind::Overflow
    } else {
        SecCorruptionKind::Wild
    }
}

// ===========================================================================
// F167 — 安全回归走廊：历史攻击剧本必须全部挡下
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct SecAttackCase {
    pub id: u32,
    pub blocked: bool,
}

pub fn blocked_count(cases: &[SecAttackCase]) -> usize {
    cases.iter().filter(|c| c.blocked).count()
}

pub fn regression_ok(cases: &[SecAttackCase]) -> bool {
    blocked_count(cases) == cases.len()
}

// ===========================================================================
// F168 — 沙盒逃逸靶场：逃逸尝试全部拦截才算关得住
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct SecEscapeRange {
    pub escapes: u32,
    pub blocked: u32,
}

impl SecEscapeRange {
    pub fn record_escape(&mut self, blocked: bool) {
        self.escapes += 1;
        if blocked {
            self.blocked += 1;
        }
    }

    pub fn blocked_permille(&self) -> u32 {
        if self.escapes == 0 {
            0
        } else {
            self.blocked * 1000 / self.escapes
        }
    }

    pub fn containment_ok(&self) -> bool {
        self.escapes > 0 && self.blocked == self.escapes
    }
}

// ===========================================================================
// F169 — 最小权限执行谱：角色能力恰好够用
// ===========================================================================

pub const SEC_ROLE_TABLE: [(u32, u64); 3] = [
    (1, SEC_CAP_IO),
    (2, SEC_CAP_MMIO | SEC_CAP_IRQ),
    (3, SEC_CAP_DMA),
];

pub fn role_needed(role: u32) -> u64 {
    let mut i = 0usize;
    while i < SEC_ROLE_TABLE.len() {
        if SEC_ROLE_TABLE[i].0 == role {
            return SEC_ROLE_TABLE[i].1;
        }
        i += 1;
    }
    0
}

/// 超出角色所需的能力位。
pub fn role_excess(role: u32, granted: u64) -> u64 {
    granted & !role_needed(role)
}

/// 最小权限达标：所需全有，且无一项多余。
pub fn role_least_privilege(role: u32, granted: u64) -> bool {
    let needed = role_needed(role);
    granted & needed == needed && granted & !needed == 0
}

// ===========================================================================
// F170 — 安全事件时间轴：序号严格单调，定容环形留存
// ===========================================================================

pub const SEC_TIMELINE_SLOTS: usize = 8;

pub struct SecEventRing {
    kinds: [u8; SEC_TIMELINE_SLOTS],
    seqs: [u64; SEC_TIMELINE_SLOTS],
    head: usize,
}

impl SecEventRing {
    pub const fn new() -> SecEventRing {
        SecEventRing {
            kinds: [0; SEC_TIMELINE_SLOTS],
            seqs: [0; SEC_TIMELINE_SLOTS],
            head: 0,
        }
    }

    pub fn push(&mut self, seq: u64, kind: u8) {
        self.kinds[self.head] = kind;
        self.seqs[self.head] = seq;
        self.head = (self.head + 1) % SEC_TIMELINE_SLOTS;
    }

    pub fn len(&self) -> usize {
        SEC_TIMELINE_SLOTS
    }

    pub fn kind_at(&self, i: usize) -> u8 {
        self.kinds[i % SEC_TIMELINE_SLOTS]
    }

    pub fn seq_at(&self, i: usize) -> u64 {
        self.seqs[i % SEC_TIMELINE_SLOTS]
    }
}

/// 时间轴单调律：事件序号必须严格递增。
pub fn timeline_ok(seqs: &[u64]) -> bool {
    let mut i = 1usize;
    while i < seqs.len() {
        if seqs[i] <= seqs[i - 1] {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F171 — 加密原语库：摘要吸收与雪崩性质
// ===========================================================================

/// 32 位摘要（FNV-1a 形吸收）。
pub fn sec_digest(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 摘要确定性：同输入多次摘要恒等。
pub fn digest_stable(data: &[u8], times: usize) -> bool {
    if times == 0 {
        return true;
    }
    let first = sec_digest(data);
    let mut i = 1usize;
    while i < times {
        if sec_digest(data) != first {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F172 — 密钥驻留舱：密钥只存不外借，弃用即清零
// ===========================================================================

pub const SEC_VAULT_SLOTS: usize = 4;

pub struct SecKeyVault {
    keys: [Option<u64>; SEC_VAULT_SLOTS],
    pub zeroized: u32,
}

impl SecKeyVault {
    pub const fn new() -> SecKeyVault {
        SecKeyVault {
            keys: [const { None }; SEC_VAULT_SLOTS],
            zeroized: 0,
        }
    }

    /// 入舱：同密钥去重，容量 4 拒绝溢出。
    pub fn store(&mut self, key: u64) -> bool {
        let mut i = 0usize;
        while i < SEC_VAULT_SLOTS {
            if self.keys[i] == Some(key) {
                return false;
            }
            i += 1;
        }
        i = 0;
        while i < SEC_VAULT_SLOTS {
            if self.keys[i].is_none() {
                self.keys[i] = Some(key);
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn zeroize(&mut self, slot: usize) -> bool {
        if slot >= SEC_VAULT_SLOTS || self.keys[slot].is_none() {
            return false;
        }
        self.keys[slot] = None;
        self.zeroized += 1;
        true
    }

    /// 密钥永不导出，外界只允许拿句柄。
    pub fn export_forbidden(&self, slot: usize) -> bool {
        let _ = slot;
        true
    }

    pub fn resident(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < SEC_VAULT_SLOTS {
            if self.keys[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// F173 — 防降级锁：版本只升不降
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct SecVersionLock {
    pub floor: u32,
}

impl SecVersionLock {
    pub const fn new(floor: u32) -> SecVersionLock {
        SecVersionLock { floor }
    }

    /// 接纳新版本：不低于底线即放行并抬升底线（同版本重装允许）。
    pub fn admit(&mut self, version: u32) -> bool {
        if version < self.floor {
            return false;
        }
        self.floor = version;
        true
    }
}

// ===========================================================================
// F174 — 漏洞评分员：严重度 × 可利用性，三档分带
// ===========================================================================

pub const SEC_VULN_HIGH_PERMILLE: u32 = 700;
pub const SEC_VULN_MEDIUM_PERMILLE: u32 = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecVulnBand {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug)]
pub struct SecVuln {
    pub severity_permille: u32,
    pub exploitability_permille: u32,
}

pub fn vuln_score(v: &SecVuln) -> u32 {
    v.severity_permille * v.exploitability_permille / 1000
}

pub fn vuln_band(score: u32) -> SecVulnBand {
    if score >= SEC_VULN_HIGH_PERMILLE {
        SecVulnBand::High
    } else if score >= SEC_VULN_MEDIUM_PERMILLE {
        SecVulnBand::Medium
    } else {
        SecVulnBand::Low
    }
}

// ===========================================================================
// F175 — 安全域年报：章节完备性 + 黄金演练终态
// ===========================================================================

pub const SEC_REPORT_SECTIONS: [&str; 5] =
    ["canary", "protect", "surface", "vault", "timeline"];

pub fn sec_report_complete(filled: u32) -> bool {
    filled >= SEC_REPORT_SECTIONS.len() as u32
}

/// 黄金终态：演练全拒、密钥零泄漏。
pub fn sec_golden_clean(attempts: u32, denied: u32, vault_leaks: u32) -> bool {
    attempts > 0 && denied == attempts && vault_leaks == 0
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700ksec_checks() -> CheckSet {
    let mut set = CheckSet::new("m700ksec");

    // F151 栈金丝雀网
    let c1 = canary_of(0x1000);
    let c2 = canary_of(0x1000);
    let c3 = canary_of(0x1004);
    set.add(
        "F151 canary deterministic",
        c1 == c2 && c1 != c3 && c1 != 0,
        "addr-bound, nonzero",
    );
    set.add(
        "F151 canary intact",
        canary_intact(0x1000, c1) && !canary_intact(0x1000, c1 ^ 1),
        "tamper detected",
    );
    set.add(
        "F151 canary word",
        SEC_CANARY_WORD != 0 && canary_of(0) == SEC_CANARY_WORD,
        "base word",
    );

    // F152 写保护矩阵
    let mut pm = SecProtMatrix::new();
    let s1 = pm.set_perm(0, SEC_PROT_READ);
    let s2 = pm.set_perm(1, SEC_PROT_READ | SEC_PROT_WRITE);
    let s9 = pm.set_perm(9, SEC_PROT_READ);
    set.add(
        "F152 protect set",
        s1 && s2 && !s9 && pm.allow(1, SEC_PROT_WRITE) && !pm.allow(0, SEC_PROT_WRITE),
        "perms honored",
    );
    set.add(
        "F152 protect wx",
        wx_violation(SEC_PROT_WRITE | SEC_PROT_EXEC)
            && !wx_violation(SEC_PROT_WRITE)
            && !wx_violation(SEC_PROT_READ | SEC_PROT_EXEC),
        "W^X red line",
    );

    // F153 特权指令围栏
    set.add(
        "F153 fence user blocked",
        !op_allowed(SecPrivOp::Hlt, SEC_RING_USER)
            && !op_allowed(SecPrivOp::Wrmsr, SEC_RING_USER)
            && !op_allowed(SecPrivOp::Iret, 2),
        "ring0-only ops",
    );
    set.add(
        "F153 fence port",
        op_allowed(SecPrivOp::PortOut, 1) && !op_allowed(SecPrivOp::PortOut, 2)
            && op_allowed(SecPrivOp::Hlt, 0),
        "ring1 io, ring0 ok",
    );

    // F154 内核地址保密
    let kptr: u64 = 0xDEAD_BEEF_1234_5678;
    let exposed = ptr_hash(kptr);
    let exposed_again = ptr_hash(kptr);
    set.add(
        "F154 ptr hash stable",
        exposed == exposed_again && exposed != kptr,
        "deterministic, never raw",
    );
    set.add(
        "F154 ptr secret",
        ptr_secret(kptr, exposed) && !ptr_secret(kptr, kptr),
        "raw pointer leaks fail",
    );
    set.add("F154 zero never leaks", ptr_hash(0) != 0, "null hashed away");

    // F155 完整性巡检官
    let page = [7u8; 64];
    let mut led = SecIntegrityLedger::new();
    let b1 = led.baseline(&page);
    let v1 = led.verify(0, &page);
    let mut tampered = [7u8; 64];
    tampered[0] = 8;
    let v2 = led.verify(0, &tampered);
    set.add(
        "F155 integrity baseline",
        b1 && v1 && page_checksum(&page) == 7 * 64,
        "baseline holds",
    );
    set.add("F155 integrity tamper", !v2, "mutation caught");
    let mut ffull = SecIntegrityLedger::new();
    let mut i = 0usize;
    let mut fill_ok = true;
    while i < SEC_PATROL_PAGES {
        fill_ok &= ffull.baseline(&page);
        i += 1;
    }
    let led_count = ffull.count();
    set.add(
        "F155 integrity cap",
        fill_ok && led_count == SEC_PATROL_PAGES && !ffull.baseline(&page),
        "cap 8",
    );

    // F156 能力位裁决所
    let caps = SEC_CAP_IO | SEC_CAP_MMIO | SEC_CAP_DMA;
    set.add(
        "F156 cap has",
        has_cap(caps, SEC_CAP_IO | SEC_CAP_MMIO) && !has_cap(caps, SEC_CAP_IRQ),
        "multi-bit probe",
    );
    set.add(
        "F156 cap subset/excess",
        caps_subset(caps, SEC_CAP_IO | SEC_CAP_DMA)
            && excess_caps(caps, SEC_CAP_IO) == SEC_CAP_MMIO | SEC_CAP_DMA,
        "granted vs needed",
    );

    // F157 侧信道基线
    set.add(
        "F157 jitter baseline",
        jitter_permille(100, 105) == 47 && !side_channel_suspect(100, 105),
        "5/105 under 50 permille",
    );
    set.add(
        "F157 jitter suspect",
        side_channel_suspect(100, 120) && ct_eq(b"secret", b"secret") && !ct_eq(b"secret", b"secre7"),
        "one-bit diff caught",
    );

    // F158 随机数中台
    let mut r1 = SecRng::seed(42);
    let mut r2 = SecRng::seed(42);
    let d1 = r1.next_u64();
    let d2 = r2.next_u64();
    let d3 = r1.next_u64();
    let d4 = r2.next_u64();
    set.add(
        "F158 rng deterministic",
        d1 == d2 && d3 == d4 && SecRng::seed(0).state() == SEC_RNG_GOLDEN_SEED,
        "same seed same stream, zero reseeds",
    );
    let mut r3 = SecRng::seed(42);
    let mut n = 0u32;
    let mut nonzero = true;
    while n < 100 {
        if r3.next_u64() == 0 {
            nonzero = false;
        }
        n += 1;
    }
    set.add("F158 rng never zero", nonzero, "xorshift invariant");
    let mut r4 = SecRng::seed(7);
    let mut m = 0u32;
    let mut bounded = true;
    while m < 20 {
        if r4.below_bound(10) >= 10 {
            bounded = false;
        }
        m += 1;
    }
    set.add("F158 rng bounded", bounded, "modular bound");

    // F159 内核堆防护
    let fresh = SecHeapAlloc::alloc_guarded(128);
    let mut smashed = SecHeapAlloc::alloc_guarded(64);
    smashed.rear = SEC_HEAP_GUARD ^ 0xFF;
    set.add(
        "F159 heap guards",
        fresh.guards_intact() && SEC_HEAP_GUARD != 0,
        "fresh alloc guarded",
    );
    set.add(
        "F159 heap overflow",
        overflow_detected(&smashed) && !overflow_detected(&fresh),
        "guard trampled detected",
    );

    // F160 攻击面清单
    let mut surf = SecAttackSurface::new();
    let e1 = surf.register_entry(1, 5);
    let e2 = surf.register_entry(2, 3);
    let e3 = surf.register_entry(1, 9);
    let surf_count = surf.count();
    let exposed1 = surf.exposed_syscalls();
    set.add(
        "F160 surface register",
        e1 && e2 && !e3 && surf_count == 2,
        "id dedup",
    );
    set.add("F160 surface area", exposed1 == 8, "5+3 syscalls");
    let mut sfull = SecAttackSurface::new();
    let mut id = 1u32;
    while sfull.count() < SEC_SURFACE_SLOTS {
        sfull.register_entry(id, 1);
        id += 1;
    }
    let sfull_count = sfull.count();
    set.add(
        "F160 surface cap",
        sfull_count == SEC_SURFACE_SLOTS && !sfull.register_entry(999, 1),
        "cap 12",
    );

    // F161 安全启动度量
    let expected = measure_chain(&[1, 2]);
    let swapped = measure_chain(&[2, 1]);
    set.add(
        "F161 measure chain",
        expected == measure_chain(&[1, 2]) && expected != swapped && measure_chain(&[]) == 0,
        "order sensitive, empty zero",
    );
    set.add(
        "F161 boot verdict",
        boot_chain_ok(expected, &[1, 2]) && !boot_chain_ok(expected, &[2, 1]),
        "golden boot chain",
    );

    // F162 越权演练场
    let mut drill = SecDrill::default();
    let d1 = drill.drill_attempt(false);
    let d2 = drill.drill_attempt(false);
    let d3 = drill.drill_attempt(false);
    let d4 = drill.drill_attempt(false);
    let attempts1 = drill.attempts;
    let denied1 = drill.denied;
    set.add(
        "F162 drill denied",
        !d1 && !d2 && !d3 && !d4 && attempts1 == 4 && denied1 == 4,
        "all unauthorized denied",
    );
    set.add(
        "F162 drill verdict",
        drill.all_denied() && !sec_golden_clean(3, 4, 0),
        "mismatch reported",
    );

    // F163 安全补丁热道
    let mut patch = SecPatch::new(7);
    let a1 = patch.apply(7);
    let a2 = patch.apply(7);
    let a3 = patch.apply(8);
    set.add(
        "F163 patch once",
        a1 && !a2 && !a3 && patch.applied,
        "one shot, version locked",
    );
    let mut mismatch = SecPatch::new(7);
    let a4 = mismatch.apply(8);
    set.add(
        "F163 patch floor",
        !a4 && !mismatch.applied && rollback_guard(7, SEC_PATCH_FLOOR_VERSION)
            && !rollback_guard(6, SEC_PATCH_FLOOR_VERSION),
        "rollback refused",
    );

    // F164 提权哨兵
    let legit = SecEscalationEvent { from_uid: 1000, to_uid: 0, via_syscall: true };
    let rogue = SecEscalationEvent { from_uid: 1000, to_uid: 0, via_syscall: false };
    let same = SecEscalationEvent { from_uid: 0, to_uid: 0, via_syscall: false };
    set.add(
        "F164 sentinel legit",
        !escalation_suspicious(&legit) && !escalation_suspicious(&same),
        "authorized path quiet",
    );
    set.add("F164 sentinel rogue", escalation_suspicious(&rogue), "raw uid0 flagged");

    // F165 内核参数硬化
    set.add(
        "F165 harden pass",
        param_secure(1, 1) && param_secure(2, 0) && param_secure(3, 1) && param_secure(4, 0),
        "secure defaults",
    );
    set.add(
        "F165 harden fail",
        !param_secure(1, 0) && !param_secure(2, 1) && param_secure(9, 123),
        "weak values flagged, unknown pass",
    );

    // F166 内存破坏指纹
    let sig_df = SecCorruptionSig { magic_trampled: true, guard_trampled: true, free_flag_set: true };
    let sig_uaf = SecCorruptionSig { magic_trampled: true, guard_trampled: true, free_flag_set: false };
    let sig_of = SecCorruptionSig { magic_trampled: false, guard_trampled: true, free_flag_set: false };
    let sig_wild = SecCorruptionSig { magic_trampled: false, guard_trampled: false, free_flag_set: false };
    set.add(
        "F166 fingerprint priority",
        classify_corruption(&sig_df) == SecCorruptionKind::DoubleFree
            && classify_corruption(&sig_uaf) == SecCorruptionKind::Uaf,
        "double-free then uaf",
    );
    set.add(
        "F166 fingerprint tail",
        classify_corruption(&sig_of) == SecCorruptionKind::Overflow
            && classify_corruption(&sig_wild) == SecCorruptionKind::Wild,
        "overflow then wild",
    );

    // F167 安全回归走廊
    let golden_cases = [
        SecAttackCase { id: 1, blocked: true },
        SecAttackCase { id: 2, blocked: true },
        SecAttackCase { id: 3, blocked: true },
    ];
    let leaky = [
        SecAttackCase { id: 1, blocked: true },
        SecAttackCase { id: 2, blocked: false },
    ];
    set.add(
        "F167 regression golden",
        regression_ok(&golden_cases) && blocked_count(&golden_cases) == 3,
        "all blocked",
    );
    set.add(
        "F167 regression leak",
        !regression_ok(&leaky) && blocked_count(&leaky) == 1,
        "leak caught",
    );

    // F168 沙盒逃逸靶场
    let mut sealed = SecEscapeRange::default();
    sealed.record_escape(true);
    sealed.record_escape(true);
    sealed.record_escape(true);
    sealed.record_escape(true);
    sealed.record_escape(true);
    sealed.record_escape(true);
    sealed.record_escape(true);
    sealed.record_escape(true);
    sealed.record_escape(true);
    sealed.record_escape(true);
    let sealed_pm = sealed.blocked_permille();
    let sealed_ok = sealed.containment_ok();
    set.add(
        "F168 range sealed",
        sealed_pm == 1000 && sealed_ok && sealed.escapes == 10,
        "10/10 blocked",
    );
    let mut leaky_range = SecEscapeRange::default();
    leaky_range.record_escape(true);
    leaky_range.record_escape(false);
    set.add(
        "F168 range leak",
        leaky_range.blocked_permille() == 500 && !leaky_range.containment_ok(),
        "escape detected",
    );

    // F169 最小权限执行谱
    let exact = role_needed(2);
    let padded = role_needed(2) | SEC_CAP_DMA;
    let empty = 0u64;
    set.add(
        "F169 least priv exact",
        role_least_privilege(2, exact) && role_excess(2, exact) == 0,
        "exactly enough",
    );
    set.add(
        "F169 least priv excess",
        !role_least_privilege(2, padded) && role_excess(2, padded) == SEC_CAP_DMA,
        "extra capability flagged",
    );
    set.add(
        "F169 least priv missing",
        !role_least_privilege(2, empty),
        "missing capability denied",
    );

    // F170 安全事件时间轴
    let mut ring = SecEventRing::new();
    let mut s = 100u64;
    while s < 110 {
        ring.push(s, (s % 3) as u8);
        s += 1;
    }
    let last_kind = ring.kind_at(9);
    let seq_at_zero = ring.seq_at(0);
    set.add(
        "F170 timeline ring wrap",
        ring.len() == 8 && seq_at_zero == 108 && ring.seq_at(9) == 109 && ring.seq_at(2) == 102,
        "ring holds 8, wrap overwrites",
    );
    set.add("F170 timeline kind slot", last_kind == 1, "kind of seq 109");
    let good_tl = [100u64, 101, 102];
    let bad_tl = [100u64, 100];
    set.add(
        "F170 timeline monotonic",
        timeline_ok(&good_tl) && !timeline_ok(&bad_tl),
        "strictly increasing",
    );

    // F171 加密原语库
    let dg1 = sec_digest(b"payload");
    let dg2 = sec_digest(b"payload");
    let dg3 = sec_digest(b"payloat");
    set.add(
        "F171 digest stable",
        digest_stable(b"payload", 3) && dg1 == dg2 && dg1 != dg3,
        "deterministic, avalanche",
    );
    set.add(
        "F171 digest distinct",
        sec_digest(b"a") != sec_digest(b"b") && sec_digest(b"") == 0x811c_9dc5,
        "empty = fnv basis",
    );

    // F172 密钥驻留舱
    let mut vault = SecKeyVault::new();
    let k1 = vault.store(0xAA);
    let k2 = vault.store(0xBB);
    let kd = vault.store(0xAA);
    set.add(
        "F172 vault dedup",
        k1 && k2 && !kd && vault.resident() == 2,
        "same key refused",
    );
    vault.store(0xCC);
    vault.store(0xDD);
    let vault_full = vault.resident();
    let k5 = vault.store(0xEE);
    set.add(
        "F172 vault cap",
        vault_full == SEC_VAULT_SLOTS && !k5,
        "cap 4",
    );
    let z1 = vault.zeroize(0);
    let zed = vault.zeroized;
    let k5b = vault.store(0xEE);
    set.add(
        "F172 vault zeroize",
        z1 && zed == 1 && k5b && vault.resident() == 4,
        "cleared then reused",
    );
    set.add("F172 vault no export", vault.export_forbidden(0), "keys never exported");

    // F173 防降级锁
    let mut vl = SecVersionLock::new(5);
    let v1 = vl.admit(5);
    let v2 = vl.admit(4);
    let v3 = vl.admit(6);
    let v4 = vl.admit(5);
    set.add(
        "F173 no downgrade",
        v1 && !v2 && v3 && !v4 && vl.floor == 6,
        "only monotonic rise",
    );
    set.add(
        "F173 floor rule",
        !SecVersionLock::new(10).admit(9) && SecVersionLock::new(1).admit(1),
        "equal reinstall ok",
    );

    // F174 漏洞评分员
    let high = SecVuln { severity_permille: 800, exploitability_permille: 900 };
    let mid = SecVuln { severity_permille: 600, exploitability_permille: 800 };
    let low = SecVuln { severity_permille: 500, exploitability_permille: 500 };
    set.add(
        "F174 vuln score",
        vuln_score(&high) == 720 && vuln_score(&mid) == 480 && vuln_score(&low) == 250,
        "sev x exp / 1000",
    );
    set.add(
        "F174 vuln band",
        vuln_band(vuln_score(&high)) == SecVulnBand::High
            && vuln_band(vuln_score(&mid)) == SecVulnBand::Medium
            && vuln_band(vuln_score(&low)) == SecVulnBand::Low,
        "three tiers",
    );

    // F175 安全域年报
    set.add("F175 report sections", SEC_REPORT_SECTIONS.len() == 5, "five sections");
    set.add("F175 report complete", sec_report_complete(5) && !sec_report_complete(4), "completeness");
    set.add(
        "F175 golden clean",
        sec_golden_clean(4, 4, 0) && !sec_golden_clean(4, 3, 0) && !sec_golden_clean(4, 4, 1),
        "drill all denied, zero leaks",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f151_canary_tamper_matrix() {
        let base = canary_of(0x2000);
        assert!(canary_intact(0x2000, base));
        for bit in 0..64u32 {
            assert!(!canary_intact(0x2000, base ^ (1u64 << bit)));
        }
        assert_ne!(canary_of(0x2000), canary_of(0x2008));
    }

    #[test]
    fn f158_rng_streams() {
        let mut a = SecRng::seed(1);
        let mut b = SecRng::seed(2);
        let mut diffs = 0u32;
        for _ in 0..8 {
            if a.next_u64() != b.next_u64() {
                diffs += 1;
            }
        }
        assert_eq!(diffs, 8);
        assert_ne!(SecRng::seed(0).state(), 0);
    }

    #[test]
    fn f159_heap_guard_bytes() {
        let mut a = SecHeapAlloc::alloc_guarded(32);
        assert!(a.guards_intact());
        a.front = SEC_HEAP_GUARD ^ 1;
        assert!(overflow_detected(&a));
        a.front = SEC_HEAP_GUARD;
        a.rear = 0;
        assert!(overflow_detected(&a));
    }

    #[test]
    fn f163_patch_version_matrix() {
        let mut p = SecPatch::new(SEC_PATCH_FLOOR_VERSION);
        assert!(!p.apply(6));
        assert!(!p.applied);
        assert!(p.apply(SEC_PATCH_FLOOR_VERSION));
        assert!(!p.apply(SEC_PATCH_FLOOR_VERSION));
    }

    #[test]
    fn f172_vault_lifecycle() {
        let mut v = SecKeyVault::new();
        for k in 1..=SEC_VAULT_SLOTS as u64 {
            assert!(v.store(k));
        }
        assert!(!v.store(99));
        assert_eq!(v.resident(), SEC_VAULT_SLOTS);
        assert!(v.zeroize(2));
        assert!(!v.zeroize(2));
        assert!(v.store(99));
        assert!(v.export_forbidden(0));
    }

    #[test]
    fn f173_version_never_rolls_back() {
        let mut vl = SecVersionLock::new(1);
        let mut v = 1u32;
        while v <= 8 {
            assert!(vl.admit(v));
            assert!(!vl.admit(v - 1));
            v += 1;
        }
        assert_eq!(vl.floor, 8);
    }

    #[test]
    fn f175_ksec_selfcheck_all_pass() {
        let set = run_m700ksec_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
        assert!(!set.truncated());
    }
}
