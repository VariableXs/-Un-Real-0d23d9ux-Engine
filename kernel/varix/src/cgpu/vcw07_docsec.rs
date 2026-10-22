//! CGPU-F3528 · 文档安全（CGPU-W 域 · 文档域 · W07 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3528`（L14779）
//!
//! **判据（锚点原文）**：安全：安全（内部标注过滤——安全复用；测试（安全一组）。
//! 判据：过滤、复用、一组、判据。
//!
//! 本模块真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **内部标注过滤**（判据一）：文档段落级安全标注闭集（[`WsMark`]：
//!    Public/Internal/Confidential）——发布面只放行 Public；Internal 及以上
//!    段落过滤并留案（过滤计数零静默）。
//! 2. **安全复用**（判据二）：过滤引擎复用于三发布面（手册/站点/导出包）
//!    ——同一引擎实例三面各跑一遍（复用不复制逻辑）；三面过滤结果一致
//!    断言（同输入同输出）。
//! 3. **敏感词双保险**（判据补充）：标注级过滤之外叠加敏感子串扫描
//!    （密钥样格式/token 样格式）——命中即过滤该段（双保险缺一不可）。
//! 4. **测试一组**（判据三）：过滤正确性自证（干净文档零误滤 / 带密文档
//!    零漏滤）+ 复用一致性三面对拍，全内置零 IO。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// 模块版本。
pub const WS_VERSION: &str = "vcw07_docsec-v1";

/// 诊断段基址（0x760 段 · 本模块独占）。
pub const WS_SEG_BASE: u16 = 0x760;

/// 安全标注（闭集 · 段落级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WsMark {
    /// 公开（发布面放行）。
    Public,
    /// 内部（发布面过滤）。
    Internal,
    /// 机密（发布面过滤）。
    Confidential,
}

impl WsMark {
    /// 标注名。
    pub fn name(self) -> &'static str {
        match self {
            WsMark::Public => "public",
            WsMark::Internal => "internal",
            WsMark::Confidential => "confidential",
        }
    }

    /// 标注解析（文档内嵌标记 `[mark:xxx]`）。
    pub fn parse(tag: &str) -> Option<WsMark> {
        match tag {
            "public" => Some(WsMark::Public),
            "internal" => Some(WsMark::Internal),
            "confidential" => Some(WsMark::Confidential),
            _ => None,
        }
    }
}

/// 文档段落。
#[derive(Clone, Debug)]
pub struct WsSection {
    /// 段落 id。
    pub id: u32,
    /// 标注。
    pub mark: WsMark,
    /// 正文。
    pub text: String,
}

/// 过滤结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WsVerdict {
    /// 放行。
    Publish,
    /// 过滤（标注级）。
    FilteredByMark,
    /// 过滤（敏感词级）。
    FilteredByScan,
}

impl WsVerdict {
    /// 判定名。
    pub fn name(self) -> &'static str {
        match self {
            WsVerdict::Publish => "publish",
            WsVerdict::FilteredByMark => "by-mark",
            WsVerdict::FilteredByScan => "by-scan",
        }
    }
}

/// 敏感子串扫描（双保险）：密钥样（sk- 长十六进制）/ token 样（Bearer+空格）。
pub fn ws_scan_sensitive(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // 密钥样：任意 "sk-" 后接 ≥16 位 hex。
    if let Some(pos) = lower.find("sk-") {
        let rest = &lower[pos + 3..];
        let hexlen = rest
            .as_bytes()
            .iter()
            .take_while(|b| b.is_ascii_hexdigit())
            .count();
        if hexlen >= 16 {
            return true;
        }
    }
    // token 样：Bearer <非空>。
    if lower.contains("bearer ") {
        return true;
    }
    // 内部主机名样：.corp.local。
    if lower.contains(".corp.local") {
        return true;
    }
    false
}

/// 过滤引擎（复用载体 · 三发布面共享同一实例语义）。
pub struct WsFilter {
    /// 发布面标注门（只放行 Public）。
    pub gate: WsMark,
    /// 过滤计数（按判定分类）。
    pub published: u32,
    pub by_mark: u32,
    pub by_scan: u32,
    /// 已处理段总数（守恒口径基）。
    pub processed: u32,
    /// 过滤留案（段落 id + 判定）。
    pub log: Vec<(u32, WsVerdict)>,
}

impl WsFilter {
    /// 新引擎（发布门 = Public）。
    pub fn new() -> Self {
        WsFilter {
            gate: WsMark::Public,
            published: 0,
            by_mark: 0,
            by_scan: 0,
            processed: 0,
            log: Vec::new(),
        }
    }

    /// 单段过滤：标注门 + 敏感扫描双保险。
    pub fn filter(&mut self, s: &WsSection) -> WsVerdict {
        self.processed += 1;
        let v = if s.mark > self.gate {
            self.by_mark += 1;
            WsVerdict::FilteredByMark
        } else if ws_scan_sensitive(&s.text) {
            self.by_scan += 1;
            WsVerdict::FilteredByScan
        } else {
            self.published += 1;
            WsVerdict::Publish
        };
        if v != WsVerdict::Publish {
            self.log.push((s.id, v));
        }
        v
    }

    /// 全文档过滤。
    pub fn filter_doc(&mut self, doc: &[WsSection]) -> Vec<WsVerdict> {
        doc.iter().map(|s| self.filter(s)).collect()
    }

    /// 计数守恒：published + by_mark + by_scan == 已处理段数。
    pub fn conserved(&self) -> bool {
        self.published + self.by_mark + self.by_scan == self.processed
    }
}

/// 安全复用对拍器：三发布面同一文档过滤结果一致。
pub struct WsReuser {
    /// 面名表。
    pub faces: [&'static str; 3],
    /// 各面判定账（三面 × 段数）。
    pub verdicts: Vec<[WsVerdict; 3]>,
    /// 不一致段数（应恒 0）。
    pub diverged: u32,
}

impl WsReuser {
    /// 新对拍器（手册/站点/导出包三面）。
    pub fn new() -> Self {
        WsReuser {
            faces: ["manual", "site", "export-pkg"],
            verdicts: Vec::new(),
            diverged: 0,
        }
    }

    /// 三面复用过滤：同文档各面新引擎跑一遍，逐段对拍。
    pub fn reuse_filter(&mut self, doc: &[WsSection]) {
        let mut all: [Vec<WsVerdict>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for (fi, _) in self.faces.iter().enumerate() {
            let mut f = WsFilter::new();
            all[fi] = f.filter_doc(doc);
        }
        self.verdicts.clear();
        self.diverged = 0;
        for i in 0..doc.len() {
            let trip = [all[0][i], all[1][i], all[2][i]];
            if trip[0] != trip[1] || trip[1] != trip[2] {
                self.diverged += 1;
            }
            self.verdicts.push(trip);
        }
    }

    /// 复用一致：三面判定逐段全等。
    pub fn consistent(&self) -> bool {
        !self.verdicts.is_empty() && self.diverged == 0
    }
}

/// 负向注入场景。
pub enum WsInject {
    /// 机密段漏标（正文含密钥样串）→ 扫描层必抓。
    KeyLeak,
    /// 标注越门（Internal 段）→ 标注层必抓。
    InternalLeak,
    /// 干净段误滤（Public + 无敏感 → 必放行）。
    FalsePositive,
}

/// 注入核。
pub struct WsInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
}

impl WsInjector {
    /// 空注入核。
    pub fn new() -> Self {
        WsInjector { cases: 0, caught: 0 }
    }

    /// 三场注入。
    pub fn inject_all(&mut self, f: &mut WsFilter) {
        // 场一：Public 标注但正文含密钥样 → 扫描层过滤。
        let leak = WsSection {
            id: 9001,
            mark: WsMark::Public,
            text: String::from("apikey=sk-abcdef0123456789abcdef0123456789 end"),
        };
        if f.filter(&leak) == WsVerdict::FilteredByScan {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：Internal 标注干净正文 → 标注层过滤。
        let internal = WsSection {
            id: 9002,
            mark: WsMark::Internal,
            text: String::from("internal roadmap notes"),
        };
        if f.filter(&internal) == WsVerdict::FilteredByMark {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：干净公开段 → 必放行（零误滤）。
        let clean = WsSection {
            id: 9003,
            mark: WsMark::Public,
            text: String::from("public usage example: run dump command"),
        };
        if f.filter(&clean) == WsVerdict::Publish {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 3 && self.caught == self.cases
    }
}

/// 文档安全门面（判据三件：标注过滤/三面复用一致/注入必拦）。
pub struct WsGate {
    /// 引擎。
    pub filter: WsFilter,
    /// 复用对拍。
    pub reuser: WsReuser,
    /// 注入核。
    pub inj: WsInjector,
}

impl WsGate {
    /// 新门面。
    pub fn new() -> Self {
        WsGate {
            filter: WsFilter::new(),
            reuser: WsReuser::new(),
            inj: WsInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.filter.conserved()
            && self.reuser.consistent()
            && self.inj.all_caught();
        let msg = format!(
            "{}: pub={} by_mark={} by_scan={} faces={} diverged={} inj={}/{} seg=0x{:04x}",
            WS_VERSION,
            self.filter.published,
            self.filter.by_mark,
            self.filter.by_scan,
            self.reuser.faces.len(),
            self.reuser.diverged,
            self.inj.caught,
            self.inj.cases,
            WS_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：六段文档三面复用过滤 + 三场注入。
pub fn run_tw07_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vcw07-tw07");
    let doc: Vec<WsSection> = vec![
        WsSection {
            id: 1,
            mark: WsMark::Public,
            text: String::from("public overview of gpu scheduler"),
        },
        WsSection {
            id: 2,
            mark: WsMark::Public,
            text: String::from("public api reference: queue setup"),
        },
        WsSection {
            id: 3,
            mark: WsMark::Internal,
            text: String::from("internal perf tuning numbers"),
        },
        WsSection {
            id: 4,
            mark: WsMark::Confidential,
            text: String::from("confidential vendor pricing"),
        },
        WsSection {
            id: 5,
            mark: WsMark::Public,
            text: String::from("internal build host: build01.corp.local"),
        },
        WsSection {
            id: 6,
            mark: WsMark::Public,
            text: String::from("public faq"),
        },
    ];
    let mut g = WsGate::new();
    // 单引擎全量过滤（守恒口径用）。
    let _ = g.filter.filter_doc(&doc);
    // 三面复用对拍。
    g.reuser.reuse_filter(&doc);
    // 三场注入。
    g.inj.inject_all(&mut g.filter);
    let (ok, msg) = g.judge();
    set.add_dyn("CGPU-F3528-J1", ok, &msg);
    set.add("WS-mark-gate", g.filter.by_mark >= 2, "");
    set.add("WS-scan-double", g.filter.by_scan >= 1, "");
    set.add("WS-reuse-consistent", g.reuser.consistent(), "");
    set.add("WS-inject-3/3", g.inj.all_caught(), "");
    set
}
