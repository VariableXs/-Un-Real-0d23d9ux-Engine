//! unx4075_inocompare — UNX-F4075 inode 比对账（缓存态 vs fs 态）（诊断段 0x8BA0）。
//!
//! 判据原文：UNX-F4075-J1 抽样 100 inode 的缓存元数据与 fake fs 实态逐字段比对一致，
//!           人为篡改注入被比对账检出并报告。
//! 可验性质（本模块自证面）：
//!   J1 抽样 100 条八字段逐字段比对，全等时 mismatched 账必须为 0；
//!   J2 任一字段被篡改后 compare_one 必须返回 Mismatch(FieldId)，该字段差异计数 +1、
//!      mismatched 账 +1，并把首坏 inode 号记入 first_bad_ino（检出即入账，不许静默）；
//!   J3 缓存缺条 / fs 缺条分别落 MissingInCache / MissingOnFs，不得记为 Match；
//!   J4 守恒：scanned == matched + mismatched + missing_cache + missing_fs（逐条一账）；
//!   J5 字段码越界（>= FIELD_COUNT）必须拒收，不得越界索引。
//! 设计纪律：
//!   1. fs 实态 DiskInode 与缓存态 CachedInode 分离成两个结构，比对只读不写；
//!      任何"顺便修正缓存"的动作都在本模块之外（对账不做修复）。
//!   2. 字段编号闭集 FieldId 0..7 与差异计数表一一对应，表长由枚举末项派生，
//!      禁止魔数散落；取字段名一律走 field() 收敛点。
//!   3. 一条 inode 一次比对只出一条裁决，裁决入账后不可改判（append-only 语义）；
//!      Sealed 之后拒绝再扫（E_VERDICT_LOCK），防账后翻案。
//!   4. 索引访问前先比长度：所有 Vec 取值先查 idx < len，未命中走 Option 出口。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8BA0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8BA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_ENTRY: u16 = SEG_BASE + 1; // 记账里查不到该 inode 号
const E_FIELD_OOR: u16 = SEG_BASE + 2; // 字段码越界（>= FIELD_COUNT）
const E_DUP_INO: u16 = SEG_BASE + 3; // 同一侧重复登记同一 inode 号
const E_BAL: u16 = SEG_BASE + 4; // 对账总账不平（逐条一账失守）
const E_EMPTY: u16 = SEG_BASE + 5; // 空抽样表提交（无样本可比）
const E_VERDICT_LOCK: u16 = SEG_BASE + 6; // 已封账再扫（裁决不可改判）
const E_INO_ZERO: u16 = SEG_BASE + 7; // inode 号 0 非法（0 号在 VFS 中恒为保留）

/// 参与比对的元数据字段闭集（POSIX/Linux stat 面向 VFS 缓存关心的 8 项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    Mode = 0,   // 文件类型 + 权限位
    Uid = 1,    // 属主
    Gid = 2,    // 属组
    Nlink = 3,  // 硬链接数
    Size = 4,   // 字节长度
    Blocks = 5, // 512B 块占用（st_blocks）
    Mtime = 6,  // 修改时间（秒，整数面）
    Gen = 7,    // i_generation，NFS 复用代次
}

/// 字段数由闭集派生的唯一常量，禁止另写魔数 8。
pub const FIELD_COUNT: usize = 8;

/// 比对相位状态机：扫账只能从 Idle 进，封账后不可回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ComparePhase {
    Idle,     // 待扫
    Scanning, // 单条比对进行中
    Sealed,   // 已封账，裁决冻结
}

/// 单条 inode 的裁决闭集：四种出口互斥，覆盖全部分支。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Match,                    // 八字段全等
    Mismatch(FieldId),        // 首个不等字段
    MissingInCache,           // fs 有、缓存无（缓存缺条）
    MissingOnFs,              // 缓存有、fs 无（脏缓存）
}

/// fs 侧实态（fake fs 的 stat 结果），仅比对读面。
pub struct DiskInode {
    pub ino: u64,     // inode 号
    pub mode: u32,    // 类型 + 权限
    pub uid: u32,     // 属主
    pub gid: u32,     // 属组
    pub nlink: u32,   // 链接数
    pub size: u64,    // 字节长度
    pub blocks: u64,  // 块占用
    pub mtime: i64,   // 修改时刻（整数秒，禁用墙钟取值，只存样本值）
    pub gen: u32,     // 复用代次
}

impl DiskInode {
    /// 字段收敛点：编号 → 值，超出闭集返回 None（不 panic）。
    pub fn field(&self, f: FieldId) -> u64 {
        match f {
            FieldId::Mode => self.mode as u64,
            FieldId::Uid => self.uid as u64,
            FieldId::Gid => self.gid as u64,
            FieldId::Nlink => self.nlink as u64,
            FieldId::Size => self.size,
            FieldId::Blocks => self.blocks,
            FieldId::Mtime => self.mtime as u64, // 位模式比较，负数亦可比
            FieldId::Gen => self.gen as u64,
        }
    }
}

/// 缓存态（inode cache 里的内存副本），字段与实态同构。
pub struct CachedInode {
    pub ino: u64,       // inode 号（主键）
    pub mode: u32,      // 类型 + 权限
    pub uid: u32,       // 属主
    pub gid: u32,       // 属组
    pub nlink: u32,     // 链接数
    pub size: u64,      // 字节长度
    pub blocks: u64,    // 块占用
    pub mtime: i64,     // 修改时刻
    pub gen: u32,       // 复用代次
    pub dirty: bool,    // 脏标记（本账只读，不参与比对）
}

impl CachedInode {
    /// 字段收敛点：与 DiskInode::field 严格同构，比对才能逐字段成立。
    pub fn field(&self, f: FieldId) -> u64 {
        match f {
            FieldId::Mode => self.mode as u64,
            FieldId::Uid => self.uid as u64,
            FieldId::Gid => self.gid as u64,
            FieldId::Nlink => self.nlink as u64,
            FieldId::Size => self.size,
            FieldId::Blocks => self.blocks,
            FieldId::Mtime => self.mtime as u64,
            FieldId::Gen => self.gen as u64,
        }
    }
}

/// 比对总账：五类计数 + 八字段差异表 + 首坏号。
pub struct CompareLedger {
    pub scanned: u32,          // 已扫条数（含缺条）
    pub matched: u32,          // 全等条数
    pub mismatched: u32,       // 至少一字段不等
    pub missing_cache: u32,    // 缓存缺条
    pub missing_fs: u32,       // fs 缺条
    pub per_field: [u32; FIELD_COUNT], // 各字段被检出次数
    pub tampered: u32,         // 人为篡改注入次数（注入账，独立于检出账）
    pub first_bad_ino: u64,    // 首个不等 inode 号（0 表示无）
    pub first_bad_field: u8,   // 首坏字段码（255 表示无）
}

impl CompareLedger {
    fn new() -> CompareLedger {
        CompareLedger {
            scanned: 0,
            matched: 0,
            mismatched: 0,
            missing_cache: 0,
            missing_fs: 0,
            per_field: [0; FIELD_COUNT],
            tampered: 0,
            first_bad_ino: 0,
            first_bad_field: 255,
        }
    }

    /// J4 守恒判定：逐条一账，无重记无漏记。
    pub fn conserved(&self) -> bool {
        self.scanned == self.matched + self.mismatched + self.missing_cache + self.missing_fs
    }

    /// 差异表求和（与 mismatched 对照：一条只记首坏字段，故 sum <= mismatched*1）。
    pub fn field_diff_total(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < FIELD_COUNT {
            s += self.per_field[i];
            i += 1;
        }
        s
    }
}

/// 比对账本体：双侧样本表 + 总账 + 相位。
pub struct InoCompare {
    pub fs: Vec<DiskInode>,       // fs 实态表
    pub cache: Vec<CachedInode>,  // 缓存态表
    pub ledger: CompareLedger,    // 总账
    pub phase: ComparePhase,      // 相位
    pub samples: u32,             // 成功登记的样本对数上界（diagnostic）
}

impl InoCompare {
    pub fn new() -> InoCompare {
        InoCompare {
            fs: Vec::new(),
            cache: Vec::new(),
            ledger: CompareLedger::new(),
            phase: ComparePhase::Idle,
            samples: 0,
        }
    }

    /// fs 侧登记：查重 + 0 号拒收（0 号在 VFS 里是保留号，不入账）。
    pub fn put_fs(&mut self, d: DiskInode) -> Result<(), u16> {
        if d.ino == 0 {
            return Err(E_INO_ZERO);
        }
        if self.find_fs(d.ino).is_some() {
            return Err(E_DUP_INO);
        }
        self.fs.push(d);
        Ok(())
    }

    /// 缓存侧登记：同样查重，双侧各查各的，跨侧重复是正常的一对。
    pub fn put_cache(&mut self, c: CachedInode) -> Result<(), u16> {
        if c.ino == 0 {
            return Err(E_INO_ZERO);
        }
        if self.find_cache(c.ino).is_some() {
            return Err(E_DUP_INO);
        }
        self.cache.push(c);
        Ok(())
    }

    /// 线性查 fs 侧（样本量级千条，线性足够；返回下标避免借用越界）。
    pub fn find_fs(&self, ino: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.fs.len() {
            if self.fs[i].ino == ino {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 线性查缓存侧。
    pub fn find_cache(&self, ino: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.cache.len() {
            if self.cache[i].ino == ino {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 字段码 → 闭集：越界拒收（J5），这是唯一入口，杜绝裸下标。
    pub fn field_by_code(code: u16) -> Result<FieldId, u16> {
        match code {
            0 => Ok(FieldId::Mode),
            1 => Ok(FieldId::Uid),
            2 => Ok(FieldId::Gid),
            3 => Ok(FieldId::Nlink),
            4 => Ok(FieldId::Size),
            5 => Ok(FieldId::Blocks),
            6 => Ok(FieldId::Mtime),
            7 => Ok(FieldId::Gen),
            _ => Err(E_FIELD_OOR),
        }
    }

    /// 相位入口守卫：Sealed 后一律拒绝，Scanning 期间不许嵌套扫描。
    fn enter_scan(&mut self) -> Result<(), u16> {
        match self.phase {
            ComparePhase::Idle => {
                self.phase = ComparePhase::Scanning;
                Ok(())
            }
            ComparePhase::Scanning => Err(E_VERDICT_LOCK),
            ComparePhase::Sealed => Err(E_VERDICT_LOCK),
        }
    }

    fn leave_scan(&mut self) {
        if self.phase == ComparePhase::Scanning {
            self.phase = ComparePhase::Idle;
        }
    }

    /// 单条比对：八字段顺序扫描，首个不等即定格（裁决唯一）。
    /// 记账：scanned 必 +1，四出口各自 +1。
    pub fn compare_one(&mut self, ino: u64) -> Result<Verdict, u16> {
        if ino == 0 {
            return Err(E_INO_ZERO);
        }
        self.enter_scan()?;
        let fi = self.find_fs(ino);
        let ci = self.find_cache(ino);
        let verdict = match (fi, ci) {
            (Some(a), Some(b)) => {
                let mut hit: Option<FieldId> = None;
                let mut code = 0u16;
                while code < FIELD_COUNT as u16 {
                    let f = InoCompare::field_by_code(code)?;
                    if self.fs[a].field(f) != self.cache[b].field(f) {
                        hit = Some(f);
                        break;
                    }
                    code += 1;
                }
                match hit {
                    Some(f) => Verdict::Mismatch(f),
                    None => Verdict::Match,
                }
            }
            (Some(_), None) => Verdict::MissingInCache,
            (None, Some(_)) => Verdict::MissingOnFs,
            (None, None) => {
                self.leave_scan();
                return Err(E_NO_ENTRY);
            }
        };
        // 入账（append-only）
        self.ledger.scanned += 1;
        match verdict {
            Verdict::Match => self.ledger.matched += 1,
            Verdict::Mismatch(f) => {
                self.ledger.mismatched += 1;
                let fi2 = f as usize;
                if fi2 < FIELD_COUNT {
                    self.ledger.per_field[fi2] += 1;
                }
                if self.ledger.first_bad_ino == 0 {
                    self.ledger.first_bad_ino = ino;
                    self.ledger.first_bad_field = f as u8;
                }
            }
            Verdict::MissingInCache => self.ledger.missing_cache += 1,
            Verdict::MissingOnFs => self.ledger.missing_fs += 1,
        }
        self.leave_scan();
        Ok(verdict)
    }

    /// 批量比对（抽样面）：空表拒收（E_EMPTY），返回实际扫码条数。
    pub fn compare_batch(&mut self, inos: &[u64]) -> Result<u32, u16> {
        if inos.is_empty() {
            return Err(E_EMPTY);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < inos.len() {
            self.compare_one(inos[i])?;
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 篡改注入：只改缓存侧指定字段，注入账 +1（模拟人为破坏）。
    /// 这一路是判据 J2 的负样本入口，必须留痕。
    pub fn tamper_cache_field(&mut self, ino: u64, code: u16, value: u64) -> Result<(), u16> {
        let f = InoCompare::field_by_code(code)?;
        let idx = match self.find_cache(ino) {
            Some(i) => i,
            None => return Err(E_NO_ENTRY),
        };
        let c = &mut self.cache[idx];
        match f {
            FieldId::Mode => c.mode = value as u32,
            FieldId::Uid => c.uid = value as u32,
            FieldId::Gid => c.gid = value as u32,
            FieldId::Nlink => c.nlink = value as u32,
            FieldId::Size => c.size = value,
            FieldId::Blocks => c.blocks = value,
            FieldId::Mtime => c.mtime = value as i64,
            FieldId::Gen => c.gen = value as u32,
        }
        self.ledger.tampered += 1;
        Ok(())
    }

    /// 封账：Idle → Sealed（单向），封账后 compare_one 一律 E_VERDICT_LOCK。
    pub fn seal(&mut self) -> Result<(), u16> {
        match self.phase {
            ComparePhase::Idle => {
                self.phase = ComparePhase::Sealed;
                Ok(())
            }
            ComparePhase::Scanning => Err(E_VERDICT_LOCK),
            ComparePhase::Sealed => Err(E_VERDICT_LOCK),
        }
    }

    /// 差异读数：字段级计数出口（闭集索引恒合法）。
    pub fn field_diff(&self, f: FieldId) -> u32 {
        self.ledger.per_field[f as usize]
    }

    /// 终审：守恒且无篡改漏检（注入数 == 检出数才叫账实相符）。
    pub fn audit_clean(&self) -> bool {
        self.ledger.conserved() && self.ledger.tampered == self.ledger.mismatched
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_ENTRY => "比对账里查不到该 inode 号（双侧皆无）",
        E_FIELD_OOR => "字段码越界（合法 0..7）",
        E_DUP_INO => "同侧重复登记同一 inode 号",
        E_BAL => "对账总账不平（scanned != 四出口之和）",
        E_EMPTY => "空抽样表提交（无样本可比）",
        E_VERDICT_LOCK => "已封账再扫（裁决不可改判）",
        E_INO_ZERO => "inode 号 0 为保留号，拒收",
        _ => "未知诊断码",
    }
}

/// 确定性伪值发生器（LCG，Knuth MMIX 常数）：只造样本，不参与判定。
fn lcg(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// 由序号造一对同构样本（fs 与 cache 逐字段相同）。
fn sample_pair(ino: u64) -> (DiskInode, CachedInode) {
    let v = lcg(ino.wrapping_add(0x9E37_79B9));
    let mode = 0o100644u32 + (v as u32 & 0xF);
    let uid = 1000u32 + (v as u32 >> 8) % 64;
    let gid = 1000u32 + (v as u32 >> 16) % 32;
    let nlink = 1u32 + (v as u32 >> 3) % 4;
    let size = 4096u64 + (v % 8192);
    let blocks = size / 512;
    let mtime = 1_700_000_000i64 + (v % 100_000) as i64;
    let gen = (v >> 40) as u32;
    (
        DiskInode {
            ino,
            mode,
            uid,
            gid,
            nlink,
            size,
            blocks,
            mtime,
            gen,
        },
        CachedInode {
            ino,
            mode,
            uid,
            gid,
            nlink,
            size,
            blocks,
            mtime,
            gen,
            dirty: false,
        },
    )
}

/// 确定性演练：正常路径 → 边界 → 注入拦截 → 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：抽 100 条同构样本登记，逐条比对全 Match，mismatched 账必须为 0（J1）。
    let mut cmp = InoCompare::new();
    let mut inos: Vec<u64> = Vec::new();
    let mut i = 0u64;
    while i < 100 {
        let ino = 0x1000 + i;
        let (d, c) = sample_pair(ino);
        cmp.put_fs(d)?;
        cmp.put_cache(c)?;
        inos.push(ino);
        i += 1;
    }
    let scanned = cmp.compare_batch(&inos)?;
    if scanned != 100 {
        return Err(E_BAL);
    }
    if cmp.ledger.matched != 100 || cmp.ledger.mismatched != 0 {
        return Err(E_BAL);
    }
    if !cmp.ledger.conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：注入 3 处篡改（Size/Nlink/Mode 各一），必须逐一检出并落字段账（J2）。
    cmp.tamper_cache_field(0x1005, 4, 1)?;
    cmp.tamper_cache_field(0x1010, 3, 9)?;
    cmp.tamper_cache_field(0x1032, 0, 0o100755)?;
    let v5 = cmp.compare_one(0x1005)?;
    let v10 = cmp.compare_one(0x1010)?;
    let v32 = cmp.compare_one(0x1032)?;
    if v5 != Verdict::Mismatch(FieldId::Size) {
        return Err(E_BAL);
    }
    if v10 != Verdict::Mismatch(FieldId::Nlink) {
        return Err(E_BAL);
    }
    if v32 != Verdict::Mismatch(FieldId::Mode) {
        return Err(E_BAL);
    }
    if cmp.field_diff(FieldId::Size) != 1
        || cmp.field_diff(FieldId::Nlink) != 1
        || cmp.field_diff(FieldId::Mode) != 1
    {
        return Err(E_BAL);
    }
    if cmp.ledger.first_bad_ino != 0x1005 || cmp.ledger.first_bad_field != 4 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：缺条两向分账——fs 独有记 MissingInCache，缓存独有记 MissingOnFs（J3）。
    let (d_only, _) = sample_pair(0x2000);
    cmp.put_fs(d_only)?;
    let (_, c_only) = sample_pair(0x2001);
    cmp.put_cache(c_only)?;
    if cmp.compare_one(0x2000)? != Verdict::MissingInCache {
        return Err(E_BAL);
    }
    if cmp.compare_one(0x2001)? != Verdict::MissingOnFs {
        return Err(E_BAL);
    }
    if cmp.ledger.missing_cache != 1 || cmp.ledger.missing_fs != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：守恒终审——扫账 105 条 = 101 Match + 3 Mismatch + 1 + 1（J4）。
    if cmp.ledger.scanned != 105 {
        return Err(E_BAL);
    }
    if !cmp.ledger.conserved() {
        return Err(E_BAL);
    }
    if !cmp.audit_clean() {
        return Err(E_BAL); // 注入 3 == 检出 3，账实相符
    }
    hits += 1;

    // R5：封账后不可改判——再比对必须被相位机拦（E_VERDICT_LOCK）。
    cmp.seal()?;
    if cmp.compare_one(0x1005) != Err(E_VERDICT_LOCK) {
        return Err(E_VERDICT_LOCK);
    }
    hits += 1;

    // I1：重复登记同号 → E_DUP_INO。
    if cmp.put_fs(sample_pair(0x1005).0) != Err(E_DUP_INO) {
        return Err(E_DUP_INO);
    }
    hits += 1;

    // I2：双侧皆无 → E_NO_ENTRY（不得凭空造 Match）。
    let mut cmp2 = InoCompare::new();
    if cmp2.compare_one(0x9999) != Err(E_NO_ENTRY) {
        return Err(E_NO_ENTRY);
    }
    hits += 1;

    // I3：空抽样表 → E_EMPTY。
    let empty: Vec<u64> = Vec::new();
    if cmp2.compare_batch(&empty) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I4：字段码越界 → E_FIELD_OOR（8/65535 双点注入）。
    if InoCompare::field_by_code(FIELD_COUNT as u16) != Err(E_FIELD_OOR) {
        return Err(E_FIELD_OOR);
    }
    if InoCompare::field_by_code(0xFFFF) != Err(E_FIELD_OOR) {
        return Err(E_FIELD_OOR);
    }
    hits += 1;

    // I5：0 号 inode 拒收（保留号），且 tamper 到不存在号被拒。
    let zero = sample_pair(1).0;
    let mut bad = DiskInode {
        ino: 0,
        mode: zero.mode,
        uid: zero.uid,
        gid: zero.gid,
        nlink: zero.nlink,
        size: zero.size,
        blocks: zero.blocks,
        mtime: zero.mtime,
        gen: zero.gen,
    };
    bad.ino = 0;
    if cmp2.put_fs(bad) != Err(E_INO_ZERO) {
        return Err(E_INO_ZERO);
    }
    if cmp2.tamper_cache_field(0x7777, 4, 1) != Err(E_NO_ENTRY) {
        return Err(E_NO_ENTRY);
    }
    hits += 1;

    Ok(hits)
}
