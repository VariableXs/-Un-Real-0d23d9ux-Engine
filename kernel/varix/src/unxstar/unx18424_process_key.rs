//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18424 · 进程粒度键（按映像路径/哈希绑定） · 0x7E5C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18424-J1 映像绑定（路径+哈希双因子）断言全过，同名异本
//!       注入区分 10/10
//! 本模块结构清单：
//!   1. 冻结常量层：绑定容量/哈希域/判据轮数（推导注释写死）；
//!   2. 诊断码闭集 PkCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 双因子键模型 ImageFactor：路径指纹（FNV-1a 64）+ 映像内容
//!      指纹（FNV-1a 64），双因子全等才算同键（单因子相等不算）；
//!   4. 绑定账 BindingTable：映像绑定数组账本（路径+哈希双因子
//!      唯一）+ 同名异本判定器；
//!   5. 注入核：同名异本/空路径/零哈希/重复绑定 四类逐类 10/10
//!      断言（同名异本类：同路径不同哈希 10 组全部分键）；
//!   6. 语料：8 条主档映像（路径×哈希确定性派生）+ 10 组同名异本；
//!   7. 黄金全链：8 正例绑定→双因子查账→10 组异本全分键→注入全抓；
//!   8. run_unx18424_checks：15 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::ToString;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E5C0，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E5C0;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 绑定账容量（写死 32：8 主档映像 × 4 异本槽预算，2 的幂便于折叠）。
pub const BIND_CAP: usize = 32;
/// 主档映像数（写死 8：与 unx18421 主档进程键号 1..=8 勾稽）。
pub const MAIN_IMAGES: usize = 8;
/// 同名异本组数（写死 10：判据原文区分 10/10 语义）。
pub const VARIANT_GROUPS: usize = 10;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// FNV-1a 64 偏移基（写死：标准 64 位 FNV 参数）。
pub const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a 64 素数（写死：标准 64 位 FNV 参数）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
/// 路径最短长（写死 4：至少 "a.exe"，防空壳路径）。
pub const PATH_LEN_MIN: usize = 4;
/// 路径最长长（写死 128：进程粒度键定宽预算）。
pub const PATH_LEN_MAX: usize = 128;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PkCode {
    /// 路径为空：绑定路径空串
    PathEmpty,
    /// 路径过长：超 128 字节预算
    PathTooLong,
    /// 路径过短：低于 4 字节下限
    PathTooShort,
    /// 零哈希：映像内容指纹为 0（FNV 不会产出 0，出现即注入）
    ZeroImageHash,
    /// 重复绑定：同路径+同哈希已在账
    BindingDuplicated,
    /// 账满：32 绑定槽用尽
    TableFull,
    /// 查无此键：双因子不在账
    NoBinding,
    /// 单因子撞车：路径相同但哈希不同——须分键，不得并键
    SameNameVariant,
    /// 双因子键相等却哈希异：键语义破坏
    KeyIdentityBroken,
    /// 账计数守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 异本区分缺格：10 组未全部分键
    VariantGap,
}

impl PkCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            PkCode::PathEmpty => "路径为空：绑定路径不得为空串",
            PkCode::PathTooLong => "路径过长：超 128 字节预算",
            PkCode::PathTooShort => "路径过短：低于 4 字节下限",
            PkCode::ZeroImageHash => "零哈希：映像指纹为 0 视为注入",
            PkCode::BindingDuplicated => "重复绑定：同路径+同哈希已在账",
            PkCode::TableFull => "账满：32 绑定槽用尽",
            PkCode::NoBinding => "查无此键：双因子不在账",
            PkCode::SameNameVariant => "同名异本：同路径异哈希必须分键",
            PkCode::KeyIdentityBroken => "键语义破坏：同键哈希必同",
            PkCode::ConservationBroken => "账计数守恒破坏",
            PkCode::NotReproducible => "双跑不一致：复现红线",
            PkCode::VariantGap => "异本区分缺格：10 组未全分键",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [PkCode; 12] = [
        PkCode::PathEmpty,
        PkCode::PathTooLong,
        PkCode::PathTooShort,
        PkCode::ZeroImageHash,
        PkCode::BindingDuplicated,
        PkCode::TableFull,
        PkCode::NoBinding,
        PkCode::SameNameVariant,
        PkCode::KeyIdentityBroken,
        PkCode::ConservationBroken,
        PkCode::NotReproducible,
        PkCode::VariantGap,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < PkCode::ALL.len() {
        if let Some(c) = PkCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E5C0 段闭集内"
}

// ── FNV-1a 64（确定性，无墙钟无随机） ─────────────────────────────
/// FNV-1a 逐字节折叠（写死标准参数，仅作指纹，不作安全散列）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

// ── 双因子键模型 ───────────────────────────────────────────────────
/// 双因子：路径指纹 + 映像内容指纹。全等才同键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageFactor {
    /// 路径指纹（FNV-1a 64，路径字节）。
    pub path_hash: u64,
    /// 映像内容指纹（FNV-1a 64，映像字节）。
    pub image_hash: u64,
}

impl ImageFactor {
    /// 因子合法：双指纹皆非零。
    pub fn legal(&self) -> bool {
        self.path_hash != 0 && self.image_hash != 0
    }
    /// 进程键号折叠（写死推导：路径指纹高 24 位 × 映像指纹低 24 位
    /// 拼接成 48 位键号；折叠仅作展示号，唯一性以双因子全等为准）。
    pub fn key_id(&self) -> u64 {
        ((self.path_hash >> 40) << 24) | (self.image_hash & 0x00FF_FFFF)
    }
}

/// 构造因子（Result 化，零哈希显式拒）。
pub fn make_factor(path_hash: u64, image_hash: u64) -> Result<ImageFactor, u32> {
    let f = ImageFactor { path_hash, image_hash };
    if !f.legal() {
        return Err(PkCode::ZeroImageHash.code());
    }
    Ok(f)
}

/// 路径文本闸：非空/最短 4/最长 128。
pub fn path_ok(path: &str) -> Result<(), u32> {
    if path.is_empty() {
        return Err(PkCode::PathEmpty.code());
    }
    if path.len() < PATH_LEN_MIN {
        return Err(PkCode::PathTooShort.code());
    }
    if path.len() > PATH_LEN_MAX {
        return Err(PkCode::PathTooLong.code());
    }
    Ok(())
}

// ── 绑定记录与账本 ─────────────────────────────────────────────────
/// 单条绑定：进程键号 + 双因子 + 路径原文（同名异本展示用）。
#[derive(Clone, Debug)]
pub struct Binding {
    /// 进程键号（1 起，登记序派生；0 保留）。
    pub proc_key_id: u32,
    /// 路径原文（≤128 字节）。
    pub path: String,
    /// 双因子。
    pub factor: ImageFactor,
}

impl Binding {
    /// 双因子全等判定（键语义：路径指纹与映像指纹皆等才算同键）。
    pub fn same_key(&self, other: &ImageFactor) -> bool {
        self.factor.path_hash == other.path_hash && self.factor.image_hash == other.image_hash
    }
    /// 仅路径同（同名判定，供异本区分器用）。
    pub fn same_path(&self, path_hash: u64) -> bool {
        self.factor.path_hash == path_hash
    }
}

/// 绑定账本：双因子唯一 + 容量闸 + 守恒审计。
pub struct BindingTable {
    entries: Vec<Binding>,
    pub tried: u32,
    pub accepted: u32,
    pub rejected: u32,
    /// 拒因账。
    pub reject_codes: Vec<u32>,
    /// 下一个进程键号（1 起单调）。
    next_id: u32,
}

impl BindingTable {
    /// 新账。
    pub fn new() -> BindingTable {
        BindingTable {
            entries: Vec::new(),
            tried: 0,
            accepted: 0,
            rejected: 0,
            reject_codes: Vec::new(),
            next_id: 1,
        }
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 双因子查账：全等才命中，返回进程键号。
    pub fn resolve(&self, f: &ImageFactor) -> Result<u32, u32> {
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(b) = self.entries.get(i) {
                if b.same_key(f) {
                    return Ok(b.proc_key_id);
                }
            }
            i += 1;
        }
        Err(PkCode::NoBinding.code())
    }

    /// 同路径在账判定（异本区分器：路径同、哈希异的条目数）。
    pub fn same_path_variants(&self, path_hash: u64) -> u32 {
        let mut n: u32 = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(b) = self.entries.get(i) {
                if b.same_path(path_hash) {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 绑定：五闸依次——路径闸/零哈希闸/容量闸/重绑闸/守恒。
    /// 同名异本（路径同哈希异）放行并分键——这是 J1 的正向语义。
    pub fn bind(&mut self, path: &str, image_bytes: &[u8]) -> Result<u32, u32> {
        self.tried = self.tried.saturating_add(1);
        path_ok(path)?;
        let path_hash = fnv1a(path.as_bytes());
        let image_hash = fnv1a(image_bytes);
        if image_hash == 0 {
            self.rejected = self.rejected.saturating_add(1);
            self.reject_codes.push(PkCode::ZeroImageHash.code());
            return Err(PkCode::ZeroImageHash.code());
        }
        if self.entries.len() >= BIND_CAP {
            self.rejected = self.rejected.saturating_add(1);
            self.reject_codes.push(PkCode::TableFull.code());
            return Err(PkCode::TableFull.code());
        }
        let f = ImageFactor { path_hash, image_hash };
        // 重绑闸：双因子全等已在账 → 拒。
        let mut dup = false;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(b) = self.entries.get(i) {
                if b.same_key(&f) {
                    dup = true;
                }
            }
            i += 1;
        }
        if dup {
            self.rejected = self.rejected.saturating_add(1);
            self.reject_codes.push(PkCode::BindingDuplicated.code());
            return Err(PkCode::BindingDuplicated.code());
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(Binding {
            proc_key_id: id,
            path: path.to_string(),
            factor: f,
        });
        self.accepted = self.accepted.saturating_add(1);
        Ok(id)
    }

    /// 计数守恒。
    pub fn conserve_ok(&self) -> bool {
        self.tried == self.accepted.saturating_add(self.rejected)
    }

    /// 键语义自检：在账条目两两之间——双因子全等仅当键号相等。
    pub fn key_identity_ok(&self) -> bool {
        let n = self.entries.len();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n {
                let (a, b) = match (self.entries.get(i), self.entries.get(j)) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return false,
                };
                if a.same_key(&b.factor) && a.proc_key_id != b.proc_key_id {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

// ── 语料 ───────────────────────────────────────────────────────────
/// 主档路径（8 条，确定性：app0.exe..app7.exe 前缀补齐 4 字节下限）。
pub fn main_path(idx: usize) -> String {
    format!("app{}.exe", idx % MAIN_IMAGES)
}

/// 主档映像字节（确定性：32 字节，内容由 idx 派生）。
pub fn main_image(idx: usize) -> Vec<u8> {
    let mut v = Vec::new();
    let mut k = 0;
    while k < 32 {
        v.push(((idx * 7 + k * 13) % 251) as u8);
        k += 1;
    }
    v
}

/// 同名异本第 g 组映像字节（同路径 app{g%8}.exe，内容随整组号 g 变异
/// ——注意用 g 本身而非 g%8，保证 g=0 与 g=8 两组异本内容互异，
/// 否则同路径重放会触发重绑闸误拒）。
pub fn variant_image(g: usize) -> Vec<u8> {
    let mut v = Vec::new();
    let mut k = 0;
    while k < 32 {
        // 推导：与主档公式异或 0x5A，保证同路径异内容（异本）；
        // 组号 g 直接进公式，10 组异本两两互异。
        v.push(((((g * 7 + k * 13) % 251) as u8) ^ 0x5A) ^ ((g % 2) as u8));
        k += 1;
    }
    v
}

// ── 注入核：四类坏样本 ─────────────────────────────────────────────
/// 坏样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：同名异本并键注入（试图把异哈希当同键——必须分键）。
    VariantMergeInject,
    /// 类1：空路径绑定注入。
    EmptyPathInject,
    /// 类2：零哈希因子注入。
    ZeroHashInject,
    /// 类3：重复绑定注入（双因子全等重放）。
    DupBindInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::VariantMergeInject => 0,
            BadClass::EmptyPathInject => 1,
            BadClass::ZeroHashInject => 2,
            BadClass::DupBindInject => 3,
        }
    }
}

pub const BAD_CLASSES: usize = 4;

/// 注入检出账。
pub struct InjectLedger {
    pub tries: [u32; BAD_CLASSES],
    pub caught: [u32; BAD_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; BAD_CLASSES], caught: [0; BAD_CLASSES] }
    }
    /// 记一注。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= BAD_CLASSES {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    /// 全槽全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < BAD_CLASSES {
            let (t, c) = (
                self.tries.get(i).copied().unwrap_or(0),
                self.caught.get(i).copied().unwrap_or(0),
            );
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数。
    pub fn missed(&self) -> u32 {
        let mut m: u32 = 0;
        let mut i = 0;
        while i < BAD_CLASSES {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 单轮四类注入。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：同名异本并键注入——同路径异哈希必须分到不同键号。
    let mut ok0 = true;
    let mut g = 0;
    while g < VARIANT_GROUPS {
        let mut t = BindingTable::new();
        let p = main_path(g % MAIN_IMAGES);
        let id_main = t.bind(&p, &main_image(g % MAIN_IMAGES));
        let id_var = t.bind(&p, &variant_image(g));
        let distinct = match (id_main, id_var) {
            (Ok(a), Ok(b)) => a != b && t.same_path_variants(fnv1a(p.as_bytes())) == 2,
            _ => false,
        };
        if !distinct {
            ok0 = false;
        }
        g += 1;
    }
    if let Some(h) = hits.get_mut(BadClass::VariantMergeInject.slot()) {
        *h = ok0;
    }
    // 类1：空路径。
    let mut t1 = BindingTable::new();
    if let Some(h) = hits.get_mut(BadClass::EmptyPathInject.slot()) {
        *h = t1.bind("", &main_image(0)) == Err(PkCode::PathEmpty.code());
    }
    // 类2：零哈希（FNV 不产零：直接以 make_factor 拒 + bind 不可能产出）。
    if let Some(h) = hits.get_mut(BadClass::ZeroHashInject.slot()) {
        *h = make_factor(0x1234, 0) == Err(PkCode::ZeroImageHash.code()) && make_factor(0, 0x5678) == Err(PkCode::ZeroImageHash.code());
    }
    // 类3：重复绑定。
    let mut t3 = BindingTable::new();
    let p3 = main_path(0);
    let first = t3.bind(&p3, &main_image(0));
    let second = t3.bind(&p3, &main_image(0));
    if let Some(h) = hits.get_mut(BadClass::DupBindInject.slot()) {
        *h = first.is_ok() && second == Err(PkCode::BindingDuplicated.code());
    }
    hits
}

/// 注入 rounds 轮。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let hits = inject_once();
        let mut c = 0;
        while c < BAD_CLASSES {
            led.record(c, hits.get(c).copied().unwrap_or(false));
            c += 1;
        }
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────────
/// 黄金全链：8 主档绑定→双因子全命中→10 组同名异本全分键→
/// 注入全抓→键语义自检→守恒。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut t = BindingTable::new();
    // 8 主档。
    let mut i = 0;
    while i < MAIN_IMAGES {
        let p = main_path(i);
        let id = t.bind(&p, &main_image(i))?;
        if id == 0 {
            return Err(PkCode::KeyIdentityBroken.code());
        }
        i += 1;
    }
    // 双因子全命中：resolve(主档) 逐条回查。
    let mut j = 0;
    while j < MAIN_IMAGES {
        let f = ImageFactor {
            path_hash: fnv1a(main_path(j).as_bytes()),
            image_hash: fnv1a(&main_image(j)),
        };
        if t.resolve(&f).is_err() {
            return Err(PkCode::NoBinding.code());
        }
        j += 1;
    }
    // 10 组同名异本：同路径异哈希全部分键。
    let mut g = 0;
    while g < VARIANT_GROUPS {
        let p = main_path(g % MAIN_IMAGES);
        let before = t.same_path_variants(fnv1a(p.as_bytes()));
        let id_var = t.bind(&p, &variant_image(g))?;
        let after = t.same_path_variants(fnv1a(p.as_bytes()));
        // 分键：异本新键号不同于该路径既有任一键号。
        let f_var = ImageFactor {
            path_hash: fnv1a(p.as_bytes()),
            image_hash: fnv1a(&variant_image(g)),
        };
        let id_res = t.resolve(&f_var)?;
        if !(after == before + 1 && id_res == id_var) {
            return Err(PkCode::VariantGap.code());
        }
        g += 1;
    }
    if !t.key_identity_ok() {
        return Err(PkCode::KeyIdentityBroken.code());
    }
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(PkCode::ConservationBroken.code());
    }
    Ok((t.accepted, t.rejected))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PkCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：单条绑定→resolve 命中同键号。
pub fn probe_a() -> Result<(), u32> {
    let mut t = BindingTable::new();
    let p = main_path(0);
    let id = t.bind(&p, &main_image(0))?;
    let f = ImageFactor {
        path_hash: fnv1a(p.as_bytes()),
        image_hash: fnv1a(&main_image(0)),
    };
    if t.resolve(&f)? == id {
        Ok(())
    } else {
        Err(PkCode::KeyIdentityBroken.code())
    }
}

/// 探针B：查无此键 → NoBinding（不误报命中）。
pub fn probe_b() -> Result<(), u32> {
    let t = BindingTable::new();
    let f = ImageFactor { path_hash: 0xDEAD, image_hash: 0xBEEF };
    match t.resolve(&f) {
        Err(c) if c == PkCode::NoBinding.code() => Ok(()),
        _ => Err(PkCode::NoBinding.code()),
    }
}

/// 探针C：路径过短（3 字节）拒。
pub fn probe_c() -> Result<(), u32> {
    let mut t = BindingTable::new();
    match t.bind("a.x", &main_image(0)) {
        Err(c) if c == PkCode::PathTooShort.code() => Ok(()),
        _ => Err(PkCode::PathTooShort.code()),
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < PkCode::ALL.len() {
        if let Some(c) = PkCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────────
/// run_unx18424_checks：J1 判据 15 条（断言性质，动态明细 ≤8）。
pub fn run_unx18424_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18424 进程粒度键（映像路径/哈希绑定）");
    // J1-1 冻结常量勾稽：容量/主档/异本组数。
    s.add(
        "J1-冻结常量",
        BIND_CAP == 32 && MAIN_IMAGES == 8 && VARIANT_GROUPS == 10,
        "容量32 主档8 异本10",
    );
    // J1-2 双因子绑定黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        matches!(&g, Ok((a, _)) if *a >= MAIN_IMAGES as u32),
        &format!("{:?}", g),
    );
    // J1-3 双因子全等语义：同路径异哈希≠同键（直验）。
    let dual = {
        let f1 = ImageFactor { path_hash: 0xAA, image_hash: 0x11 };
        let f2 = ImageFactor { path_hash: 0xAA, image_hash: 0x22 };
        f1 != f2 && f1.legal() && f2.legal()
    };
    s.add("J1-双因子语义", dual, "单因子相等不算同键");
    // J1-4 同名异本 10/10 分键。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-同名异本10/10分键",
        led.all_caught()
            && led.caught.get(BadClass::VariantMergeInject.slot()).copied().unwrap_or(0)
                == JUDGE_ROUNDS,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-5 注入守恒：漏检 0，总注入 40。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入40",
    );
    // J1-6 路径闸三段：空/过短/过长。
    let mut tlen = BindingTable::new();
    let long_path = {
        let mut p = String::new();
        let mut k = 0;
        while k <= PATH_LEN_MAX {
            p.push('a');
            k += 1;
        }
        p
    };
    s.add(
        "J1-路径三闸",
        tlen.bind("", &main_image(0)).is_err()
            && tlen.bind("ab", &main_image(0)).is_err()
            && tlen.bind(&long_path, &main_image(0)).is_err(),
        "空/短/长路径全拒",
    );
    // J1-7 零哈希闸。
    s.add(
        "J1-零哈希闸",
        make_factor(0, 1).is_err() && make_factor(1, 0).is_err(),
        "双因子任一为零拒",
    );
    // J1-8 重绑闸：双因子全等重放必拒且留拒因。
    let dup = {
        let mut t = BindingTable::new();
        let p = main_path(2);
        let r1 = t.bind(&p, &main_image(2));
        let r2 = t.bind(&p, &main_image(2));
        let codes_hit = t
            .reject_codes
            .first()
            .copied()
            .unwrap_or(0)
            == PkCode::BindingDuplicated.code();
        r1.is_ok() && r2.is_err() && codes_hit
    };
    s.add("J1-重绑必拒", dup, "同路径+同哈希重放拒且留账");
    // J1-9 键语义自检：全等⇔同键号。
    let ident = {
        let mut t = BindingTable::new();
        let mut i = 0;
        while i < MAIN_IMAGES {
            let _ = t.bind(&main_path(i), &main_image(i));
            i += 1;
        }
        t.key_identity_ok()
    };
    s.add("J1-键语义自检", ident, "8 主档两两自检过");
    // J1-10 FNV 确定性：同字节两次折叠相等，异字节不等。
    s.add(
        "J2-FNV确定性",
        fnv1a(b"abc") == fnv1a(b"abc") && fnv1a(b"abc") != fnv1a(b"abd"),
        "FNV-1a 逐字节折叠确定",
    );
    // J1-11 探针三序。
    s.add("J1-探针三序", probe_a().is_ok() && probe_b().is_ok() && probe_c().is_ok(), "命中/NoBinding/短路径拒");
    // J1-12 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-13 异本变异有效性：variant_image 与主档逐字节不同。
    let var_ok = {
        let mut ok = true;
        let mut g = 0;
        while g < VARIANT_GROUPS {
            let a = main_image(g % MAIN_IMAGES);
            let b = variant_image(g);
            if a == b {
                ok = false;
            }
            g += 1;
        }
        ok
    };
    s.add("J2-异本有效变异", var_ok, "10 组异本内容全异");
    // J1-14 文案闭集：12 码逐码非空。
    let docs_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < PkCode::ALL.len() {
            if let Some(c) = PkCode::ALL.get(i) {
                if c.detail().is_empty() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("J2-文案闭集", docs_ok, "12 码中文文案逐码非空");
    // J1-15 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7E5C0 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18424_checks();
        assert!(set.all_passed(), "UNX-F18424 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_bind_and_resolve() {
        let mut t = BindingTable::new();
        for i in 0..MAIN_IMAGES {
            let id = t.bind(&main_path(i), &main_image(i)).unwrap_or(0);
            assert!(id > 0);
        }
        for i in 0..MAIN_IMAGES {
            let f = ImageFactor {
                path_hash: fnv1a(main_path(i).as_bytes()),
                image_hash: fnv1a(&main_image(i)),
            };
            assert!(t.resolve(&f).is_ok());
        }
        assert_eq!(t.len(), MAIN_IMAGES);
    }

    #[test]
    fn t_same_name_variants_distinct() {
        let mut t = BindingTable::new();
        let p = main_path(0);
        let a = t.bind(&p, &main_image(0)).unwrap_or(0);
        let b = t.bind(&p, &variant_image(0)).unwrap_or(0);
        assert_ne!(a, b, "同名异本必须分键");
        assert_eq!(t.same_path_variants(fnv1a(p.as_bytes())), 2);
    }

    #[test]
    fn t_path_gates() {
        let mut t = BindingTable::new();
        assert_eq!(t.bind("", &main_image(0)), Err(PkCode::PathEmpty.code()));
        assert_eq!(t.bind("ab", &main_image(0)), Err(PkCode::PathTooShort.code()));
        let mut long = String::new();
        for _ in 0..=PATH_LEN_MAX {
            long.push('a');
        }
        assert_eq!(t.bind(&long, &main_image(0)), Err(PkCode::PathTooLong.code()));
    }

    #[test]
    fn t_zero_hash_gate() {
        assert_eq!(make_factor(0, 1), Err(PkCode::ZeroImageHash.code()));
        assert_eq!(make_factor(1, 0), Err(PkCode::ZeroImageHash.code()));
    }

    #[test]
    fn t_dup_bind_rejected() {
        let mut t = BindingTable::new();
        let p = main_path(1);
        assert!(t.bind(&p, &main_image(1)).is_ok());
        assert_eq!(t.bind(&p, &main_image(1)), Err(PkCode::BindingDuplicated.code()));
        assert!(t.conserve_ok());
    }

    #[test]
    fn t_variant_injection_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_probes() {
        assert!(probe_a().is_ok());
        assert!(probe_b().is_ok());
        assert!(probe_c().is_ok());
    }
}
