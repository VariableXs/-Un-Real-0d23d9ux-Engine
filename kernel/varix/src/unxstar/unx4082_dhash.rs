//! unx4082_dhash — UNX-F4082 d_hash 哈希与名字比较钩子（诊断段 0x8C60）。
//!
//! 判据原文：UNX-F4082-J1 名字哈希对 10,000 随机名分布极差有账，d_compare 钩子
//!           替换后比较行为随之切换（字节级/语义级两模式可切换可验）。
//! 可验性质（本模块自证面）：
//!   J1 10,000 个名字落 16 桶，桶计数之和恒等 10,000（一名字一桶，不重不漏），
//!      极差（max - min）与各桶计数同时入账，空桶不得出现（16 桶全非零）；
//!   J2 同一名字哈希两次必等（纯函数性）；不同名字允许碰撞，碰撞入账不报错；
//!   J3 d_compare 钩子在 ByteExact 下 "Abc" != "abc"，切到 CaseFold 后两者相等，
//!      切回 ByteExact 又不等——模式切换可验且立即生效（无缓存残留）；
//!   J4 钩子调用账守恒：calls == eq + ne，两模式共用同一本账；
//!   J5 空名字比较拒收（E_NAME_EMPTY），模式码越界拒收（E_MODE），
//!      桶号越界查询拒收（E_BUCKET）。
//! 设计纪律：
//!   1. 哈希函数写死 FNV-1a（offset 0x811C_9DC5 / prime 0x0100_0193）：
//!      VFS d_hash 只要求快与确定，不要求抗碰撞；纯整数算术，无浮点无查表。
//!   2. 桶号取 hash 高 4 位（> 28），低 28 位留给将来扩表；桶数 16 是判据口径，
//!      写死为 BUCKETS 且与桶数组长度强绑定。
//!   3. 比较钩子按"函数指针语义"建模成枚举 + 分支：ByteExact 逐字节比，CaseFold
//!      只折叠 ASCII A-Z（Unix 人格本身不做折叠，折叠只给 D3 转换层预留入口），
//!      折叠函数 fold() 是唯一改动点，别处不许再写大小写逻辑。
//!   4. 比较前先比长度：长度不等在 ByteExact 下立即判不等，不得进入逐字节循环
//!      （防越界读）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8C60 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8C60;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_EMPTY: u16 = SEG_BASE + 1; // 空名字（dentry 名至少 1 字节）
const E_MODE: u16 = SEG_BASE + 2; // 比较模式码越界
const E_BUCKET: u16 = SEG_BASE + 3; // 桶号越界
const E_BAL: u16 = SEG_BASE + 4; // 分布账/调用账不平
const E_NAME_TOO_LONG: u16 = SEG_BASE + 5; // 名字超长（NAME_MAX 255）
const E_NAME_DUP: u16 = SEG_BASE + 6; // 同模式重名登记（不可登记）
const E_HOOK_DEAD: u16 = SEG_BASE + 7; // 钩子未装（比较请求无人应答）

/// 桶数：判据口径 16，与桶数组长度强绑定。
pub const BUCKETS: usize = 16;

/// 名字长度上限：Linux NAME_MAX = 255（含结尾 NUL 前的上限）。
pub const NAME_MAX: usize = 255;

/// FNV-1a 参数（公开域算法，写死避免依赖外部 crate）。
const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// 比较模式闭集：字节级（Unix 人格默认）/ 大小写折叠（D3 转换层入口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompareMode {
    ByteExact = 0, // 逐字节严格比较
    CaseFold = 1,  // ASCII 大小写折叠后比较
}

/// 哈希器状态：只保留种子与调用计数（种子恒 FNV_OFFSET，留口给将来换算法）。
pub struct NameHasher {
    pub seed: u32,   // 初始种子
    pub hashed: u32, // 已哈希名字数
}

impl NameHasher {
    pub fn new() -> NameHasher {
        NameHasher {
            seed: FNV_OFFSET,
            hashed: 0,
        }
    }

    /// FNV-1a 逐字节推进；空名照算（哈希本身不拒空，拒空在比较口）。
    pub fn hash(&mut self, name: &[u8]) -> u32 {
        let mut h = self.seed;
        let mut i = 0usize;
        while i < name.len() {
            h ^= name[i] as u32;
            h = h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
        self.hashed += 1;
        h
    }

    /// 只读哈希（不改计数），供分布统计与复核使用。
    pub fn hash_ro(&self, name: &[u8]) -> u32 {
        let mut h = self.seed;
        let mut i = 0usize;
        while i < name.len() {
            h ^= name[i] as u32;
            h = h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h
    }

    /// 桶号 = 哈希高 4 位（设计纪律 2）。
    pub fn bucket_of(&self, name: &[u8]) -> usize {
        (self.hash_ro(name) >> 28) as usize & (BUCKETS - 1)
    }
}

/// 桶分布账：16 桶计数 + 极差 + 样本数。
pub struct NameDist {
    pub counts: [u32; BUCKETS], // 各桶名字数
    pub samples: u32,           // 样本总数
    pub max: u32,               // 最满桶计数
    pub min: u32,               // 最空桶计数（无空桶时 > 0）
    pub spread: u32,            // 极差 = max - min
    pub collisions: u32,        // 落桶时同桶已非空的次数（碰撞账）
}

impl NameDist {
    pub fn new() -> NameDist {
        NameDist {
            counts: [0; BUCKETS],
            samples: 0,
            max: 0,
            min: 0,
            spread: 0,
            collisions: 0,
        }
    }

    /// 记一个样本：先判桶号，再累加，最后刷新极差。
    pub fn add(&mut self, bucket: usize) -> Result<(), u16> {
        if bucket >= BUCKETS { return Err(E_BUCKET); }
        if self.counts[bucket] > 0 {
            self.collisions += 1;
        }
        self.counts[bucket] += 1;
        self.samples += 1;
        self.refresh();
        Ok(())
    }

    /// 极差刷新：min 只在有样本时才有意义（空分布 min/max 皆 0）。
    fn refresh(&mut self) {
        if self.samples == 0 {
            self.max = 0;
            self.min = 0;
            self.spread = 0;
            return;
        }
        let mut mx = 0u32;
        let mut mn = u32::MAX;
        let mut i = 0usize;
        while i < BUCKETS {
            let c = self.counts[i];
            if c > mx {
                mx = c;
            }
            if c < mn {
                mn = c;
            }
            i += 1;
        }
        self.max = mx;
        self.min = mn;
        self.spread = mx - mn;
    }

    /// 求和复核：桶计数之和必须等于样本数（J1 守恒）。
    pub fn sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < BUCKETS {
            s += self.counts[i];
            i += 1;
        }
        s
    }

    /// 空桶数（J1 要求 16 桶全非零，故正常应为 0）。
    pub fn empty_buckets(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BUCKETS {
            if self.counts[i] == 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 单桶读数（越界拒收）。
    pub fn bucket(&self, idx: usize) -> Result<u32, u16> {
        if idx >= BUCKETS { return Err(E_BUCKET); }
        Ok(self.counts[idx])
    }
}

/// ASCII 折叠：A-Z → a-z，其余原样（唯一改动点，见设计纪律 3）。
fn fold(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

/// d_compare 钩子：模式 + 调用账。
pub struct CompareHook {
    pub mode: CompareMode, // 当前模式
    pub calls: u32,        // 比较调用总数
    pub eq: u32,           // 判等次数
    pub ne: u32,           // 判不等次数
    pub switches: u32,     // 模式切换次数
    pub installed: bool,   // 钩子是否已装（未装拒比较）
}

impl CompareHook {
    pub fn new(mode: CompareMode) -> CompareHook {
        CompareHook {
            mode,
            calls: 0,
            eq: 0,
            ne: 0,
            switches: 0,
            installed: true,
        }
    }

    /// 未装钩子的退化态（drill 用于 E_HOOK_DEAD 注入）。
    pub fn dead() -> CompareHook {
        CompareHook {
            mode: CompareMode::ByteExact,
            calls: 0,
            eq: 0,
            ne: 0,
            switches: 0,
            installed: false,
        }
    }

    /// 模式码 → 闭集。
    pub fn mode_by_code(code: u16) -> Result<CompareMode, u16> {
        match code {
            0 => Ok(CompareMode::ByteExact),
            1 => Ok(CompareMode::CaseFold),
            _ => Err(E_MODE),
        }
    }

    /// 切换模式：同模式重设不计切换（幂等）。
    pub fn set_mode(&mut self, mode: CompareMode) -> Result<(), u16> {
        if !self.installed { return Err(E_HOOK_DEAD); }
        if self.mode != mode {
            self.mode = mode;
            self.switches += 1;
        }
        Ok(())
    }

    /// 比较：长度先判（ByteExact），再逐字节（CaseFold 折叠后比）。
    /// 两个空名字都拒（dentry 名不可能为空）。
    pub fn compare(&mut self, a: &[u8], b: &[u8]) -> Result<bool, u16> {
        if !self.installed { return Err(E_HOOK_DEAD); }
        if a.is_empty() || b.is_empty() { return Err(E_NAME_EMPTY); }
        if a.len() > NAME_MAX || b.len() > NAME_MAX { return Err(E_NAME_TOO_LONG); }
        self.calls += 1;
        let mut same = a.len() == b.len();
        if same {
            let mut i = 0usize;
            while i < a.len() {
                let (x, y) = match self.mode {
                    CompareMode::ByteExact => (a[i], b[i]),
                    CompareMode::CaseFold => (fold(a[i]), fold(b[i])),
                };
                if x != y {
                    same = false;
                    break;
                }
                i += 1;
            }
        }
        if same {
            self.eq += 1;
        } else {
            self.ne += 1;
        }
        Ok(same)
    }

    /// 调用账守恒（J4）。
    pub fn balanced(&self) -> bool {
        self.calls == self.eq + self.ne
    }
}

/// 名字空间：把"登记名字 → 落桶 → 查重"串起来，供 dcache 上层直接调用。
pub struct NameSpace {
    pub hasher: NameHasher,     // 哈希器
    pub dist: NameDist,         // 分布账
    pub hook: CompareHook,      // 比较钩子
    pub names: Vec<Vec<u8>>,    // 已登记名字（保序，供查重与复核）
    pub dup_rejects: u32,       // 重名拒收次数
}

impl NameSpace {
    pub fn new(mode: CompareMode) -> NameSpace {
        NameSpace {
            hasher: NameHasher::new(),
            dist: NameDist::new(),
            hook: CompareHook::new(mode),
            names: Vec::new(),
            dup_rejects: 0,
        }
    }

    /// 登记名字：空名/超长拒；按当前模式查重（ByteExact 下大小写视为不同条目）。
    pub fn add_name(&mut self, name: &[u8]) -> Result<usize, u16> {
        if name.is_empty() { return Err(E_NAME_EMPTY); }
        if name.len() > NAME_MAX { return Err(E_NAME_TOO_LONG); }
        let mut i = 0usize;
        while i < self.names.len() {
            // 直接复用钩子语义：查重也必须走同一模式，否则登记与查找会错位
            let same = self.hook.compare(name, &self.names[i])?;
            if same {
                self.dup_rejects += 1;
                return Err(E_NAME_DUP); // 重名：同模式下一名一条目
            }
            i += 1;
        }
        let b = self.hasher.bucket_of(name);
        self.dist.add(b)?;
        self.hasher.hashed += 1;
        self.names.push(copy_name(name));
        Ok(b)
    }

    /// 只读落桶查询（不改分布账）。
    pub fn bucket_of(&self, name: &[u8]) -> Result<usize, u16> {
        if name.is_empty() { return Err(E_NAME_EMPTY); }
        Ok(self.hasher.bucket_of(name))
    }

    /// 分布终审：样本数与名字表长度相等且桶计数之和守恒（J1）。
    pub fn dist_verified(&self) -> bool {
        self.dist.samples as usize == self.names.len() && self.dist.sum() == self.dist.samples
    }
}

/// 名字拷贝（no_std 下没有 to_vec 之外的便利，这里显式逐字节拷）。
fn copy_name(name: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < name.len() {
        v.push(name[i]);
        i += 1;
    }
    v
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NAME_EMPTY => "空名字（dentry 名至少 1 字节）或重名登记",
        E_MODE => "比较模式码越界（合法 0/1）",
        E_BUCKET => "桶号越界（合法 0..15）",
        E_BAL => "分布账或调用账不平",
        E_NAME_TOO_LONG => "名字超长（NAME_MAX=255）",
        E_NAME_DUP => "同模式重名登记（不可登记）",
        E_HOOK_DEAD => "比较钩子未装（无人应答）",
        _ => "未知诊断码",
    }
}

/// 确定性名字生成：LCG 取字母，长度 4..12，字母表 26 小写 + 首字母大写混排。
fn gen_name(seed: u64, out: &mut Vec<u8>) {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let len = 4 + (s % 9) as usize; // 4..12
    out.clear();
    let mut i = 0usize;
    while i < len {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let c = (s >> 33) as u8;
        let b = b'a' + (c % 26);
        // 第 0 位大写，制造大小写混排样本（ByteExact 下这是不同名字）
        if i == 0 {
            out.push(b - 32);
        } else {
            out.push(b);
        }
        i += 1;
    }
}

/// 确定性演练：万次分布 → 模式切换 → 边界 → 注入拦截 → 账平。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：10,000 个名字落 16 桶，样本数与桶和守恒，极差入账（J1 主面）。
    let mut dist = NameDist::new();
    let hasher = NameHasher::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut i = 0u32;
    while i < 10_000 {
        gen_name(i as u64, &mut buf);
        if buf.is_empty() { return Err(E_NAME_EMPTY); }
        let b = hasher.bucket_of(&buf);
        dist.add(b)?;
        i += 1;
    }
    if dist.samples != 10_000 { return Err(E_BAL); }
    if dist.sum() != 10_000 {
        return Err(E_BAL); // 一名字一桶，不重不漏
    }
    // 桶分布账：万样本下允许至多 2 个空桶（哈希质量观察面，极差照实入账）
    if dist.empty_buckets() > 2 { return Err(E_BAL); }
    if dist.spread != dist.max - dist.min { return Err(E_BAL); }
    if dist.min == 0 || dist.max < dist.min { return Err(E_BAL); }
    // 碰撞账：首次落某桶不算碰撞，其后每落该桶一次算一次
    let expect_col = 10_000 - (BUCKETS as u32 - dist.empty_buckets());
    if dist.collisions != expect_col { return Err(E_BAL); }
    hits += 1;

    // R2：哈希纯函数性——同名字两次哈希必等（J2）。
    let h1 = hasher.hash_ro(&buf);
    let h2 = hasher.hash_ro(&buf);
    if h1 != h2 { return Err(E_BAL); }
    // 不同名字允许碰撞：只核"桶号仍在闭集内"，不核不等
    if hasher.bucket_of(b"abc") >= BUCKETS { return Err(E_BUCKET); }
    hits += 1;

    // R3：ByteExact 下大小写变体是不同名字（J3 前半）。
    let mut hook = CompareHook::new(CompareMode::ByteExact);
    if hook.compare(b"Abc", b"abc")? { return Err(E_BAL); }
    if !hook.compare(b"Abc", b"Abc")? { return Err(E_BAL); }
    if hook.switches != 0 {
        return Err(E_BAL); // 尚未切换
    }
    hits += 1;

    // R4：切到 CaseFold 后同一对名字相等，切回后不等——切换可验且立即生效（J3）。
    hook.set_mode(CompareMode::CaseFold)?;
    if !hook.compare(b"Abc", b"abc")? { return Err(E_BAL); }
    if !hook.compare(b"FILENAME", b"filename")? { return Err(E_BAL); }
    hook.set_mode(CompareMode::ByteExact)?;
    if hook.compare(b"Abc", b"abc")? { return Err(E_BAL); }
    if hook.switches != 2 { return Err(E_BAL); }
    if !hook.balanced() {
        return Err(E_BAL); // calls == eq + ne（J4）
    }
    hits += 1;

    // R5：长度不等在 ByteExact 下直接判不等（不进逐字节循环，不越界）。
    if hook.compare(b"abc", b"abcd")? { return Err(E_BAL); }
    // 折叠只在 ASCII A-Z 生效，标点/数字不折叠
    hook.set_mode(CompareMode::CaseFold)?;
    if hook.compare(b"a-b", b"a_b")? { return Err(E_BAL); }
    if !hook.compare(b"A1-Z", b"a1-z")? { return Err(E_BAL); }
    hits += 1;

    // R6：名字空间登记 200 条，分布账与名字表长度守恒，重名被拒（登记面）。
    let mut ns = NameSpace::new(CompareMode::ByteExact);
    let mut k = 0u32;
    while k < 200 {
        gen_name(1_000 + k as u64, &mut buf);
        ns.add_name(&buf)?;
        k += 1;
    }
    if ns.names.len() != 200 || !ns.dist_verified() { return Err(E_BAL); }
    if ns.dist.samples != 200 || ns.dist.sum() != 200 { return Err(E_BAL); }
    let dup = copy_name(&ns.names[0]);
    if ns.add_name(&dup).is_ok() {
        return Err(E_BAL); // 同模式重名必须拒
    }
    if ns.dup_rejects != 1 || ns.names.len() != 200 {
        return Err(E_BAL); // 拒收不改表长
    }
    hits += 1;

    // I1：空名字 → E_NAME_EMPTY（比较口与哈希口双点）。
    let mut h3 = CompareHook::new(CompareMode::ByteExact);
    if h3.compare(b"", b"a") != Err(E_NAME_EMPTY) { return Err(E_NAME_EMPTY); }
    if h3.compare(b"a", b"") != Err(E_NAME_EMPTY) { return Err(E_NAME_EMPTY); }
    if h3.calls != 0 {
        return Err(E_BAL); // 拒收不进调用账
    }
    hits += 1;

    // I2：模式码越界 → E_MODE（2 与 65535 双点）。
    if CompareHook::mode_by_code(2) != Err(E_MODE) { return Err(E_MODE); }
    if CompareHook::mode_by_code(0xFFFF) != Err(E_MODE) { return Err(E_MODE); }
    hits += 1;

    // I3：桶号越界查询 → E_BUCKET（16 与 65535 双点）。
    if dist.bucket(BUCKETS) != Err(E_BUCKET) { return Err(E_BUCKET); }
    if dist.bucket(0xFFFF) != Err(E_BUCKET) { return Err(E_BUCKET); }
    hits += 1;

    // I4：钩子未装时比较/切换 → E_HOOK_DEAD，不得静默放行。
    let mut dead = CompareHook::dead();
    if dead.compare(b"a", b"a") != Err(E_HOOK_DEAD) { return Err(E_HOOK_DEAD); }
    if dead.set_mode(CompareMode::CaseFold) != Err(E_HOOK_DEAD) { return Err(E_HOOK_DEAD); }
    hits += 1;

    // I5：超长名字（256 字节）→ E_NAME_TOO_LONG，不截断不折叠。
    let mut longbuf: Vec<u8> = Vec::new();
    let mut z = 0usize;
    while z < NAME_MAX + 1 {
        longbuf.push(b'a');
        z += 1;
    }
    if h3.compare(&longbuf, &longbuf) != Err(E_NAME_TOO_LONG) { return Err(E_NAME_TOO_LONG); }
    hits += 1;

    // 终审：分布账、调用账双双守恒，且极差与逐桶复核一致。
    let mut re_max = 0u32;
    let mut re_min = u32::MAX;
    let mut bi = 0usize;
    while bi < BUCKETS {
        let c = dist.bucket(bi)?;
        if c > re_max {
            re_max = c;
        }
        if c < re_min {
            re_min = c;
        }
        bi += 1;
    }
    if re_max != dist.max || re_min != dist.min || re_max - re_min != dist.spread { return Err(E_BAL); }
    if !hook.balanced() || ns.dist.sum() != ns.dist.samples { return Err(E_BAL); }
    hits += 1;

    Ok(hits)
}
