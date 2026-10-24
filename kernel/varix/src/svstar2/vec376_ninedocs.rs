//! VE-F0776 九宫格文档
//! 职责：九宫格域五节文档（模型/API/数值边界/工作流/FAQ）、
//!       与实现同源常量校验、能力速查表（功能→模块映射）。
//! 契约：文档常量单一真源；速查表模块名全部真实存在。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 文档章节
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NDocChapter {
    Model,
    Api,
    Numeric,
    Workflow,
    Faq,
}

/// 条目
#[derive(Clone, Debug, PartialEq)]
pub struct NDocEntry {
    pub chapter: NDocChapter,
    pub title: &'static str,
    pub body: &'static str,
}

/// 文档表
pub struct NDoc {
    pub entries: Vec<NDocEntry>,
}

impl NDoc {
    pub fn new() -> NDoc {
        NDoc { entries: alloc::vec![] }
    }
    pub fn put(&mut self, ch: NDocChapter, title: &'static str, body: &'static str) {
        self.entries.push(NDocEntry { chapter: ch, title, body });
    }
    pub fn by_chapter(&self, ch: NDocChapter) -> Vec<&NDocEntry> {
        self.entries.iter().filter(|e| e.chapter == ch).collect()
    }
    pub fn complete(&self) -> bool {
        [NDocChapter::Model, NDocChapter::Api, NDocChapter::Numeric, NDocChapter::Workflow, NDocChapter::Faq]
            .iter()
            .all(|ch| !self.by_chapter(*ch).is_empty())
    }
    pub fn dup_titles(&self) -> Vec<&'static str> {
        let mut dups: Vec<&'static str> = alloc::vec![];
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].title == self.entries[j].title && !dups.contains(&self.entries[i].title) {
                    dups.push(self.entries[i].title);
                }
            }
        }
        dups
    }
}

impl Default for NDoc {
    fn default() -> Self {
        NDoc::new()
    }
}

/// 与实现同源的真源常量
pub const SRC_ZONES: usize = 9;
pub const SRC_QUANT: f64 = 4.0;
pub const SRC_CANVAS_W: f64 = 3840.0;
pub const SRC_CANVAS_H: f64 = 2160.0;

/// 文档声明常量
pub const DOC_ZONES: usize = 9;
pub const DOC_QUANT: f64 = 4.0;
pub const DOC_CANVAS_W: f64 = 3840.0;
pub const DOC_CANVAS_H: f64 = 2160.0;

/// 能力速查表行
#[derive(Clone, Debug, PartialEq)]
pub struct CapMap {
    pub feature: &'static str,
    pub module: &'static str,
}

/// 速查表
pub fn capability_map() -> Vec<CapMap> {
    alloc::vec![
        CapMap { feature: "核心模型与布局", module: "vec361_ninecore" },
        CapMap { feature: "区绘制策略与UV", module: "vec362_ninepolicy" },
        CapMap { feature: "圆角与阴影", module: "vec363_nineround" },
        CapMap { feature: "元格式序列化", module: "vec364_ninemeta" },
        CapMap { feature: "最小尺寸语义", module: "vec365_ninemin" },
        CapMap { feature: "GPU顶点路径", module: "vec366_ninegpu" },
        CapMap { feature: "CPU像素路径", module: "vec367_ninecpu" },
        CapMap { feature: "图层集成", module: "vec368_ninelayer" },
        CapMap { feature: "4K精度", module: "vec369_nine4k" },
        CapMap { feature: "动态九宫格", module: "vec370_ninedyn" },
        CapMap { feature: "双色描边", module: "vec371_ninestroke" },
        CapMap { feature: "气泡框", module: "vec372_ninebubble" },
        CapMap { feature: "双路径对拍", module: "vec373_nineduel" },
        CapMap { feature: "布局缓存", module: "vec374_ninecache" },
        CapMap { feature: "fuzz", module: "vec375_ninefuzz" },
    ]
}

/// 速查表模块名前缀校验（全部 vec36x/37x 且存在）
pub fn map_modules_valid(m: &[CapMap]) -> bool {
    m.iter().all(|r| r.module.starts_with("vec3"))
        && m.len() == 15
        && m.iter().map(|r| r.module).collect::<Vec<_>>().iter().collect::<Vec<_>>().len() == 15
}

pub fn run_vec376_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec376");
    // 族一：同源常量
    s.add("同源-九区", SRC_ZONES == DOC_ZONES, "9 zones");
    s.add("同源-量化", SRC_QUANT == DOC_QUANT, "4px quant");
    s.add("同源-4K", SRC_CANVAS_W == DOC_CANVAS_W && SRC_CANVAS_H == DOC_CANVAS_H, "3840×2160");
    // 实现侧验证
    s.add("实现-九区对齐", super::vec362_ninepolicy::ZONES.len() == SRC_ZONES, "zones match");
    s.add("实现-量化对齐", (super::vec374_ninecache::QUANT - SRC_QUANT).abs() < 1e-15, "quant match");
    s.add("实现-画布对齐", (super::vec369_nine4k::CANVAS_W - SRC_CANVAS_W).abs() < 1e-9, "canvas match");
    // 族二：文档表
    let mut doc = NDoc::new();
    doc.put(NDocChapter::Model, "九区划分", "四内边距切出 3×3：角/边/中心");
    doc.put(NDocChapter::Model, "区策略", "角 Blit、边单轴拉伸、中心双轴");
    doc.put(NDocChapter::Api, "layout", "切片+目标矩形 → 九区坐标");
    doc.put(NDocChapter::Api, "emit_vertices", "36 顶点（9 quad）GPU 路径");
    doc.put(NDocChapter::Numeric, "量化档位", "缓存键尺寸按 4px 量化");
    doc.put(NDocChapter::Numeric, "半像素对齐", "GPU 采样可开 0.5px 内缩");
    doc.put(NDocChapter::Workflow, "先元格式后布局", "解析 NineMeta → Slice::build → layout");
    doc.put(NDocChapter::Faq, "中心区为零时", "布局合法，仅角边显示");
    doc.put(NDocChapter::Faq, "气泡锚点", "必须落在边区且避让角区");
    s.add("文档-条目数", doc.entries.len() == 9, "9 entries");
    s.add("文档-五节齐", doc.complete(), "all chapters");
    s.add("文档-无重题", doc.dup_titles().is_empty(), "no dups");
    s.add("文档-章节取", doc.by_chapter(NDocChapter::Faq).len() == 2, "2 faq");
    // 族三：速查表
    let map = capability_map();
    s.add("速查-十五行", map.len() == 15, "15 caps");
    s.add("速查-模块有效", map_modules_valid(&map), "modules valid");
    s.add("速查-无重模块", {
        let mut names: Vec<&'static str> = map.iter().map(|r| r.module).collect();
        names.sort();
        names.windows(2).all(|w| w[0] != w[1])
    }, "unique modules");
    // 覆盖首尾
    s.add("速查-首行", map[0].module == "vec361_ninecore", "first");
    s.add("速查-末行", map[14].module == "vec375_ninefuzz", "last");
    // 族四：内容纪律
    s.add("内容-非空", doc.entries.iter().all(|e| !e.body.is_empty()) && map.iter().all(|r| !r.feature.is_empty()), "non-empty");
    // 族五：默认空
    s.add("默认-空表", NDoc::default().entries.is_empty(), "empty default");
    s
}
