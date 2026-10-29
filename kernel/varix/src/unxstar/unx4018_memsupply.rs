//! unx4018_memsupply — UNX-F4018 内存供给接口消费锚（fake A4）（诊断段 0x84E0）。
//!
//! 判据原文：
//!   J1 fake 分配器配额账（六类对象各自配额）峰值不超限：每类 used ≤ quota
//!      且 peak 记录准确。
//!   J2 分配失败注入路径返回 ENOMEM（=12）且六类对象计数账零破坏：失败不
//!      改变 used/peak/成功账，只进失败账。
//!   J3 释放语义：释放不得超过已分配量，跨类释放拒收（E_CLASS_MISMATCH）。
//!   J4 六类账目总量守恒：Σused == 成功分配 - 成功释放；Σpeak 记录各峰值。
//! 设计纪律：
//!   1. 六类对象锚定 VFS 常驻对象：inode/dentry/file/mount/superblock/buffer；
//!      每类独立配额（quota）与计数（used/peak），互不挪用（硬隔离）。
//!   2. 分配请求以「对象个数」计量（本模块只做配额账，不做真实字节分配），
//!      但保留 size_class 便于后续接入真实分配器。
//!   3. 失败注入不改变任何成功侧账目——这是 fake A4 的契约核心（判据 J2）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x84E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOMEM: u16 = SEG_BASE + 1;      // 配额耗尽（ENOMEM 投影）
const E_CLASS_BAD: u16 = SEG_BASE + 2;  // 对象类别非法
const E_CLASS_MISMATCH: u16 = SEG_BASE + 3; // 跨类释放/释放超量
const E_QUOTA_ZERO: u16 = SEG_BASE + 4; // 配额为 0 的类不可分配
const E_SIZE_BAD: u16 = SEG_BASE + 5;   // 尺寸类非法（0 或超上限）
const E_BAL: u16 = SEG_BASE + 6;        // 六类账目总量不平

/// ENOMEM 数值（与 errno 基座一致，判据 J2 要求返回 12）。
pub const ENOMEM: u16 = 12;

/// 六类 VFS 常驻对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjClass {
    Inode = 0,
    Dentry = 1,
    File = 2,
    Mount = 3,
    Superblock = 4,
    Buffer = 5,
}

pub const CLASS_COUNT: usize = 6;

/// 类别名（导出与日志）。
pub const CLASS_NAMES: [&'static str; CLASS_COUNT] = [
    "inode", "dentry", "file", "mount", "superblock", "buffer",
];

/// 每类配额锚（fake A4 的静态额度表，取自典型 64MB 分区量级估算）。
pub const QUOTA_TAB: [u32; CLASS_COUNT] = [
    65_536, // inode 缓存上限
    131_072, // dentry 缓存上限
    8_192,  // 打开 file 上限
    1_024,  // mount 上限
    256,    // sb 上限
    16_384, // buffer 上限
];

/// 尺寸类上限（单对象字节量级，保留给真实分配器）。
pub const SIZE_CLASS_MAX: u32 = 8;

/// 单类配额账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassQuota {
    pub class: ObjClass, // 类别
    pub quota: u32,      // 配额上限
    pub used: u32,       // 当前占用
    pub peak: u32,       // 历史峰值
    pub granted: u32,    // 累计成功分配
    pub freed: u32,      // 累计成功释放
    pub denied: u32,     // 累计拒绝（配额耗尽）
}

impl ClassQuota {
    fn new(c: ObjClass, quota: u32) -> ClassQuota {
        ClassQuota {
            class: c,
            quota,
            used: 0,
            peak: 0,
            granted: 0,
            freed: 0,
            denied: 0,
        }
    }

    /// 峰值不超限（判据 J1）。
    pub fn peak_ok(&self) -> bool {
        self.peak <= self.quota && self.used <= self.quota
    }
}

/// 供给状态机：Open（有配额余量）↔ Saturated（至少一类触顶）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SupplyPhase {
    Open = 0,      // 六类均有余量
    Saturated = 1, // 至少一类配额触顶
}

/// 内存供给锚：六类配额 + 失败注入 + 总量账。
pub struct MemSupply {
    pub classes: [ClassQuota; CLASS_COUNT], // 六类账
    pub fail_injected: u32,                 // 人为失败注入次数
    pub fail_armed: bool,                   // 失败注入是否武装
    pub total_granted: u32,                 // 总量成功分配
    pub total_freed: u32,                   // 总量成功释放
    pub total_denied: u32,                  // 总量拒绝
    pub phase: SupplyPhase,                 // 供给状态机
}

impl MemSupply {
    pub fn new() -> MemSupply {
        MemSupply {
            classes: [
                ClassQuota::new(ObjClass::Inode, QUOTA_TAB[0]),
                ClassQuota::new(ObjClass::Dentry, QUOTA_TAB[1]),
                ClassQuota::new(ObjClass::File, QUOTA_TAB[2]),
                ClassQuota::new(ObjClass::Mount, QUOTA_TAB[3]),
                ClassQuota::new(ObjClass::Superblock, QUOTA_TAB[4]),
                ClassQuota::new(ObjClass::Buffer, QUOTA_TAB[5]),
            ],
            fail_injected: 0,
            fail_armed: false,
            total_granted: 0,
            total_freed: 0,
            total_denied: 0,
            phase: SupplyPhase::Open,
        }
    }

    /// 状态机读出口。
    pub fn phase(&self) -> SupplyPhase {
        self.phase
    }

    /// 重算供给状态：任一类的 used 触顶即 Saturated，否则 Open。
    fn refresh_phase(&mut self) {
        let mut sat = false;
        for q in self.classes.iter() {
            if q.used >= q.quota {
                sat = true;
                break;
            }
        }
        self.phase = if sat { SupplyPhase::Saturated } else { SupplyPhase::Open };
    }

    /// 武装/解除失败注入（下一次分配必失败）。
    pub fn arm_fail(&mut self, on: bool) {
        self.fail_armed = on;
    }

    fn idx(c: ObjClass) -> usize {
        c as usize
    }

    /// 分配：配额校验 → 失败注入校验 → 记账（判据 J1/J2）。
    pub fn alloc(&mut self, c: ObjClass, n: u32, size_class: u32) -> Result<u32, u16> {
        let i = MemSupply::idx(c);
        if i >= CLASS_COUNT {
            return Err(E_CLASS_BAD);
        }
        if size_class == 0 || size_class > SIZE_CLASS_MAX {
            return Err(E_SIZE_BAD);
        }
        if n == 0 {
            return Err(E_SIZE_BAD);
        }
        // 失败注入优先于配额（模拟 A4 返回 NULL 的路径）。
        if self.fail_armed {
            self.fail_armed = false;
            self.fail_injected += 1;
            self.classes[i].denied += 1;
            self.total_denied += 1;
            return Err(ENOMEM);
        }
        if self.classes[i].quota == 0 {
            return Err(E_QUOTA_ZERO);
        }
        let next = match self.classes[i].used.checked_add(n) {
            Some(v) => v,
            None => {
                self.classes[i].denied += 1;
                self.total_denied += 1;
                return Err(ENOMEM);
            }
        };
        if next > self.classes[i].quota {
            // 配额耗尽：账面不变，只记拒绝（判据 J2 的「零破坏」）。
            self.classes[i].denied += 1;
            self.total_denied += 1;
            return Err(ENOMEM);
        }
        self.classes[i].used = next;
        if next > self.classes[i].peak {
            self.classes[i].peak = next;
        }
        self.classes[i].granted += n;
        self.total_granted += n;
        self.refresh_phase(); // 触顶即转饱和
        Ok(self.classes[i].used)
    }

    /// 释放：不得超量、不得跨类（判据 J3）。
    pub fn free(&mut self, c: ObjClass, n: u32) -> Result<u32, u16> {
        let i = MemSupply::idx(c);
        if i >= CLASS_COUNT {
            return Err(E_CLASS_BAD);
        }
        if n == 0 {
            return Err(E_SIZE_BAD);
        }
        if n > self.classes[i].used {
            return Err(E_CLASS_MISMATCH);
        }
        self.classes[i].used -= n;
        self.classes[i].freed += n;
        self.total_freed += n;
        self.refresh_phase(); // 腾出余量即转开放
        Ok(self.classes[i].used)
    }

    /// 总量守恒：Σused == granted - freed（判据 J4）。
    pub fn conserve(&self) -> bool {
        let mut used_sum = 0u32;
        let mut delta = 0i64;
        for q in self.classes.iter() {
            used_sum += q.used;
            delta += q.granted as i64 - q.freed as i64;
        }
        if used_sum as i64 != delta {
            return false;
        }
        self.total_granted as i64 - self.total_freed as i64 == used_sum as i64
            && self.total_denied == {
                let mut d = 0u32;
                for q in self.classes.iter() {
                    d += q.denied;
                }
                d
            }
    }

    /// 六类峰值全不超限（判据 J1 的汇总面）。
    pub fn peaks_ok(&self) -> bool {
        for q in self.classes.iter() {
            if !q.peak_ok() {
                return false;
            }
        }
        true
    }

    /// 单类账导出。
    pub fn class_of(&self, c: ObjClass) -> ClassQuota {
        self.classes[MemSupply::idx(c)]
    }

    /// 类别名表导出。
    pub fn class_names(&self) -> [&'static str; CLASS_COUNT] {
        CLASS_NAMES
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOMEM => "配额耗尽（投影 ENOMEM=12）",
        E_CLASS_BAD => "对象类别非法",
        E_CLASS_MISMATCH => "跨类释放或释放超量",
        E_QUOTA_ZERO => "配额为 0 的类不可分配",
        E_SIZE_BAD => "尺寸类非法（0 或超上限 8）",
        E_BAL => "六类账目总量不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：六类配额 → 峰值校验 → 失败注入 → 释放 → 注入拦截 → 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut m = MemSupply::new();

    // R1：六类各分配一次，六类 used/granted 各自独立（判据 J1 分发面）。
    let kinds = [
        ObjClass::Inode, ObjClass::Dentry, ObjClass::File,
        ObjClass::Mount, ObjClass::Superblock, ObjClass::Buffer,
    ];
    for k in kinds.iter() {
        m.alloc(*k, 1, 1)?;
    }
    for k in kinds.iter() {
        if m.class_of(*k).used != 1 || m.class_of(*k).granted != 1 {
            return Err(E_BAL);
        }
    }
    if m.total_granted != 6 || !m.peaks_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：单类批量分配到峰值——inode 类 1,000 个，peak=1,001 仍 ≤65,536。
    m.alloc(ObjClass::Inode, 1_000, 2)?;
    if m.class_of(ObjClass::Inode).peak != 1_001 {
        return Err(E_BAL);
    }
    if !m.peaks_ok() || !m.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：配额上限逼近——把 file 类填到配额满（配额 8,192 已用 1）。
    m.alloc(ObjClass::File, 8_191, 3)?;
    if m.class_of(ObjClass::File).used != 8_192 {
        return Err(E_BAL);
    }
    if m.phase() != SupplyPhase::Saturated {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：再分配 1 个 → ENOMEM，且账面零破坏（判据 J2 的配额面）。
    let before_used = m.class_of(ObjClass::File).used;
    let before_granted = m.class_of(ObjClass::File).granted;
    if m.alloc(ObjClass::File, 1, 3) != Err(ENOMEM) {
        return Err(E_NOMEM);
    }
    if m.class_of(ObjClass::File).used != before_used
        || m.class_of(ObjClass::File).granted != before_granted
    {
        return Err(E_BAL);
    }
    if m.class_of(ObjClass::File).denied != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：人为失败注入——武装后下一次分配必 ENOMEM，失败账 +1。
    m.arm_fail(true);
    if m.alloc(ObjClass::Dentry, 1, 1) != Err(ENOMEM) {
        return Err(E_NOMEM);
    }
    if m.fail_injected != 1 || m.class_of(ObjClass::Dentry).used != 1 {
        return Err(E_BAL);
    }
    // 注入是一次性的：再次分配成功。
    m.alloc(ObjClass::Dentry, 1, 1)?;
    if m.class_of(ObjClass::Dentry).used != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：释放——inode 释放 1,001 回 0，峰值保留（peak 不回落）。
    m.free(ObjClass::Inode, 1_001)?;
    if m.class_of(ObjClass::Inode).used != 0 || m.class_of(ObjClass::Inode).peak != 1_001 {
        return Err(E_BAL);
    }
    if !m.conserve() || !m.peaks_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：六类名表 6 项可导出、内容锚定。
    let names = m.class_names();
    if names.len() != 6 || names[0] != "inode" || names[5] != "buffer" {
        return Err(E_CLASS_BAD);
    }
    hits += 1;

    // R8：反复分配/释放 1,000 次，峰值恒定且守恒（长跑面）。
    for _ in 0..1_000u32 {
        m.alloc(ObjClass::Buffer, 2, 4)?;
        m.free(ObjClass::Buffer, 2)?;
    }
    if m.class_of(ObjClass::Buffer).used != 1 || m.class_of(ObjClass::Buffer).peak != 3 {
        return Err(E_BAL);
    }
    if !m.conserve() {
        return Err(E_BAL);
    }
    // file 类仍触顶 → 全局饱和；释放 file 后回到开放态。
    if m.phase() != SupplyPhase::Saturated {
        return Err(E_BAL);
    }
    m.free(ObjClass::File, 8_192)?;
    if m.phase() != SupplyPhase::Open || !m.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：释放超量注入被拒（判据 J3）。
    if m.free(ObjClass::Buffer, 5) != Err(E_CLASS_MISMATCH) {
        return Err(E_CLASS_MISMATCH);
    }
    hits += 1;

    // I2：尺寸类 0 / 超上限双注入被拒。
    if m.alloc(ObjClass::Inode, 1, 0) != Err(E_SIZE_BAD) {
        return Err(E_SIZE_BAD);
    }
    if m.alloc(ObjClass::Inode, 1, SIZE_CLASS_MAX + 1) != Err(E_SIZE_BAD) {
        return Err(E_SIZE_BAD);
    }
    hits += 1;

    // I3：请求 0 个注入被拒（无意义分配不入账）。
    if m.alloc(ObjClass::Mount, 0, 1) != Err(E_SIZE_BAD) {
        return Err(E_SIZE_BAD);
    }
    hits += 1;

    // I4：失败注入不破坏六类计数——逐类 used 与 granted-freed 仍平。
    if !m.conserve() {
        return Err(E_BAL);
    }
    for k in kinds.iter() {
        let q = m.class_of(*k);
        if q.granted as i64 - q.freed as i64 != q.used as i64 {
            return Err(E_BAL);
        }
    }
    hits += 1;

    Ok(hits)
}
