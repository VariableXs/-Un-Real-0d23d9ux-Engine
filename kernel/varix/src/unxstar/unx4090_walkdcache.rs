//! unx4090_walkdcache — UNX-F4090 路径逐分量解析缓存联动（walk × dcache 接口）（诊断段 0x8D60）。
//!
//! 判据原文：UNX-F4090-J1 walk 每分量先查缓存（查询计数=分量数），缓存全命中负载下
//!           fs 层穿透为零（穿透计数账=0）——为 B09 主循环供给接口。
//! 可验性质（本模块自证面）：
//!   J1 每走一个 path 分量，先查一次 dcache：cache_queries 增量 == components 增量，
//!      逐次走查零漏查（不许跳过缓存直接下 fs）；
//!   J2 预热后同一路径再走 1,000 次，fs_penetrations 增量为 0，且结果 ino 与首走一致
//!      （缓存生效的硬指标）；
//!   J3 穿透账只在缓存未命中时 +1，且每次未命中最多穿透一层（一层一查，不许一穿透
//!      顺手把后代全加载——那是预取，不在本接口职责内）；
//!   J4 守恒：cache_hits + fs_penetrations == cache_queries（一查询必落命中或穿透）；
//!      走过的分量总数 == Σ各次路径分量数；
//!   J5 非法路径全拒且不污染账：空路径（E_PATH_EMPTY）、相对路径（E_PATH_REL）、
//!      空分量（E_PATH_BAD）、"." / ".."（E_PATH_DOT）、分量数超限（E_PATH_DEEP）、
//!      分量超长（E_NAME_LONG）、解析中断（E_NO_DENT）。
//! 设计纪律：
//!   1. 路径解析在字节面上做，不分配中间字符串：扫 '/' 定位分量区间，再用区间去查
//!      缓存（name 比较自带长度，天然支持含 NUL 的名字）。
//!   2. 每次 walk 从 root_ino 起，不接受调用方传入的中间 ino：入口统一，账才可核。
//!   3. 缓存与 fs 的键都是 (parent, name)：同一名字在不同父下是不同条目，跨目录
//!      同名不会串（这是路径解析最容易踩的坑）。
//!   4. 失败即整体失败：任一分量解析不到就返回 Err，已走完的前缀缓存保留（它们确实
//!      存在过），但本次 walk 不计成功——失败账单列，不与成功账混。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8D60 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8D60;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_PATH_EMPTY: u16 = SEG_BASE + 1; // 空路径
const E_PATH_REL: u16 = SEG_BASE + 2; // 非绝对路径（不以 '/' 开头）
const E_PATH_BAD: u16 = SEG_BASE + 3; // 空分量（"//"）
const E_PATH_DOT: u16 = SEG_BASE + 4; // "." / ".." 分量（本层不支持）
const E_PATH_DEEP: u16 = SEG_BASE + 5; // 分量数超限
const E_NAME_LONG: u16 = SEG_BASE + 6; // 单个分量超长（NAME_MAX=255）
const E_NO_DENT: u16 = SEG_BASE + 7; // 分量解析不到（fs 侧无此项）
const E_BAL: u16 = SEG_BASE + 8; // 查询/命中/穿透账不平

/// 单分量长度上限（Linux NAME_MAX）。
pub const NAME_MAX: usize = 255;

/// 单条路径分量数上限（PATH_MAX 级余量取 64，超限视为病态）。
pub const MAX_COMPONENTS: usize = 64;

/// 走查相位：Start → Descending → Done / Error。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkState {
    Start = 0,      // 未开始
    Descending = 1, // 逐分量下行中
    Done = 2,       // 成功到目标
    Error = 3,      // 解析失败
}

/// 单个目录项（缓存与 fs 共用同构）。
pub struct CompEntry {
    pub parent: u64,   // 父目录 ino
    pub name: Vec<u8>, // 名字
    pub ino: u64,      // 子项 ino
}

/// dcache 侧：条目表 + 命中账。
pub struct CompCache {
    pub entries: Vec<CompEntry>, // 缓存条目
    pub queries: u32,            // 被查询次数（每分量一次）
    pub hits: u32,               // 命中次数
}

impl CompCache {
    pub fn new() -> CompCache {
        CompCache {
            entries: Vec::new(),
            queries: 0,
            hits: 0,
        }
    }

    /// 查一次（计账），返回子项 ino。
    pub fn query(&mut self, parent: u64, name: &[u8]) -> Option<u64> {
        self.queries += 1;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].parent == parent && bytes_eq(&self.entries[i].name, name) {
                self.hits += 1;
                return Some(self.entries[i].ino);
            }
            i += 1;
        }
        None
    }

    /// 只读查（不计账，供校验用）。
    pub fn peek(&self, parent: u64, name: &[u8]) -> Option<u64> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].parent == parent && bytes_eq(&self.entries[i].name, name) {
                return Some(self.entries[i].ino);
            }
            i += 1;
        }
        None
    }

    /// 填入（已存在则覆盖同键值，避免重复项）。
    pub fn fill(&mut self, parent: u64, name: &[u8], ino: u64) {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].parent == parent && bytes_eq(&self.entries[i].name, name) {
                self.entries[i].ino = ino;
                return;
            }
            i += 1;
        }
        self.entries.push(CompEntry {
            parent,
            name: copy(name),
            ino,
        });
    }
}

/// fs 侧：条目表 + 穿透计数（penetration 是唯一穿透度量）。
pub struct CompFs {
    pub entries: Vec<CompEntry>, // 实态条目
    pub penetrations: u32,       // 被穿透次数
}

impl CompFs {
    pub fn new() -> CompFs {
        CompFs {
            entries: Vec::new(),
            penetrations: 0,
        }
    }

    /// 建目录项：父必须已存在于树中（由调用方保证顺序），同键唯一。
    pub fn add(&mut self, parent: u64, name: &[u8], ino: u64) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_PATH_BAD);
        }
        if name.len() > NAME_MAX {
            return Err(E_NAME_LONG);
        }
        if self.peek(parent, name).is_some() {
            return Err(E_PATH_BAD); // 同父同名重复
        }
        self.entries.push(CompEntry {
            parent,
            name: copy(name),
            ino,
        });
        Ok(())
    }

    /// 只读查。
    pub fn peek(&self, parent: u64, name: &[u8]) -> Option<u64> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].parent == parent && bytes_eq(&self.entries[i].name, name) {
                return Some(self.entries[i].ino);
            }
            i += 1;
        }
        None
    }

    /// 真穿透（计一次），再查表。
    pub fn penetrate(&mut self, parent: u64, name: &[u8]) -> Option<u64> {
        self.penetrations += 1;
        self.peek(parent, name)
    }
}

/// 走查统计账。
pub struct WalkStat {
    pub walks: u32,          // 成功走查次数
    pub failed_walks: u32,   // 失败走查次数
    pub components: u32,     // 累计分量数
    pub cache_queries: u32,  // 累计缓存查询数（应等于 components）
    pub cache_hits: u32,     // 缓存命中
    pub fs_penetrations: u32, // 穿透 fs 次数
}

impl WalkStat {
    fn new() -> WalkStat {
        WalkStat {
            walks: 0,
            failed_walks: 0,
            components: 0,
            cache_queries: 0,
            cache_hits: 0,
            fs_penetrations: 0,
        }
    }

    /// J1：查询数恒等于分量数（一分量一查）。
    pub fn query_matches_components(&self) -> bool {
        self.cache_queries == self.components
    }

    /// J4：命中 + 穿透 == 查询。
    pub fn path_conserved(&self) -> bool {
        self.cache_hits + self.fs_penetrations == self.cache_queries
    }
}

/// 走查器：root + fs + cache + 账 + 相位。
pub struct Walker {
    pub root: u64,        // 起始目录 ino
    pub fs: CompFs,       // 实态
    pub cache: CompCache, // 缓存
    pub stat: WalkStat,   // 统计账
    pub state: WalkState, // 相位
}

impl Walker {
    pub fn new(root: u64) -> Walker {
        Walker {
            root,
            fs: CompFs::new(),
            cache: CompCache::new(),
            stat: WalkStat::new(),
            state: WalkState::Start,
        }
    }

    /// 逐分量走查（J1/J2/J3 的唯一入口）。
    /// 路径必须是绝对路径；末尾 '/' 允许；中间空分量、"."

    pub fn walk(&mut self, path: &[u8]) -> Result<u64, u16> {
        if path.is_empty() {
            return Err(E_PATH_EMPTY); // 空路径不入账
        }
        if path[0] != b'/' {
            return Err(E_PATH_REL);
        }
        // 先做一遍结构预检（分量数、长度、点分量），保证失败时账不被污染
        let mut ncomp = 0usize;
        let mut pre = 1usize; // 跳过首个 '/'
        let mut first = true;
        while pre <= path.len() {
            // 找本分量结束
            let mut end = pre;
            while end < path.len() && path[end] != b'/' {
                end += 1;
            }
            let clen = end - pre;
            if clen == 0 {
                if end >= path.len() {
                    break; // 收尾的 '/' 或纯根路径 "/"
                }
                // 中间空分量（"//"）：首个 '/' 后的空段也算非法
                if !(first && pre == 1) {
                    return Err(E_PATH_BAD);
                }
            } else {
                if clen > NAME_MAX {
                    return Err(E_NAME_LONG);
                }
                let c = &path[pre..end];
                if clen == 1 && c[0] == b'.' {
                    return Err(E_PATH_DOT);
                }
                if clen == 2 && c[0] == b'.' && c[1] == b'.' {
                    return Err(E_PATH_DOT);
                }
                ncomp += 1;
                first = false;
            }
            if end >= path.len() {
                break;
            }
            pre = end + 1;
            if pre > path.len() {
                break;
            }
        }
        if ncomp > MAX_COMPONENTS {
            return Err(E_PATH_DEEP);
        }
        if ncomp == 0 {
            // 纯根路径 "/"：不查缓存、不穿透，直接给 root（零分量合法）
            self.state = WalkState::Done;
            self.stat.walks += 1;
            return Ok(self.root);
        }

        // 正式下行
        self.state = WalkState::Descending;
        let mut cur = self.root;
        let mut i = 1usize;
        while i <= path.len() {
            let mut end = i;
            while end < path.len() && path[end] != b'/' {
                end += 1;
            }
            if end == i {
                // 空段（收尾或纯根）
                if end >= path.len() {
                    break;
                }
                i = end + 1;
                continue;
            }
            let name = &path[i..end];
            self.stat.components += 1;
            // ① 先查缓存（一分量一查，绝不跳过）
            let hit = self.cache.query(cur, name);
            self.stat.cache_queries += 1;
            match hit {
                Some(ino) => {
                    self.stat.cache_hits += 1;
                    cur = ino;
                }
                None => {
                    // ② 未命中才穿透一层
                    match self.fs.penetrate(cur, name) {
                        Some(ino) => {
                            self.stat.fs_penetrations += 1;
                            self.cache.fill(cur, name, ino);
                            cur = ino;
                        }
                        None => {
                            self.stat.fs_penetrations += 1; // 穿透已发生，无论有没有
                            self.stat.failed_walks += 1;
                            self.state = WalkState::Error;
                            return Err(E_NO_DENT);
                        }
                    }
                }
            }
            if end >= path.len() {
                break;
            }
            i = end + 1;
        }
        self.state = WalkState::Done;
        self.stat.walks += 1;
        Ok(cur)
    }

    /// 账终审：查询 == 分量、命中 + 穿透 == 查询、walker 与 cache 两本查询账一致。
    pub fn audit(&self) -> bool {
        self.stat.query_matches_components()
            && self.stat.path_conserved()
            && self.cache.queries == self.stat.cache_queries
            && self.cache.hits == self.stat.cache_hits
            && self.fs.penetrations == self.stat.fs_penetrations
    }
}

/// 字节相等。
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 字节拷贝。
fn copy(a: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < a.len() {
        v.push(a[i]);
        i += 1;
    }
    v
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_PATH_EMPTY => "空路径",
        E_PATH_REL => "非绝对路径（不以 '/' 开头）",
        E_PATH_BAD => "空分量（\"//\"）或同父同名重复建项",
        E_PATH_DOT => "路径含 \".\" / \"..\" 分量（本层不支持）",
        E_PATH_DEEP => "分量数超限（> 64）",
        E_NAME_LONG => "单分量超长（NAME_MAX=255）",
        E_NO_DENT => "分量解析不到（fs 侧无此项）",
        E_BAL => "查询/命中/穿透账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：建链 → 冷走 → 热走千次零穿透 → 深路径 → 失败 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut w = Walker::new(1);

    // R0：建链 1→2→…→9（"next"），并为每个目录挂 9 个旁支文件 f0..f8。
    let mut d = 1u64;
    while d <= 8 {
        w.fs.add(d, b"next", d + 1)?;
        let mut k = 0u32;
        while k < 9 {
            let mut nm: Vec<u8> = Vec::new();
            nm.push(b'f');
            nm.push(b'0' + k as u8);
            w.fs.add(d, &nm, 100 * d + k as u64)?;
            k += 1;
        }
        d += 1;
    }
    hits += 1;

    // R1：冷走 "/next/next/next" → ino 4，3 分量 3 查询 3 穿透（J1/J3）。
    let ino = w.walk(b"/next/next/next")?;
    if ino != 4 {
        return Err(E_BAL);
    }
    if w.stat.components != 3 || w.stat.cache_queries != 3 {
        return Err(E_BAL);
    }
    if w.stat.fs_penetrations != 3 || w.stat.cache_hits != 0 {
        return Err(E_BAL);
    }
    if w.cache.entries.len() != 3 {
        return Err(E_BAL);
    }
    if w.state != WalkState::Done {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：热走一次 → 全命中，穿透增量为 0（J2 首验）。
    let pen_before = w.stat.fs_penetrations;
    if w.walk(b"/next/next/next")? != 4 {
        return Err(E_BAL);
    }
    if w.stat.fs_penetrations != pen_before {
        return Err(E_BAL);
    }
    if w.stat.cache_hits != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：热走 1,000 次 → 穿透增量恒 0，查询增量恰 3,000（J2 主面）。
    let pen2 = w.stat.fs_penetrations;
    let mut n = 0u32;
    while n < 1_000 {
        if w.walk(b"/next/next/next")? != 4 {
            return Err(E_BAL);
        }
        n += 1;
    }
    if w.stat.fs_penetrations != pen2 {
        return Err(E_BAL);
    }
    if w.stat.cache_queries != 3 + 3 + 3_000 {
        return Err(E_BAL);
    }
    if w.stat.cache_hits != 3 + 3_000 {
        return Err(E_BAL);
    }
    if !w.audit() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：深路径 8 分量 → ino 9；前 3 段命中、后 5 段穿透（各一次）。
    let p4 = w.stat.fs_penetrations;
    let ino9 = w.walk(b"/next/next/next/next/next/next/next/next")?;
    if ino9 != 9 {
        return Err(E_BAL);
    }
    if w.stat.fs_penetrations - p4 != 5 {
        return Err(E_BAL); // 只穿透缺失的 5 段
    }
    if !w.audit() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：跨目录同名不串——"/next/f3" 与 "/next/next/f3" 必须给出不同 ino。
    let a = w.walk(b"/next/f3")?;
    let b = w.walk(b"/next/next/f3")?;
    if a != 203 || b != 303 {
        return Err(E_BAL);
    }
    if a == b {
        return Err(E_BAL); // 同名不同父必须分开
    }
    hits += 1;

    // R6：根路径 "/" 与收尾 '/' 均合法：零分量不查缓存（J1 的边界）。
    let q_before = w.stat.cache_queries;
    if w.walk(b"/")? != 1 {
        return Err(E_BAL);
    }
    if w.walk(b"/next/")? != 2 {
        return Err(E_BAL);
    }
    if w.stat.cache_queries != q_before + 1 {
        return Err(E_BAL); // "/" 零查询，"/next/" 一查询
    }
    hits += 1;

    // I1：空路径 / 相对路径 → 拒，且账不动（前置校验先于计账）。
    let c_before = w.stat.components;
    if w.walk(b"") != Err(E_PATH_EMPTY) {
        return Err(E_PATH_EMPTY);
    }
    if w.walk(b"next/next") != Err(E_PATH_REL) {
        return Err(E_PATH_REL);
    }
    if w.stat.components != c_before {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：空分量 "//" → E_PATH_BAD；点分量 → E_PATH_DOT（三路）。
    if w.walk(b"/next//f0") != Err(E_PATH_BAD) {
        return Err(E_PATH_BAD);
    }
    if w.walk(b"/next/./f0") != Err(E_PATH_DOT) {
        return Err(E_PATH_DOT);
    }
    if w.walk(b"/next/../f0") != Err(E_PATH_DOT) {
        return Err(E_PATH_DOT);
    }
    hits += 1;

    // I3：分量超长 → E_NAME_LONG（256 字节单段）。
    let mut longpath: Vec<u8> = Vec::new();
    longpath.push(b'/');
    let mut z = 0usize;
    while z < NAME_MAX + 1 {
        longpath.push(b'q');
        z += 1;
    }
    if w.walk(&longpath) != Err(E_NAME_LONG) {
        return Err(E_NAME_LONG);
    }
    hits += 1;

    // I4：分量数超限 → E_PATH_DEEP（65 段 "next"）。
    let mut deep: Vec<u8> = Vec::new();
    let mut seg = 0usize;
    while seg <= MAX_COMPONENTS {
        deep.push(b'/');
        deep.push(b'n');
        deep.push(b'e');
        deep.push(b'x');
        deep.push(b't');
        seg += 1;
    }
    if w.walk(&deep) != Err(E_PATH_DEEP) {
        return Err(E_PATH_DEEP);
    }
    hits += 1;

    // I5：解析不到 → E_NO_DENT，失败账 +1，穿透账 +1（穿透发生即计），分量账照记。
    let f_before = w.stat.failed_walks;
    let p_before2 = w.stat.fs_penetrations;
    if w.walk(b"/next/nosuch") != Err(E_NO_DENT) {
        return Err(E_NO_DENT);
    }
    if w.stat.failed_walks != f_before + 1 {
        return Err(E_BAL);
    }
    if w.stat.fs_penetrations != p_before2 + 1 {
        return Err(E_BAL);
    }
    if w.state != WalkState::Error {
        return Err(E_BAL);
    }
    hits += 1;

    // 终审：查询==分量、命中+穿透==查询、两本账一致（J1/J4）。
    if !w.audit() {
        return Err(E_BAL);
    }
    if w.stat.cache_queries != w.stat.components {
        return Err(E_BAL);
    }
    if w.fs.penetrations != w.stat.fs_penetrations {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
