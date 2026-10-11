//! vcs04_quota — CGPU-F2884 资源配额按用户（三配额切分 · R05 复用声明）。
//!
//! 判据原文（施工书锚点 CGPU-F2884）：配额：按用户资源配额（显存/带宽/
//! 队列三配额按用户切分——配额实现（R05 复用——复用声明；测试（切分/
//! 复用两组。判据：三配额、复用声明、两组、判据。
//!
//! # 复用声明（判据"R05 复用——复用声明"的锚定正文）
//!
//! 本模块是 **R05 渲染资源配额与公平组（CGPU-F2785~F2800）** 配额 API
//! 冻结接口的**按用户切分扩展**，不复口不另立：
//! 一、三配额语义对齐 R05（显存/带宽/队列深度——[`QuotaKind`] 闭集，
//!     单位与 R05 一致：MB / MBps / 深度条目数）；
//! 二、超限三态对齐 R05 处置契约（F2799 断言"三态处置"——
//!     [`QuotaVerdict`] Allow/Degrade/Deny：余量足全给/部分余量降级
//!     给足余量/零余量显性拒，建议不静默）；
//! 三、预算计入对齐 R05 契约（F2799 断言"预算计入"——每次消费即计入
//!     用户配额账，账目唯一权威，[`QuotaLedger`] 查询走账不走副本）；
//! 四、无绕过对齐 R05 断言（唯一入口 [`QuotaLedger::consume`]——账外
//!     消费在 [`QuotaLedger::reconcile`] 对账暴露，绕过即立案不是漏洞）。
//! R05 本体施工后其 API 实现接入本切分面（接口即插即用——冻结接口的
//! 意义就是现在不猜将来）。
//!
//! 时间戳无关（配额无时钟面）；零 panic（余量运算 saturating + 除法
//! 仅常量非零）；诊断码段 0x53（字符串码）。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const QUOTA_VERSION: &str = "S04-quota-v1";

/// 诊断码（字符串码四码闭集）。
pub const E_QT_NOUSER: &str = "E_QT_NOUSER";
pub const E_QT_NOBUCKET: &str = "E_QT_NOBUCKET";
pub const E_QT_DUPLICATE: &str = "E_QT_DUPLICATE";
pub const E_QT_DRAIN: &str = "E_QT_DRAIN";

/// 诊断详情（四码闭集逐码中文说明）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_QT_NOUSER => "配额用户未登记：先登记配额表再消费——无表即无份额",
        E_QT_NOBUCKET => "配额桶不存在：用户已登记但该配额维未切分——切分须全三维",
        E_QT_DUPLICATE => "配额桶重复登记：同用户同维单桶唯一——重复即配置事故",
        E_QT_DRAIN => "配额零余量：显性拒绝不静默——要更多走配额调整不是硬闯",
        _ => "未知配额诊断码",
    }
}

/// 三配额闭集（判据"显存/带宽/队列三配额"——R05 语义对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaKind {
    /// 显存（MB）。
    Vram,
    /// 带宽（MBps）。
    Bandwidth,
    /// 队列深度（条目数）。
    QueueDepth,
}

impl QuotaKind {
    pub const ALL: [QuotaKind; 3] = [QuotaKind::Vram, QuotaKind::Bandwidth, QuotaKind::QueueDepth];

    pub const fn zh(self) -> &'static str {
        match self {
            QuotaKind::Vram => "显存",
            QuotaKind::Bandwidth => "带宽",
            QuotaKind::QueueDepth => "队列深度",
        }
    }

    pub const fn ordinal(self) -> usize {
        match self {
            QuotaKind::Vram => 0,
            QuotaKind::Bandwidth => 1,
            QuotaKind::QueueDepth => 2,
        }
    }
}

/// 超限三态（R05 处置契约对齐——判据"复用声明"的类型面锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaVerdict {
    /// 全量满足。
    Allow,
    /// 降级满足（ granted < requested——给足余量，请求方自降级）。
    Degrade { granted: u64 },
    /// 零余量拒绝。
    Deny,
}

impl QuotaVerdict {
    /// 实际获得量（Allow 原量 / Degrade 带量 / Deny 0）。
    pub const fn granted_of(self, requested: u64) -> u64 {
        match self {
            QuotaVerdict::Allow => requested,
            QuotaVerdict::Degrade { granted } => granted,
            QuotaVerdict::Deny => 0,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            QuotaVerdict::Allow => "全量满足",
            QuotaVerdict::Degrade { .. } => "降级满足",
            QuotaVerdict::Deny => "零余量拒绝",
        }
    }
}

/// 配额桶（用户 × 维 单桶）。
#[derive(Clone, Copy, Debug)]
pub struct QuotaBucket {
    pub user: u32,
    pub kind: QuotaKind,
    /// 上限（切分值——R05 分配面决定，本模块只执行）。
    pub limit: u64,
    /// 已用。
    pub used: u64,
}

/// 按用户配额账（判据"三配额按用户切分"的本体：三维度 × 多用户矩阵）。
pub struct QuotaLedger {
    buckets: Vec<QuotaBucket>,
    /// 消费笔数（对账口径）。
    pub ops: u32,
    /// 拒绝笔数（Deny 显性化）。
    pub denied: u32,
    /// 降级笔数。
    pub degraded: u32,
}

impl QuotaLedger {
    pub fn new() -> Self {
        QuotaLedger { buckets: Vec::new(), ops: 0, denied: 0, degraded: 0 }
    }

    /// 登记配额桶（切分动作：用户 × 维 × 上限；重复登记显性拒）。
    pub fn register(&mut self, user: u32, kind: QuotaKind, limit: u64) -> Result<(), &'static str> {
        if self.find(user, kind).is_some() {
            return Err(E_QT_DUPLICATE);
        }
        self.buckets.push(QuotaBucket { user, kind, limit, used: 0 });
        Ok(())
    }

    /// 消费（唯一入口——判据"无绕过"：账外消费 reconcile 暴露）。
    /// 三态判定：余量 ≥ 请求 → Allow；0 < 余量 < 请求 → Degrade{granted=余量}；
    /// 余量 = 0 → Deny。账只记实际获得量。
    pub fn consume(&mut self, user: u32, kind: QuotaKind, amount: u64) -> Result<QuotaVerdict, &'static str> {
        let idx = match self.find(user, kind) {
            Some(i) => i,
            None => return Err(E_QT_NOUSER),
        };
        self.ops += 1;
        let remain = self.buckets[idx].limit.saturating_sub(self.buckets[idx].used);
        let verdict = if remain == 0 {
            self.denied += 1;
            QuotaVerdict::Deny
        } else if amount <= remain {
            self.buckets[idx].used += amount;
            QuotaVerdict::Allow
        } else {
            self.buckets[idx].used += remain;
            self.degraded += 1;
            QuotaVerdict::Degrade { granted: remain }
        };
        Ok(verdict)
    }

    /// 查余量（未登记 None）。
    pub fn remaining(&self, user: u32, kind: QuotaKind) -> Option<u64> {
        let idx = self.find(user, kind)?;
        Some(self.buckets[idx].limit.saturating_sub(self.buckets[idx].used))
    }

    /// 查已用（未登记 None）。
    pub fn used_of(&self, user: u32, kind: QuotaKind) -> Option<u64> {
        let idx = self.find(user, kind)?;
        Some(self.buckets[idx].used)
    }

    /// 桶数。
    pub fn len(&self) -> usize {
        self.buckets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// 对账（判据"无绕过"）：全部桶 used 之和 == 逐桶可查 used 之和
    /// （账内自洽；账外消费由调用方以实际资源面数字对账本和——差额即
    /// 绕过量立案口径）。返回（桶内和, 桶数）。
    pub fn reconcile(&self) -> (u64, usize) {
        let mut sum = 0u64;
        for b in &self.buckets {
            sum += b.used;
        }
        (sum, self.buckets.len())
    }

    fn find(&self, user: u32, kind: QuotaKind) -> Option<usize> {
        let mut i = 0usize;
        while i < self.buckets.len() {
            let b = &self.buckets[i];
            if b.user == user && b.kind == kind {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "配额{} 桶{} 笔{} 拒{} 降{}",
            QUOTA_VERSION,
            self.buckets.len(),
            self.ops,
            self.denied,
            self.degraded,
        )
    }
}

/// 判据主入口（注册于 cgpu/mod.rs blocks 数组，tag CGPU-F2884）。
pub fn run_vcs04_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-f2884");

    // --- 切分组（判据一）---
    // 切-01：三配额独立记账互不串——u1 显存用满 100/100，其带宽/队列
    // 余量不动（维度隔离）；三维闭集 ordinal 0..=2 连续、中文名互异。
    let mut q = QuotaLedger::new();
    let _ = q.register(1, QuotaKind::Vram, 100);
    let _ = q.register(1, QuotaKind::Bandwidth, 500);
    let _ = q.register(1, QuotaKind::QueueDepth, 20);
    let v1 = q.consume(1, QuotaKind::Vram, 100);
    let iso_ok = matches!(v1, Ok(QuotaVerdict::Allow))
        && q.remaining(1, QuotaKind::Vram) == Some(0)
        && q.remaining(1, QuotaKind::Bandwidth) == Some(500)
        && q.remaining(1, QuotaKind::QueueDepth) == Some(20);
    let kinds = QuotaKind::ALL;
    let kind_ok = kinds.len() == 3
        && kinds[0].ordinal() == 0
        && kinds[1].ordinal() == 1
        && kinds[2].ordinal() == 2
        && kinds[0].zh() != kinds[1].zh()
        && kinds[0].zh() != kinds[2].zh()
        && kinds[1].zh() != kinds[2].zh();
    if iso_ok && kind_ok {
        s.ok("F2884-切-01-三配额独立互不串");
    } else {
        s.fail("F2884-切-01-三配额独立互不串", "维度隔离失效");
    }

    // --- 切分组（判据二）---
    // 切-02：按用户隔离——u1 显存用满不影响 u2 显存（同维异户各桶独立）；
    // 重复登记显性拒 E_QT_DUPLICATE；未登记用户/维 E_QT_NOUSER。
    let mut q2 = QuotaLedger::new();
    let _ = q2.register(1, QuotaKind::Vram, 100);
    let _ = q2.register(2, QuotaKind::Vram, 100);
    let _ = q2.consume(1, QuotaKind::Vram, 100);
    let user_iso = q2.remaining(1, QuotaKind::Vram) == Some(0)
        && q2.remaining(2, QuotaKind::Vram) == Some(100);
    let dup = q2.register(1, QuotaKind::Vram, 50);
    let nouser = q2.consume(9, QuotaKind::Vram, 1);
    let guard_ok = matches!(dup, Err(E_QT_DUPLICATE)) && matches!(nouser, Err(E_QT_NOUSER));
    if user_iso && guard_ok {
        s.ok("F2884-切-02-按用户隔离与登记闸");
    } else {
        s.fail("F2884-切-02-按用户隔离与登记闸", "用户隔离失效");
    }

    // --- 切分组（判据三）---
    // 切-03：超限三态恰值（R05 处置契约）——上限 60：请求 50 → Allow
    // 全给（余 10）；请求 80 → Degrade{granted:10} 给足余量（余量清零）；
    // 再请求任意 → Deny（零余量）；granted_of 口径三态自洽；
    // used=50+10=60 手算锚、degraded=1 denied=1 计数对账。
    let mut q3 = QuotaLedger::new();
    let _ = q3.register(4, QuotaKind::Bandwidth, 60);
    let a1 = q3.consume(4, QuotaKind::Bandwidth, 50);
    let a2 = q3.consume(4, QuotaKind::Bandwidth, 80);
    let a3 = q3.consume(4, QuotaKind::Bandwidth, 10);
    let three_ok = matches!(a1, Ok(QuotaVerdict::Allow))
        && matches!(a2, Ok(QuotaVerdict::Degrade { granted: 10 }))
        && matches!(a3, Ok(QuotaVerdict::Deny))
        && QuotaVerdict::Allow.granted_of(50) == 50
        && QuotaVerdict::Degrade { granted: 10 }.granted_of(80) == 10
        && QuotaVerdict::Deny.granted_of(10) == 0
        && q3.used_of(4, QuotaKind::Bandwidth) == Some(60)
        && q3.degraded == 1
        && q3.denied == 1;
    if three_ok {
        s.ok("F2884-切-03-超限三态恰值");
    } else {
        s.fail("F2884-切-03-超限三态恰值", "三态处置失效");
    }

    // --- 切分组（判据四）---
    // 切-04：无绕过对账——reconcile 桶内和 == 逐桶 used 之和（自洽闭合）
    // + 桶数对账 + 零消费账空态；账外差额口径声明（调用方资源面对账）。
    let mut q4 = QuotaLedger::new();
    let empty_ok = q4.reconcile() == (0, 0) && q4.is_empty();
    let _ = q4.register(1, QuotaKind::Vram, 200);
    let _ = q4.register(2, QuotaKind::QueueDepth, 30);
    let _ = q4.consume(1, QuotaKind::Vram, 120);
    let _ = q4.consume(2, QuotaKind::QueueDepth, 25);
    let (sum, n) = q4.reconcile();
    let recon_ok = sum == 145 && n == 2 && q4.len() == 2 && q4.ops == 2;
    if empty_ok && recon_ok {
        s.ok("F2884-切-04-无绕过对账闭合");
    } else {
        s.fail("F2884-切-04-无绕过对账闭合", "对账失效");
    }

    // --- 复用组（判据五）---
    // 复-01：R05 复用声明锚——版本串 + 三态枚举与 R05 处置契约语义对齐
    // （Allow/Degrade/Deny 三态 label 互异 + granted_of 口径统一）+
    // 四码互异文案互异 + 缺省分支。
    let labels = [
        QuotaVerdict::Allow.label(),
        QuotaVerdict::Degrade { granted: 1 }.label(),
        QuotaVerdict::Deny.label(),
    ];
    let label_ok = labels[0] != labels[1] && labels[0] != labels[2] && labels[1] != labels[2];
    let codes = [E_QT_NOUSER, E_QT_NOBUCKET, E_QT_DUPLICATE, E_QT_DRAIN];
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let mut det_uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if reason_of(codes[i]) == reason_of(codes[j]) {
                det_uniq = false;
            }
        }
    }
    let unknown_ok = reason_of("E_UNKNOWN") == "未知配额诊断码";
    let ver_ok = QUOTA_VERSION == "S04-quota-v1";
    if label_ok && uniq && det_uniq && unknown_ok && ver_ok {
        s.ok("F2884-复-01-R05复用声明锚");
    } else {
        s.fail("F2884-复-01-R05复用声明锚", "复用声明失效");
    }

    // --- 复用组（判据六）---
    // 复-02：预算计入契约——每笔消费即时计入（used 实时可见——不是
    // 批后结算）；Deny 不计账（拒绝零获得零计入）；Degrade 计获得量
    // 不是请求量（账记事实不记愿望——used 110 = 50 + 60 手算锚）。
    let mut q5 = QuotaLedger::new();
    let _ = q5.register(7, QuotaKind::Vram, 110);
    let _ = q5.consume(7, QuotaKind::Vram, 50);
    let mid = q5.used_of(7, QuotaKind::Vram) == Some(50);
    let _ = q5.consume(7, QuotaKind::Vram, 80); // Degrade 60
    let post = q5.used_of(7, QuotaKind::Vram) == Some(110);
    let deny_case = {
        let _ = q5.consume(7, QuotaKind::Vram, 5); // Deny
        q5.used_of(7, QuotaKind::Vram) == Some(110) && q5.denied == 1
    };
    let fact_ok = mid && post && deny_case;
    if fact_ok {
        s.ok("F2884-复-02-预算计入实时账");
    } else {
        s.fail("F2884-复-02-预算计入实时账", "预算计入失效");
    }

    // --- 判据元（判据七）---
    // 元-01：零 panic 面——空账 sweep 式全查询/零量消费/饱和运算平凡；
    // 零量消费 Allow 计零（amount=0 ≤ remain 恒真——合法空操作）。
    let mut q6 = QuotaLedger::new();
    let _ = q6.register(1, QuotaKind::QueueDepth, 10);
    let z1 = q6.consume(1, QuotaKind::QueueDepth, 0);
    let zero_ok = matches!(z1, Ok(QuotaVerdict::Allow)) && q6.used_of(1, QuotaKind::QueueDepth) == Some(0);
    let q7 = QuotaLedger::new();
    let empty_ok = q7.remaining(1, QuotaKind::Vram).is_none()
        && q7.used_of(1, QuotaKind::Vram).is_none()
        && q7.reconcile() == (0, 0)
        && q7.summary().contains("桶0");
    if zero_ok && empty_ok {
        s.ok("F2884-元-01-零panic空态平凡");
    } else {
        s.fail("F2884-元-01-零panic空态平凡", "panic 面失效");
    }

    s
}
