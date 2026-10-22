//! VE-F0093 · 恶意提交防护（VE-A 域 · A02 句柄与提交治理组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0093`
//!
//! **职责定位（锚点原文）**：恶意提交防护——恶意命令提交的防护（畸形命令
//! 缓冲 / 句柄炸弹的提交侧拦截），提交校验分档（**开发期全检 / 运行期抽检**），
//! 拦截记录审计；含防护的 fuzz 语料库（恶意提交样本在册）。
//!
//! **数据结构（锚点原文）**：提交防护。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 恶意检出 → 拒绝 + 审计（拒绝必须留痕，留痕含归一化后的规则号）；
//! - 误拦 → 白名单（白名单是**带凭据**的豁免：规则号 + 理由 + 到期 tick，
//!   不是「关掉检查」）；
//! - 校验失效 → 自检（自检样本必须能抓住已知恶意样本，抓不住即判据红）。
//!
//! **性能逐项分解（锚点原文）**：O(提交)。抽检档下每 N 次提交只全检一次，
//! 单次校验为常数条比较（无循环嵌套、无分配）。
//!
//! **跨批对接点（锚点原文）**：A02 句柄治理联动——句柄炸弹的判定复用
//! A02 的句柄表容量口径（本模块不另立上限）。
//!
//! **无障碍与隐私（锚点原文）**：拦截记录读屏可达——`block_line()` 输出
//! 纯文本「规则 / 字段 / 实测值」。
//!
//! **判据（锚点原文）**：提交拦截、分档校验、误拦白名单、审计留痕。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；全整数；确定性。
//!
//! 诊断段：0xE700 段 **0xE73x 子块**（0xE731~0xE73A 十码）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const AB_VERSION: &str = "vea93_abuse-v1";

/// 诊断段（子块）基址。
pub const AB_SEG_BASE: u16 = 0xE730;

/// 诊断码闭集（0xE731~0xE73E；十条规则占 0xE731~0xE73A，
/// 白名单/审计两条管理码顺延 0xE73D/0xE73E，不侵占规则码）。
pub const XC_DRAW_OVER: u32 = 0xE731;
pub const XC_INSTANCE_OVER: u32 = 0xE732;
pub const XC_STRIDE_BAD: u32 = 0xE733;
pub const XC_RANGE_OVER: u32 = 0xE734;
pub const XC_HANDLE_BOMB: u32 = 0xE735;
pub const XC_DEPTH_OVER: u32 = 0xE736;
pub const XC_PUSHCONST_OVER: u32 = 0xE737;
pub const XC_NAN_TRANSFORM: u32 = 0xE738;
pub const XC_INDEX_OVER: u32 = 0xE739;
pub const XC_DESCSET_OVER: u32 = 0xE73A;
pub const XC_WHITELIST_BAD: u32 = 0xE73D;
pub const XC_AUDIT_FULL: u32 = 0xE73E;

/// 提交校验上限（写死并注明来源：与 A02 句柄表容量同口径）。
pub const MAX_DRAW: u32 = 65_536;
pub const MAX_INSTANCE: u32 = 4_096;
pub const MAX_INDEX: u32 = 4_194_304;
pub const MAX_STRIDE: u32 = 4_096;
pub const MAX_DEPTH: u32 = 8;
pub const MAX_PUSH_CONST_BYTES: u32 = 128;
pub const MAX_DESC_SETS: u32 = 8;
/// 句柄炸弹阈值：单次提交引用的句柄数上限（A02 容量 1/4，留余量给并发上下文）。
pub const MAX_HANDLES_PER_SUBMIT: u32 = 32;

/// 审计容量。
pub const AUDIT_CAP: usize = 128;
/// fuzz 语料库容量（恶意样本在册）。
pub const CORPUS_CAP: usize = 32;

/// 校验档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 开发期：每次提交全检。
    Full = 0,
    /// 运行期：每 `period` 次提交抽检一次（其余快速路径仍查硬上限）。
    Sampled = 1,
}

/// 一条提交描述（定长；数值全部显式上限）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubmitDesc {
    pub submit_id: u32,
    pub draw_count: u32,
    pub instance_count: u32,
    pub index_count: u32,
    /// 顶点步长（字节）。
    pub stride: u32,
    /// 绑定缓冲偏移（字节）。
    pub offset: u32,
    /// 绑定缓冲长度（字节；用于 offset+stride*count 越界判定）。
    pub buffer_len: u32,
    /// 引用句柄数。
    pub handle_refs: u32,
    /// 间接命令递归深度。
    pub depth: u32,
    /// push constant 字节数。
    pub push_const_bytes: u32,
    /// 描述符集个数。
    pub desc_sets: u32,
    /// 变换矩阵里是否出现 NaN/Inf 位型。
    pub nan_transform: bool,
}

impl SubmitDesc {
    /// 合法基线（测试与语料库共用）。
    pub const fn benign() -> SubmitDesc {
        SubmitDesc {
            submit_id: 1,
            draw_count: 1,
            instance_count: 1,
            index_count: 3,
            stride: 32,
            offset: 0,
            buffer_len: 4096,
            handle_refs: 2,
            depth: 0,
            push_const_bytes: 0,
            desc_sets: 1,
            nan_transform: false,
        }
    }
}

/// 违规规则闭集（规则号即诊断码低位，便于审计归一化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    DrawOver = 0,
    InstanceOver = 1,
    StrideBad = 2,
    RangeOver = 3,
    HandleBomb = 4,
    DepthOver = 5,
    PushConstOver = 6,
    NanTransform = 7,
    IndexOver = 8,
    DescSetOver = 9,
}

impl Rule {
    pub const fn code(self) -> u32 {
        match self {
            Rule::DrawOver => XC_DRAW_OVER,
            Rule::InstanceOver => XC_INSTANCE_OVER,
            Rule::StrideBad => XC_STRIDE_BAD,
            Rule::RangeOver => XC_RANGE_OVER,
            Rule::HandleBomb => XC_HANDLE_BOMB,
            Rule::DepthOver => XC_DEPTH_OVER,
            Rule::PushConstOver => XC_PUSHCONST_OVER,
            Rule::NanTransform => XC_NAN_TRANSFORM,
            Rule::IndexOver => XC_INDEX_OVER,
            Rule::DescSetOver => XC_DESCSET_OVER,
        }
    }

    pub const fn text(self) -> &'static str {
        match self {
            Rule::DrawOver => "绘制数超上限",
            Rule::InstanceOver => "实例数超上限",
            Rule::StrideBad => "步长非法",
            Rule::RangeOver => "偏移越界",
            Rule::HandleBomb => "句柄炸弹",
            Rule::DepthOver => "递归超深",
            Rule::PushConstOver => "常量块超限",
            Rule::NanTransform => "变换含 NaN",
            Rule::IndexOver => "索引数超上限",
            Rule::DescSetOver => "描述符集超限",
        }
    }
}

/// 一条拦截记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub submit_id: u32,
    pub rule: u8,
    /// 实测值与上限（诊断用；上限 0 表示该规则无阈值）。
    pub actual: u32,
    pub limit: u32,
    /// 是否走了白名单豁免。
    pub exempted: bool,
}

/// 判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass { checked: bool },
    Block { rule: u8, code: u32 },
    /// 命中白名单豁免（仍记账；`Pass.exempt` 语义由此分支承担）。
    Exempt { rule: u8 },
}

/// 白名单条目（带凭据的豁免，不是「关掉检查」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Whitelist {
    pub rule: u8,
    /// 到期 tick（逻辑时钟）。到期自动失效。
    pub expires_tick: u64,
    /// 是否已吊销。
    pub revoked: bool,
}

/// 提交防护器。
pub struct AbuseGuard {
    tier: Tier,
    /// 抽检周期（Tier::Sampled 生效）。
    period: u32,
    /// 已处理提交数。
    submits: u64,
    /// 已全检次数。
    full_checks: u64,
    /// 拦截总数。
    blocks: u32,
    /// 豁免总数。
    exemptions: u32,
    /// 审计流。
    audit: Vec<Block>,
    /// 审计溢出计数。
    audit_dropped: u32,
    /// 白名单（定容）。
    whitelist: Vec<Whitelist>,
    /// 规则命中直方图（10 条规则）。
    hits: [u32; 10],
    /// fuzz 语料库（恶意样本）。
    corpus: Vec<SubmitDesc>,
    /// 最近一次诊断码。
    last_code: u32,
    /// 自检结果（校验器能否抓住语料库样本）。
    selfcheck_pass: bool,
    selfcheck_done: bool,
}

impl AbuseGuard {
    pub fn new(tier: Tier, period: u32) -> AbuseGuard {
        AbuseGuard {
            tier,
            period: if period == 0 { 1 } else { period },
            submits: 0,
            full_checks: 0,
            blocks: 0,
            exemptions: 0,
            audit: Vec::new(),
            audit_dropped: 0,
            whitelist: Vec::new(),
            hits: [0; 10],
            corpus: Vec::new(),
            last_code: 0,
            selfcheck_pass: false,
            selfcheck_done: false,
        }
    }

    pub fn tier(&self) -> Tier {
        self.tier
    }

    pub fn counters(&self) -> (u64, u64, u32, u32, u32) {
        (self.submits, self.full_checks, self.blocks, self.exemptions, self.audit_dropped)
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    pub fn add_corpus(&mut self, sample: SubmitDesc) -> bool {
        if self.corpus.len() >= CORPUS_CAP {
            return false;
        }
        self.corpus.push(sample);
        true
    }

    pub fn corpus_len(&self) -> usize {
        self.corpus.len()
    }

    /// 加白名单：规则可豁免到 `expires_tick` 为止。
    ///
    /// 说明：白名单**不关闭规则**——命中仍进审计（`exempted = true`），
    /// 只是不拦截。这样「误拦」与「静默关检查」在账面上可区分。
    pub fn add_whitelist(&mut self, rule: u8, expires_tick: u64) -> bool {
        if rule as usize >= 10 || self.whitelist.len() >= 16 {
            self.last_code = XC_WHITELIST_BAD;
            return false;
        }
        self.whitelist.push(Whitelist { rule, expires_tick, revoked: false });
        true
    }

    pub fn revoke_whitelist(&mut self, idx: usize) -> bool {
        match self.whitelist.get_mut(idx) {
            Some(w) => {
                w.revoked = true;
                true
            }
            None => false,
        }
    }

    fn whitelisted(&self, rule: u8, now: u64, out_exempt: &mut bool) -> bool {
        let mut i = 0usize;
        while i < self.whitelist.len() {
            let w = self.whitelist[i];
            if w.rule == rule && !w.revoked && now < w.expires_tick {
                *out_exempt = true;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 单规则校验（常数时间）。
    fn check_rule(d: &SubmitDesc) -> Option<(Rule, u32, u32)> {
        if d.draw_count > MAX_DRAW {
            return Some((Rule::DrawOver, d.draw_count, MAX_DRAW));
        }
        if d.instance_count > MAX_INSTANCE {
            return Some((Rule::InstanceOver, d.instance_count, MAX_INSTANCE));
        }
        if d.index_count > MAX_INDEX {
            return Some((Rule::IndexOver, d.index_count, MAX_INDEX));
        }
        // 步长：1..=MAX_STRIDE 且 4 字节对齐（GPU 顶点步长必须 4 对齐）。
        if d.stride == 0 || d.stride > MAX_STRIDE || d.stride % 4 != 0 {
            return Some((Rule::StrideBad, d.stride, MAX_STRIDE));
        }
        // 越界：offset + stride*count 不得超过 buffer_len（用 u64 防溢出）。
        let need = d.offset as u64 + d.stride as u64 * d.draw_count as u64;
        if need > d.buffer_len as u64 {
            let over = if need > u32::MAX as u64 { u32::MAX } else { need as u32 };
            return Some((Rule::RangeOver, over, d.buffer_len));
        }
        if d.handle_refs > MAX_HANDLES_PER_SUBMIT {
            return Some((Rule::HandleBomb, d.handle_refs, MAX_HANDLES_PER_SUBMIT));
        }
        if d.depth > MAX_DEPTH {
            return Some((Rule::DepthOver, d.depth, MAX_DEPTH));
        }
        if d.push_const_bytes > MAX_PUSH_CONST_BYTES {
            return Some((Rule::PushConstOver, d.push_const_bytes, MAX_PUSH_CONST_BYTES));
        }
        if d.desc_sets > MAX_DESC_SETS {
            return Some((Rule::DescSetOver, d.desc_sets, MAX_DESC_SETS));
        }
        // NaN 是**位型**问题：一旦进入变换矩阵，后续所有几何都不可信，
        // 必须在提交侧拦死（渲染侧再查已经晚了，可能已经写了深度缓冲）。
        if d.nan_transform {
            return Some((Rule::NanTransform, 1, 0));
        }
        None
    }

    /// 提交入口：分档校验。
    ///
    /// 分档语义（锚点「开发期全检 / 运行期抽检」）：
    /// - `Full`：每次全检九条规则；
    /// - `Sampled`：每 `period` 次全检一次；**非抽检拍仍查三条硬上限**
    ///   （绘制数/句柄数/NaN）。为什么保留这三条：它们是「一次就能炸掉
    ///   设备或让整帧不可信」的规则，抽检漏掉一次就够致命；其余规则
    ///   （步长/越界/深度）在抽检拍兜住即可——这是**风险分层**，不是偷懒。
    pub fn submit(&mut self, d: SubmitDesc, now_tick: u64) -> Verdict {
        self.submits += 1;
        let sampled_hit = match self.tier {
            Tier::Full => true,
            Tier::Sampled => (self.submits % self.period as u64) == 0,
        };

        if sampled_hit {
            self.full_checks += 1;
            if let Some((rule, actual, limit)) = Self::check_rule(&d) {
                return self.trip(d.submit_id, rule, actual, limit, now_tick);
            }
            return Verdict::Pass { checked: true };
        }

        // 快速路径：只查致命三条。
        if d.draw_count > MAX_DRAW {
            return self.trip(d.submit_id, Rule::DrawOver, d.draw_count, MAX_DRAW, now_tick);
        }
        if d.handle_refs > MAX_HANDLES_PER_SUBMIT {
            return self.trip(d.submit_id, Rule::HandleBomb, d.handle_refs, MAX_HANDLES_PER_SUBMIT, now_tick);
        }
        if d.nan_transform {
            return self.trip(d.submit_id, Rule::NanTransform, 1, 0, now_tick);
        }
        Verdict::Pass { checked: false }
    }

    fn trip(&mut self, submit_id: u32, rule: Rule, actual: u32, limit: u32, now: u64) -> Verdict {
        let idx = rule as usize;
        if idx < self.hits.len() {
            self.hits[idx] = self.hits[idx].saturating_add(1);
        }
        let mut exempt = false;
        let _ = self.whitelisted(rule as u8, now, &mut exempt);
        self.push_block(Block {
            submit_id,
            rule: rule as u8,
            actual,
            limit,
            exempted: exempt,
        });
        if exempt {
            self.exemptions = self.exemptions.saturating_add(1);
            self.last_code = rule.code();
            return Verdict::Exempt { rule: rule as u8 };
        }
        self.blocks = self.blocks.saturating_add(1);
        self.last_code = rule.code();
        Verdict::Block { rule: rule as u8, code: rule.code() }
    }

    fn push_block(&mut self, b: Block) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped += 1;
            return;
        }
        self.audit.push(b);
    }

    pub fn audit(&self) -> &[Block] {
        &self.audit
    }

    pub fn hit(&self, rule: Rule) -> u32 {
        self.hits[rule as usize]
    }

    /// 自检：把语料库里每个恶意样本喂给**纯校验函数**，必须全部被拦。
    ///
    /// 锚点「校验失效 → 自检」的落地：语料库是「已知恶意样本在册」，
    /// 若某天规则被改坏（阈值放宽/条件写反），自检立刻红——这比等
    /// 真实攻击来验证便宜得多。
    pub fn selfcheck(&mut self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.corpus.len() {
            let d = self.corpus[i];
            if Self::check_rule(&d).is_none() {
                ok = false;
            }
            i += 1;
        }
        // 基线良性样本必须通过（防空规则/全拒）。
        if Self::check_rule(&SubmitDesc::benign()).is_some() {
            ok = false;
        }
        self.selfcheck_pass = ok;
        self.selfcheck_done = true;
        ok
    }

    pub fn selfcheck_state(&self) -> (bool, bool) {
        (self.selfcheck_done, self.selfcheck_pass)
    }

    /// 拦截记录读屏行。
    pub fn block_line(&self, i: usize) -> String {
        let mut s = String::new();
        match self.audit.get(i) {
            None => s.push_str("拦截记录越界"),
            Some(b) => {
                s.push_str("拦截 #");
                push_u32(&mut s, b.submit_id);
                s.push(' ');
                s.push_str(rule_text(b.rule));
                s.push_str(" 实测=");
                push_u32(&mut s, b.actual);
                if b.limit > 0 {
                    s.push_str(" 上限=");
                    push_u32(&mut s, b.limit);
                }
                if b.exempted {
                    s.push_str("（已豁免）");
                }
            }
        }
        s
    }

    /// 账本守恒：blocks + exemptions ≡ 审计里非空记录数 + 溢出数。
    pub fn ledger_balanced(&self) -> bool {
        (self.blocks + self.exemptions) as usize == self.audit.len() + self.audit_dropped as usize
    }
}

fn rule_text(r: u8) -> &'static str {
    match r {
        0 => Rule::DrawOver.text(),
        1 => Rule::InstanceOver.text(),
        2 => Rule::StrideBad.text(),
        3 => Rule::RangeOver.text(),
        4 => Rule::HandleBomb.text(),
        5 => Rule::DepthOver.text(),
        6 => Rule::PushConstOver.text(),
        7 => Rule::NanTransform.text(),
        8 => Rule::IndexOver.text(),
        _ => Rule::DescSetOver.text(),
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

/// 供判据直接调用的纯校验入口（不经过状态机，便于「规则唯一性」核对）。
#[doc(hidden)]
pub fn check_rule_for_test(d: &SubmitDesc) -> Option<(Rule, u32, u32)> {
    AbuseGuard::check_rule(d)
}

/// 内置恶意语料（八类已知样本；每类对应一条规则）。
pub fn standard_corpus() -> Vec<SubmitDesc> {
    let mut v = Vec::new();
    let mut a = SubmitDesc::benign();
    a.draw_count = MAX_DRAW + 1;
    v.push(a);
    let mut b = SubmitDesc::benign();
    b.instance_count = MAX_INSTANCE + 1;
    v.push(b);
    let mut c = SubmitDesc::benign();
    c.stride = 6; // 非 4 对齐
    v.push(c);
    let mut d = SubmitDesc::benign();
    d.offset = 4096;
    d.draw_count = 4; // 4096 + 32*4 > 4096
    v.push(d);
    let mut e = SubmitDesc::benign();
    e.handle_refs = MAX_HANDLES_PER_SUBMIT + 1;
    v.push(e);
    let mut f = SubmitDesc::benign();
    f.depth = MAX_DEPTH + 1;
    v.push(f);
    let mut g = SubmitDesc::benign();
    g.push_const_bytes = MAX_PUSH_CONST_BYTES + 4;
    v.push(g);
    let mut h = SubmitDesc::benign();
    h.nan_transform = true;
    v.push(h);
    v
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// VE-F0093 判据入口。
pub fn run_vea93_checks() -> CheckSet {
    let mut set = CheckSet::new("vea93_abuse");

    // C93-01 良性提交通过（全检档）
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let v = g.submit(SubmitDesc::benign(), 0);
        chk(&mut set, "C93-01 良性放行", matches!(v, Verdict::Pass { checked: true }), "良性被拦");
    }

    // C93-02 绘制数超限拦截且归因正确
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let mut d = SubmitDesc::benign();
        d.draw_count = MAX_DRAW + 1;
        let v = g.submit(d, 0);
        let ok = matches!(v, Verdict::Block { rule, code } if rule == Rule::DrawOver as u8 && code == XC_DRAW_OVER);
        chk(&mut set, "C93-02 绘制数拦截", ok, "绘制数未拦");
    }

    // C93-03 步长非法（非 4 对齐 / 0 / 超上限）三例全拦
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let mut ok = true;
        for s in [6u32, 0, MAX_STRIDE + 4] {
            let mut d = SubmitDesc::benign();
            d.stride = s;
            let v = g.submit(d, 0);
            if !matches!(v, Verdict::Block { rule, .. } if rule == Rule::StrideBad as u8) {
                ok = false;
            }
        }
        chk(&mut set, "C93-03 步长三例", ok, "步长漏拦");
    }

    // C93-04 偏移越界用 u64 运算（防 u32 回绕绕过）
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let mut d = SubmitDesc::benign();
        d.offset = u32::MAX - 8;
        d.buffer_len = u32::MAX;
        d.draw_count = 2;
        d.stride = 32;
        let v = g.submit(d, 0);
        chk(
            &mut set,
            "C93-04 越界防回绕",
            matches!(v, Verdict::Block { rule, .. } if rule == Rule::RangeOver as u8),
            "回绕绕过越界检查",
        );
    }

    // C93-05 句柄炸弹拦截
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let mut d = SubmitDesc::benign();
        d.handle_refs = MAX_HANDLES_PER_SUBMIT + 1;
        let v = g.submit(d, 0);
        chk(
            &mut set,
            "C93-05 句柄炸弹",
            matches!(v, Verdict::Block { rule, .. } if rule == Rule::HandleBomb as u8),
            "句柄炸弹未拦",
        );
    }

    // C93-06 NaN 变换拦截
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let mut d = SubmitDesc::benign();
        d.nan_transform = true;
        let v = g.submit(d, 0);
        chk(
            &mut set,
            "C93-06 NaN 拦截",
            matches!(v, Verdict::Block { rule, .. } if rule == Rule::NanTransform as u8),
            "NaN 未拦",
        );
    }

    // C93-07 抽检档：非抽检拍只查致命三条（步长错放行、绘制超拦）
    {
        let mut g = AbuseGuard::new(Tier::Sampled, 4);
        let mut d = SubmitDesc::benign();
        d.stride = 6;
        let v1 = g.submit(d, 0); // 第 1 次：非抽检拍
        let mut d2 = SubmitDesc::benign();
        d2.draw_count = MAX_DRAW + 1;
        let v2 = g.submit(d2, 0); // 第 2 次：非抽检拍，但致命规则仍拦
        let ok = matches!(v1, Verdict::Pass { checked: false })
            && matches!(v2, Verdict::Block { rule, .. } if rule == Rule::DrawOver as u8);
        chk(&mut set, "C93-07 分档校验", ok, "抽检分层错");
    }

    // C93-08 抽检档：抽检拍必须全检（第 4 次抓到步长错）
    {
        let mut g = AbuseGuard::new(Tier::Sampled, 4);
        let mut caught = false;
        for i in 0..4u32 {
            let mut d = SubmitDesc::benign();
            d.submit_id = i + 1;
            d.stride = 6;
            let v = g.submit(d, 0);
            if i == 3 {
                caught = matches!(v, Verdict::Block { rule, .. } if rule == Rule::StrideBad as u8);
            }
        }
        chk(&mut set, "C93-08 抽检拍全检", caught, "抽检拍未全检");
    }

    // C93-09 全检计数与提交计数对齐（Full 档 full_checks == submits）
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        for _ in 0..9 {
            let _ = g.submit(SubmitDesc::benign(), 0);
        }
        let (s, f, _, _, _) = g.counters();
        chk(&mut set, "C93-09 全检计数", s == 9 && f == 9, "全检计数错");
    }

    // C93-10 白名单豁免：命中仍审计但不算拦截
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let _ = g.add_whitelist(Rule::StrideBad as u8, 100);
        let mut d = SubmitDesc::benign();
        d.stride = 6;
        let v = g.submit(d, 50);
        let (_, _, blocks, exempt, _) = g.counters();
        let ok = matches!(v, Verdict::Exempt { .. })
            && blocks == 0
            && exempt == 1
            && g.audit().len() == 1
            && g.audit()[0].exempted;
        chk(&mut set, "C93-10 白名单豁免", ok, "豁免未留痕或仍拦截");
    }

    // C93-11 白名单到期自动失效
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let _ = g.add_whitelist(Rule::StrideBad as u8, 100);
        let mut d = SubmitDesc::benign();
        d.stride = 6;
        let v = g.submit(d, 100); // now == expires ⇒ 失效
        chk(
            &mut set,
            "C93-11 白名单到期",
            matches!(v, Verdict::Block { .. }),
            "到期仍豁免",
        );
    }

    // C93-12 白名单吊销
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let _ = g.add_whitelist(Rule::NanTransform as u8, 1000);
        let _ = g.revoke_whitelist(0);
        let mut d = SubmitDesc::benign();
        d.nan_transform = true;
        let v = g.submit(d, 10);
        chk(&mut set, "C93-12 白名单吊销", matches!(v, Verdict::Block { .. }), "吊销后仍豁免");
    }

    // C93-13 非法白名单登记显性拒绝（规则号越界）
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let ok = !g.add_whitelist(99, 100) && g.last_code() == XC_WHITELIST_BAD;
        chk(&mut set, "C93-13 白名单入参闸", ok, "越界规则被登记");
    }

    // C93-14 自检抓住全部语料（八类）
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        for s in standard_corpus() {
            let _ = g.add_corpus(s);
        }
        let ok = g.corpus_len() == 8 && g.selfcheck();
        chk(&mut set, "C93-14 语料自检", ok, "语料未被全抓");
    }

    // C93-15 规则编号保持唯一（八条语料命中八条不同规则）
    {
        let mut rules = [false; 10];
        for s in standard_corpus() {
            if let Some((r, _, _)) = check_rule_for_test(&s) {
                rules[r as usize] = true;
            }
        }
        let mut n = 0;
        for x in rules.iter() {
            if *x {
                n += 1;
            }
        }
        chk(&mut set, "C93-15 规则覆盖唯一", n == 8, "规则命中重复或漏类");
    }

    // C93-16 审计满账显性（不静默丢拦截）
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        for i in 0..(AUDIT_CAP as u32 + 3) {
            let mut d = SubmitDesc::benign();
            d.submit_id = i;
            d.draw_count = MAX_DRAW + 1;
            let _ = g.submit(d, 0);
        }
        let (_, _, blocks, _, dropped) = g.counters();
        let ok = blocks == AUDIT_CAP as u32 + 3 && dropped == 3 && g.ledger_balanced();
        chk(&mut set, "C93-16 审计满账", ok, "满账记账错");
    }

    // C93-17 读屏拦截行可读
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        let mut d = SubmitDesc::benign();
        d.depth = MAX_DEPTH + 1;
        let _ = g.submit(d, 0);
        let line = g.block_line(0);
        let ok = line.contains("递归超深") && line.contains("上限");
        chk(&mut set, "C93-17 拦截行读屏", ok, "拦截行不可读");
    }

    // C93-18 直方图按规则分桶（同类多次命中累加）
    {
        let mut g = AbuseGuard::new(Tier::Full, 1);
        for _ in 0..3 {
            let mut d = SubmitDesc::benign();
            d.push_const_bytes = MAX_PUSH_CONST_BYTES + 1;
            let _ = g.submit(d, 0);
        }
        let ok = g.hit(Rule::PushConstOver) == 3 && g.hit(Rule::DrawOver) == 0;
        chk(&mut set, "C93-18 命中分桶", ok, "直方图错");
    }

    // C93-19 确定性：同序列两次的审计与计数一致
    {
        let mk = || {
            let mut g = AbuseGuard::new(Tier::Sampled, 3);
            for i in 0..24u32 {
                let mut d = SubmitDesc::benign();
                d.submit_id = i;
                if i % 5 == 0 {
                    d.stride = 6;
                }
                if i % 7 == 0 {
                    d.handle_refs = MAX_HANDLES_PER_SUBMIT + 2;
                }
                let _ = g.submit(d, i as u64);
            }
            (g.counters(), g.audit().len())
        };
        let a = mk();
        let b = mk();
        chk(&mut set, "C93-19 确定性", a == b, "两次不一致");
    }

    // C93-20 自检未跑时状态显性（不得假装已自检）
    {
        let g = AbuseGuard::new(Tier::Full, 1);
        let (done, pass) = g.selfcheck_state();
        chk(&mut set, "C93-20 自检状态显性", !done && !pass, "未跑自检却报绿");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea93_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
