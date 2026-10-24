//! cgi03_bwaccount — CGPU-F1283 带宽预算账户（PCIe 带宽的账户化治理）。
//!
//! 判据原文（施工书锚点 CGPU-F1283）：账户化：带宽预算账户（每传输类（上传/
//! 下载/交换/流送）一个账户——C 域账户体系复用声明）；账户优先级（呈现类>
//! 交互类>后台类）；账户记账（带宽消耗全量记账复用 F1122 理念）；账户守恒
//! （Σ账户≤PCIe 有效带宽——守恒校验）。判据：账户化、优先级、记账、守恒、
//! 判据。
//!
//! 职责定位：按传输类的带宽配给+记账+守恒+抢占留痕。有效带宽口径声明式复用
//! cgi02_pciebw（F1282）：理论带宽 × 有效系数（EFFICIENCY_DEFAULT_PPM）——
//! 本模块不重定义带宽物理口径，只做账户分配层。与 cgc04_framearbiter（帧内
//! 时间预算）分工——彼管每帧毫秒怎么分、此管链路 MB/s 怎么分。全量记账复用
//! F1122 理念（每笔留痕 seq 单调）。诊断码段 0x40xx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// 带宽物理口径：声明式复用 cgi02_pciebw（F1282）——不重定义，单一事实源。
use crate::cgpu::cgi02_pciebw::{
    effective_mbps, gen_theory_mbps, EFFICIENCY_DEFAULT_PPM, PcieGen,
};

/// 模块版本。
pub const BW_ACCOUNT_VERSION: &str = "I03-bwaccount-v1";

/// 占用预警线：账户已用 90% 即进入预警（预算修正建议触发线）。
pub const PREWARN_USED_PPM: u64 = 900_000;

/// 抢占线：呈现账户需水 > 其余量时，允许按优先级倒序抢占（后台先失）。
pub const PREEMPT_PPM: u64 = 950_000;

/// 传输类账户闭集（判据四类：上传/下载/交换/流送）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BwClass {
    /// 交换（呈现路径：显存↔帧缓冲，呈现类）。
    Swap,
    /// 上传（CPU→GPU 资源，交互类）。
    Upload,
    /// 下载（GPU→CPU 回读，交互类）。
    Download,
    /// 流送（后台纹理/资源流送，后台类）。
    Stream,
}

impl BwClass {
    pub const ALL: [BwClass; 4] = [
        BwClass::Swap,
        BwClass::Upload,
        BwClass::Download,
        BwClass::Stream,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            BwClass::Swap => 0,
            BwClass::Upload => 1,
            BwClass::Download => 2,
            BwClass::Stream => 3,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            BwClass::Swap => "交换(呈现)",
            BwClass::Upload => "上传(交互)",
            BwClass::Download => "下载(交互)",
            BwClass::Stream => "流送(后台)",
        }
    }

    /// 账户优先级（判据"呈现类>交互类>后台类"的闭集映射：
    /// Swap=3 > Upload=2 > Download=1 > Stream=0；同级内上传优先——
    /// 上传常是输入与资源就绪的关键路径）。
    pub const fn priority(self) -> u8 {
        match self {
            BwClass::Swap => 3,
            BwClass::Upload => 2,
            BwClass::Download => 1,
            BwClass::Stream => 0,
        }
    }
}

/// 诊断码（0x40xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4001 传输类闭集外。
    BwClassUnknown,
    /// 0x4002 账户容量和超有效带宽（Σcap > cap——构造拒，守恒前置）。
    BwCapOver,
    /// 0x4003 账户水位不足且无可抢占（申请显性拒绝——不静默压带宽）。
    BwExhausted,
    /// 0x4004 守恒破坏（Σused > Σcap——不变量核验失败立案）。
    BwConservBroke,
    /// 0x4005 后台账户被呈现抢占（正常机制但全量留痕）。
    BwPreempted,
    /// 0x4006 记账序断裂（seq 不单调——账本完整性破坏）。
    BwLedgerBroken,
    /// 0x4007 零带宽记账（0 MB/s 消耗=非法账目）。
    BwZeroCharge,
    /// 0x4008 带宽口径解析失败（代际×lane 组合非法——复用层透传）。
    BwLinkInvalid,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::BwClassUnknown => "传输类闭集外：只认 交换/上传/下载/流送 四类账户",
            XcCode::BwCapOver => "账户容量和超链路有效带宽：守恒前置破坏，构造拒绝",
            XcCode::BwExhausted => "账户水位不足且无可抢占：申请显性拒绝带归因",
            XcCode::BwConservBroke => "守恒破坏：账户用量和超出账户容量和",
            XcCode::BwPreempted => "后台账户被高优先级抢占：机制正常但全量留痕可回放",
            XcCode::BwLedgerBroken => "记账序断裂：账本 seq 必须单调",
            XcCode::BwZeroCharge => "零带宽记账非法：0 MB/s 不是一次真实传输",
            XcCode::BwLinkInvalid => "链路口径解析失败：PCIe 代际×lane 组合非法",
        }
    }
}

/// 单账户账页。
#[derive(Clone, Copy, Debug)]
pub struct BwAccount {
    /// 传输类。
    pub cls: BwClass,
    /// 配额（MB/s，构造冻结）。
    pub cap_mbps: u64,
    /// 已用（MB/s，记账累加）。
    pub used_mbps: u64,
}

impl BwAccount {
    /// 占用率（ppm）。
    pub const fn used_ppm(&self) -> u64 {
        if self.cap_mbps == 0 {
            return 1_000_000;
        }
        // const fn 禁 Ord::min——手写比较（同语义）。
        let used = if self.used_mbps > self.cap_mbps { self.cap_mbps } else { self.used_mbps };
        used * 1_000_000 / self.cap_mbps
    }

    /// 剩余水位（MB/s）。
    pub const fn remain_mbps(&self) -> u64 {
        if self.used_mbps >= self.cap_mbps {
            0
        } else {
            self.cap_mbps - self.used_mbps
        }
    }
}

/// 记账行（全量账：判据"带宽消耗全量记账"）。
#[derive(Clone, Copy, Debug)]
pub struct BwEntry {
    /// 账序（单调）。
    pub seq: u32,
    /// 传输类。
    pub cls: BwClass,
    /// 本笔带宽（MB/s）。
    pub mbps: u64,
    /// 是否抢占来的（抢占留痕——判据优先级纪律的可回放性）。
    pub preempted_from: Option<BwClass>,
}

/// 带宽账户账本（判据"账户化+记账+守恒"本体）。
pub struct BwAccountBook {
    /// 四账户（ordinal 对齐 BwClass）。
    accounts: [BwAccount; 4],
    /// 链路有效带宽（MB/s，构造冻结——Σcap ≤ 此值）。
    cap_mbps: u64,
    /// 全量账。
    ledger: Vec<BwEntry>,
    /// 账序发生器。
    seq: u32,
    /// 立案计数。
    pub cases: u32,
    /// 抢占计数。
    pub preempts: u32,
}

impl BwAccountBook {
    /// 从 PCIe 链路口径构造（复用 cgi02：理论×有效系数——判据"Σ账户≤PCIe
    /// 有效带宽"的 cap 来源单点）。
    pub fn from_link(gen: PcieGen, lanes: u32) -> Result<Self, XcCode> {
        let theory = gen_theory_mbps(gen, lanes).map_err(|_| XcCode::BwLinkInvalid)?;
        let eff = effective_mbps(theory, EFFICIENCY_DEFAULT_PPM);
        Self::with_cap(eff)
    }

    /// 显式有效带宽构造（复用方已有口径时直供）。
    pub fn with_cap(cap_mbps: u64) -> Result<Self, XcCode> {
        if cap_mbps == 0 {
            return Err(XcCode::BwLinkInvalid);
        }
        let mut book = BwAccountBook {
            accounts: [
                BwAccount { cls: BwClass::Swap, cap_mbps: 0, used_mbps: 0 },
                BwAccount { cls: BwClass::Upload, cap_mbps: 0, used_mbps: 0 },
                BwAccount { cls: BwClass::Download, cap_mbps: 0, used_mbps: 0 },
                BwAccount { cls: BwClass::Stream, cap_mbps: 0, used_mbps: 0 },
            ],
            cap_mbps,
            ledger: Vec::new(),
            seq: 0,
            cases: 0,
            preempts: 0,
        };
        // 默认配额：呈现 50% / 上传 20% / 下载 15% / 流送 15%（和=100%≤cap）。
        book.accounts[0].cap_mbps = cap_mbps / 2;
        book.accounts[1].cap_mbps = cap_mbps / 5;
        book.accounts[2].cap_mbps = cap_mbps * 3 / 20;
        book.accounts[3].cap_mbps = cap_mbps * 3 / 20;
        book.guard_conserv_cap()?;
        Ok(book)
    }

    /// 显式配额构造（Σcap ≤ 有效带宽强制校验——判据守恒的构造期防线）。
    pub fn with_quota(cap_mbps: u64, quota: [u64; 4]) -> Result<Self, XcCode> {
        let mut sum = 0u64;
        for &q in &quota {
            sum = sum.saturating_add(q);
        }
        if sum > cap_mbps {
            return Err(XcCode::BwCapOver);
        }
        if cap_mbps == 0 {
            return Err(XcCode::BwLinkInvalid);
        }
        let book = BwAccountBook {
            accounts: [
                BwAccount { cls: BwClass::Swap, cap_mbps: quota[0], used_mbps: 0 },
                BwAccount { cls: BwClass::Upload, cap_mbps: quota[1], used_mbps: 0 },
                BwAccount { cls: BwClass::Download, cap_mbps: quota[2], used_mbps: 0 },
                BwAccount { cls: BwClass::Stream, cap_mbps: quota[3], used_mbps: 0 },
            ],
            cap_mbps,
            ledger: Vec::new(),
            seq: 0,
            cases: 0,
            preempts: 0,
        };
        book.guard_conserv_cap()?;
        Ok(book)
    }

    /// 构造期守恒：Σcap ≤ 链路有效带宽。
    fn guard_conserv_cap(&self) -> Result<(), XcCode> {
        let mut sum = 0u64;
        for a in &self.accounts {
            sum = sum.saturating_add(a.cap_mbps);
        }
        if sum > self.cap_mbps {
            return Err(XcCode::BwCapOver);
        }
        Ok(())
    }

    /// 链路有效带宽。
    pub const fn link_cap_mbps(&self) -> u64 {
        self.cap_mbps
    }

    /// 账户只读。
    pub fn account(&self, cls: BwClass) -> Result<&BwAccount, XcCode> {
        match cls {
            BwClass::Swap => Ok(&self.accounts[0]),
            BwClass::Upload => Ok(&self.accounts[1]),
            BwClass::Download => Ok(&self.accounts[2]),
            BwClass::Stream => Ok(&self.accounts[3]),
        }
    }

    /// 记一笔带宽消耗（判据主路径：水位判定 + 全量记账 + 必要时抢占）。
    ///
    /// 裁定序：①零带宽拒 → ②本账户水位足则直记 → ③水位不足按优先级
    /// 倒序找可抢占账户（只抢比自己优先级低的）→ ④都不可则显性拒绝。
    pub fn charge(&mut self, cls: BwClass, mbps: u64) -> Result<BwEntry, XcCode> {
        if mbps == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::BwZeroCharge);
        }
        let i = cls.ordinal();
        let need = mbps;

        // ② 直记路径。
        if self.accounts[i].remain_mbps() >= need {
            return Ok(self.commit(cls, need, None));
        }

        // ③ 抢占路径：只抢优先级更低的账户，从最低优先级开始（后台先失）。
        let my_prio = cls.priority();
        let mut lack = need - self.accounts[i].remain_mbps();
        let mut preempted: Option<BwClass> = None;
        let order = [BwClass::Stream, BwClass::Download, BwClass::Upload, BwClass::Swap];
        for cand in order {
            if lack == 0 {
                break;
            }
            if cand.priority() >= my_prio {
                continue; // 只抢更低的
            }
            let ci = cand.ordinal();
            let free = self.accounts[ci].used_mbps.min(lack);
            if free > 0 {
                self.accounts[ci].used_mbps -= free;
                lack -= free;
                preempted = Some(cand);
                self.preempts = self.preempts.saturating_add(1);
                // 抢占留痕（被抢方记负向事件：以 BwPreempted 语义入账）
                self.seq = self.seq.saturating_add(1);
                self.ledger.push(BwEntry {
                    seq: self.seq,
                    cls: cand,
                    mbps: free,
                    preempted_from: Some(cls),
                });
            }
        }

        // ④ 水位终判：抢占后仍不足 → 显性拒绝（不静默超卖）。
        if self.accounts[i].remain_mbps() < need {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::BwExhausted);
        }
        Ok(self.commit(cls, need, preempted))
    }

    /// 统一入账。
    fn commit(&mut self, cls: BwClass, mbps: u64, preempted_from: Option<BwClass>) -> BwEntry {
        let i = cls.ordinal();
        self.accounts[i].used_mbps = self.accounts[i].used_mbps.saturating_add(mbps);
        self.seq = self.seq.saturating_add(1);
        let e = BwEntry { seq: self.seq, cls, mbps, preempted_from };
        self.ledger.push(e);
        e
    }

    /// 帧末/时段末释放（占用是瞬时的——带宽按窗口记账后释放）。
    pub fn release(&mut self, cls: BwClass) {
        let i = cls.ordinal();
        self.accounts[i].used_mbps = 0;
    }

    pub fn release_all(&mut self) {
        for a in &mut self.accounts {
            a.used_mbps = 0;
        }
    }

    /// 预警扫描（占用 ≥ PREWARN_USED_PPM 的账户清单）。
    pub fn prewarn(&self) -> Vec<BwClass> {
        let mut out = Vec::new();
        for a in &self.accounts {
            if a.used_ppm() >= PREWARN_USED_PPM {
                out.push(a.cls);
            }
        }
        out
    }

    /// 预算修正建议（C 域集成复用声明：对齐 cgc02 BudgetCorrection 的池语义——
    /// 只出单不执行）：预警账户若为后台类 → 建议收缩其配额补给呈现；
    /// 呈现类预警 → 建议从流送账户划拨。
    pub fn corrections(&self) -> Vec<(BwClass, i64)> {
        let mut out = Vec::new();
        for a in &self.accounts {
            if a.used_ppm() < PREWARN_USED_PPM {
                continue;
            }
            match a.cls {
                BwClass::Stream => out.push((BwClass::Stream, -50_000)),
                BwClass::Swap | BwClass::Upload => out.push((BwClass::Stream, -30_000)),
                BwClass::Download => out.push((BwClass::Upload, -20_000)),
            }
        }
        out
    }

    /// 守恒不变量核验（判据"Σ账户≤PCIe 有效带宽"的运行期机械化）：
    /// ① Σused ≤ Σcap；② Σcap ≤ 链路 cap；③ 账本 seq 单调。立案写账。
    pub fn conserves(&mut self) -> Result<(), XcCode> {
        let mut used = 0u64;
        let mut cap = 0u64;
        for a in &self.accounts {
            used = used.saturating_add(a.used_mbps);
            cap = cap.saturating_add(a.cap_mbps);
        }
        if used > cap || cap > self.cap_mbps {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::BwConservBroke);
        }
        let mut prev = 0u32;
        for e in &self.ledger {
            if e.seq <= prev {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::BwLedgerBroken);
            }
            prev = e.seq;
        }
        Ok(())
    }

    /// 全量账只读。
    pub fn ledger(&self) -> &[BwEntry] {
        &self.ledger
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "带宽账{} 链路{}MB/s 账[交换{}/{} 上传{}/{} 下载{}/{} 流送{}/{}] 抢占{} 立案{}",
            BW_ACCOUNT_VERSION,
            self.cap_mbps,
            self.accounts[0].used_mbps, self.accounts[0].cap_mbps,
            self.accounts[1].used_mbps, self.accounts[1].cap_mbps,
            self.accounts[2].used_mbps, self.accounts[2].cap_mbps,
            self.accounts[3].used_mbps, self.accounts[3].cap_mbps,
            self.preempts,
            self.cases,
        )
    }
}
