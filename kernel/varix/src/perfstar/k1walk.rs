//! K1 十七域 · 通用十二查登记册（AI-K1 深化批次三 · 分工书通用验收标准）。
//!
//! 分工书《Varix STAR I start · AI分工完成图》通用验收标准十二查里，有六查是
//! **每一项都要登记、都要可查**的元数据，此前散落在各域注释里、机器不可检：
//!
//! | 查 | 原文摘 | 本件落点 |
//! | --- | --- | --- |
//! | 4 | 「视觉项**四档 DPI**（1:1/125%/150%/200%）截图全绿」 | [`DPI_STEPS`] + [`WalkEntry::dpi_checked`] |
//! | 5 | 「有**参数清单** + 排布舒适 + **双入口可达**（分类页 + F301 搜索）」 | [`WalkEntry::knobs`] / [`WalkEntry::dual_entry`] |
//! | 6 | 「**三落位登记**：A 设置直调 / B 设置跳转 / C 免调节（**含理由**）」 | [`Locus`] + [`WalkEntry::locus_reason`] |
//! | 7 | 「**导航路径链** ≤4 段路径链登记且逐步走达」 | [`WalkEntry::path`]（定长 4 段 + 实际段数） |
//! | 8 | 「**说明句**：名称 + 一句话说明 + 调节控件三件套齐全」 | [`WalkEntry::name`]/[`blurb`]/[`control`] + [`WalkEntry::render_blurb`] |
//! | 3 | 「判据中的**帧率/延迟/内存数值**全达标」 | [`WalkEntry::perf_line_ms`]（性能线数值登记） |
//!
//! 一处一事实：十七域的登记集中在本件（对账单一源），各域 `_ext` 不再重复
//! 抄一份；本件同时提供校验器，检查项由 F041（B 域共同前提域）合并注册。

use crate::checks::CheckSet;

/// 十七域（与 perfstar::DOMAIN_NAMES 同序，一处一事实由检查项守护）。
pub const WALK_DOMAINS: usize = 17;
/// 4K 走查四档（分工书第 4 查）。
pub const DPI_STEPS: [u32; 4] = [100, 125, 150, 200];
/// 路径链段数上限（分工书第 7 查「≤4 段」）。
pub const PATH_MAX: usize = 4;

/// 三落位（分工书第 6 查）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locus {
    /// A：设置中心直接调节。
    A,
    /// B：设置中心跳转到归属页调节。
    B,
    /// C：免调节（自动策略，不暴露旋钮）。
    C,
}

impl Locus {
    pub const fn tag(self) -> &'static str {
        match self {
            Locus::A => "A",
            Locus::B => "B",
            Locus::C => "C",
        }
    }
    /// 是否需要在界面上提供调节入口（C 不需要）。
    pub const fn needs_ui(self) -> bool {
        matches!(self, Locus::A | Locus::B)
    }
}

/// 调节控件（分工书第 8 查「名称 + 一句话说明 + 调节控件」三件套之三）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// 滑杆（连续量）。
    Slider,
    /// 开关（布尔）。
    Toggle,
    /// 下拉（枚举档）。
    Select,
    /// 只读展示（自动策略，防乱调）。
    ReadOnly,
    /// 按钮（动作型，非持久化参数）。
    Button,
}

impl Control {
    pub const fn name(self) -> &'static str {
        match self {
            Control::Slider => "滑杆",
            Control::Toggle => "开关",
            Control::Select => "下拉",
            Control::ReadOnly => "只读展示",
            Control::Button => "按钮",
        }
    }
    /// 是否可持久化（按钮是一次动作，没有持久值）。
    pub const fn persistable(self) -> bool {
        !matches!(self, Control::Button)
    }
}

/// 一条十二查登记条目。
#[derive(Clone, Copy, Debug)]
pub struct WalkEntry {
    /// 域标签（如 `"F041"`）。
    pub domain: &'static str,
    /// 名称（用户看到的那个名字）。
    pub name: &'static str,
    /// 一句话说明（第 8 查第二件）。
    pub blurb: &'static str,
    /// 调节控件（第 8 查第三件）。
    pub control: Control,
    /// 三落位（第 6 查）。
    pub locus: Locus,
    /// 落位理由（第 6 查明确要求「含理由」）。
    pub locus_reason: &'static str,
    /// 导航路径链（第 7 查，≤4 段）。
    pub path: [&'static str; PATH_MAX],
    /// 路径实际段数。
    pub path_len: u8,
    /// 可调参数条数（第 5 查，0 = 无参数）。
    pub knobs: u8,
    /// 性能线数值（毫秒；0 = 该域无毫秒级硬线）。
    pub perf_line_ms: u32,
    /// 4K 四档走查是否做过（第 4 查）。
    pub dpi_checked: bool,
}

impl WalkEntry {
    /// 双入口可达（第 5 查）：分类页路径存在 + 可被 F301 搜到（名称非空即可搜）。
    pub fn dual_entry(&self) -> bool {
        self.path_len >= 1 && !self.name.is_empty()
    }
    /// 路径链合法：1..=4 段，且前 `path_len` 段非空、后续段必须为空（不留残值）。
    pub fn path_ok(&self) -> bool {
        let n = self.path_len as usize;
        if n == 0 || n > PATH_MAX {
            return false;
        }
        for i in 0..n {
            if self.path[i].is_empty() {
                return false;
            }
        }
        for i in n..PATH_MAX {
            if !self.path[i].is_empty() {
                return false;
            }
        }
        true
    }
    /// 登记完整性（第 6/8 查）：名称、说明、理由三者齐，落位与控件自洽。
    pub fn complete(&self) -> bool {
        !self.name.is_empty()
            && !self.blurb.is_empty()
            && !self.locus_reason.is_empty()
            && self.path_ok()
            && self.dual_entry()
            // 免调节（C）不该配可持久控件——配了就是「说不调其实能调」的假话
            && (self.locus.needs_ui() || !self.control.persistable() || self.control == Control::ReadOnly)
    }
    /// 渲染说明句（第 8 查三件套的呈现形态：名称 + 一句话说明 + 控件）。
    pub fn render_blurb(&self, out: &mut [u8]) -> usize {
        let parts: [&[u8]; 5] = [
            self.name.as_bytes(),
            "：".as_bytes(),
            self.blurb.as_bytes(),
            "（".as_bytes(),
            self.control.name().as_bytes(),
        ];
        let mut p = 0usize;
        for part in parts {
            let n = part.len().min(out.len() - p);
            out[p..p + n].copy_from_slice(&part[..n]);
            p += n;
            if p >= out.len() {
                break;
            }
        }
        if p < out.len() {
            out[p] = b')';
            p += 1;
        }
        p
    }
    /// 路径链渲染（第 7 查「逐步走达」的呈现形态：`A > B > C`）。
    pub fn render_path(&self, out: &mut [u8]) -> usize {
        let mut p = 0usize;
        for i in 0..self.path_len as usize {
            if i > 0 {
                let n = " > ".as_bytes().len().min(out.len() - p);
                out[p..p + n].copy_from_slice(&" > ".as_bytes()[..n]);
                p += n;
            }
            let seg = self.path[i].as_bytes();
            let n = seg.len().min(out.len() - p);
            out[p..p + n].copy_from_slice(&seg[..n]);
            p += n;
            if p >= out.len() {
                break;
            }
        }
        p
    }
}

/// 十七域登记册（定长，对账单一源）。
pub struct WalkBook {
    entries: [Option<WalkEntry>; WALK_DOMAINS],
    n: usize,
}

impl WalkBook {
    pub const fn new() -> Self {
        WalkBook { entries: [None; WALK_DOMAINS], n: 0 }
    }
    pub fn register(&mut self, e: WalkEntry) -> bool {
        if self.n >= WALK_DOMAINS {
            return false;
        }
        self.entries[self.n] = Some(e);
        self.n += 1;
        true
    }
    /// 按域标签查。
    pub fn get(&self, domain: &str) -> Option<WalkEntry> {
        for i in 0..self.n {
            if let Some(e) = self.entries[i] {
                if e.domain == domain {
                    return Some(e);
                }
            }
        }
        None
    }
    /// 全部登记项（对账导出）。
    pub fn audit(&self, out: &mut [WalkEntry]) -> usize {
        let n = self.n.min(out.len());
        for i in 0..n {
            if let Some(e) = self.entries[i] {
                out[i] = e;
            }
        }
        n
    }
    /// 全部登记项是否都完整（第 6/8 查）。
    pub fn all_complete(&self) -> bool {
        self.n == WALK_DOMAINS && (0..self.n).all(|i| self.entries[i].map(|e| e.complete()).unwrap_or(false))
    }
    /// 落位分布（A/B/C 各多少项——全是 C 说明没人敢给用户入口，是设计信号）。
    pub fn locus_counts(&self) -> [usize; 3] {
        let mut c = [0usize; 3];
        for i in 0..self.n {
            if let Some(e) = self.entries[i] {
                match e.locus {
                    Locus::A => c[0] += 1,
                    Locus::B => c[1] += 1,
                    Locus::C => c[2] += 1,
                }
            }
        }
        c
    }
    /// 域标签是否唯一且齐全（与 perfstar::DOMAIN_NAMES 前缀对齐）。
    pub fn domains_unique(&self) -> bool {
        let mut seen = [false; WALK_DOMAINS];
        for i in 0..self.n {
            let e = match self.entries[i] {
                Some(e) => e,
                None => return false,
            };
            for j in 0..i {
                if let Some(o) = self.entries[j] {
                    if o.domain == e.domain {
                        return false;
                    }
                }
            }
            seen[i] = true;
        }
        seen.iter().all(|&s| s)
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

/// 4K 走查四档是否齐全（第 4 查）。
pub fn dpi_steps_complete() -> bool {
    DPI_STEPS.len() == 4 && DPI_STEPS == [100, 125, 150, 200]
}

/// **十七域登记全集**（一处一事实：对账表与本文档同源，改这里即改对账）。
pub fn k1_book() -> WalkBook {
    let mut b = WalkBook::new();
    b.register(WalkEntry {
        domain: "F041",
        name: "帧率账本",
        blurb: "逐帧记录合成耗时，卡顿可以回放着看",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "自动记账，暴露旋钮只会让账本失真（用户能改的不是帧率，是自己的用法）",
        path: ["设置中心", "系统", "性能与诊断", "帧率账本"],
        path_len: 4,
        knobs: 2,
        perf_line_ms: 12, // 80fps 目标线 12.5ms 向下取整登记
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F042",
        name: "掉帧归因",
        blurb: "自动找出偷走这一帧的是哪类原因",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "归因是诊断结论不是设置项，用户要的是答案不是旋钮",
        path: ["设置中心", "系统", "性能与诊断", "掉帧归因"],
        path_len: 4,
        knobs: 0,
        perf_line_ms: 0,
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F043",
        name: "启动画像",
        blurb: "把应用启动拆成五段，慢在哪一眼可见",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "画像随启动自动采集，受隐私总闸控制（关闸在 F036 同开关）",
        path: ["星卡详情", "启动画像", "", ""],
        path_len: 2,
        knobs: 0,
        perf_line_ms: 0,
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F044",
        name: "预取指纹",
        blurb: "只预读真的会被摸到的那些页",
        control: Control::Toggle,
        locus: Locus::B,
        locus_reason: "策略细节属存储栈旋钮，普通用户只需要一个总开关",
        path: ["设置中心", "系统", "存储", "预取指纹"],
        path_len: 4,
        knobs: 1,
        perf_line_ms: 0,
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F045",
        name: "页缓存水位",
        blurb: "后台悄悄回收最早的图页，系统不卡也不崩",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "主册原文：自动策略不暴露旋钮给普通用户——防乱调",
        path: ["设置中心", "系统", "内存", "页缓存水位"],
        path_len: 4,
        knobs: 7,
        perf_line_ms: 500, // 水位判定 500ms 一次
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F046",
        name: "写合并窗口",
        blurb: "写入合并后再落盘，U 盘安静寿命长",
        control: Control::Select,
        locus: Locus::B,
        locus_reason: "档位可手工覆盖（空闲/正常/重载），但默认自适应",
        path: ["设置中心", "系统", "存储", "写合并窗口"],
        path_len: 4,
        knobs: 8,
        perf_line_ms: 0,
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F047",
        name: "延迟预算",
        blurb: "给每类交互分配延迟预算，超支能追责到类",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "预算表是编译期常量（变更走 ADR），不是用户可调项",
        path: ["设置中心", "系统", "性能与诊断", "延迟预算"],
        path_len: 4,
        knobs: 5,
        perf_line_ms: 2, // 2000μs 总预算
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F048",
        name: "CPU 频率策略",
        blurb: "该快的时候快，该静的时候静",
        control: Control::Select,
        locus: Locus::A,
        locus_reason: "性能三档与快速设置面板（F076）叠加，用户可直接设边界",
        path: ["快速设置", "性能模式", "", ""],
        path_len: 2,
        knobs: 7,
        perf_line_ms: 50, // 点按到满频 <50ms
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F049",
        name: "空转清零",
        blurb: "没有动画时合成器真的睡了",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "零唤醒是设计目标不是偏好项，没有「少睡一点」的选项",
        path: ["设置中心", "系统", "性能与诊断", "空转"],
        path_len: 4,
        knobs: 0,
        perf_line_ms: 0,
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F050",
        name: "中断合并",
        blurb: "把洪水般的事件合并成平滑的位置更新",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "合并预算（2000ns）是硬件量级常量，用户无需也无意义调节",
        path: ["设置中心", "系统", "性能与诊断", "中断合并"],
        path_len: 4,
        knobs: 2,
        perf_line_ms: 8, // 合并延迟 >8ms 即收缩窗口
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F051",
        name: "大页策略",
        blurb: "让地址翻译的路牌少掉 512 倍",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "预留与降级全自动（物理连续内存不足才降级），无用户决策点",
        path: ["设置中心", "系统", "内存", "大页"],
        path_len: 4,
        knobs: 0,
        perf_line_ms: 0,
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F052",
        name: "内核堆健康",
        blurb: "长期运行的内核堆依然整洁，不出现有空房开不出的窘境",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "档位边界由分配谱数据决定（数据先行），不是用户拍的数",
        path: ["设置中心", "系统", "内核堆健康", ""],
        path_len: 3,
        knobs: 0,
        perf_line_ms: 0, // P99 <1μs，非毫秒级
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F053",
        name: "启动时间线",
        blurb: "四链并行推进，快是设计出来的不是许愿出来的",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "并行度由依赖矩阵决定，改它需要改依赖而不是改设置",
        path: ["诊断中心", "启动时间线", "", ""],
        path_len: 2,
        knobs: 0,
        perf_line_ms: 8_000, // 8 秒开机线
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F054",
        name: "图像解码",
        blurb: "4K 图即换即见，不等也不卡",
        control: Control::Select,
        locus: Locus::B,
        locus_reason: "指令集档（SSE4.2/AVX2）可查看；强制降档仅供排障",
        path: ["设置中心", "系统", "图形", "解码指令集"],
        path_len: 4,
        knobs: 1,
        perf_line_ms: 150, // 4K PNG 解码线
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F055",
        name: "字形图集",
        blurb: "滚多久都不越滚越卡，热字早就在图集里",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "预热与扩页全自动（命中率驱动），无用户可调项",
        path: ["设置中心", "系统", "图形", "字形图集"],
        path_len: 4,
        knobs: 0,
        perf_line_ms: 0,
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F056",
        name: "脏区合成",
        blurb: "只重画真正变了的那一小块",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "脏区是合成器内部事实，用户能观察（脏区叠显）但不能调",
        path: ["设置中心", "系统", "图形", "脏区"],
        path_len: 4,
        knobs: 0,
        perf_line_ms: 0,
        dpi_checked: true,
    });
    b.register(WalkEntry {
        domain: "F057",
        name: "IO 分级",
        blurb: "下载不拖慢前台，前台也不饿死后台",
        control: Control::Toggle,
        locus: Locus::A,
        locus_reason: "分级总闸直调（关掉=全部同级，排障用）；细分策略免调",
        path: ["设置中心", "系统", "存储", "IO 分级"],
        path_len: 4,
        knobs: 1,
        perf_line_ms: 50, // 前台截止期 50ms
        dpi_checked: true,
    });
    b
}

// ---------------------------------------------------------------------------
// 域自检（挂 F041：B 域共同前提域，十二查为全域通用纪律）
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("K1-walk-12checks");
    let book = k1_book();
    // 1) 十七域登记齐全且域标签唯一（与 perfstar::DOMAIN_NAMES 前缀同构）。
    cs.add("walk_17_domains_unique", book.len() == WALK_DOMAINS && book.domains_unique(), "");
    // 2) 全部登记项完整（名称 + 说明 + 理由 + 路径链 + 双入口 + 落位控件自洽）。
    cs.add("walk_all_complete", book.all_complete(), "");
    // 3) 路径链 ≤4 段且逐步走达（第 7 查）。
    let mut path_ok = true;
    let mut out = [WalkEntry {
        domain: "",
        name: "",
        blurb: "",
        control: Control::ReadOnly,
        locus: Locus::C,
        locus_reason: "",
        path: [""; PATH_MAX],
        path_len: 0,
        knobs: 0,
        perf_line_ms: 0,
        dpi_checked: false,
    }; WALK_DOMAINS];
    let n = book.audit(&mut out);
    for i in 0..n {
        path_ok &= out[i].path_ok() && out[i].dual_entry();
    }
    cs.add("walk_paths_within_4", n == WALK_DOMAINS && path_ok, "");
    // 4) 三落位分布：A/B/C 都有（全 C = 用户永远没入口，是设计缺陷信号）。
    let c = book.locus_counts();
    cs.add("walk_locus_distribution", c[0] >= 1 && c[1] >= 1 && c[2] >= 1 && c[0] + c[1] + c[2] == WALK_DOMAINS, "");
    // 5) 4K 走查四档齐全（第 4 查）。
    cs.add("walk_dpi_four_steps", dpi_steps_complete() && DPI_STEPS[0] == 100 && DPI_STEPS[3] == 200, "");
    // 6) 4K 走查标记：十七域全部登记为已走查（未走查的域不得冒充）。
    let mut checked = true;
    for i in 0..n {
        checked &= out[i].dpi_checked;
    }
    cs.add("walk_dpi_checked_all", checked, "");
    // 7) 说明句三件套渲染（名称 + 一句话说明 + 控件）可产出非空串。
    let e = book.get("F041").unwrap();
    let mut buf = [0u8; 96];
    let len = e.render_blurb(&mut buf);
    // 「（」U+FF08 的 UTF-8 是 EF BC 88；字节字面量不能写非 ASCII，用转义序列
    let open_paren: &[u8] = b"\xEF\xBC\x88";
    cs.add(
        "walk_blurb_rendered",
        len > 0 && buf[..len].windows(3).any(|w| w == open_paren) && buf[..len].ends_with(b")"),
        "",
    );
    // 8) 路径链渲染（`A > B > C` 形态，逐步走达可展示）。
    let plen = e.render_path(&mut buf);
    cs.add("walk_path_rendered", plen > 0 && buf[..plen].windows(3).any(|w| w == b" > "), "");
    // 9) 落位与控件自洽：C 类（免调节）不得配可持久控件（ReadOnly 除外）。
    let mut coherent = true;
    for i in 0..n {
        let en = &out[i];
        if !en.locus.needs_ui() && en.control.persistable() && en.control != Control::ReadOnly {
            coherent = false;
        }
    }
    cs.add("walk_locus_control_coherent", coherent, "");
    // 10) 性能线登记：有硬线的域必须给出数值（第 3 查）。
    let mut perf_ok = true;
    for tag in ["F041", "F045", "F047", "F048", "F050", "F053", "F054", "F057"] {
        match book.get(tag) {
            Some(en) => perf_ok &= en.perf_line_ms > 0,
            None => perf_ok = false,
        }
    }
    cs.add("walk_perf_lines_registered", perf_ok, "");
    // 11) 控件可持久化语义（按钮无持久值）。
    cs.add("walk_control_persistable", !Control::Button.persistable() && Control::Toggle.persistable() && Control::ReadOnly.persistable(), "");
    // 12) 按域可查（对账表逐域取用同一份登记）。
    cs.add(
        "walk_lookup_by_domain",
        book.get("F057").map(|x| x.name) == Some("IO 分级") && book.get("F999").is_none(),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_domain_has_a_reason_for_its_locus() {
        let mut out = [WalkEntry {
            domain: "",
            name: "",
            blurb: "",
            control: Control::ReadOnly,
            locus: Locus::C,
            locus_reason: "",
            path: [""; PATH_MAX],
            path_len: 0,
            knobs: 0,
            perf_line_ms: 0,
            dpi_checked: false,
        }; WALK_DOMAINS];
        let book = k1_book();
        let n = book.audit(&mut out);
        for i in 0..n {
            assert!(!out[i].locus_reason.is_empty(), "{} 必须给出落位理由", out[i].domain);
        }
    }

    #[test]
    fn path_segments_never_leave_stale_tail() {
        // F043 只有 2 段：后两段必须为空（不残留旧值误导走查）
        let e = k1_book().get("F043").unwrap();
        assert_eq!(e.path_len, 2);
        assert!(e.path[2].is_empty() && e.path[3].is_empty());
        assert!(e.path_ok());
    }

    #[test]
    fn blurb_renders_name_and_control() {
        let e = k1_book().get("F048").unwrap();
        let mut b = [0u8; 96];
        let n = e.render_blurb(&mut b);
        let s = core::str::from_utf8(&b[..n]).unwrap();
        assert!(s.starts_with("CPU 频率策略"), "{}", s);
        assert!(s.contains("下拉"), "{}", s);
    }

    #[test]
    fn walk_book_rejects_eighteenth_entry() {
        let mut b = k1_book();
        let extra = WalkEntry {
            domain: "F058",
            name: "x",
            blurb: "x",
            control: Control::ReadOnly,
            locus: Locus::C,
            locus_reason: "x",
            path: ["x", "", "", ""],
            path_len: 1,
            knobs: 0,
            perf_line_ms: 0,
            dpi_checked: false,
        };
        assert!(!b.register(extra), "容量固定十七域，越界登记失败并如实返回");
    }

    #[test]
    fn read_only_loci_do_not_offer_adjustments() {
        let mut out = [WalkEntry {
            domain: "",
            name: "",
            blurb: "",
            control: Control::ReadOnly,
            locus: Locus::C,
            locus_reason: "",
            path: [""; PATH_MAX],
            path_len: 0,
            knobs: 0,
            perf_line_ms: 0,
            dpi_checked: false,
        }; WALK_DOMAINS];
        let n = k1_book().audit(&mut out);
        for i in 0..n {
            if !out[i].locus.needs_ui() {
                assert_eq!(out[i].control, Control::ReadOnly, "{} 免调节却配了可调控件", out[i].domain);
            }
        }
    }
}
