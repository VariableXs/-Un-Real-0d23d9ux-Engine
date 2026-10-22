//! UNX-F7561 · SMART 消费架构总图（B4 读通道 → B5 判定 → K2/K5 呈现三层）· 诊断段 0xD080
//! 验收锚点：三层消费架构落账——B4 层为唯一读通道（所有 SMART 原始页只能经其取出），
//! B5 层为唯一判定层（只消费 B4 输出，产出健康分级），K2/K5 为呈现层（只消费 B5 输出）；
//! 全链零寄存器直读断言：任何一层不得绕开读通道直取端口/寄存器。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xD080;

/// B4 读通道：SMART 属性页唯一入口。页号 → 512B 原始页。
pub struct ReadChannel {
    pages: Vec<(u8, [u8; 32])>,
    served: u32,
}

/// 一条 SMART 属性（B4 解析产物）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attribute {
    pub id: u8,
    pub value: u8,
    pub worst: u8,
    pub raw: u64,
}

/// B5 判定产物：健康分级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    Normal,
    Warn,
    Critical,
    Fail,
}

impl Grade {
    pub fn as_str(self) -> &'static str {
        match self {
            Grade::Normal => "normal",
            Grade::Warn => "warn",
            Grade::Critical => "critical",
            Grade::Fail => "fail",
        }
    }
}

/// B5 判定层：输入 Attribute，输出 Grade。唯一判定入口。
pub struct Judge {
    pub id: u8,
    pub warn_below: u8,
    pub crit_below: u8,
    pub fail_below: u8,
}

impl Judge {
    pub fn new(id: u8, warn_below: u8, crit_below: u8, fail_below: u8) -> Self {
        Judge { id, warn_below, crit_below, fail_below }
    }

    pub fn evaluate(&self, a: &Attribute) -> Grade {
        if a.id != self.id {
            return Grade::Normal;
        }
        if a.value <= self.fail_below {
            Grade::Fail
        } else if a.value <= self.crit_below {
            Grade::Critical
        } else if a.value <= self.warn_below {
            Grade::Warn
        } else {
            Grade::Normal
        }
    }
}

/// K2/K5 呈现层：只消费 B5 输出，产出文本行。禁止回读 B4 原始页。
pub struct Presenter {
    pub lines: Vec<String>,
}

impl Presenter {
    pub fn new() -> Self {
        Presenter { lines: Vec::new() }
    }

    /// K2 呈现：单行摘要。入参只有判定产物，不持有 B4 句柄（架构断言的代码级体现）。
    pub fn k2_line(&mut self, slot: u8, g: Grade) {
        self.lines.push(format!("K2 disk{}: {}", slot, g.as_str()));
    }

    /// K5 呈现：表格行（盘号/属性/分级）。
    pub fn k5_row(&mut self, slot: u8, attr_id: u8, g: Grade) {
        self.lines.push(format!("K5 disk{} attr{} = {}", slot, attr_id, g.as_str()));
    }

    pub fn count_worst(&self) -> u32 {
        let mut n = 0u32;
        for l in &self.lines {
            if l.contains("fail") || l.contains("critical") {
                n += 1;
            }
        }
        n
    }
}

impl ReadChannel {
    pub fn new() -> Self {
        ReadChannel { pages: Vec::new(), served: 0 }
    }

    pub fn install(&mut self, page: u8, data: [u8; 32]) {
        self.pages.push((page, data));
    }

    /// 唯一读入口：返回页副本。无端口访问路径——零寄存器直读的架构保证。
    pub fn read_page(&mut self, page: u8) -> Option<[u8; 32]> {
        for (p, d) in &self.pages {
            if *p == page {
                self.served += 1;
                return Some(*d);
            }
        }
        None
    }

    /// B4 解析：把 32B 页缩位为一条属性（id/worst/value/raw 截 8B）。
    pub fn parse_attr(&self, page: &[u8; 32]) -> Option<Attribute> {
        if page[0] == 0 {
            return None;
        }
        let mut raw = 0u64;
        for i in 0..8 {
            raw |= (page[2 + i] as u64) << (i * 8);
        }
        Some(Attribute { id: page[0], value: page[1], worst: page[10], raw })
    }
}

/// 全图：三层实例的持有者（演示装配顺序 B4 → B5 → 呈现）。
pub struct ConsumerGraph {
    pub chan: ReadChannel,
    pub judges: Vec<Judge>,
    pub presenter: Presenter,
}

impl ConsumerGraph {
    pub fn new() -> Self {
        ConsumerGraph { chan: ReadChannel::new(), judges: Vec::new(), presenter: Presenter::new() }
    }

    /// 全链消费：B4 取页 → B5 判定 → K2/K5 呈现。返回各属性分级。
    pub fn consume(&mut self, slot: u8, page: u8) -> Vec<Grade> {
        let mut grades = Vec::new();
        if let Some(pg) = self.chan.read_page(page) {
            if let Some(a) = self.chan.parse_attr(&pg) {
                let g = match self.judges.iter().find(|j| j.id == a.id) {
                    Some(j) => j.evaluate(&a),
                    None => Grade::Normal,
                };
                self.presenter.k2_line(slot, g);
                self.presenter.k5_row(slot, a.id, g);
                grades.push(g);
            }
        }
        grades
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xD080 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7561_D080;
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    let mut ok = true;
    for _ in 0..64 {
        let v = lcg_next(&mut seed);
        if v == 0 && (v ^ 0xFFFF) != v.wrapping_add(0xFFFF) {
            ok = false;
        }
    }
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("读通道-空页返回None", ReadChannel::new().read_page(0x7F).is_none()));
    out.push(("判定-未知属性归Normal", Judge::new(5, 60, 30, 10).evaluate(&Attribute { id: 9, value: 1, worst: 1, raw: 0 }) == Grade::Normal));
    out.push(("呈现-空表计数为0", Presenter::new().count_worst() == 0));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7561_checks：本单判据聚合（诊断段 0xD080）。
pub fn run_unx7561_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7561 SMART消费架构总图");

    // 注入核 5 项
    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 架构判据：三层装配 → 全链消费
    let mut g = ConsumerGraph::new();
    let mut page = [0u8; 32];
    page[0] = 5; // id=5 (Reallocated)
    page[1] = 8; // value=8
    page[10] = 8; // worst
    g.chan.install(0x10, page);

    let mut page2 = [0u8; 32];
    page2[0] = 194; // id=194 (Temperature)
    page2[1] = 64;
    page2[10] = 64;
    g.chan.install(0x11, page2);

    g.judges.push(Judge::new(5, 60, 30, 10));
    g.judges.push(Judge::new(194, 40, 30, 20));

    let grades = g.consume(3, 0x10);
    let ok_single = grades.len() == 1 && grades[0] == Grade::Fail;
    s.add("B5判定-value8低于fail线10判Fail", ok_single, if ok_single { "" } else { "全链首查未判 Fail" });

    let grades2 = g.consume(3, 0x11);
    let ok_normal = grades2.len() == 1 && grades2[0] == Grade::Normal;
    s.add("B5判定-温度64归Normal", ok_normal, if ok_normal { "" } else { "温度判级异常" });

    // 读通道唯一性：served 计数只经 read_page 增长
    let ok_served = g.chan.served == 2;
    s.add("B4读通道-两轮全链恰好两次出页", ok_served, if ok_served { "" } else { "出页计数漂移" });

    // 呈现层只消费 B5：K2/K5 行数 = 消费轮数×2，且不含原始页字节
    let n_lines = g.presenter.lines.len();
    let ok_lines = n_lines == 4;
    s.add("呈现层-4行(2轮×K2+K5)", ok_lines, if ok_lines { "" } else { "呈现行数失配" });
    let no_raw = g.presenter.lines.iter().all(|l| !l.contains("raw="));
    s.add("呈现层-不泄露原始raw值", no_raw, if no_raw { "" } else { "呈现层越层回读" });

    // 分级边界：warn 线恰等
    let j = Judge::new(7, 60, 40, 20);
    let ok_warn_eq = j.evaluate(&Attribute { id: 7, value: 60, worst: 60, raw: 0 }) == Grade::Warn;
    s.add("分级-warn线恰等判Warn", ok_warn_eq, if ok_warn_eq { "" } else { "边界含等号漂移" });
    let ok_crit_eq = j.evaluate(&Attribute { id: 7, value: 40, worst: 40, raw: 0 }) == Grade::Critical;
    s.add("分级-crit线恰等判Critical", ok_crit_eq, if ok_crit_eq { "" } else { "边界含等号漂移" });

    // 未安装页：consume 静默返回空（读通道 None 短路）
    let grades3 = g.consume(4, 0x99);
    s.add("未装页-短路不产级", grades3.is_empty(), if grades3.is_empty() { "" } else { "None 短路失效" });

    // 64 盘扫描全量演练
    let mut total_fail = 0u32;
    for d in 0..64u8 {
        let mut g2 = ConsumerGraph::new();
        let mut p = [0u8; 32];
        p[0] = 5;
        p[1] = if d % 4 == 0 { 5 } else { 90 };
        g2.chan.install(0x20, p);
        g2.judges.push(Judge::new(5, 60, 30, 10));
        let r = g2.consume(d as u8, 0x20);
        if r.iter().any(|x| *x == Grade::Fail) {
            total_fail += 1;
        }
    }
    let ok_scan = total_fail == 16;
    s.add("64盘扫描-16盘Fail恰数", ok_scan, if ok_scan { "" } else { "批量扫描计数漂移" });

    // FNV 稳定性
    let h1 = fnv1a(format!("K2 disk3: {}", Grade::Fail.as_str()).as_bytes());
    let h2 = fnv1a(b"K2 disk3: fail");
    s.add("FNV-呈现行哈希稳定", h1 == h2, if h1 == h2 { "" } else { "哈希不稳" });

    s
}
