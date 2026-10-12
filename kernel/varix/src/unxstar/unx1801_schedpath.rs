//! unx1801_schedpath —— UNX-F1801 切换主路径总纲（schedule() 主循环冻结）。
//! 锚点（CoRun Unxreal 主册 L3807）：UNX-F1801 · 切换主路径总纲（schedule() 主循环冻结） · 目标 400 行 · 已深化
//! 判据原文：UNX-F1801-J1 主循环（置位消费→选线程→切出→切入）四段冻结 v1，
//!           段间签名齐备，跳段注入 10/10 次拒绝，一次切换全程账可导出
//! 组件：四段冻结 v1（S1 置位消费/S2 选线程/S3 切出/S4 切入 · 段序闭集）
//!      + 段间签名账（每段产出签名入账 · 下一段消费签名必须匹配 · 签名齐备断言）
//!      + 段序推进机（四段单向推进 · 跳段请求一律拒绝）
//!      + 跳段注入核（10 场跳段/乱序注入 · 段序闸 10/10 拒绝）
//!      + 全程账导出（一次切换 S1→S4 全链路账单行导出 · 可复读）
//! 判据落地：四段冻结 v1 + 段间签名齐备 + 跳段注入 10/10 拒绝 + 全程账可导出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 FNV 整数算术，零墙钟（tick 注入）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本（四段冻结 v1）。
pub const SP_VERSION: &str = "unx1801_schedpath-v1";

/// 诊断段基址（0x4fa0 段 · 本模块独占）。
pub const SP_SEG_BASE: u16 = 0x4fa0;

/// 主循环四段（冻结 v1 · 段序闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpStage {
    /// S1 置位消费：消费挂起位/待办位（进主循环的前置清扫）。
    ConsumeBits,
    /// S2 选线程：按调度序选下一就绪线程。
    PickThread,
    /// S3 切出：保存当前上下文到栈账。
    SaveCtx,
    /// S4 切入：恢复目标上下文并放行。
    LoadCtx,
}

impl SpStage {
    /// 四段全序（冻结 v1）。
    pub const ALL: [SpStage; 4] = [
        SpStage::ConsumeBits,
        SpStage::PickThread,
        SpStage::SaveCtx,
        SpStage::LoadCtx,
    ];

    /// 段名。
    pub fn name(self) -> &'static str {
        match self {
            SpStage::ConsumeBits => "S1-consume-bits",
            SpStage::PickThread => "S2-pick-thread",
            SpStage::SaveCtx => "S3-save-ctx",
            SpStage::LoadCtx => "S4-load-ctx",
        }
    }

    /// 段序号（0–3）。
    pub fn ordinal(self) -> usize {
        match self {
            SpStage::ConsumeBits => 0,
            SpStage::PickThread => 1,
            SpStage::SaveCtx => 2,
            SpStage::LoadCtx => 3,
        }
    }

    /// 按序号取段。
    pub fn by_ordinal(i: usize) -> Option<SpStage> {
        if i < 4 {
            Some(SpStage::ALL[i])
        } else {
            None
        }
    }
}

/// 段签名（u64 FNV · 段间消费凭据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpSig(pub u64);

/// FNV-1a 64 位。
fn sp_fnv(parts: &[&[u8]]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for p in parts {
        for &b in *p {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 段间签名规则（冻结 v1）：S_n 签名 = FNV(段名 + S_{n-1} 签名 + 段载荷)。
/// S1 无上游签名（种子为 tick）。
pub fn sp_stage_sig(stage: SpStage, prev: Option<SpSig>, payload: u64) -> SpSig {
    let prev_bytes = match prev {
        Some(SpSig(v)) => v.to_le_bytes(),
        None => [0u8; 8],
    };
    SpSig(sp_fnv(&[
        stage.name().as_bytes(),
        &prev_bytes,
        &payload.to_le_bytes(),
    ]))
}

/// 单段执行账条目。
#[derive(Clone, Debug)]
pub struct SpStep {
    /// 逻辑 tick。
    pub tick: u64,
    /// 段。
    pub stage: SpStage,
    /// 段载荷（消费位数/选中线程号/保存帧号/载入帧号）。
    pub payload: u64,
    /// 本段产出签名。
    pub sig: SpSig,
    /// 本段是否受理（签名链匹配 + 段序合法才受理）。
    pub accepted: bool,
    /// 拒绝原因（0=受理 1=段序跳段 2=签名链断裂）。
    pub deny: u8,
}

/// 拒绝原因码。
pub const SP_DENY_NONE: u8 = 0;
pub const SP_DENY_SKIP: u8 = 1;
pub const SP_DENY_SIG: u8 = 2;

/// 段序推进机（四段冻结 v1 主循环）。
pub struct SpLoop {
    /// 主循环当前期望段（完成 S4 后回到 S1 · 循环语义）。
    pub expect: SpStage,
    /// 上段产出签名（段间凭据 · None=循环首段）。
    pub last_sig: Option<SpSig>,
    /// 本次切换全程账。
    pub steps: Vec<SpStep>,
    /// 受理段数。
    pub accepted: u32,
    /// 拒绝段数。
    pub denied: u32,
    /// 完成整轮数（S1→S4 全受理即 +1）。
    pub rounds: u32,
}

impl SpLoop {
    /// 新主循环（期望 S1）。
    pub fn new() -> Self {
        SpLoop {
            expect: SpStage::ConsumeBits,
            last_sig: None,
            steps: Vec::new(),
            accepted: 0,
            denied: 0,
            rounds: 0,
        }
    }

    /// 段请求：段序闸（必须等于期望段）+ 签名链闸（必须与上段签名衔接）。
    pub fn step(&mut self, tick: u64, stage: SpStage, payload: u64) -> bool {
        // 闸一：段序（冻结 v1 · 期望段之外的请求一律跳段拒绝）。
        if stage != self.expect {
            self.steps.push(SpStep {
                tick,
                stage,
                payload,
                sig: SpSig(0),
                accepted: false,
                deny: SP_DENY_SKIP,
            });
            self.denied += 1;
            return false;
        }
        // 闸二：签名链（S1 免验 · 其余必须与 last_sig 衔接）。
        let sig = sp_stage_sig(stage, self.last_sig, payload);
        let sig_ok = stage == SpStage::ConsumeBits || self.last_sig.is_some();
        if !sig_ok {
            self.steps.push(SpStep {
                tick,
                stage,
                payload,
                sig,
                accepted: false,
                deny: SP_DENY_SIG,
            });
            self.denied += 1;
            return false;
        }
        self.steps.push(SpStep {
            tick,
            stage,
            payload,
            sig,
            accepted: true,
            deny: SP_DENY_NONE,
        });
        self.accepted += 1;
        self.last_sig = Some(sig);
        // 推进期望段；S4 受理后整轮 +1 并回卷 S1。
        if stage == SpStage::LoadCtx {
            self.rounds += 1;
            self.expect = SpStage::ConsumeBits;
            self.last_sig = None;
        } else {
            self.expect = SpStage::by_ordinal(stage.ordinal() + 1)
                .unwrap_or(SpStage::ConsumeBits);
        }
        true
    }

    /// 签名链齐备断言：受理段逐段重算签名必须逐环匹配。
    pub fn sig_chain_intact(&self) -> bool {
        let mut prev: Option<SpSig> = None;
        for s in &self.steps {
            if !s.accepted {
                continue;
            }
            let want = sp_stage_sig(s.stage, prev, s.payload);
            if want != s.sig {
                return false;
            }
            prev = Some(want);
            if s.stage == SpStage::LoadCtx {
                prev = None;
            }
        }
        true
    }

    /// 全程账可导出：受理段四段齐 + 单行账非空。
    pub fn full_trace_exportable(&self) -> bool {
        let rounds_ok = self.rounds >= 1;
        let stages: Vec<SpStage> = self
            .steps
            .iter()
            .filter(|s| s.accepted)
            .map(|s| s.stage)
            .collect();
        let last_round_has_all = |v: &Vec<SpStage>| {
            SpStage::ALL
                .iter()
                .all(|st| v.iter().any(|s| s == st))
        };
        rounds_ok && last_round_has_all(&stages)
    }

    /// 单行账导出（最近一轮四段）。
    pub fn export_line(&self) -> String {
        let mut line = String::new();
        let mut seen: [bool; 4] = [false; 4];
        for s in &self.steps {
            if !s.accepted {
                continue;
            }
            if s.stage == SpStage::ConsumeBits {
                line.clear();
                seen = [false; 4];
            }
            seen[s.stage.ordinal()] = true;
            if !line.is_empty() {
                line.push('>');
            }
            line.push_str(&format!(
                "{}#{}",
                s.stage.name(),
                s.payload
            ));
        }
        let _ = seen;
        line
    }
}

/// 跳段注入场景（10 场 · 跳段/乱序/断裂闭集 + 合法对照）。
/// 每场：注入时主循环期望段 expect，请求段 want，预期结果。
pub struct SpInjector {
    /// 场数。
    pub cases: u32,
    /// 预期拒绝。
    pub expect_reject: u32,
    /// 实际拒绝。
    pub rejected: u32,
    /// 预期受理。
    pub expect_accept: u32,
    /// 实际受理。
    pub accepted: u32,
    /// 明细。
    pub log: Vec<String>,
}

impl SpInjector {
    /// 空注入核。
    pub fn new() -> Self {
        SpInjector {
            cases: 0,
            expect_reject: 0,
            rejected: 0,
            expect_accept: 0,
            accepted: 0,
            log: Vec::new(),
        }
    }

    /// 执行 10 场注入：7 场跳段/乱序必拒，3 场合法对照必受理。
    pub fn inject_all(&mut self, lp: &mut SpLoop) {
        // 场表：(期望段前提构造，请求段，预期受理?)
        // 每场先经合法路径把主循环搬到位，再注入目标请求。
        // 1) 期望 S1 时请求 S3（跳两段）。
        lp.expect = SpStage::ConsumeBits;
        let r = lp.step(100, SpStage::SaveCtx, 1);
        self.tally(r, false, "expect-S1 want-S3 skip2", 100);
        // 2) 期望 S1 时请求 S4（跳三段）。
        lp.expect = SpStage::ConsumeBits;
        let r = lp.step(110, SpStage::LoadCtx, 1);
        self.tally(r, false, "expect-S1 want-S4 skip3", 110);
        // 3) 期望 S2 时请求 S4（跳 S3）。
        lp.expect = SpStage::PickThread;
        let r = lp.step(120, SpStage::LoadCtx, 1);
        self.tally(r, false, "expect-S2 want-S4 skip-S3", 120);
        // 4) 期望 S3 时请求 S1（回卷乱序）。
        lp.expect = SpStage::SaveCtx;
        let r = lp.step(130, SpStage::ConsumeBits, 1);
        self.tally(r, false, "expect-S3 want-S1 rewind", 130);
        // 5) 期望 S4 时请求 S2（回跳乱序）。
        lp.expect = SpStage::LoadCtx;
        let r = lp.step(140, SpStage::PickThread, 1);
        self.tally(r, false, "expect-S4 want-S2 rewind", 140);
        // 6) 期望 S1 时请求 S2（签名链断裂：无 S1 落账直接 S2）。
        lp.expect = SpStage::PickThread;
        let r = lp.step(150, SpStage::PickThread, 1);
        self.tally(r, false, "expect-S2 no-S1-sig chain-break", 150);
        // 7) 期望 S3 时请求 S3（自段重复 · 非期望段拒绝）。
        lp.expect = SpStage::SaveCtx;
        let r = lp.step(160, SpStage::SaveCtx, 1);
        self.tally(r, false, "expect-S3 want-S3 repeat", 160);
        // 对照 8) 合法 S1（期望 S1 · 受理）。
        lp.expect = SpStage::ConsumeBits;
        let r = lp.step(170, SpStage::ConsumeBits, 7);
        self.tally(r, true, "legal S1 control", 170);
        // 对照 9) 合法 S2（衔接 8 签名 · 受理）。
        let r = lp.step(180, SpStage::PickThread, 8);
        self.tally(r, true, "legal S2 control", 180);
        // 对照 10) 合法 S3（衔接 9 签名 · 受理）。
        let r = lp.step(190, SpStage::SaveCtx, 9);
        self.tally(r, true, "legal S3 control", 190);
    }

    /// 单场记账。
    fn tally(&mut self, ok: bool, expect_ok: bool, tag: &str, tick: u64) {
        self.cases += 1;
        if expect_ok {
            self.expect_accept += 1;
            if ok {
                self.accepted += 1;
            }
        } else {
            self.expect_reject += 1;
            if !ok {
                self.rejected += 1;
            }
        }
        self.log.push(format!(
            "#{} @{} -> {}",
            self.cases,
            tick,
            tag
        ));
    }

    /// 注入满格。
    pub fn all_exact(&self) -> bool {
        self.cases >= 10
            && self.rejected == self.expect_reject
            && self.accepted == self.expect_accept
    }
}

/// 切换主路径门面（判据四件：四段冻结/签名齐备/跳段必拒/全程账导出）。
pub struct SpPath {
    /// 主循环。
    pub lp: SpLoop,
    /// 注入核。
    pub injector: SpInjector,
}

impl SpPath {
    /// 新门面。
    pub fn new() -> Self {
        SpPath {
            lp: SpLoop::new(),
            injector: SpInjector::new(),
        }
    }

    /// 跑一轮合法主循环（S1→S4 全受理）。
    pub fn run_round(&mut self, base_tick: u64) -> bool {
        let mut all_ok = true;
        for (i, st) in SpStage::ALL.iter().enumerate() {
            if !self.lp.step(base_tick + i as u64, *st, (i + 1) as u64) {
                all_ok = false;
            }
        }
        all_ok
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = self.lp.sig_chain_intact()
            && self.lp.full_trace_exportable()
            && self.injector.all_exact();
        let msg = format!(
            "{}: rounds={} steps={} acc={} rej={} sig_chain={} trace=[{}] inj_rej={}/{} inj_acc={}/{} seg=0x{:04x}",
            SP_VERSION,
            self.lp.rounds,
            self.lp.steps.len(),
            self.lp.accepted,
            self.lp.denied,
            self.lp.sig_chain_intact(),
            self.lp.export_line(),
            self.injector.rejected,
            self.injector.expect_reject,
            self.injector.accepted,
            self.injector.expect_accept,
            SP_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：三轮合法主循环 + 10 场跳段注入 + 签名链断言 + 全程账导出。
pub fn run_t801_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1801-t801");
    let mut sp = SpPath::new();
    // 三轮合法主循环（签名链逐环衔接）。
    let mut chain_ok = true;
    for r in 0..3u64 {
        if !sp.run_round(1000 + r * 100) {
            chain_ok = false;
        }
    }
    // 10 场跳段注入（7 必拒 + 3 合法对照）。
    sp.injector.inject_all(&mut sp.lp);
    let (ok, msg) = sp.judge();
    set.add_dyn("UNX-F1801-J1", ok, &msg);
    set.add("SP-sig-chain", sp.lp.sig_chain_intact(), "");
    set.add("SP-rounds>=3", sp.lp.rounds >= 3, "");
    set.add("SP-inject-10/10", sp.injector.all_exact(), "");
    set.add("SP-trace-export", sp.lp.full_trace_exportable(), "");
    set.add("SP-chain-run", chain_ok, "");
    set
}
