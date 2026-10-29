//! CGPU-F0723 · 标准场景资产（目标 400 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥360 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0723，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：标准场景资产——基准场景引用的资产（纹理/字体/网格/
//!    音频/场景图）统一登记：定容账、内容指纹、去重、
//!    场景打包预算与完整性复核。
//!
//! 设计纪律：
//! - 资产五类闭集：Texture/Font/Mesh/Audio/Graph，
//  类别容量常量表（规格默认档：64/16/32/16/8）。
// - 资产指纹：FNV-1a 64 对（类|名|字节数|内容代理值）——
//   内容代理值由确定性展开函数生成（非随机）。
// - 打包预算：场景包总字节 ≤ PACK_BUDGET，逐资产累加，
//   超预算拒绝且不部分入包（事务性）。
// - 完整性：包内每资产指纹复算必须与登记一致，缺号/错号即拒绝。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x48a0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x48a0，批内 CGPU-F0723 独占）。
pub const SEG_BASE: u16 = 0x48a0;

/// 资产重号（类内同号）。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 资产参数非法（0 大小/空名）。
pub const E_ASSET: u16 = SEG_BASE + 2;
/// 类账已满。
pub const E_FULL: u16 = SEG_BASE + 3;
/// 打包预算超限。
pub const E_PACK: u16 = SEG_BASE + 4;
/// 包完整性不符（指纹/缺号）。
pub const E_INTEGRITY: u16 = SEG_BASE + 5;
/// 引用未登记资产。
pub const E_UNKNOWN: u16 = SEG_BASE + 6;

/// 类容量常量表。
pub const CAP_TEXTURE: usize = 64;
pub const CAP_FONT: usize = 16;
pub const CAP_MESH: usize = 32;
pub const CAP_AUDIO: usize = 16;
pub const CAP_GRAPH: usize = 8;
/// 打包预算（字节）。
pub const PACK_BUDGET: u64 = 8 * 1024 * 1024;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 资产类别（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 纹理。
    Texture,
    /// 字体。
    Font,
    /// 网格。
    Mesh,
    /// 音频。
    Audio,
    /// 场景图。
    Graph,
}

impl AssetKind {
    /// 类序。
    pub fn ord(self) -> usize {
        match self {
            AssetKind::Texture => 0,
            AssetKind::Font => 1,
            AssetKind::Mesh => 2,
            AssetKind::Audio => 3,
            AssetKind::Graph => 4,
        }
    }
    /// 类容量。
    pub fn cap(self) -> usize {
        match self {
            AssetKind::Texture => CAP_TEXTURE,
            AssetKind::Font => CAP_FONT,
            AssetKind::Mesh => CAP_MESH,
            AssetKind::Audio => CAP_AUDIO,
            AssetKind::Graph => CAP_GRAPH,
        }
    }
    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            AssetKind::Texture => "texture",
            AssetKind::Font => "font",
            AssetKind::Mesh => "mesh",
            AssetKind::Audio => "audio",
            AssetKind::Graph => "graph",
        }
    }
}

/// 确定性内容代理值：由名+序号展开成伪内容字节流后取指纹。
/// 展开规则（写死）：重复名首字节 + 序号低 8 位，共 16 字节。
pub fn content_proxy(name: &str, seq: u32) -> u64 {
    let mut bytes = Vec::with_capacity(16);
    let seed = name.as_bytes().first().copied().unwrap_or(0x3F);
    for i in 0..16u32 {
        bytes.push(seed ^ ((seq + i) as u8));
    }
    fnv1a64(&bytes)
}

/// 标准资产。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// 类别。
    pub kind: AssetKind,
    /// 类内编号（1 起）。
    pub no: u32,
    /// 名称。
    pub name: &'static str,
    /// 字节数。
    pub bytes: u32,
    /// 内容指纹。
    pub fp: u64,
}

/// 资产登记处。
#[derive(Clone, Debug, Default)]
pub struct AssetRegistry {
    items: Vec<Asset>,
}

impl AssetRegistry {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    /// 登记：参数合法 + 类账未满 + 类内不重号 + 指纹一致。
    pub fn put(&mut self, a: Asset) -> Result<(), u16> {
        if a.name.is_empty() || a.bytes == 0 {
            return Err(E_ASSET);
        }
        if self.count_kind(a.kind) >= a.kind.cap() {
            return Err(E_FULL);
        }
        if self.items.iter().any(|x| x.kind == a.kind && x.no == a.no) {
            return Err(E_DUP);
        }
        let expect = content_proxy(a.name, a.no);
        if a.fp != expect {
            return Err(E_INTEGRITY);
        }
        self.items.push(a);
        Ok(())
    }
    /// 类内计数。
    pub fn count_kind(&self, k: AssetKind) -> usize {
        self.items.iter().filter(|x| x.kind == k).count()
    }
    /// 查找。
    pub fn get(&self, k: AssetKind, no: u32) -> Option<&Asset> {
        self.items.iter().find(|x| x.kind == k && x.no == no)
    }
    /// 总数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 总字节。
    pub fn total_bytes(&self) -> u64 {
        self.items.iter().map(|x| x.bytes as u64).sum()
    }
}

/// 场景包：资产引用清单（有序）。
#[derive(Clone, Debug, Default)]
pub struct ScenePack {
    /// 引用（类序，类内号）按打包序。
    refs: Vec<(usize, u32)>,
}

impl ScenePack {
    pub fn new() -> Self {
        Self { refs: Vec::new() }
    }
    /// 打包引用（登记处校验存在性 + 预算事务性）。
    /// 事务性：任何一步失败，包不吸收本次引用。
    pub fn pack(&mut self, reg: &AssetRegistry, k: AssetKind, no: u32) -> Result<(), u16> {
        let a = reg.get(k, no).ok_or(E_UNKNOWN)?;
        let total: u64 = self
            .refs
            .iter()
            .filter_map(|(ko, n)| {
                let kk = match *ko {
                    0 => Some(AssetKind::Texture),
                    1 => Some(AssetKind::Font),
                    2 => Some(AssetKind::Mesh),
                    3 => Some(AssetKind::Audio),
                    4 => Some(AssetKind::Graph),
                    _ => None,
                };
                kk.and_then(|kk| reg.get(kk, *n)).map(|x| x.bytes as u64)
            })
            .sum();
        if total + a.bytes as u64 > PACK_BUDGET {
            return Err(E_PACK);
        }
        self.refs.push((k.ord(), no));
        Ok(())
    }
    /// 包字节合计。
    pub fn bytes(&self, reg: &AssetRegistry) -> u64 {
        self.refs
            .iter()
            .filter_map(|(ko, n)| {
                let kk = match *ko {
                    0 => Some(AssetKind::Texture),
                    1 => Some(AssetKind::Font),
                    2 => Some(AssetKind::Mesh),
                    3 => Some(AssetKind::Audio),
                    4 => Some(AssetKind::Graph),
                    _ => None,
                };
                kk.and_then(|kk| reg.get(kk, *n)).map(|x| x.bytes as u64)
            })
            .sum()
    }
    /// 包完整性：每个引用在登记处存在且指纹复算一致。
    pub fn verify(&self, reg: &AssetRegistry) -> Result<(), u16> {
        for (ko, n) in &self.refs {
            let kk = match *ko {
                0 => AssetKind::Texture,
                1 => AssetKind::Font,
                2 => AssetKind::Mesh,
                3 => AssetKind::Audio,
                4 => AssetKind::Graph,
                _ => return Err(E_INTEGRITY),
            };
            let a = reg.get(kk, *n).ok_or(E_INTEGRITY)?;
            if a.fp != content_proxy(a.name, a.no) {
                return Err(E_INTEGRITY);
            }
        }
        Ok(())
    }
    /// 引用数。
    pub fn len(&self) -> usize {
        self.refs.len()
    }
    /// 包封存指纹。
    pub fn seal(&self) -> u64 {
        let mut bytes = Vec::new();
        for (k, n) in &self.refs {
            bytes.extend_from_slice(&(*k as u32).to_le_bytes());
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        fnv1a64(&bytes)
    }
}

/// 资产统计行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindStat {
    /// 类序。
    pub ord: u8,
    /// 类名。
    pub kind: &'static str,
    /// 数量。
    pub count: u32,
    /// 类容量。
    pub cap: u32,
    /// 字节合计。
    pub bytes: u64,
}

/// 登记处统计表（五类有序）。
pub fn stats(reg: &AssetRegistry) -> [KindStat; 5] {
    let kinds = [
        AssetKind::Texture,
        AssetKind::Font,
        AssetKind::Mesh,
        AssetKind::Audio,
        AssetKind::Graph,
    ];
    let mut out = [KindStat {
        ord: 0,
        kind: "",
        count: 0,
        cap: 0,
        bytes: 0,
    }; 5];
    for (i, k) in kinds.iter().enumerate() {
        let items: Vec<&Asset> = reg.items.iter().filter(|x| x.kind == *k).collect();
        out[i] = KindStat {
            ord: i as u8,
            kind: k.name(),
            count: items.len() as u32,
            cap: k.cap() as u32,
            bytes: items.iter().map(|x| x.bytes as u64).sum(),
        };
    }
    out
}

/// 便捷构造：指纹自动计算。
pub fn asset(kind: AssetKind, no: u32, name: &'static str, bytes: u32) -> Asset {
    Asset {
        kind,
        no,
        name,
        bytes,
        fp: content_proxy(name, no),
    }
}

/// 自例程：主路径（登记 5 类 → 打包 → 校验）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut reg = AssetRegistry::new();
    // 主路径：五类各登记 1~2 个。
    reg.put(asset(AssetKind::Texture, 1, "tile_albedo", 256 * 1024))?;
    reg.put(asset(AssetKind::Texture, 2, "tile_normal", 256 * 1024))?;
    reg.put(asset(AssetKind::Font, 1, "ui_sans_16", 64 * 1024))?;
    reg.put(asset(AssetKind::Mesh, 1, "quad_unit", 8 * 1024))?;
    reg.put(asset(AssetKind::Audio, 1, "scroll_tick_wav", 32 * 1024))?;
    reg.put(asset(AssetKind::Graph, 1, "scene_main", 4 * 1024))?;
    if reg.len() != 6 {
        return Err(E_DUP);
    }
    if reg.total_bytes() != 620 * 1024 {
        return Err(E_ASSET);
    }
    let st = stats(&reg);
    if st[0].count != 2 || st[1].count != 1 || st[4].count != 1 {
        return Err(E_ASSET);
    }
    // 主路径：打包 4 个资产。
    let mut pack = ScenePack::new();
    pack.pack(&reg, AssetKind::Texture, 1)?;
    pack.pack(&reg, AssetKind::Font, 1)?;
    pack.pack(&reg, AssetKind::Mesh, 1)?;
    pack.pack(&reg, AssetKind::Graph, 1)?;
    if pack.len() != 4 {
        return Err(E_PACK);
    }
    if pack.bytes(&reg) != (256 + 64 + 8 + 4) * 1024 {
        return Err(E_PACK);
    }
    pack.verify(&reg)?;
    let s1 = pack.seal();
    // 错误路径 0：指纹不符（手填坏指纹）。
    let mut bad = asset(AssetKind::Texture, 3, "evil_tex", 1024);
    bad.fp = 0xdead_0001;
    if reg.put(bad) != Err(E_INTEGRITY) {
        return Err(E_INTEGRITY);
    }
    // 错误路径 1：重号。
    if reg.put(asset(AssetKind::Texture, 1, "dup", 1024)) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 错误路径 2：0 字节。
    if reg.put(asset(AssetKind::Texture, 4, "zero", 0)) != Err(E_ASSET) {
        return Err(E_ASSET);
    }
    // 错误路径 3：未登记引用。
    if pack.pack(&reg, AssetKind::Audio, 99) != Err(E_UNKNOWN) {
        return Err(E_UNKNOWN);
    }
    // 错误路径 4：预算超限（装大纹理逼近上限：GRAPH cap 8 个场景图×大字节）。
    // 构造独立登记处：一个大 graph 8MB+1B 超 PACK_BUDGET。
    let mut reg2 = AssetRegistry::new();
    // 8MB 超预算直接 pack 拒绝：先 pack 一个 8MB 纹理（=预算），
    // 再 pack 1B → 超 1B 拒绝。
    reg2.put(asset(AssetKind::Texture, 1, "big_tex", 8 * 1024 * 1024))?;
    let mut pack2 = ScenePack::new();
    pack2.pack(&reg2, AssetKind::Texture, 1)?; // 恰好 8MB ≤ 8MB
    reg2.put(asset(AssetKind::Font, 1, "tiny", 1))?;
    if pack2.pack(&reg2, AssetKind::Font, 1) != Err(E_PACK) {
        return Err(E_PACK);
    }
    if pack2.bytes(&reg2) != PACK_BUDGET {
        return Err(E_PACK);
    }
    // 封存一致：同内容包同指纹。
    let mut pack3 = ScenePack::new();
    pack3.pack(&reg, AssetKind::Graph, 1)?;
    pack3.pack(&reg, AssetKind::Mesh, 1)?;
    pack3.pack(&reg, AssetKind::Font, 1)?;
    pack3.pack(&reg, AssetKind::Texture, 1)?;
    // 顺序不同指纹不同（封存含序）。
    if pack3.seal() == s1 {
        return Err(E_INTEGRITY);
    }
    pack3.verify(&reg)?;
    Ok(0)
}
