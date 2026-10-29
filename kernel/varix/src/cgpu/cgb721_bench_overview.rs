//! CGPU-F0721 · 基准内容总览（目标 390 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0721，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：基准内容总览——把 E06 基准内容组（场景/资产/画像/变体/
//!    数据集）五类账目汇总成一张总览：分类计数、覆盖矩阵、
//!    完备度千分比与缺口清单，供收口与移交引用。
//!
//! 设计纪律：
//! - 内容五类闭集：Scenario/Asset/Profile/Variant/Dataset，
//!   每类定容上限（容量常量表写死，规格默认档）。
//! - 来源四档闭集：builtin/derived/imported/fuzz，
//!   覆盖矩阵 = 5 类 × 4 来源 = 20 格。
//! - 总览恒等式：每类账长 ≤ 容量；全账条目类内编号唯一
//  （守恒：Σ类账长 == 总条目数）。
//! - 完备度：已覆盖格 * 1000 / 20，整数截断；
//  缺口清单：空格的（类，来源）对按（类序,来源序）升序导出。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x4860 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4860，批内 CGPU-F0721 独占）。
pub const SEG_BASE: u16 = 0x4860;

/// 来源档越界引用（掩码 > 0x0F）。
pub const E_SOURCE: u16 = SEG_BASE + 1;
/// 定容账已满。
pub const E_FULL: u16 = SEG_BASE + 2;
/// 类内编号重号。
pub const E_DUP: u16 = SEG_BASE + 3;
/// 账目守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 总览为空（五类全空不许出总览）。
pub const E_EMPTY: u16 = SEG_BASE + 5;
/// 名称非法（空）。
pub const E_NAME: u16 = SEG_BASE + 6;

/// 五类容量（常量表：规格默认档）。
pub const CAP_SCENARIO: usize = 32;
pub const CAP_ASSET: usize = 64;
pub const CAP_PROFILE: usize = 16;
pub const CAP_VARIANT: usize = 48;
pub const CAP_DATASET: usize = 8;
/// 来源档。
pub const SOURCES: [&str; 4] = ["builtin", "derived", "imported", "fuzz"];
/// 覆盖矩阵总格数。
pub const GRID_CELLS: usize = 5 * 4;

/// 内容类别（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentClass {
    /// 场景。
    Scenario,
    /// 资产。
    Asset,
    /// 画像。
    Profile,
    /// 变体。
    Variant,
    /// 数据集。
    Dataset,
}

impl ContentClass {
    /// 类序号。
    pub fn ord(self) -> usize {
        match self {
            ContentClass::Scenario => 0,
            ContentClass::Asset => 1,
            ContentClass::Profile => 2,
            ContentClass::Variant => 3,
            ContentClass::Dataset => 4,
        }
    }
    /// 类容量。
    pub fn cap(self) -> usize {
        match self {
            ContentClass::Scenario => CAP_SCENARIO,
            ContentClass::Asset => CAP_ASSET,
            ContentClass::Profile => CAP_PROFILE,
            ContentClass::Variant => CAP_VARIANT,
            ContentClass::Dataset => CAP_DATASET,
        }
    }
    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            ContentClass::Scenario => "scenario",
            ContentClass::Asset => "asset",
            ContentClass::Profile => "profile",
            ContentClass::Variant => "variant",
            ContentClass::Dataset => "dataset",
        }
    }
    /// 按序号还原类别。
    pub fn from_ord(o: usize) -> Result<Self, u16> {
        match o {
            0 => Ok(ContentClass::Scenario),
            1 => Ok(ContentClass::Asset),
            2 => Ok(ContentClass::Profile),
            3 => Ok(ContentClass::Variant),
            4 => Ok(ContentClass::Dataset),
            _ => Err(E_SOURCE),
        }
    }
}

/// 内容条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentItem {
    /// 类别。
    pub class: ContentClass,
    /// 类内编号（1 起，类内唯一）。
    pub no: u32,
    /// 名称（非空）。
    pub name: &'static str,
    /// 来源覆盖掩码（bit i = SOURCES[i] 已覆盖）。
    pub src_mask: u8,
}

impl ContentItem {
    /// 掩码合法性。
    pub fn mask_ok(&self) -> bool {
        self.src_mask <= 0x0F
    }
    /// 覆盖档数。
    pub fn src_count(&self) -> u32 {
        self.src_mask.count_ones()
    }
}

/// 基准内容总账：五类合一（条目按登记序存放）。
#[derive(Clone, Debug, Default)]
pub struct ContentBook {
    items: Vec<ContentItem>,
}

impl ContentBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    /// 登记条目：名非空 + 掩码合法 + 类账未满 + 类内编号不重。
    pub fn put(&mut self, it: ContentItem) -> Result<(), u16> {
        if it.name.is_empty() {
            return Err(E_NAME);
        }
        if !it.mask_ok() {
            return Err(E_SOURCE);
        }
        if self.items.iter().filter(|x| x.class == it.class).count() >= it.class.cap() {
            return Err(E_FULL);
        }
        if self.items.iter().any(|x| x.class == it.class && x.no == it.no) {
            return Err(E_DUP);
        }
        self.items.push(it);
        Ok(())
    }
    /// 类账长。
    pub fn class_len(&self, c: ContentClass) -> usize {
        self.items.iter().filter(|x| x.class == c).count()
    }
    /// 总条目数。
    pub fn total(&self) -> usize {
        self.items.len()
    }
    /// 守恒审计：Σ类账长 == 总条目数（同一账本天然成立，
    /// 但显式复算防未来分账漂移）。
    pub fn audit(&self) -> Result<(), u16> {
        let mut sum = 0usize;
        for o in 0..5 {
            let c = ContentClass::from_ord(o)?;
            sum += self.class_len(c);
        }
        if sum != self.items.len() {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 覆盖矩阵：20 格布尔（某类是否在某个来源档有内容）。
    pub fn grid(&self) -> Result<[bool; GRID_CELLS], u16> {
        let mut g = [false; GRID_CELLS];
        for it in &self.items {
            for s in 0..4u8 {
                if it.src_mask & (1 << s) != 0 {
                    g[it.class.ord() * 4 + s as usize] = true;
                }
            }
        }
        Ok(g)
    }
    /// 完备度（千分比，整数截断）。
    pub fn completeness(&self) -> Result<u32, u16> {
        let g = self.grid()?;
        if self.items.is_empty() {
            return Err(E_EMPTY);
        }
        let cov = g.iter().filter(|&&b| b).count();
        Ok((cov * 1000 / GRID_CELLS) as u32)
    }
    /// 缺口清单：空格（类序,来源序）升序。
    pub fn gaps(&self) -> Result<Vec<(usize, usize)>, u16> {
        let g = self.grid()?;
        let mut out = Vec::new();
        for c in 0..5 {
            for s in 0..4 {
                if !g[c * 4 + s] {
                    out.push((c, s));
                }
            }
        }
        Ok(out)
    }
    /// 按类导出名称表（登记序）。
    pub fn names_of(&self, c: ContentClass) -> Vec<&'static str> {
        self.items
            .iter()
            .filter(|x| x.class == c)
            .map(|x| x.name)
            .collect()
    }
}

/// 总览行（导出格式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewRow {
    /// 类序。
    pub class_ord: u8,
    /// 类名。
    pub class: &'static str,
    /// 登记数。
    pub count: usize,
    /// 容量。
    pub cap: usize,
    /// 占用千分比（count*1000/cap）。
    pub fill_pm: u32,
}

/// 总览表：五行（按类序），并附全局完备度。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overview {
    /// 五行。
    pub rows: [OverviewRow; 5],
    /// 完备度千分比。
    pub complete_pm: u32,
    /// 缺口数。
    pub gap_count: u32,
}

/// 生成总览。
pub fn overview(book: &ContentBook) -> Result<Overview, u16> {
    book.audit()?;
    if book.total() == 0 {
        return Err(E_EMPTY);
    }
    let mut rows = core::array::from_fn(|_| OverviewRow {
        class_ord: 0,
        class: "",
        count: 0,
        cap: 0,
        fill_pm: 0,
    });
    for o in 0..5usize {
        let c = ContentClass::from_ord(o)?;
        let n = book.class_len(c);
        rows[o] = OverviewRow {
            class_ord: o as u8,
            class: c.name(),
            count: n,
            cap: c.cap(),
            fill_pm: (n * 1000 / c.cap()) as u32,
        };
    }
    let complete_pm = book.completeness()?;
    let gap_count = book.gaps()?.len() as u32;
    Ok(Overview {
        rows,
        complete_pm,
        gap_count,
    })
}

/// 自例程：主路径（五类各录 2 条→总览）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut b = ContentBook::new();
    // 错误路径 0：空总览。
    if overview(&b) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 主路径：五类各录 2 条，来源掩码各异。
    let samples = [
        (ContentClass::Scenario, "web_scroll_home"),
        (ContentClass::Scenario, "web_scroll_feed"),
        (ContentClass::Asset, "img_tile_128"),
        (ContentClass::Asset, "font_woff2"),
        (ContentClass::Profile, "news_reader"),
        (ContentClass::Profile, "social_feed"),
        (ContentClass::Variant, "slow_net"),
        (ContentClass::Variant, "low_mem"),
        (ContentClass::Dataset, "e06_v1"),
        (ContentClass::Dataset, "e06_v2"),
    ];
    let masks = [0x01u8, 0x03, 0x05, 0x0F, 0x02];
    let mut no = 1u32;
    for (i, (c, n)) in samples.iter().enumerate() {
        b.put(ContentItem {
            class: *c,
            no,
            name: n,
            src_mask: masks[i % masks.len()],
        })?;
        if i % 2 == 1 {
            no = 1; // 每类两条：类内编号 1,2
        } else {
            no = 2;
        }
    }
    b.audit()?;
    if b.total() != 10 {
        return Err(E_LEDGER);
    }
    if b.class_len(ContentClass::Asset) != 2 {
        return Err(E_LEDGER);
    }
    // 总览：20 格覆盖情况（按掩码并集）——
    // scenario {0x01,0x03} → bits{0,1} = 2 格
    // asset {0x05,0x0F} → bits{0,1,2,3} = 4 格
    // profile {0x02,0x01} → bits{0,1} = 2 格
    // variant {0x03,0x05} → bits{0,1,2} = 3 格
    // dataset {0x0F,0x02} → bits{0,1,2,3} = 4 格
    // 合计覆盖格 = 15 → 完备度 750‰，缺口 5。
    let ov = overview(&b)?;
    if ov.complete_pm != 750 {
        return Err(E_LEDGER);
    }
    if ov.gap_count != 5 {
        return Err(E_LEDGER);
    }
    let gaps = b.gaps()?;
    // 缺口首格应为 (0,2)（scenario 无 imported）。
    if gaps[0] != (0, 2) {
        return Err(E_LEDGER);
    }
    // 类行数值：asset fill = 2*1000/64 = 31。
    if ov.rows[1].fill_pm != 31 {
        return Err(E_LEDGER);
    }
    // 错误路径 1：掩码越界。
    if b.put(ContentItem {
        class: ContentClass::Scenario,
        no: 3,
        name: "bad_mask",
        src_mask: 0x10,
    }) != Err(E_SOURCE)
    {
        return Err(E_SOURCE);
    }
    // 错误路径 2：类内重号。
    if b.put(ContentItem {
        class: ContentClass::Scenario,
        no: 1,
        name: "dup_no",
        src_mask: 0x01,
    }) != Err(E_DUP)
    {
        return Err(E_DUP);
    }
    // 错误路径 3：空名。
    if b.put(ContentItem {
        class: ContentClass::Scenario,
        no: 3,
        name: "",
        src_mask: 0x01,
    }) != Err(E_NAME)
    {
        return Err(E_NAME);
    }
    // 错误路径 4：dataset 定容 8——灌满后再录拒绝。
    let mut b2 = ContentBook::new();
    for i in 1..=8u32 {
        b2.put(ContentItem {
            class: ContentClass::Dataset,
            no: i,
            name: "ds",
            src_mask: 0x01,
        })?;
    }
    if b2.put(ContentItem {
        class: ContentClass::Dataset,
        no: 9,
        name: "ds9",
        src_mask: 0x01,
    }) != Err(E_FULL)
    {
        return Err(E_FULL);
    }
    // 错误路径 5：类序还原越界。
    if ContentClass::from_ord(5) != Err(E_SOURCE) {
        return Err(E_SOURCE);
    }
    Ok(0)
}
