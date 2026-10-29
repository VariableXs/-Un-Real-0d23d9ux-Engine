//! UNX-F10299 · O3 对接面中检（十八项对平）
//! 段位 0x11880 · 判据锚 UNX-F10299-J1 · 域 O3 对接面/中检
//! 结构：MidCheckBook（中检账，AXIS 18 项）+ 对平核 reconcile + 缺项检出
//! 语义：对接面移交前必须十八项逐项对平（实现侧 ↔ 判据侧），
//!       缺项、口径漂移、单侧绿均判中检不过；缺项 10/10 次检出。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x11880;

/// 中检项总数（卡面口径：十八项）
pub const AXIS_TOTAL: usize = 18;

/// 单项对平状态
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemState {
    /// 实现侧绿 + 判据侧绿
    BothGreen,
    /// 仅实现侧绿（判据缺失）
    ImplOnly,
    /// 仅判据侧绿（实现缺失）
    CheckOnly,
    /// 双侧红
    BothRed,
    /// 缺项（任一侧未申报）
    Missing,
}

impl ItemState {
    pub fn reconciled(self) -> bool {
        self == ItemState::BothGreen
    }
    pub fn is_missing(self) -> bool {
        self == ItemState::Missing
    }
    pub fn code(self) -> u16 {
        (SEG_BASE as u16) + match self {
            ItemState::BothGreen => 1,
            ItemState::ImplOnly => 2,
            ItemState::CheckOnly => 3,
            ItemState::BothRed => 4,
            ItemState::Missing => 5,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ItemState::BothGreen => "BOTH-GREEN",
            ItemState::ImplOnly => "IMPL-ONLY",
            ItemState::CheckOnly => "CHECK-ONLY",
            ItemState::BothRed => "BOTH-RED",
            ItemState::Missing => "MISSING",
        }
    }
}

/// 中检单项
#[derive(Clone, Debug)]
pub struct MidItem {
    pub idx: usize,
    pub title: String,
    pub impl_green: Option<bool>,
    pub check_green: Option<bool>,
}

impl MidItem {
    pub fn state(&self) -> ItemState {
        match (self.impl_green, self.check_green) {
            (None, _) | (_, None) => ItemState::Missing,
            (Some(true), Some(true)) => ItemState::BothGreen,
            (Some(true), Some(false)) => ItemState::ImplOnly,
            (Some(false), Some(true)) => ItemState::CheckOnly,
            (Some(false), Some(false)) => ItemState::BothRed,
        }
    }
    /// 缺项检出：未申报任一侧即缺项
    pub fn is_deficient(&self) -> bool {
        matches!(self.state(), ItemState::Missing | ItemState::ImplOnly | ItemState::CheckOnly | ItemState::BothRed)
    }
}

/// 中检账（AXIS 18 项）
pub struct MidCheckBook {
    items: Vec<MidItem>,
    /// 中检执行次数
    runs: u32,
    /// 缺项检出事件
    missing_hits: u32,
}

impl MidCheckBook {
    pub fn new() -> Self {
        MidCheckBook { items: Vec::new(), runs: 0, missing_hits: 0 }
    }

    /// 申报一项（idx 必须在 0..AXIS_TOTAL 且不重复）
    pub fn declare(&mut self, idx: usize, title: &str, impl_green: Option<bool>, check_green: Option<bool>) -> bool {
        if idx >= AXIS_TOTAL || self.items.iter().any(|i| i.idx == idx) {
            return false;
        }
        self.items.push(MidItem { idx, title: title.to_string(), impl_green, check_green });
        true
    }

    /// 十八项逐项对平：申报满 AXIS_TOTAL 且全部 BothGreen
    pub fn reconcile(&mut self) -> bool {
        self.runs += 1;
        let mut misses = 0u32;
        for i in 0..AXIS_TOTAL {
            match self.items.iter().find(|x| x.idx == i) {
                None => misses += 1,
                Some(it) => {
                    if it.is_deficient() {
                        if it.state() == ItemState::Missing {
                            misses += 1;
                        }
                    }
                }
            }
        }
        if misses > 0 {
            self.missing_hits += misses;
            return false;
        }
        self.items.iter().all(|it| it.state() == ItemState::BothGreen)
    }

    /// 缺项明细文本（判据诊断用）
    pub fn missing_text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for i in 0..AXIS_TOTAL {
            match self.items.iter().find(|x| x.idx == i) {
                None => parts.push(format!("[{}](缺项-未申报)", i)),
                Some(it) => {
                    let st = it.state();
                    if st != ItemState::BothGreen {
                        parts.push(format!("[{}]({}:{})", i, st.name(), it.title));
                    }
                }
            }
        }
        if parts.is_empty() {
            "全绿".to_string()
        } else {
            parts.join(", ")
        }
    }

    pub fn runs(&self) -> u32 {
        self.runs
    }
    pub fn missing_hits(&self) -> u32 {
        self.missing_hits
    }
    pub fn declared(&self) -> usize {
        self.items.len()
    }

    /// 中检报告指纹（FNV-1a）
    pub fn report_fp(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        for it in &self.items {
            h ^= it.idx as u32;
            h = h.wrapping_mul(0x01000193);
            for b in it.title.bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            h ^= it.state().code() as u32;
            h = h.wrapping_mul(0x01000193);
        }
        h ^ (SEG_BASE & 0xFFFF)
    }

    pub fn render(&self) -> String {
        format!(
            "midcheck seg={:#06x} axis={} declared={} runs={} missing_hits={}",
            SEG_BASE,
            AXIS_TOTAL,
            self.declared(),
            self.runs,
            self.missing_hits
        )
    }
}

/// 十八项标准申报（全绿口径）：对接面移交的正样本
pub fn standard_axis_titles() -> [&'static str; AXIS_TOTAL] {
    [
        "契约成文", "移交清单", "幂等校验", "版本锚", "回流通道", "演练注入",
        "重试协议", "只读视图", "审计账", "增量同步", "冲突仲裁", "回流状态机",
        "双档标记", "ktest 桥", "性能预算", "降级预案", "交接确认书", "本项中检",
    ]
}

/// 受控注入核：kind=0 缺项（末位不申报）/ kind=1 口径漂移（单侧绿）/ kind=2 重复申报
pub fn inject_fault(b: &mut MidCheckBook, kind: u8) -> &'static str {
    match kind {
        0 => {
            // 申报 0..17 共 17 项，末位缺
            for (i, t) in standard_axis_titles().iter().enumerate().take(AXIS_TOTAL - 1) {
                b.declare(i, t, Some(true), Some(true));
            }
            "missing-tail"
        }
        1 => {
            // 全申报但第 7 项仅实现侧绿
            for (i, t) in standard_axis_titles().iter().enumerate() {
                let ck = Some(true);
                let im = Some(i != 7);
                b.declare(i, t, im, ck);
            }
            "one-sided-green"
        }
        2 => {
            b.declare(0, "dup", Some(true), Some(true));
            let ok = b.declare(0, "dup2", Some(true), Some(true));
            if ok {
                "dup-declared"
            } else {
                "dup-rejected"
            }
        }
        _ => "no-op",
    }
}

pub fn run_unx10299_checks() -> CheckSet {
    let mut s = CheckSet::new("O3");
    // J1-01 基线：空账缺项
    {
        let mut b = MidCheckBook::new();
        let ok = b.reconcile();
        s.add("基线-空账缺项拒绝", !ok && b.missing_hits() >= AXIS_TOTAL as u32, "空账对平须拒绝且记 18 缺项");
    }
    // J1-02 正路径：十八项全绿对平 10/10
    {
        let mut ok_all = true;
        for _ in 0..10 {
            let mut b = MidCheckBook::new();
            for (i, t) in standard_axis_titles().iter().enumerate() {
                if !b.declare(i, t, Some(true), Some(true)) {
                    ok_all = false;
                }
            }
            if !b.reconcile() {
                ok_all = false;
            }
        }
        s.add("正路径-中检十连", ok_all, "十八项逐项对平 10/10 须达成");
    }
    // J1-03 缺项检出 10/10
    {
        let mut hits = 0;
        for _ in 0..10 {
            let mut b = MidCheckBook::new();
            let _ = inject_fault(&mut b, 0);
            if !b.reconcile() {
                hits += 1;
            }
        }
        s.add("检出-缺项十连", hits == 10, "末位缺项 10/10 须判不过");
    }
    // J1-04 口径漂移（单侧绿）判不过
    {
        let mut b = MidCheckBook::new();
        let _ = inject_fault(&mut b, 1);
        let ok = b.reconcile();
        let txt = b.missing_text();
        s.add(
            "检出-单侧绿判不过",
            !ok && txt.contains("[7]") && txt.contains("CHECK-ONLY"),
            "第 7 项实现缺失（仅判据侧绿）须判不过且明细可读",
        );
    }
    // J1-05 重复申报拒绝
    {
        let mut b = MidCheckBook::new();
        let r = inject_fault(&mut b, 2);
        s.add("申报-重复拒绝", r == "dup-rejected" && b.declared() == 1, "同 idx 重复申报须拒");
        s.add("申报-越界拒绝", !b.declare(AXIS_TOTAL, "oob", Some(true), Some(true)), "idx≥18 须拒");
    }
    // J1-06 缺项计数口径
    {
        let mut b = MidCheckBook::new();
        // 只申报 16 项（缺 2 项）
        for (i, t) in standard_axis_titles().iter().enumerate().take(16) {
            b.declare(i, t, Some(true), Some(true));
        }
        b.reconcile();
        s.add("口径-缺二记二", b.missing_hits() == 2, "未申报两项须各记一次缺项");
    }
    // J1-07 BothRed 判不过
    {
        let mut b = MidCheckBook::new();
        for (i, t) in standard_axis_titles().iter().enumerate() {
            let g = Some(i != 3);
            b.declare(i, t, g, g);
        }
        s.add(
            "检出-双侧红判不过",
            !b.reconcile() && b.missing_hits() == 0,
            "双侧红判不过但不算缺项（口径分离）",
        );
    }
    // J1-08 报告指纹敏感
    {
        let mut a = MidCheckBook::new();
        let mut bb = MidCheckBook::new();
        for (i, t) in standard_axis_titles().iter().enumerate() {
            a.declare(i, t, Some(true), Some(true));
            let cg = Some(i != 11);
            bb.declare(i, t, Some(true), cg);
        }
        a.reconcile();
        bb.reconcile();
        s.add("指纹-状态敏感", a.report_fp() != bb.report_fp(), "对平状态不同指纹须不同");
    }
    // J1-09 渲染与标准标题对位
    {
        let mut b = MidCheckBook::new();
        for (i, t) in standard_axis_titles().iter().enumerate() {
            b.declare(i, t, Some(true), Some(true));
        }
        b.reconcile();
        let txt = b.missing_text();
        let rd = b.render();
        s.add(
            "渲染-全绿文本与账面",
            txt == "全绿" && rd.contains("declared=18") && rd.contains("0x11880"),
            "全绿对平文本须为「全绿」且渲染含申报数与段位",
        );
    }
    // J1-10 十轮回放指纹稳定
    {
        let fp0 = {
            let mut b = MidCheckBook::new();
            for (i, t) in standard_axis_titles().iter().enumerate() {
                b.declare(i, t, Some(true), Some(true));
            }
            b.reconcile();
            b.report_fp()
        };
        let mut stable = true;
        for _ in 0..10 {
            let mut b = MidCheckBook::new();
            for (i, t) in standard_axis_titles().iter().enumerate() {
                b.declare(i, t, Some(true), Some(true));
            }
            b.reconcile();
            if b.report_fp() != fp0 {
                stable = false;
            }
        }
        s.add("回放-中检指纹十轮稳定", stable, "同构十轮回放指纹一致");
    }
    s
}
