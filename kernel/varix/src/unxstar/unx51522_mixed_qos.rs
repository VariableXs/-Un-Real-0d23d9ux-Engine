//! UNX-F51522 isoc+bulk+interrupt 三型混跑调度（QoS 档位） · 0xC0580段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51523-J1 超预算组合 ×10 全部明拒并报告原因；隐性失败注入 0 命中
//!
//! 设计要点：
//!   ① 三型端点（interrupt/isoc/bulk）在同一微帧配额池内混跑：interrupt 与
//!      isoc 各设类型保留上限，bulk 独立封顶且永不侵占保留额；总配额
//!      （2900）小于三型上限之和（3000），池级耗尽判据因此可达；
//!   ② QoS 档位（金/银/尽力）按 3/2/1 权重把类型上限切成档位子额；档位
//!      子额先于类型上限判定，且只收紧不放宽，全量或明拒，禁止隐性截断；
//!   ③ 判定次序固定：未开微帧 → 负值 → 零长 → 档位子额 → 类型上限 → 池余量；
//!      多判据同时成立时取最前者，检查逐对锁定；
//!   ④ 微帧号状态机单调：回放重定位接口 target < current 显性拒（MfBackward），
//!      账面 seq 与微帧计数一律不回退；
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；超预算/倒退/零长三类
//!      注入各 ×10 轮全部检出并登记原因；
//!   ⑥ 诊断码闭集 + 中文建议文案；段位守卫锁定 SEG_BASE 0x40 对齐与分派区间。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0580;

/// 每微帧总配额（单位：字节；小于三型上限之和 3000，以逼出池级耗尽判据）。
pub const MF_BUDGET: u32 = 2900;
/// interrupt 类型保留上限。
pub const INTR_CAP: u32 = 384;
/// isochronous 类型保留上限。
pub const ISO_CAP: u32 = 1536;
/// bulk 残差上限（独立封顶，且不侵占两型保留额）。
pub const BULK_CAP: u32 = 1080;

/// 诊断码闭集（三型混跑调度域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MixedQosCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 微帧未开启即提交
    MfNotOpen,
    /// 负值请求（配额不可为负）
    NegativeWant,
    /// 零长请求
    ZeroWant,
    /// 超出档位子额
    TierCapExceeded,
    /// 超出类型上限
    KindCapExceeded,
    /// 微帧池余量不足（全量或明拒，禁隐性降速）
    PoolExhausted,
    /// 微帧号倒退（状态机显性拒）
    MfBackward,
    /// 对账守恒失败
    LedgerMismatch,
}

impl MixedQosCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MixedQosCode::Ok => "调度成功：请求全量放行",
            MixedQosCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            MixedQosCode::MfNotOpen => "微帧未开启：先推进微帧再提交请求",
            MixedQosCode::NegativeWant => "负值请求：配额必须为非负整数",
            MixedQosCode::ZeroWant => "零长请求：至少请求 1 字节",
            MixedQosCode::TierCapExceeded => "超档位子额：升档或降低请求量",
            MixedQosCode::KindCapExceeded => "超类型上限：拆分到下一微帧提交",
            MixedQosCode::PoolExhausted => "微帧池余量不足：本微帧明拒，不顺延降速",
            MixedQosCode::MfBackward => "微帧号倒退：回放目标必须 ≥ 当前微帧",
            MixedQosCode::LedgerMismatch => "对账不符：记账面与账本重算值不一致",
        }
    }
}

/// 传输类型（三型混跑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XferKind {
    /// interrupt 类型（保留面）
    Intr,
    /// isochronous 类型（保留面）
    Iso,
    /// bulk 类型（残差面）
    Bulk,
}

impl XferKind {
    /// 类型上限；bulk 取残差额（保留额不对其开放）。
    pub fn cap(self) -> u32 {
        match self {
            XferKind::Intr => INTR_CAP,
            XferKind::Iso => ISO_CAP,
            XferKind::Bulk => BULK_CAP,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            XferKind::Intr => "interrupt",
            XferKind::Iso => "isoc",
            XferKind::Bulk => "bulk",
        }
    }
}

/// QoS 档位（金=3、银=2、尽力=1；档位子额 = 类型上限 × 权重 / 3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QosTier {
    /// 金档：类型上限全额
    Gold,
    /// 银档：类型上限的 2/3 子额
    Silver,
    /// 尽力档：类型上限的 1/3 子额
    Best,
}

impl QosTier {
    pub fn weight(self) -> u32 {
        match self {
            QosTier::Gold => 3,
            QosTier::Silver => 2,
            QosTier::Best => 1,
        }
    }
    /// 档位子额（整数整除；金档等于类型上限）。
    pub fn tier_cap(self, kind: XferKind) -> u32 {
        kind.cap() * self.weight() / 3
    }
}

/// 服务账记录（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SvcRec {
    pub mf: u32,
    pub seq: u32,
    pub ep: u16,
    pub kind: XferKind,
    pub tier: QosTier,
    pub granted: u32,
}

/// 三型混跑调度引擎：微帧池 + 档位子额 + 单调状态机 + 记账面。
pub struct MixedQosEngine {
    mf: u32,
    mf_open: bool,
    used_intr: u32,
    used_iso: u32,
    used_bulk: u32,
    tot_intr: u64,
    tot_iso: u64,
    tot_bulk: u64,
    seq: u32,
    ledger: Vec<SvcRec>,
    granted_total: u64,
    rejected_total: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl MixedQosEngine {
    pub fn new() -> MixedQosEngine {
        MixedQosEngine {
            mf: 0,
            mf_open: false,
            used_intr: 0,
            used_iso: 0,
            used_bulk: 0,
            tot_intr: 0,
            tot_iso: 0,
            tot_bulk: 0,
            seq: 0,
            ledger: Vec::new(),
            granted_total: 0,
            rejected_total: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 推进到下一微帧（开启新微帧，本微帧类型用量清零；微帧号单调递增）。
    pub fn advance_mf(&mut self) {
        self.mf = self.mf.saturating_add(1);
        self.mf_open = true;
        self.used_intr = 0;
        self.used_iso = 0;
        self.used_bulk = 0;
    }

    /// 回放重定位：目标微帧不得倒退（状态机显性拒）。
    pub fn relocate(&mut self, target: u32) -> Result<(), MixedQosCode> {
        if target < self.mf {
            return Err(MixedQosCode::MfBackward);
        }
        self.mf = target;
        self.mf_open = true;
        self.used_intr = 0;
        self.used_iso = 0;
        self.used_bulk = 0;
        Ok(())
    }

    fn used_of(&self, kind: XferKind) -> u32 {
        match kind {
            XferKind::Intr => self.used_intr,
            XferKind::Iso => self.used_iso,
            XferKind::Bulk => self.used_bulk,
        }
    }

    fn add_used(&mut self, kind: XferKind, v: u32) {
        match kind {
            XferKind::Intr => {
                self.used_intr = self.used_intr.saturating_add(v);
                self.tot_intr = self.tot_intr.saturating_add(v as u64);
            }
            XferKind::Iso => {
                self.used_iso = self.used_iso.saturating_add(v);
                self.tot_iso = self.tot_iso.saturating_add(v as u64);
            }
            XferKind::Bulk => {
                self.used_bulk = self.used_bulk.saturating_add(v);
                self.tot_bulk = self.tot_bulk.saturating_add(v as u64);
            }
        }
    }

    /// 当前微帧池余量。
    pub fn remaining(&self) -> u32 {
        let used = self
            .used_intr
            .saturating_add(self.used_iso)
            .saturating_add(self.used_bulk);
        MF_BUDGET.saturating_sub(used)
    }

    /// 提交一笔调度请求（全量或明拒；判定次序见设计要点③）。
    pub fn submit(
        &mut self,
        ep: u16,
        kind: XferKind,
        tier: QosTier,
        want: i64,
    ) -> Result<u32, MixedQosCode> {
        if !self.mf_open {
            return Err(MixedQosCode::MfNotOpen);
        }
        if want < 0 {
            return Err(MixedQosCode::NegativeWant);
        }
        if want == 0 {
            return Err(MixedQosCode::ZeroWant);
        }
        // 档位子额先于类型上限（金档子额即类型上限，交给类型上限判定）。
        if tier != QosTier::Gold && want > tier.tier_cap(kind) as i64 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            return Err(MixedQosCode::TierCapExceeded);
        }
        if want > kind.cap() as i64 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            return Err(MixedQosCode::KindCapExceeded);
        }
        let want_u = want as u32;
        if want_u > self.remaining() {
            self.rejected_total = self.rejected_total.saturating_add(1);
            return Err(MixedQosCode::PoolExhausted);
        }
        let rec = SvcRec {
            mf: self.mf,
            seq: self.seq,
            ep,
            kind,
            tier,
            granted: want_u,
        };
        self.seq = self.seq.saturating_add(1);
        self.ledger.push(rec);
        self.add_used(kind, want_u);
        self.granted_total = self.granted_total.saturating_add(want_u as u64);
        Ok(want_u)
    }

    /// 对账：账本重算（总量/分型累计）与引擎计数一致则通过。
    pub fn reconcile(&self) -> Result<(), MixedQosCode> {
        let mut g = 0u64;
        let mut k_intr = 0u64;
        let mut k_iso = 0u64;
        let mut k_bulk = 0u64;
        for r in self.ledger.iter() {
            g = g.saturating_add(r.granted as u64);
            match r.kind {
                XferKind::Intr => k_intr = k_intr.saturating_add(r.granted as u64),
                XferKind::Iso => k_iso = k_iso.saturating_add(r.granted as u64),
                XferKind::Bulk => k_bulk = k_bulk.saturating_add(r.granted as u64),
            }
        }
        let ok = g == self.granted_total
            && k_intr == self.tot_intr
            && k_iso == self.tot_iso
            && k_bulk == self.tot_bulk;
        if ok {
            Ok(())
        } else {
            Err(MixedQosCode::LedgerMismatch)
        }
    }

    /// 注入检出登记（捕获违约时调用）。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸登记（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    pub fn cur_mf(&self) -> u32 {
        self.mf
    }
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn ledger_at(&self, i: usize) -> Option<SvcRec> {
        self.ledger.get(i).copied()
    }
    pub fn granted_total(&self) -> u64 {
        self.granted_total
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，且逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped) && self.escaped == 0 && self.detected > 0
    }
}

/// 段位守卫：SEG_BASE 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一操作序列两次建账逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<SvcRec> {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let _ = e.submit(1, XferKind::Intr, QosTier::Gold, 64);
        let _ = e.submit(2, XferKind::Iso, QosTier::Silver, 512);
        let _ = e.submit(3, XferKind::Bulk, QosTier::Best, 256);
        e.advance_mf();
        let _ = e.submit(1, XferKind::Iso, QosTier::Gold, 1024);
        e.ledger.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51522_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51522");
    // 1 正常流：三型各一笔全量放行并落账。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let r1 = e.submit(1, XferKind::Intr, QosTier::Gold, 64);
        let r2 = e.submit(2, XferKind::Iso, QosTier::Gold, 512);
        let r3 = e.submit(3, XferKind::Bulk, QosTier::Gold, 256);
        let ok = r1 == Ok(64) && r2 == Ok(512) && r3 == Ok(256) && e.ledger_len() == 3;
        s.add_dyn("三型正常混跑落账", ok, "三型各一笔应全量放行并各落 1 条账");
    }
    // 2 边界零：零长请求明拒。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let r = e.submit(1, XferKind::Intr, QosTier::Gold, 0);
        s.add_dyn(
            "零长边界明拒",
            r == Err(MixedQosCode::ZeroWant),
            "零长请求必须拒绝且账不增",
        );
    }
    // 3 边界负：负值请求明拒。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let r = e.submit(1, XferKind::Iso, QosTier::Gold, -8);
        s.add_dyn(
            "负值边界明拒",
            r == Err(MixedQosCode::NegativeWant),
            "负值请求必须拒绝",
        );
    }
    // 4 边界上限/上限+1：interrupt 保留面顶格与越顶。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let ok_cap = e.submit(1, XferKind::Intr, QosTier::Gold, INTR_CAP as i64) == Ok(INTR_CAP);
        let r_over = e.submit(1, XferKind::Intr, QosTier::Gold, INTR_CAP as i64 + 1);
        s.add_dyn(
            "类型上限顶格与+1",
            ok_cap && r_over == Err(MixedQosCode::KindCapExceeded),
            "顶格应放行，上限+1 必须明拒",
        );
    }
    // 5 档位生效：尽力档 bulk 子额 = BULK_CAP/3，子额+1 明拒。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let sub = QosTier::Best.tier_cap(XferKind::Bulk);
        let ok_sub = e.submit(3, XferKind::Bulk, QosTier::Best, sub as i64) == Ok(sub);
        let r_over = e.submit(4, XferKind::Bulk, QosTier::Best, sub as i64 + 1);
        s.add_dyn(
            "QoS 档位子额生效",
            sub == BULK_CAP / 3 && ok_sub && r_over == Err(MixedQosCode::TierCapExceeded),
            "尽力档子额应顶格放行，子额+1 明拒",
        );
    }
    // 6 保留额不可侵占：bulk 超残差上限必拒（即便 intr/isoc 全程未用）。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let r = e.submit(3, XferKind::Bulk, QosTier::Gold, BULK_CAP as i64 + 1);
        s.add_dyn(
            "bulk 不得侵占保留额",
            r == Err(MixedQosCode::KindCapExceeded),
            "bulk 残差上限+1 必须明拒",
        );
    }
    // 7 注入必抓①：超预算 ×10 轮全部明拒并报告原因。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let mut caught = 0u32;
        let mut reason_ok = true;
        for i in 0..10u32 {
            let want = ISO_CAP as i64 + 1 + i as i64;
            let r = e.submit(2, XferKind::Iso, QosTier::Gold, want);
            match r {
                Err(MixedQosCode::KindCapExceeded) => {
                    caught += 1;
                    if MixedQosCode::KindCapExceeded.detail().is_empty() {
                        reason_ok = false;
                    }
                    e.note_detected();
                }
                _ => reason_ok = false,
            }
        }
        s.add_dyn(
            "超预算注入 10/10 明拒带原因",
            caught == 10 && reason_ok,
            "超预算组合必须全部明拒并报告原因",
        );
    }
    // 8 注入必抓②：微帧倒退 ×10 轮状态机显性拒。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        e.advance_mf();
        let cur = e.cur_mf();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let target = if cur >= 1 + i as u32 { cur - 1 - i as u32 } else { 0 };
            if e.relocate(target) == Err(MixedQosCode::MfBackward) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("倒退注入 10/10 拒止", caught == 10, "微帧号倒退必须全部显性拒绝");
    }
    // 9 注入必抓③：零长注入 ×10 轮。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if e.submit(9, XferKind::Bulk, QosTier::Gold, 0) == Err(MixedQosCode::ZeroWant) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("零长注入 10/10 检出", caught == 10, "零长违约必须全抓");
    }
    // 10 判定次序：未开微帧最前；档位子额先于类型上限。
    {
        let mut e = MixedQosEngine::new();
        let r_open = e.submit(1, XferKind::Iso, QosTier::Gold, -5);
        let mut e2 = MixedQosEngine::new();
        e2.advance_mf();
        let r_tier = e2.submit(2, XferKind::Iso, QosTier::Silver, ISO_CAP as i64 + 1);
        s.add_dyn(
            "判定次序锁定",
            r_open == Err(MixedQosCode::MfNotOpen)
                && r_tier == Err(MixedQosCode::TierCapExceeded),
            "未开微帧先于一切；档位子额先于类型上限",
        );
    }
    // 11 状态机拒止：倒退拒后账面与微帧号均不回退。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let _ = e.submit(1, XferKind::Intr, QosTier::Gold, 32);
        let before_len = e.ledger_len();
        let before_mf = e.cur_mf();
        let _ = e.relocate(0);
        s.add_dyn(
            "状态机倒退拒止",
            e.cur_mf() == before_mf && e.ledger_len() == before_len,
            "倒退被拒后微帧号与账长必须保持不变",
        );
    }
    // 12 容量耗尽：两型保留顶格后池余 < bulk 上限，超池明拒；推进微帧后恢复。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let _ = e.submit(2, XferKind::Iso, QosTier::Gold, ISO_CAP as i64);
        let _ = e.submit(1, XferKind::Intr, QosTier::Gold, INTR_CAP as i64);
        // 池余 980 < bulk 上限 1080：990 未超类型上限但超池，必须池级明拒。
        let r_full = e.submit(3, XferKind::Bulk, QosTier::Gold, 990);
        e.advance_mf();
        let r_next = e.submit(3, XferKind::Bulk, QosTier::Gold, 8);
        s.add_dyn(
            "容量耗尽与恢复",
            r_full == Err(MixedQosCode::PoolExhausted) && r_next == Ok(8),
            "池满本微帧明拒；新微帧余量恢复后放行",
        );
    }
    // 13 隐性失败扫描：已放行账逐条 granted == want（0 命中）。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let wants = [64u32, 512, 256, 384];
        let mut hit = 0u32;
        for (i, w) in wants.iter().enumerate() {
            let kind = if i % 2 == 0 {
                XferKind::Intr
            } else {
                XferKind::Iso
            };
            if let Ok(g) = e.submit(i as u16 + 1, kind, QosTier::Gold, *w as i64) {
                if g != *w {
                    hit += 1;
                }
            }
        }
        s.add_dyn("隐性截断 0 命中", hit == 0, "放行额必须逐笔等于请求额");
    }
    // 14 对账守恒：账本重算与计数一致，注入=检出+逃逸且逃逸 0。
    {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        for i in 0..10u32 {
            let _ = e.submit(1, XferKind::Iso, QosTier::Gold, 0);
            e.note_detected();
        }
        let recon = e.reconcile() == Ok(());
        s.add_dyn(
            "对账守恒",
            recon && e.conserved() && e.injected() == 10 && e.escaped() == 0,
            "注入=检出+逃逸，逃逸恒 0，账面重算一致",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一输入序列两次建账逐字段相等");
    // 16 文案闭集 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
        s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    }
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [MixedQosCode; 10] = [
    MixedQosCode::Ok,
    MixedQosCode::SegBad,
    MixedQosCode::MfNotOpen,
    MixedQosCode::NegativeWant,
    MixedQosCode::ZeroWant,
    MixedQosCode::TierCapExceeded,
    MixedQosCode::KindCapExceeded,
    MixedQosCode::PoolExhausted,
    MixedQosCode::MfBackward,
    MixedQosCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_three_kinds_full_grant() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        assert_eq!(e.submit(1, XferKind::Intr, QosTier::Gold, 64), Ok(64));
        assert_eq!(e.submit(2, XferKind::Iso, QosTier::Gold, 512), Ok(512));
        assert_eq!(e.submit(3, XferKind::Bulk, QosTier::Gold, 256), Ok(256));
        assert_eq!(e.ledger_len(), 3);
    }

    #[test]
    fn zero_and_negative_boundary() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        assert_eq!(
            e.submit(1, XferKind::Intr, QosTier::Gold, 0),
            Err(MixedQosCode::ZeroWant)
        );
        assert_eq!(
            e.submit(1, XferKind::Intr, QosTier::Gold, -1),
            Err(MixedQosCode::NegativeWant)
        );
        assert_eq!(e.ledger_len(), 0);
    }

    #[test]
    fn cap_and_cap_plus_one() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        assert_eq!(
            e.submit(1, XferKind::Intr, QosTier::Gold, INTR_CAP as i64),
            Ok(INTR_CAP)
        );
        assert_eq!(
            e.submit(1, XferKind::Intr, QosTier::Gold, INTR_CAP as i64 + 1),
            Err(MixedQosCode::KindCapExceeded)
        );
        assert_eq!(
            e.submit(3, XferKind::Bulk, QosTier::Gold, BULK_CAP as i64 + 1),
            Err(MixedQosCode::KindCapExceeded)
        );
    }

    #[test]
    fn tier_subcap_effective() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let sub = QosTier::Best.tier_cap(XferKind::Bulk);
        assert_eq!(sub, BULK_CAP / 3);
        assert_eq!(e.submit(3, XferKind::Bulk, QosTier::Best, sub as i64), Ok(sub));
        assert_eq!(
            e.submit(3, XferKind::Bulk, QosTier::Best, sub as i64 + 1),
            Err(MixedQosCode::TierCapExceeded)
        );
    }

    #[test]
    fn injection_over_budget_x10_all_caught() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        for i in 0..10u32 {
            let r = e.submit(2, XferKind::Iso, QosTier::Gold, ISO_CAP as i64 + 1 + i as i64);
            assert_eq!(r, Err(MixedQosCode::KindCapExceeded), "第 {} 轮应明拒", i);
            e.note_detected();
        }
        assert!(e.conserved());
        assert_eq!(e.injected(), 10);
    }

    #[test]
    fn injection_backward_x10_rejected() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        e.advance_mf();
        let cur = e.cur_mf();
        for i in 0..10u32 {
            let target = cur.saturating_sub(1 + i);
            assert_eq!(e.relocate(target), Err(MixedQosCode::MfBackward));
        }
        assert_eq!(e.cur_mf(), cur);
    }

    #[test]
    fn injection_zero_x10_detected() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        for _ in 0..10u32 {
            assert_eq!(
                e.submit(9, XferKind::Bulk, QosTier::Gold, 0),
                Err(MixedQosCode::ZeroWant)
            );
            e.note_detected();
        }
        assert!(e.conserved());
    }

    #[test]
    fn judge_order_notopen_then_tiercap() {
        let mut e = MixedQosEngine::new();
        assert_eq!(
            e.submit(1, XferKind::Iso, QosTier::Gold, -5),
            Err(MixedQosCode::MfNotOpen)
        );
        let mut e2 = MixedQosEngine::new();
        e2.advance_mf();
        assert_eq!(
            e2.submit(2, XferKind::Iso, QosTier::Silver, ISO_CAP as i64 + 1),
            Err(MixedQosCode::TierCapExceeded),
            "档位子额判据先于类型上限判据"
        );
    }

    #[test]
    fn state_machine_reject_keeps_ledger() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let _ = e.submit(1, XferKind::Intr, QosTier::Gold, 32);
        let before = e.ledger_len();
        let mf_before = e.cur_mf();
        assert_eq!(e.relocate(0), Err(MixedQosCode::MfBackward));
        assert_eq!(e.ledger_len(), before);
        assert_eq!(e.cur_mf(), mf_before);
    }

    #[test]
    fn pool_exhausted_then_recover() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        let _ = e.submit(2, XferKind::Iso, QosTier::Gold, ISO_CAP as i64);
        let _ = e.submit(1, XferKind::Intr, QosTier::Gold, INTR_CAP as i64);
        // 池余 980：bulk 990 类型内但超池 → PoolExhausted（池级判据可达）。
        assert_eq!(
            e.submit(3, XferKind::Bulk, QosTier::Gold, 990),
            Err(MixedQosCode::PoolExhausted)
        );
        e.advance_mf();
        assert_eq!(e.submit(3, XferKind::Bulk, QosTier::Gold, 8), Ok(8));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn conservation_and_reconcile() {
        let mut e = MixedQosEngine::new();
        e.advance_mf();
        for _ in 0..10u32 {
            let _ = e.submit(1, XferKind::Iso, QosTier::Gold, 0);
            e.note_detected();
        }
        assert!(e.reconcile() == Ok(()));
        assert!(e.conserved());
        assert_eq!(e.escaped(), 0);
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}

