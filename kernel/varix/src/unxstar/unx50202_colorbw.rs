//! UNX-F50202 色彩页/黑白页分账计数（0x43f80 段）
//!
//! 锚点：UNX-F50202-J1 分账 ×30 与 ColorModel 逐页一致；判据锚定 §AI-63 §2 专题十。
//!
//! 分账的难点不是「彩色/黑白两个计数器」，而是**谁有权认定一页是彩色**。设备说
//! 「按黑白出纸」、文档说「这页有彩色对象」、用户说「我选了彩色模式」——三方
//! 各执一词时，如果计数跟着「最终出纸效果」走，换台设备账就变了；跟着「文档
//! 对象」走，又会在真灰度机上多收钱。这里采用**逐页模型事实**：每一页在排版期
//! 就确定 [`ColorModel`]，分账只认这个逐页事实，之后再叠加「降级记录」解释差异。
//!
//! 三条纪律：
//! 1. [`ColorModel::is_color`] 是唯一分类器：`Mono`/`Gray` 归黑白，`Color`/`Cmyk`
//!    归彩色。任何地方不得再写一套 `if model == ...` 的判断；
//! 2. 降级只许单向：彩色 → 灰度/黑白可以（设备能力不足），反向一律拒绝，
//!    且降级必须发生在 `sealed == false` 之前——封账后再改模型就是篡改；
//! 3. 分账守恒：`color + bw == pages × copies` 逐任务成立，合计亦然。
//!
//! 诊断码闭集：`SEG_BASE + 1 ..= SEG_BASE + 11`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x43f80 独占 0x40）。
pub const SEG_BASE: u32 = 0x43f80;

/// 样本任务数（卡面 30 条）。
pub const JOB_COUNT: usize = 30;

/// 每任务页槽（超过 12 页的文档由调用方分片，模块只守 12 页内的逐页事实）。
pub const PAGE_SLOTS: usize = 12;

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitCode {
    JobCountDrift,
    PageSlotDrift,
    ModelClassDrift,
    ColorSumDrift,
    BwSumDrift,
    DowngradeIllegal,
    UpgradeForbidden,
    SealedMutation,
    DigestDrift,
    CopiesDrift,
    Unknown,
}

impl SplitCode {
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SplitCode::JobCountDrift => 1,
                SplitCode::PageSlotDrift => 2,
                SplitCode::ModelClassDrift => 3,
                SplitCode::ColorSumDrift => 4,
                SplitCode::BwSumDrift => 5,
                SplitCode::DowngradeIllegal => 6,
                SplitCode::UpgradeForbidden => 7,
                SplitCode::SealedMutation => 8,
                SplitCode::DigestDrift => 9,
                SplitCode::CopiesDrift => 10,
                SplitCode::Unknown => 11,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            SplitCode::JobCountDrift => "任务数与冻结样本数不等，分账分母改变",
            SplitCode::PageSlotDrift => "页槽长度与声明页数不符，逐页事实缺格",
            SplitCode::ModelClassDrift => "同一模型的彩色判定前后不一致",
            SplitCode::ColorSumDrift => "彩色面合计与逐页复算不等",
            SplitCode::BwSumDrift => "黑白面合计与逐页复算不等",
            SplitCode::DowngradeIllegal => "非法降级：灰度/黑白不得升级为彩色",
            SplitCode::UpgradeForbidden => "封账后仍试图改变模型",
            SplitCode::SealedMutation => "封账任务被改写，账不可再变",
            SplitCode::DigestDrift => "分账摘要不稳定",
            SplitCode::CopiesDrift => "份数折算未乘入或重复乘入",
            SplitCode::Unknown => "未归类的分账异常",
        }
    }

    pub const ALL: [SplitCode; 11] = [
        SplitCode::JobCountDrift,
        SplitCode::PageSlotDrift,
        SplitCode::ModelClassDrift,
        SplitCode::ColorSumDrift,
        SplitCode::BwSumDrift,
        SplitCode::DowngradeIllegal,
        SplitCode::UpgradeForbidden,
        SplitCode::SealedMutation,
        SplitCode::DigestDrift,
        SplitCode::CopiesDrift,
        SplitCode::Unknown,
    ];
}

/// 逐页色模型。顺序即设备能力由低到高。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorModel {
    Mono,
    Gray,
    Color,
    Cmyk,
}

impl ColorModel {
    pub fn val(self) -> u8 {
        match self {
            ColorModel::Mono => 0,
            ColorModel::Gray => 1,
            ColorModel::Color => 2,
            ColorModel::Cmyk => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ColorModel::Mono => "mono",
            ColorModel::Gray => "gray",
            ColorModel::Color => "color",
            ColorModel::Cmyk => "cmyk",
        }
    }

    /// 唯一分类器：黑白两档不算彩色。
    pub fn is_color(self) -> bool {
        matches!(self, ColorModel::Color | ColorModel::Cmyk)
    }

    /// 能力等级：用于判定降级方向。
    pub fn tier(self) -> u8 {
        match self {
            ColorModel::Mono => 0,
            ColorModel::Gray => 1,
            ColorModel::Color => 2,
            ColorModel::Cmyk => 3,
        }
    }

    /// 从裸值还原；未知值一律 `None`，不折叠成 `Mono`——折叠会让坏数据悄悄变黑白。
    pub fn from_val(v: u8) -> Option<ColorModel> {
        match v {
            0 => Some(ColorModel::Mono),
            1 => Some(ColorModel::Gray),
            2 => Some(ColorModel::Color),
            3 => Some(ColorModel::Cmyk),
            _ => None,
        }
    }
}

/// 一个任务的逐页模型事实。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorJob {
    pub id: u16,
    pub pages: u8,
    pub copies: u8,
    pub models: [ColorModel; PAGE_SLOTS],
    /// 封账标记：一旦置位，模型与份数都不许再动。
    pub sealed: bool,
}

impl ColorJob {
    pub const ZERO: ColorJob = ColorJob {
        id: 0,
        pages: 0,
        copies: 0,
        models: [ColorModel::Mono; PAGE_SLOTS],
        sealed: false,
    };

    /// 页槽自洽：声明页数在容量内，超出部分必须是 `Mono` 占位。
    pub fn slots_ok(&self) -> bool {
        if self.pages == 0 || self.pages as usize > PAGE_SLOTS || self.copies == 0 {
            return false;
        }
        let mut i = self.pages as usize;
        while i < PAGE_SLOTS {
            match self.models.get(i) {
                Some(m) => {
                    if *m != ColorModel::Mono {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 逐页统计彩色页数（不含份数）。
    pub fn color_pages(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        let limit = if (self.pages as usize) < PAGE_SLOTS {
            self.pages as usize
        } else {
            PAGE_SLOTS
        };
        while i < limit {
            if let Some(m) = self.models.get(i) {
                if m.is_color() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn bw_pages(&self) -> u32 {
        let total = self.pages as u32;
        total.saturating_sub(self.color_pages())
    }

    /// 分账面数（乘份数）。
    pub fn color_faces(&self) -> u32 {
        self.color_pages().saturating_mul(self.copies as u32)
    }

    pub fn bw_faces(&self) -> u32 {
        self.bw_pages().saturating_mul(self.copies as u32)
    }

    pub fn total_faces(&self) -> u32 {
        (self.pages as u32).saturating_mul(self.copies as u32)
    }

    /// 分账守恒：彩色 + 黑白 == 总面数。
    pub fn balanced(&self) -> bool {
        self.color_faces().saturating_add(self.bw_faces()) == self.total_faces()
    }

    /// 降级：只许 `tier` 变小，且不许在封账后发生。
    ///
    /// 返回 `true` 表示确实改写了模型；被规则拒绝时返回 `false` 且原值不变。
    pub fn downgrade(&mut self, page: usize, to: ColorModel) -> bool {
        if self.sealed {
            return false;
        }
        let cur = match self.models.get(page) {
            Some(m) => *m,
            None => return false,
        };
        if to.tier() >= cur.tier() {
            return false;
        }
        if let Some(slot) = self.models.get_mut(page) {
            *slot = to;
            true
        } else {
            false
        }
    }

    /// 封账：此后任何模型/份数改动都必须失败。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 份数改写：封账后禁止。
    pub fn set_copies(&mut self, c: u8) -> bool {
        if self.sealed || c == 0 {
            return false;
        }
        self.copies = c;
        true
    }
}

/// 逐页模型序列：只由 (任务下标, 页下标) 决定，双跑同值。
fn model_at(job: usize, page: usize) -> ColorModel {
    let order = [
        ColorModel::Mono,
        ColorModel::Gray,
        ColorModel::Color,
        ColorModel::Mono,
        ColorModel::Cmyk,
        ColorModel::Gray,
        ColorModel::Color,
        ColorModel::Mono,
        ColorModel::Gray,
        ColorModel::Cmyk,
        ColorModel::Mono,
        ColorModel::Color,
    ];
    match order.get((job * 5 + page * 3) % order.len()) {
        Some(m) => *m,
        None => ColorModel::Mono,
    }
}

/// 30 条样本（页面数 3..=12，份数 1..=4）。
pub fn sample_jobs() -> [ColorJob; JOB_COUNT] {
    let mut out = [ColorJob::ZERO; JOB_COUNT];
    let mut i = 0usize;
    while i < JOB_COUNT {
        let pages = 3 + (i % 10);
        let mut models = [ColorModel::Mono; PAGE_SLOTS];
        let mut p = 0usize;
        while p < pages {
            models[p] = model_at(i, p);
            p += 1;
        }
        out[i] = ColorJob {
            id: i as u16,
            pages: pages as u8,
            copies: (1 + (i % 4)) as u8,
            models,
            sealed: i % 7 == 0,
        };
        i += 1;
    }
    out
}

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 分账账本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitLedger {
    pub color: u64,
    pub bw: u64,
    pub pages: u64,
    pub jobs: u32,
    digest: u64,
}

impl SplitLedger {
    pub fn new() -> SplitLedger {
        SplitLedger {
            color: 0,
            bw: 0,
            pages: 0,
            jobs: 0,
            digest: 0xcbf2_9ce4_8422_2325,
        }
    }

    pub fn absorb(&mut self, j: &ColorJob) -> bool {
        if !j.slots_ok() || !j.balanced() {
            return false;
        }
        self.color = self.color.saturating_add(j.color_faces() as u64);
        self.bw = self.bw.saturating_add(j.bw_faces() as u64);
        self.pages = self.pages.saturating_add(j.total_faces() as u64);
        self.jobs = self.jobs.saturating_add(1);
        let mut h = self.digest;
        h = fnv_mix(h, j.id as u64);
        h = fnv_mix(h, j.color_faces() as u64);
        h = fnv_mix(h, j.bw_faces() as u64);
        h = fnv_mix(h, j.copies as u64);
        self.digest = h;
        true
    }

    pub fn closed(&self) -> bool {
        self.color.saturating_add(self.bw) == self.pages
    }

    pub fn digest(&self) -> u64 {
        let mut h = self.digest;
        h = fnv_mix(h, self.color);
        h = fnv_mix(h, self.bw);
        h = fnv_mix(h, self.pages);
        h
    }

    pub fn rebuild(jobs: &[ColorJob]) -> SplitLedger {
        let mut l = SplitLedger::new();
        let mut i = 0usize;
        while i < jobs.len() {
            if let Some(j) = jobs.get(i) {
                let _ = l.absorb(j);
            }
            i += 1;
        }
        l
    }
}

impl Default for SplitLedger {
    fn default() -> Self {
        SplitLedger::new()
    }
}

/// 逐页复算：不信任 `color_pages()`，从原始模型数组重新数一遍。
pub fn recount_color(j: &ColorJob) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < PAGE_SLOTS && i < j.pages as usize {
        if let Some(m) = j.models.get(i) {
            match *m {
                ColorModel::Color | ColorModel::Cmyk => n += 1,
                ColorModel::Mono | ColorModel::Gray => {}
            }
        }
        i += 1;
    }
    n
}

/// 模型分类矩阵自检：四种模型的判定必须与 `is_color` 一致。
pub fn model_matrix_ok() -> bool {
    let all = [
        ColorModel::Mono,
        ColorModel::Gray,
        ColorModel::Color,
        ColorModel::Cmyk,
    ];
    let mut i = 0usize;
    while i < all.len() {
        if let Some(m) = all.get(i) {
            let expect = matches!(m, ColorModel::Color | ColorModel::Cmyk);
            if m.is_color() != expect {
                return false;
            }
            if ColorModel::from_val(m.val()) != Some(*m) {
                return false;
            }
        }
        i += 1;
    }
    ColorModel::from_val(4).is_none() && ColorModel::from_val(255).is_none()
}

/// 降级路径审计：彩色任务在未封账时可以降到灰度，封账后必须拒绝。
pub fn downgrade_audit() -> bool {
    let mut j = ColorJob {
        id: 1,
        pages: 3,
        copies: 1,
        models: [ColorModel::Color, ColorModel::Cmyk, ColorModel::Mono, ColorModel::Mono,
                 ColorModel::Mono, ColorModel::Mono, ColorModel::Mono, ColorModel::Mono,
                 ColorModel::Mono, ColorModel::Mono, ColorModel::Mono, ColorModel::Mono],
        sealed: false,
    };
    let before = j.color_faces();
    if !j.downgrade(0, ColorModel::Gray) {
        return false;
    }
    if j.color_faces() != before - 1 {
        return false;
    }
    // 升级必须被拒。
    if j.downgrade(0, ColorModel::Color) {
        return false;
    }
    // 同档不迁移。
    if j.downgrade(1, ColorModel::Cmyk) {
        return false;
    }
    // 封账后禁止。
    j.seal();
    if j.downgrade(1, ColorModel::Mono) || j.set_copies(9) {
        return false;
    }
    true
}

/// 判据。
pub fn run_unx50202_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50202");
    let jobs = sample_jobs();

    // J1-1 诊断码闭集
    let mut code_ok = SplitCode::ALL.len() == 11;
    let mut i = 0usize;
    while i < SplitCode::ALL.len() {
        if let Some(c) = SplitCode::ALL.get(i) {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                code_ok = false;
            }
        }
        i += 1;
    }
    s.add("50202-J1-诊断码闭集", code_ok, "11 条码连续且文案非空");

    // J1-2 样本基数
    let mut count_ok = jobs.len() == JOB_COUNT;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = jobs.get(i) {
            if j.id as usize != i || !j.slots_ok() {
                count_ok = false;
            }
        } else {
            count_ok = false;
        }
        i += 1;
    }
    s.add("50202-J1-样本基数", count_ok, "30 条样本页槽自洽");

    // J1-3 逐页一致：color_pages 与逐页复算相等
    let mut page_ok = true;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = jobs.get(i) {
            if j.color_pages() != recount_color(j) {
                page_ok = false;
            }
            if j.color_pages().saturating_add(j.bw_pages()) != j.pages as u32 {
                page_ok = false;
            }
        }
        i += 1;
    }
    s.add("50202-J1-逐页一致", page_ok, "逐页模型复算与声明页数一致");

    // J1-4 分账守恒（逐任务）
    let mut bal_ok = true;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = jobs.get(i) {
            if !j.balanced() {
                bal_ok = false;
            }
        }
        i += 1;
    }
    s.add("50202-J1-分账守恒", bal_ok, "彩色面 + 黑白面 == 页数 × 份数");

    // J1-5 份数折算：单份与多份都要乘对
    let mut copies_ok = true;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = jobs.get(i) {
            let want = (j.pages as u32).saturating_mul(j.copies as u32);
            if j.total_faces() != want {
                copies_ok = false;
            }
            if j.color_faces() != j.color_pages() * j.copies as u32 {
                copies_ok = false;
            }
        }
        i += 1;
    }
    s.add("50202-J1-份数折算", copies_ok, "分账面数按份数等比放大");

    // J1-6 合计闭合
    let ledger = SplitLedger::rebuild(&jobs);
    s.add(
        "50202-J1-合计闭合",
        ledger.closed() && ledger.jobs == JOB_COUNT as u32,
        "合计彩色 + 黑白 == 合计面数",
    );

    // J1-7 模型分类矩阵
    s.add("50202-J1-模型矩阵", model_matrix_ok(), "四模型分类与裸值还原自洽");

    // J1-8 降级单向
    s.add(
        "50202-J1-降级单向",
        downgrade_audit(),
        "彩色可降灰、升级/同档/封账后一律拒绝",
    );

    // J1-9 摘要稳定
    let d1 = ledger.digest();
    let d2 = SplitLedger::rebuild(&sample_jobs()).digest();
    s.add("50202-J1-摘要稳定", d1 == d2 && d1 != 0, "双跑同摘要");

    // J1-10 越界模型拒绝：坏数据不得折叠成黑白
    s.add(
        "50202-J1-越界模型拒绝",
        ColorModel::from_val(4).is_none() && ColorModel::from_val(0x80).is_none(),
        "未知模型值返回 None",
    );

    // J1-11 零值/超页防护
    let zero_pages = ColorJob {
        pages: 0,
        copies: 1,
        ..ColorJob::ZERO
    };
    let over_pages = ColorJob {
        pages: (PAGE_SLOTS + 1) as u8,
        copies: 1,
        ..ColorJob::ZERO
    };
    let mut l2 = SplitLedger::new();
    s.add(
        "50202-J1-零值防护",
        !l2.absorb(&zero_pages) && !l2.absorb(&over_pages) && l2.jobs == 0,
        "零页与超页任务均被拒",
    );

    // J1-12 封账语义：封账任务份数不可改
    let mut sealed = ColorJob {
        id: 7,
        pages: 2,
        copies: 2,
        ..ColorJob::ZERO
    };
    sealed.seal();
    s.add(
        "50202-J1-封账语义",
        !sealed.set_copies(3) && sealed.copies == 2,
        "封账后份数保持原值",
    );

    // 动态明细
    s.add_dyn(
        "50202-J1-分账明细",
        ledger.closed(),
        &format!(
            "彩色={} 黑白={} 总面={} 任务={}",
            ledger.color, ledger.bw, ledger.pages, ledger.jobs
        ),
    );
    s.add_dyn(
        "50202-J1-模型明细",
        model_matrix_ok(),
        &format!("模型四档 tier=0..3，越界值一律 None"),
    );
    s.add_dyn(
        "50202-J1-段位明细",
        SEG_BASE % 0x40 == 0,
        &format!("段基址={:#x} 样本={}", SEG_BASE, JOB_COUNT),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50202_checks_green() {
        let set = run_unx50202_checks();
        assert!(set.all_passed(), "UNX-F50202 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50202_split_closed() {
        let jobs = sample_jobs();
        let l = SplitLedger::rebuild(&jobs);
        assert_eq!(l.jobs, JOB_COUNT as u32);
        assert_eq!(l.color + l.bw, l.pages);
        assert!(l.color > 0 && l.bw > 0);
    }

    #[test]
    fn unx50202_downgrade_one_way() {
        assert!(downgrade_audit());
        assert!(!ColorModel::Gray.is_color());
        assert!(ColorModel::Cmyk.is_color());
    }

    #[test]
    fn unx50202_recount_matches() {
        for j in sample_jobs().iter() {
            assert_eq!(j.color_pages(), recount_color(j));
            assert_eq!(j.color_pages() + j.bw_pages(), j.pages as u32);
        }
    }
}
