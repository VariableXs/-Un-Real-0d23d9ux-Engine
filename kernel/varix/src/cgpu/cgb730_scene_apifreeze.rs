//! CGPU-F0730 · 场景内容 API 冻结 v1（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0730，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：场景内容 API 冻结 v1——对场景内容子系统的对外 API
//!    做 v1 冻结：类型化签名（参数类别向量）、结构等价判定、
//!    二进制兼容分级与冻结登记簿（登记簿本身只追加）。
//!
//! 设计纪律（与 F0713 的差异：本单以"类型化签名 + 二进制兼容
//! 分级"为核心，登记簿 append-only，无删除概念）：
// - 参数类别闭集八类：U8/U16/U32/U64/Bool/Ref/Buf/Handle。
// - 结构等价：参数类别向量逐位相等 + 返回类别相等。
// - 二进制兼容三态闭集：Identical（全同）/Compatible（只增
//   参数且新增参数在尾部）/Breaking（其余任何差异）。
// - 冻结登记簿 append-only：条目录入后不可改；重复键拒绝。
// - v1 门：所有标记 Frozen 意图的条目必须 Compatible 或 Identical
//   相对基准签名；Breaking 即拒绝冻结。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4980 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4980，批内 CGPU-F0730 独占）。
pub const SEG_BASE: u16 = 0x4980;

/// 重复键。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 键非法（0 或超上限）。
pub const E_KEY: u16 = SEG_BASE + 2;
/// 签名非法（空参数向量或参数超上限）。
pub const E_SIG: u16 = SEG_BASE + 3;
/// 破坏性变更（Breaking 相对基准）。
pub const E_BREAKING: u16 = SEG_BASE + 4;
/// 登记簿为空不许冻结。
pub const E_EMPTY: u16 = SEG_BASE + 5;
/// 未知键查询。
pub const E_NO_KEY: u16 = SEG_BASE + 6;

/// 键上限。
pub const KEY_MAX: u32 = 1024;
/// 参数向量上限。
pub const PARAMS_MAX: usize = 8;

/// 参数类别（闭集八类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PKind {
    /// u8。
    U8,
    /// u16。
    U16,
    /// u32。
    U32,
    /// u64。
    U64,
    /// bool。
    Bool,
    /// 引用。
    Ref,
    /// 缓冲。
    Buf,
    /// 句柄。
    Handle,
}

impl PKind {
    /// 二进制大小类（字节，写死）：
    /// U8=1 U16=2 U32=4 U64=8 Bool=1 Ref=8 Buf=8 Handle=4。
    pub fn size(self) -> u32 {
        match self {
            PKind::U8 | PKind::Bool => 1,
            PKind::U16 => 2,
            PKind::U32 | PKind::Handle => 4,
            PKind::U64 | PKind::Ref | PKind::Buf => 8,
        }
    }
    /// 类序（指纹用）。
    pub fn ord(self) -> u8 {
        match self {
            PKind::U8 => 0,
            PKind::U16 => 1,
            PKind::U32 => 2,
            PKind::U64 => 3,
            PKind::Bool => 4,
            PKind::Ref => 5,
            PKind::Buf => 6,
            PKind::Handle => 7,
        }
    }
}

/// 类型化签名：参数类别向量 + 返回类别。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// 参数类别向量（非空，≤8）。
    pub params: Vec<PKind>,
    /// 返回类别。
    pub ret: PKind,
}

impl Signature {
    /// 结构校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.params.is_empty() || self.params.len() > PARAMS_MAX {
            return Err(E_SIG);
        }
        Ok(())
    }
    /// 参数栈字节数（Σ size，溢出安全）。
    pub fn stack_bytes(&self) -> Result<u64, u16> {
        let mut s: u64 = 0;
        for p in &self.params {
            s = s
                .checked_add(p.size() as u64)
                .ok_or(E_SIG)?;
        }
        Ok(s)
    }
    /// 签名指纹。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for p in &self.params {
            h ^= p.ord() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h ^= 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.ret.ord() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h
    }
}

/// 二进制兼容三态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compat {
    /// 完全同构。
    Identical,
    /// 尾部加参兼容。
    Compatible,
    /// 破坏性。
    Breaking,
}

/// 结构等价 + 兼容分级：
///   参数向量相等且返回相等 → Identical；
///   前缀相等且 new 比 base 长（尾部加参）且返回相等 → Compatible；
///   其余 → Breaking。
pub fn compat(base: &Signature, new: &Signature) -> Result<Compat, u16> {
    base.validate()?;
    new.validate()?;
    if base.params == new.params && base.ret == new.ret {
        return Ok(Compat::Identical);
    }
    if base.ret == new.ret && new.params.len() > base.params.len() {
        let prefix_same = base
            .params
            .iter()
            .zip(new.params.iter())
            .all(|(a, b)| a == b);
        if prefix_same {
            return Ok(Compat::Compatible);
        }
    }
    Ok(Compat::Breaking)
}

/// 冻结登记簿条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreezeEntry {
    /// API 键（1..=1024）。
    pub key: u32,
    /// 名称（用键语义标注，存静态短名）。
    pub name: &'static str,
    /// 基准签名（v0）。
    pub base: Signature,
    /// 意图层级：true=本版要冻结。
    pub freeze_intent: bool,
}

/// 登记簿（append-only）。
#[derive(Clone, Debug, Default)]
pub struct FreezeBook {
    entries: Vec<FreezeEntry>,
}

impl FreezeBook {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }
    /// 录入（append-only：只增不改不删）。
    pub fn add(&mut self, e: FreezeEntry) -> Result<(), u16> {
        if e.key == 0 || e.key > KEY_MAX {
            return Err(E_KEY);
        }
        e.base.validate()?;
        if self.entries.iter().any(|x| x.key == e.key) {
            return Err(E_DUP);
        }
        self.entries.push(e);
        Ok(())
    }
    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 查键。
    pub fn get(&self, key: u32) -> Result<&FreezeEntry, u16> {
        self.entries.iter().find(|x| x.key == key).ok_or(E_NO_KEY)
    }
    /// 登记簿指纹（按键升序折叠条目指纹）。
    pub fn book_fp(&self) -> u64 {
        let mut keys: Vec<u32> = self.entries.iter().map(|e| e.key).collect();
        for i in 0..keys.len() {
            let mut m = i;
            for j in i + 1..keys.len() {
                if keys[j] < keys[m] {
                    m = j;
                }
            }
            keys.swap(i, m);
        }
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        for k in &keys {
            let e = self.entries.iter().find(|e| e.key == *k);
            let f = e.map(|x| {
                let mut hh = x.base.fp();
                hh ^= x.key as u64;
                hh = hh.wrapping_mul(0x0000_0100_0000_01B3);
                hh
            });
            h ^= f.unwrap_or(0);
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

/// 冻结 v1 门：对每个 freeze_intent 条目给候选签名，
/// 必须 Compatible/Identical；任一 Breaking 即整门拒绝。
/// 返回（冻结条目数, 登记簿指纹）。
pub fn freeze_v1(
    book: &FreezeBook,
    candidates: &[(u32, Signature)],
) -> Result<(usize, u64), u16> {
    if book.is_empty() {
        return Err(E_EMPTY);
    }
    let mut frozen = 0usize;
    for e in book.entries.iter().filter(|e| e.freeze_intent) {
        let cand = candidates
            .iter()
            .find(|(k, _)| *k == e.key)
            .map(|(_, s)| s)
            .ok_or(E_NO_KEY)?;
        match compat(&e.base, cand)? {
            Compat::Identical | Compat::Compatible => frozen += 1,
            Compat::Breaking => return Err(E_BREAKING),
        }
    }
    Ok((frozen, book.book_fp()))
}

/// 兼容统计行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatStat {
    /// 同构数。
    pub identical: u32,
    /// 兼容数。
    pub compatible: u32,
    /// 破坏数。
    pub breaking: u32,
}

/// 候选批统计（不设门，只统计——供报告）。
pub fn compat_stat(book: &FreezeBook, candidates: &[(u32, Signature)]) -> Result<CompatStat, u16> {
    let mut st = CompatStat {
        identical: 0,
        compatible: 0,
        breaking: 0,
    };
    for e in &book.entries {
        if let Some((_, s)) = candidates.iter().find(|(k, _)| *k == e.key) {
            match compat(&e.base, s)? {
                Compat::Identical => st.identical += 1,
                Compat::Compatible => st.compatible += 1,
                Compat::Breaking => st.breaking += 1,
            }
        }
    }
    Ok(st)
}

/// 便捷构造签名。
pub fn sig(params: &[PKind], ret: PKind) -> Signature {
    Signature {
        params: params.to_vec(),
        ret,
    }
}

/// 自例程：主路径（登记 3 API → 候选 → freeze_v1）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut book = FreezeBook::new();
    // 主路径：三个 API（v0 签名）。
    book.add(FreezeEntry {
        key: 1,
        name: "scene_load",
        base: sig(&[PKind::U16, PKind::Buf], PKind::Handle),
        freeze_intent: true,
    })?;
    book.add(FreezeEntry {
        key: 2,
        name: "scene_unload",
        base: sig(&[PKind::Handle], PKind::Bool),
        freeze_intent: true,
    })?;
    book.add(FreezeEntry {
        key: 3,
        name: "scene_telem",
        base: sig(&[PKind::U16], PKind::U32),
        freeze_intent: false, // 不冻结，实验中
    })?;
    if book.len() != 3 {
        return Err(E_EMPTY);
    }
    // 错误路径 0：重复键。
    if book.add(FreezeEntry {
        key: 1,
        name: "dup",
        base: sig(&[PKind::U8], PKind::U8),
        freeze_intent: false,
    }) != Err(E_DUP)
    {
        return Err(E_DUP);
    }
    // 错误路径 1：键越界。
    if book.add(FreezeEntry {
        key: 0,
        name: "zero",
        base: sig(&[PKind::U8], PKind::U8),
        freeze_intent: false,
    }) != Err(E_KEY)
    {
        return Err(E_KEY);
    }
    // 主路径：候选（key1 同构；key2 尾部加一个 U32 → Compatible）。
    let cands = vec![
        (1u32, sig(&[PKind::U16, PKind::Buf], PKind::Handle)),
        (2u32, sig(&[PKind::Handle, PKind::U32], PKind::Bool)),
        (3u32, sig(&[PKind::U16, PKind::Bool], PKind::U32)),
    ];
    let (frozen, fp) = freeze_v1(&book, &cands)?;
    if frozen != 2 {
        return Err(E_BREAKING);
    }
    if fp == 0 {
        return Err(E_EMPTY);
    }
    // 统计：key1 Identical，key2 Compatible，key3 Breaking → (1,1,1)。
    let st = compat_stat(&book, &cands)?;
    if st.identical != 1 || st.compatible != 1 || st.breaking != 1 {
        return Err(E_SIG);
    }
    // 错误路径 2：key2 改返回类别 → Breaking → freeze_v1 拒。
    let cands_bad = vec![
        (1u32, sig(&[PKind::U16, PKind::Buf], PKind::Handle)),
        (2u32, sig(&[PKind::Handle], PKind::U32)),
    ];
    if freeze_v1(&book, &cands_bad) != Err(E_BREAKING) {
        return Err(E_BREAKING);
    }
    // 错误路径 3：缺候选。
    if freeze_v1(&book, &[]) != Err(E_NO_KEY) {
        return Err(E_NO_KEY);
    }
    // 错误路径 4：空簿冻结。
    let empty = FreezeBook::new();
    if freeze_v1(&empty, &cands) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 错误路径 5：签名空参数。
    let bad_sig = sig(&[], PKind::U8);
    if bad_sig.validate() != Err(E_SIG) {
        return Err(E_SIG);
    }
    // 栈字节：key1 = 2+8=10；key2 = 8。
    let e1 = book.get(1)?;
    if e1.base.stack_bytes()? != 10 {
        return Err(E_SIG);
    }
    let e2 = book.get(2)?;
    if e2.base.stack_bytes()? != 8 {
        return Err(E_SIG);
    }
    // 错误路径 6：未知键。
    if book.get(999) != Err(E_NO_KEY) {
        return Err(E_NO_KEY);
    }
    // 兼容三态纯函数直测：中段改参（非尾加）→ Breaking。
    let c_mid = compat(
        &sig(&[PKind::U8, PKind::U16, PKind::U32], PKind::Bool),
        &sig(&[PKind::U8, PKind::U32, PKind::U32], PKind::Bool),
    )?;
    if c_mid != Compat::Breaking {
        return Err(E_BREAKING);
    }
    // 同构。
    let c_id = compat(
        &sig(&[PKind::U8], PKind::U8),
        &sig(&[PKind::U8], PKind::U8),
    )?;
    if c_id != Compat::Identical {
        return Err(E_BREAKING);
    }
    Ok(0)
}
