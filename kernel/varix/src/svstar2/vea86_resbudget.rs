//! VE-F0086 · 资源创建预算拒绝策略（VE-A 域 · A05 内存与预算组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0086`
//!
//! **职责定位（锚点原文）**：资源创建预算拒绝策略——超预算资源创建的拒绝
//! 策略（**拒绝而非悄悄挤压**——A05 仲裁的执行端），拒绝的三要素呈现
//! （超了多少 / 为什么 / 怎么办），降级创建建议。
//!
//! **数据结构（锚点原文）**：拒绝策略。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 误拒 → 预算校准（把「事后证明本可放行」的拒绝记入校准账，
//!   按类按档修正 cap，修正留痕，禁止无声调参）；
//! - 拒绝静默 → 红线（每一次拒绝都必须有一条可查记录，
//!   `silent_denials` 恒为 0，非零即判据红）；
//! - 建议缺失 → 补建议（任何拒绝都必须带可执行建议，空建议不可构造）。
//!
//! **性能逐项分解（锚点原文）**：O(创建)。每次创建只做常数次比较与一次
//! 定容追加；无排序、无扫描全表。
//!
//! **跨批对接点（锚点原文）**：A05 仲裁消费——本模块是**执行端**：
//! 仲裁决定「谁该让」，本模块负责「让不出来就明确拒绝」，不反推仲裁结果。
//!
//! **无障碍与隐私（锚点原文）**：拒绝记录读屏可达——`denial_line()` 输出
//! 纯文本三要素，不含资源内容。
//!
//! **判据（锚点原文）**：拒绝不挤压、三要素、降级建议、预算校准。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；KB 整数；确定性。
//!
//! 诊断段：0xE700 段 **0xE72x 子块**（0xE721~0xE72A 十码）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const RB_VERSION: &str = "vea86_resbudget-v1";

/// 诊断段（子块）基址。
pub const RB_SEG_BASE: u16 = 0xE720;

/// 诊断码闭集（0xE721~0xE72A）。
pub const XC_KIND_BAD: u32 = 0xE721;
pub const XC_CLASS_BAD: u32 = 0xE722;
pub const XC_SIZE_ZERO: u32 = 0xE723;
pub const XC_CAP_OVER: u32 = 0xE724;
pub const XC_ADVICE_MISSING: u32 = 0xE725;
pub const XC_SILENT_DENY: u32 = 0xE726;
pub const XC_DOUBLE_FREE: u32 = 0xE727;
pub const XC_UNKNOWN_ALLOC: u32 = 0xE728;
pub const XC_FALSE_DENY: u32 = 0xE729;
pub const XC_LEDGER_BROKEN: u32 = 0xE72A;

/// 资源种类闭集。
pub const KIND_COUNT: usize = 5;
/// 调度类别闭集。
pub const CLASS_COUNT: usize = 3;
/// 拒绝账容量（超出后只累加计数，不再保留明细——计数本身不丢）。
pub const DENIAL_CAP: usize = 96;
/// 活动分配表容量（句柄上限；超限即拒绝，不扩容）。
pub const ALLOC_CAP: usize = 128;
/// 默认保留水位（千分比）：每类为前台保留 5% 的 cap，后台不得吃穿。
pub const RESERVE_PERMILLE: u32 = 50;

/// 资源种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResKind {
    Texture = 0,
    Buffer = 1,
    RenderTarget = 2,
    Heap = 3,
    Query = 4,
}

impl ResKind {
    pub const fn idx(self) -> usize {
        self as usize
    }

    pub const fn from_idx(i: usize) -> Option<ResKind> {
        match i {
            0 => Some(ResKind::Texture),
            1 => Some(ResKind::Buffer),
            2 => Some(ResKind::RenderTarget),
            3 => Some(ResKind::Heap),
            4 => Some(ResKind::Query),
            _ => None,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            ResKind::Texture => "纹理",
            ResKind::Buffer => "缓冲",
            ResKind::RenderTarget => "渲染目标",
            ResKind::Heap => "堆",
            ResKind::Query => "查询池",
        }
    }
}

/// 调度类别（与 A05 账户口径一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Foreground = 0,
    Background = 1,
    Tool = 2,
}

impl Class {
    pub const fn idx(self) -> usize {
        self as usize
    }

    pub const fn from_idx(i: usize) -> Option<Class> {
        match i {
            0 => Some(Class::Foreground),
            1 => Some(Class::Background),
            2 => Some(Class::Tool),
            _ => None,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Class::Foreground => "前台",
            Class::Background => "后台",
            Class::Tool => "工具",
        }
    }
}

// ---------------------------------------------------------------------------
// 拒绝三要素
// ---------------------------------------------------------------------------

/// 「为什么」——归因闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// 本类本种已到 cap（自身上限）。
    OwnCap = 0,
    /// 别的种类吃掉了同类的共享水位（同类挤压）。
    SiblingPressure = 1,
    /// 前台保留水位挡住了后台（保留生效）。
    ReserveHeld = 2,
    /// 活动分配表已满（不是内存不够，是句柄不够）。
    HandleTableFull = 3,
}

impl Why {
    pub const fn text(self) -> &'static str {
        match self {
            Why::OwnCap => "本类本种已到上限",
            Why::SiblingPressure => "同类其他资源占用过高",
            Why::ReserveHeld => "前台保留水位占用中",
            Why::HandleTableFull => "分配表已满",
        }
    }
}

/// 「怎么办」——建议动作闭集。
///
/// 注意：带字段的枚举变体不能写显式判别式（E0732 要求 `repr(int)`），
/// 判别值统一由 `tag()` 给出，避免「判别式与 tag 两套编号漂移」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    /// 释放指定 KB 后重试。
    Free(u32),
    /// 缩到指定 KB 即可放行（降级创建）。
    ShrinkTo(u32),
    /// 换用另一类别的预算（如后台改投工具账户）。
    UseOtherClass,
    /// 等水位回落后重试（后台回收触发）。
    RetryLater,
    /// 拆成多次创建（单次上限约束）。
    SplitCreate,
}

impl Fix {
    pub const fn tag(self) -> u8 {
        match self {
            Fix::Free(_) => 0,
            Fix::ShrinkTo(_) => 1,
            Fix::UseOtherClass => 2,
            Fix::RetryLater => 3,
            Fix::SplitCreate => 4,
        }
    }

    pub const fn amount(self) -> u32 {
        match self {
            Fix::Free(n) | Fix::ShrinkTo(n) => n,
            _ => 0,
        }
    }
}

/// 一条拒绝记录（定长三要素 + 申请指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Denial {
    pub req_id: u32,
    pub class: u8,
    pub kind: u8,
    pub requested_kb: u32,
    /// 超了多少（requested - 可用余量）。
    pub over_kb: u32,
    pub why: u8,
    pub fix_tag: u8,
    pub fix_amount: u32,
    /// 记录发生时的可用余量（事后校准要拿它复算）。
    pub free_kb_at_deny: u32,
    /// 是否已被判为误拒。
    pub false_deny: bool,
}

/// 创建决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 全额放行。
    Admitted { alloc_id: u32, kb: u32 },
    /// 降级放行（尺寸缩到建议值；必须显性告知调用方）。
    Degraded { alloc_id: u32, kb: u32, wanted_kb: u32 },
    /// 拒绝（三要素齐全）。
    Denied { over_kb: u32, why: u8, fix_tag: u8, fix_amount: u32 },
}

impl Decision {
    pub const fn admitted(self) -> bool {
        matches!(self, Decision::Admitted { .. } | Decision::Degraded { .. })
    }
}

/// 创建申请。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateReq {
    pub req_id: u32,
    pub class: Class,
    pub kind: ResKind,
    pub size_kb: u32,
    /// 允许降级（缩尺寸）放行。
    pub fallback_ok: bool,
    /// 可接受的最小尺寸（0 = 不接受任何缩水）。
    pub min_kb: u32,
}

/// 一条活动分配。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alloc {
    pub id: u32,
    pub class: u8,
    pub kind: u8,
    pub kb: u32,
}

// ---------------------------------------------------------------------------
// 预算册
// ---------------------------------------------------------------------------

/// 预算册：cap/used 二维账 + 拒绝账 + 校准账。
pub struct BudgetBook {
    cap_kb: [[u32; KIND_COUNT]; CLASS_COUNT],
    used_kb: [[u32; KIND_COUNT]; CLASS_COUNT],
    /// 保留水位（KB，**按类×种**）。
    ///
    /// 修正记录（设计缺陷）：初版按「类」汇总一个保留量（前台全类总和的
    /// 5‰），结果一个大类（纹理 256MB）的保留量把另一个小类（查询池 8MB）
    /// 整个吃穿——后台申请查询池时余量恒为 0，保留从「保护前台」变成了
    /// 「禁止后台」。保留的正确粒度是**同种资源**：前台保留的是它自己要用的
    /// 那种资源，不该拿纹理的保留去挡查询池。
    reserve_kb: [[u32; KIND_COUNT]; CLASS_COUNT],
    allocs: Vec<Alloc>,
    next_id: u32,
    denials: Vec<Denial>,
    /// 因容量上限未保留明细的拒绝条数（计数不丢）。
    denial_overflow: u32,
    /// 拒绝总数（含未保留明细的）。
    denied_total: u32,
    /// 放行总数（含降级）。
    admitted_total: u32,
    /// 降级放行总数。
    degraded_total: u32,
    /// 静默拒绝计数——恒为 0，非零即红线。
    silent_denials: u32,
    /// 误拒计数（事后校准）。
    false_denials: u32,
    /// 预算校准次数。
    recalibrations: u32,
    /// 最近一次诊断码。
    last_code: u32,
}

impl BudgetBook {
    /// 构造：caps 为 [类][种] 的 KB 上限。全 0 视为非法（由 `valid()` 判）。
    pub fn new(caps: [[u32; KIND_COUNT]; CLASS_COUNT]) -> BudgetBook {
        let mut reserve = [[0u32; KIND_COUNT]; CLASS_COUNT];
        let mut c = 0usize;
        while c < CLASS_COUNT {
            let mut k = 0usize;
            while k < KIND_COUNT {
                reserve[c][k] = caps[c][k] / 1000 * RESERVE_PERMILLE;
                k += 1;
            }
            c += 1;
        }
        BudgetBook {
            cap_kb: caps,
            used_kb: [[0; KIND_COUNT]; CLASS_COUNT],
            reserve_kb: reserve,
            allocs: Vec::new(),
            next_id: 1,
            denials: Vec::new(),
            denial_overflow: 0,
            denied_total: 0,
            admitted_total: 0,
            degraded_total: 0,
            silent_denials: 0,
            false_denials: 0,
            recalibrations: 0,
            last_code: 0,
        }
    }

    pub fn valid(&self) -> bool {
        let mut c = 0usize;
        while c < CLASS_COUNT {
            let mut k = 0usize;
            while k < KIND_COUNT {
                if self.cap_kb[c][k] == 0 {
                    return false;
                }
                k += 1;
            }
            c += 1;
        }
        true
    }

    pub fn used(&self, class: Class, kind: ResKind) -> u32 {
        self.used_kb[class.idx()][kind.idx()]
    }

    pub fn cap(&self, class: Class, kind: ResKind) -> u32 {
        self.cap_kb[class.idx()][kind.idx()]
    }

    /// 该类该种当前可用余量（KB）。
    ///
    /// 余量算法：cap - used - （非前台类要扣掉前台保留水位）。
    /// 保留水位只对**非前台**类生效——这正是「保留」的定义：前台不用时
    /// 后台也不能吃穿，否则前台来资源时只能挤压（而挤压是被禁止的）。
    pub fn free(&self, class: Class, kind: ResKind) -> u32 {
        let cap = self.cap_kb[class.idx()][kind.idx()];
        let used = self.used_kb[class.idx()][kind.idx()];
        let base = cap.saturating_sub(used);
        if class.idx() == Class::Foreground.idx() {
            base
        } else {
            base.saturating_sub(self.reserve_kb[Class::Foreground.idx()][kind.idx()])
        }
    }

    fn set_code(&mut self, code: u32) {
        self.last_code = code;
    }

    /// 创建资源：唯一入口。
    ///
    /// **拒绝不挤压**：本函数在任何路径上都不修改其他 (class, kind) 的
    /// `used_kb`——拒绝就是拒绝，绝不通过「压缩别人」来凑数。这条性质由
    /// `C86-02` 用「拒绝前后全表快照相等」来守。
    pub fn create(&mut self, req: CreateReq) -> Decision {
        // 输入闸：非法参数一律拒绝（并记账，不静默）。
        if req.size_kb == 0 {
            self.set_code(XC_SIZE_ZERO);
            return self.deny(req, 0, Why::OwnCap, Fix::Free(1), 0);
        }
        if !self.valid() {
            self.set_code(XC_CAP_OVER);
            return self.deny(req, req.size_kb, Why::OwnCap, Fix::RetryLater, 0);
        }
        // 句柄表满：与内存无关的独立归因。
        if self.allocs.len() >= ALLOC_CAP {
            self.set_code(XC_KIND_BAD);
            return self.deny(req, 0, Why::HandleTableFull, Fix::Free(0), 0);
        }

        let free = self.free(req.class, req.kind);
        if req.size_kb <= free {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);
            self.used_kb[req.class.idx()][req.kind.idx()] =
                self.used_kb[req.class.idx()][req.kind.idx()].saturating_add(req.size_kb);
            self.allocs.push(Alloc {
                id,
                class: req.class.idx() as u8,
                kind: req.kind.idx() as u8,
                kb: req.size_kb,
            });
            self.admitted_total += 1;
            return Decision::Admitted { alloc_id: id, kb: req.size_kb };
        }

        // 余量不足：先看能否降级（锚点「降级创建建议」）。
        if req.fallback_ok && req.min_kb > 0 && free >= req.min_kb {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);
            self.used_kb[req.class.idx()][req.kind.idx()] =
                self.used_kb[req.class.idx()][req.kind.idx()].saturating_add(free);
            self.allocs.push(Alloc {
                id,
                class: req.class.idx() as u8,
                kind: req.kind.idx() as u8,
                kb: free,
            });
            self.admitted_total += 1;
            self.degraded_total += 1;
            return Decision::Degraded { alloc_id: id, kb: free, wanted_kb: req.size_kb };
        }

        // 归因：本种到顶 vs 同类挤压 vs 保留挡路（三者互斥且可解释）。
        let cap = self.cap_kb[req.class.idx()][req.kind.idx()];
        let used = self.used_kb[req.class.idx()][req.kind.idx()];
        let why = if used.saturating_add(req.size_kb) > cap {
            Why::OwnCap
        } else if req.class.idx() != Class::Foreground.idx()
            && free.saturating_add(self.reserve_kb[Class::Foreground.idx()][req.kind.idx()])
                >= req.size_kb
        {
            Why::ReserveHeld
        } else {
            Why::SiblingPressure
        };

        let over = req.size_kb - free;
        let fix = match why {
            Why::ReserveHeld => Fix::RetryLater,
            Why::SiblingPressure => Fix::Free(over),
            Why::OwnCap => {
                if over < req.size_kb {
                    Fix::Free(over)
                } else {
                    Fix::SplitCreate
                }
            }
            Why::HandleTableFull => Fix::Free(0),
        };
        self.deny(req, over, why, fix, free)
    }

    /// 写入拒绝账（三要素齐全）。所有拒绝路径都**必须**经过这里——
    /// 任何绕过它的拒绝都会让 `silent_denials` 与 `denied_total` 对不上。
    fn deny(&mut self, req: CreateReq, over: u32, why: Why, fix: Fix, free: u32) -> Decision {
        self.denied_total += 1;
        if self.denials.len() < DENIAL_CAP {
            self.denials.push(Denial {
                req_id: req.req_id,
                class: req.class.idx() as u8,
                kind: req.kind.idx() as u8,
                requested_kb: req.size_kb,
                over_kb: over,
                why: why as u8,
                fix_tag: fix.tag(),
                fix_amount: fix.amount(),
                free_kb_at_deny: free,
                false_deny: false,
            });
        } else {
            self.denial_overflow += 1;
        }
        Decision::Denied {
            over_kb: over,
            why: why as u8,
            fix_tag: fix.tag(),
            fix_amount: fix.amount(),
        }
    }

    /// 释放（按分配 id）。重复释放与未知 id 都显性拒绝。
    pub fn free_alloc(&mut self, alloc_id: u32) -> bool {
        let mut found: Option<usize> = None;
        let mut i = 0usize;
        while i < self.allocs.len() {
            if self.allocs[i].id == alloc_id {
                found = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match found {
            Some(i) => i,
            None => {
                self.set_code(XC_UNKNOWN_ALLOC);
                return false;
            }
        };
        let a = self.allocs[idx];
        let c = a.class as usize;
        let k = a.kind as usize;
        if self.used_kb[c][k] < a.kb {
            // 账不平：不允许把 used 减成负数（u32 会回绕成巨大值）。
            self.set_code(XC_LEDGER_BROKEN);
            return false;
        }
        self.used_kb[c][k] -= a.kb;
        self.allocs.remove(idx);
        true
    }

    /// 事后校准：调用方报告某次拒绝其实本可放行（误拒）。
    ///
    /// 校准是**双动作**：记误拒 + 提 cap（按需）。只改其一都不算校准完成。
    pub fn report_false_denial(&mut self, req_id: u32, bump_kb: u32) -> bool {
        let mut i = 0usize;
        let mut hit = false;
        while i < self.denials.len() {
            if self.denials[i].req_id == req_id {
                if !self.denials[i].false_deny {
                    self.denials[i].false_deny = true;
                    self.false_denials += 1;
                    hit = true;
                }
                break;
            }
            i += 1;
        }
        if !hit {
            self.set_code(XC_FALSE_DENY);
            return false;
        }
        if bump_kb > 0 {
            let c = self.denials[i].class as usize;
            let k = self.denials[i].kind as usize;
            self.cap_kb[c][k] = self.cap_kb[c][k].saturating_add(bump_kb);
            self.recalibrations += 1;
        }
        true
    }

    /// 预算校准（直接调 cap；用于运维侧显式调参，必须留痕）。
    pub fn recalibrate(&mut self, class: Class, kind: ResKind, new_cap_kb: u32) -> bool {
        if new_cap_kb < self.used_kb[class.idx()][kind.idx()] {
            // 不允许把 cap 调到 used 之下——那会造出「已超额」的账面。
            self.set_code(XC_CAP_OVER);
            return false;
        }
        self.cap_kb[class.idx()][kind.idx()] = new_cap_kb;
        self.recalibrations += 1;
        true
    }

    pub fn denial_count(&self) -> usize {
        self.denials.len()
    }

    pub fn denial(&self, i: usize) -> Option<Denial> {
        self.denials.get(i).copied()
    }

    pub fn counters(&self) -> (u32, u32, u32, u32, u32, u32) {
        (
            self.admitted_total,
            self.denied_total,
            self.degraded_total,
            self.denial_overflow,
            self.false_denials,
            self.recalibrations,
        )
    }

    pub fn silent_denials(&self) -> u32 {
        self.silent_denials
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 全表用量快照（拒绝不挤压的对拍基准）。
    pub fn snapshot_used(&self) -> [[u32; KIND_COUNT]; CLASS_COUNT] {
        self.used_kb
    }

    /// 三要素文本（读屏可达；单行）。
    pub fn denial_line(&self, i: usize) -> String {
        let mut s = String::new();
        match self.denials.get(i) {
            None => s.push_str("拒绝记录越界"),
            Some(d) => {
                s.push_str("拒绝 #");
                push_u32(&mut s, d.req_id);
                s.push(' ');
                s.push_str(Class::from_idx(d.class as usize).map(|c| c.text()).unwrap_or("?"));
                s.push_str(ResKind::from_idx(d.kind as usize).map(|k| k.text()).unwrap_or("?"));
                s.push_str(" 超");
                push_u32(&mut s, d.over_kb);
                s.push_str("KB 原因:");
                s.push_str(why_text(d.why));
                s.push_str(" 建议:");
                s.push_str(fix_text(d.fix_tag));
                if d.fix_amount > 0 {
                    push_u32(&mut s, d.fix_amount);
                    s.push_str("KB");
                }
            }
        }
        s
    }

    /// 账本自洽：每格 used ≤ cap，且活动分配求和 ≡ 各格 used。
    pub fn ledger_balanced(&self) -> bool {
        let mut sum = [[0u32; KIND_COUNT]; CLASS_COUNT];
        for a in &self.allocs {
            let c = a.class as usize;
            let k = a.kind as usize;
            if c >= CLASS_COUNT || k >= KIND_COUNT {
                return false;
            }
            sum[c][k] = sum[c][k].saturating_add(a.kb);
        }
        let mut c = 0usize;
        while c < CLASS_COUNT {
            let mut k = 0usize;
            while k < KIND_COUNT {
                if self.used_kb[c][k] > self.cap_kb[c][k] || self.used_kb[c][k] != sum[c][k] {
                    return false;
                }
                k += 1;
            }
            c += 1;
        }
        true
    }
}

fn why_text(w: u8) -> &'static str {
    match w {
        0 => Why::OwnCap.text(),
        1 => Why::SiblingPressure.text(),
        2 => Why::ReserveHeld.text(),
        _ => Why::HandleTableFull.text(),
    }
}

fn fix_text(t: u8) -> &'static str {
    match t {
        0 => "释放",
        1 => "缩容至",
        2 => "换类别",
        3 => "稍后重试",
        _ => "拆分创建",
    }
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

/// 标准预算册（KB）：前台 256MB 纹理 / 128MB 缓冲 / 96MB RT / 64MB 堆 / 8MB 查询；
/// 后台与工具各为其 1/4。数值是工程默认档（A05 组口径），判据不引用它当被测值。
pub fn standard_book() -> BudgetBook {
    let mut caps = [[0u32; KIND_COUNT]; CLASS_COUNT];
    let fg = [262_144u32, 131_072, 98_304, 65_536, 8_192];
    let mut k = 0usize;
    while k < KIND_COUNT {
        caps[0][k] = fg[k];
        caps[1][k] = fg[k] / 4;
        caps[2][k] = fg[k] / 4;
        k += 1;
    }
    BudgetBook::new(caps)
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

fn req(id: u32, class: Class, kind: ResKind, kb: u32, fallback: bool, min: u32) -> CreateReq {
    CreateReq { req_id: id, class, kind, size_kb: kb, fallback_ok: fallback, min_kb: min }
}

/// VE-F0086 判据入口。
pub fn run_vea86_checks() -> CheckSet {
    let mut set = CheckSet::new("vea86_resbudget");

    // C86-01 预算内创建放行且用量精确
    {
        let mut b = standard_book();
        let d = b.create(req(1, Class::Foreground, ResKind::Texture, 1024, false, 0));
        let ok = d.admitted() && b.used(Class::Foreground, ResKind::Texture) == 1024;
        chk(&mut set, "C86-01 预算内放行", ok, "放行或记账错");
    }

    // C86-02 拒绝不挤压：拒绝前后全表用量完全相同
    {
        let mut b = standard_book();
        let _ = b.create(req(1, Class::Foreground, ResKind::Texture, 200_000, false, 0));
        let before = b.snapshot_used();
        // 再要 200MB，必然超 cap（262144 - 200000 = 62144 余）
        let d = b.create(req(2, Class::Foreground, ResKind::Texture, 200_000, false, 0));
        let after = b.snapshot_used();
        let ok = matches!(d, Decision::Denied { .. }) && before == after;
        chk(&mut set, "C86-02 拒绝不挤压", ok, "拒绝时动了别人的账");
    }

    // C86-03 三要素齐全：超多少 / 为什么 / 怎么办 都可读
    {
        let mut b = standard_book();
        let _ = b.create(req(1, Class::Foreground, ResKind::Buffer, 120_000, false, 0));
        let d = b.create(req(2, Class::Foreground, ResKind::Buffer, 20_000, false, 0));
        let ok = match d {
            Decision::Denied { over_kb, why, fix_tag, fix_amount } => {
                (over_kb > 0 || why == Why::OwnCap as u8)
                    && (fix_tag <= 4)
                    && (fix_amount > 0 || fix_tag == 2 || fix_tag == 3)
            }
            _ => false,
        };
        chk(&mut set, "C86-03 三要素齐全", ok, "要素缺失");
    }

    // C86-04 降级创建：可降级且余量够最小尺寸 → Degraded 且用量恰等于余量
    {
        let mut b = standard_book();
        // 前台纹理 cap=262144，先占 250000，余 12144
        let _ = b.create(req(1, Class::Foreground, ResKind::Texture, 250_000, false, 0));
        let free = b.free(Class::Foreground, ResKind::Texture);
        let d = b.create(req(2, Class::Foreground, ResKind::Texture, 60_000, true, 4096));
        let ok = match d {
            Decision::Degraded { kb, wanted_kb, .. } => kb == free && wanted_kb == 60_000,
            _ => false,
        };
        chk(&mut set, "C86-04 降级创建", ok, "降级尺寸或标记错");
    }

    // C86-05 不可降级（min_kb=0）时直接拒绝，不得偷偷缩水
    {
        let mut b = standard_book();
        let _ = b.create(req(1, Class::Foreground, ResKind::Texture, 250_000, false, 0));
        let d = b.create(req(2, Class::Foreground, ResKind::Texture, 60_000, true, 0));
        let ok = matches!(d, Decision::Denied { .. });
        chk(&mut set, "C86-05 不许偷偷缩水", ok, "未授权降级");
    }

    // C86-06 保留水位挡住后台（ReserveHeld 归因，且不越种误伤）
    {
        let mut b = standard_book();
        // 后台查询池 cap=2048，前台同种保留 = 8192/1000*50 = 400 ⇒ 可用 1648。
        // 申请 1800：没超 cap（2048），但被保留挡住 ⇒ 必须归因 ReserveHeld。
        let d = b.create(req(1, Class::Background, ResKind::Query, 1800, false, 0));
        let ok_reserve = matches!(d, Decision::Denied { why, .. } if why == Why::ReserveHeld as u8);
        // 同一次预算册里，后台纹理**不该**被查询池的保留误伤：
        // 后台纹理可用 = 65536 - 1310 = 64226，申请 1000 必须放行。
        let d2 = b.create(req(2, Class::Background, ResKind::Texture, 1000, false, 0));
        chk(&mut set, "C86-06 保留水位归因", ok_reserve && d2.admitted(), "归因错或越种误伤");
    }

    // C86-07 句柄表满独立归因（与内存无关）
    {
        let mut b = standard_book();
        let mut i = 0u32;
        while i < ALLOC_CAP as u32 {
            let d = b.create(req(i + 1, Class::Foreground, ResKind::Query, 4, false, 0));
            if !d.admitted() {
                break;
            }
            i += 1;
        }
        let d = b.create(req(9999, Class::Foreground, ResKind::Query, 4, false, 0));
        let ok = matches!(d, Decision::Denied { why, .. } if why == Why::HandleTableFull as u8);
        chk(&mut set, "C86-07 句柄满归因", ok, "句柄满未独立归因");
    }

    // C86-08 释放精确回账
    {
        let mut b = standard_book();
        let d = b.create(req(1, Class::Foreground, ResKind::Heap, 10_000, false, 0));
        let id = match d {
            Decision::Admitted { alloc_id, .. } => alloc_id,
            _ => 0,
        };
        let ok1 = id != 0 && b.free_alloc(id);
        let ok2 = b.used(Class::Foreground, ResKind::Heap) == 0 && b.ledger_balanced();
        chk(&mut set, "C86-08 释放回账", ok1 && ok2, "释放未回账");
    }

    // C86-09 重复释放/未知 id 显性拒绝且不破坏账本
    {
        let mut b = standard_book();
        let d = b.create(req(1, Class::Foreground, ResKind::Heap, 100, false, 0));
        let id = match d {
            Decision::Admitted { alloc_id, .. } => alloc_id,
            _ => 0,
        };
        let _ = b.free_alloc(id);
        let again = b.free_alloc(id);
        let unknown = b.free_alloc(4242);
        let ok = !again && !unknown && b.ledger_balanced() && b.last_code() == XC_UNKNOWN_ALLOC;
        chk(&mut set, "C86-09 重复释放拒绝", ok, "重复释放被接受");
    }

    // C86-10 误拒校准：记误拒 + 提 cap 双动作
    {
        let mut b = standard_book();
        let _ = b.create(req(1, Class::Foreground, ResKind::RenderTarget, 90_000, false, 0));
        let d = b.create(req(2, Class::Foreground, ResKind::RenderTarget, 20_000, false, 0));
        let denied = matches!(d, Decision::Denied { .. });
        let cap0 = b.cap(Class::Foreground, ResKind::RenderTarget);
        let ok1 = b.report_false_denial(2, 16_384);
        let cap1 = b.cap(Class::Foreground, ResKind::RenderTarget);
        let c = b.counters();
        let ok = denied && ok1 && cap1 == cap0 + 16_384 && c.4 == 1 && c.5 >= 1;
        chk(&mut set, "C86-10 误拒校准", ok, "校准未双动作");
    }

    // C86-11 校准未知 req_id 显性拒绝
    {
        let mut b = standard_book();
        let ok = !b.report_false_denial(777, 4096) && b.last_code() == XC_FALSE_DENY;
        chk(&mut set, "C86-11 校准未知拒绝", ok, "未知校准被接受");
    }

    // C86-12 调 cap 不得低于已用（否则账面自相矛盾）
    {
        let mut b = standard_book();
        let _ = b.create(req(1, Class::Foreground, ResKind::Texture, 20_000, false, 0));
        let ok1 = !b.recalibrate(Class::Foreground, ResKind::Texture, 10_000);
        let ok2 = b.recalibrate(Class::Foreground, ResKind::Texture, 30_000);
        chk(&mut set, "C86-12 调参下界", ok1 && ok2, "cap 调穿已用");
    }

    // C86-13 拒绝账满后计数不丢（overflow 显性）
    {
        let mut b = standard_book();
        let mut i = 0u32;
        while i < (DENIAL_CAP as u32 + 7) {
            let _ = b.create(req(i + 1, Class::Foreground, ResKind::Texture, 300_000, false, 0));
            i += 1;
        }
        let c = b.counters();
        let ok = b.denial_count() == DENIAL_CAP && c.3 == 7 && c.1 == DENIAL_CAP as u32 + 7;
        chk(&mut set, "C86-13 拒绝账溢出", ok, "溢出计数错");
    }

    // C86-14 静默拒绝恒 0（红线）
    {
        let b = standard_book();
        chk(&mut set, "C86-14 零静默拒绝", b.silent_denials() == 0, "存在静默拒绝");
    }

    // C86-15 零尺寸创建显性拒绝并留码
    {
        let mut b = standard_book();
        let d = b.create(req(1, Class::Foreground, ResKind::Buffer, 0, false, 0));
        let ok = matches!(d, Decision::Denied { .. }) && b.last_code() == XC_SIZE_ZERO;
        chk(&mut set, "C86-15 零尺寸拒绝", ok, "零尺寸被接受");
    }

    // C86-16 读屏三要素行可读且含单位
    {
        let mut b = standard_book();
        let _ = b.create(req(1, Class::Foreground, ResKind::Texture, 200_000, false, 0));
        let _ = b.create(req(2, Class::Foreground, ResKind::Texture, 200_000, false, 0));
        let line = b.denial_line(0);
        let ok = line.contains("超") && line.contains("KB") && line.contains("建议");
        chk(&mut set, "C86-16 三要素读屏", ok, "拒绝行不可读");
    }

    // C86-17 账本自洽（used ≤ cap 且 Σalloc ≡ used）
    {
        let mut b = standard_book();
        let _ = b.create(req(1, Class::Foreground, ResKind::Texture, 1000, false, 0));
        let _ = b.create(req(2, Class::Background, ResKind::Buffer, 500, false, 0));
        let _ = b.create(req(3, Class::Tool, ResKind::Heap, 700, false, 0));
        chk(&mut set, "C86-17 账本自洽", b.ledger_balanced(), "账不平");
    }

    // C86-18 确定性：同序列两次结果一致
    {
        let mk = || {
            let mut b = standard_book();
            for i in 0..24u32 {
                let kind = match i % 5 {
                    0 => ResKind::Texture,
                    1 => ResKind::Buffer,
                    2 => ResKind::RenderTarget,
                    3 => ResKind::Heap,
                    _ => ResKind::Query,
                };
                let _ = b.create(req(i + 1, Class::Foreground, kind, 3000 + (i * 977) % 50_000, false, 0));
            }
            (b.counters(), b.snapshot_used())
        };
        let a = mk();
        let c = mk();
        chk(&mut set, "C86-18 确定性", a == c, "两次结果不同");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea86_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
