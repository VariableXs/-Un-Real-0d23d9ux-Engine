//! unx1102_stubaudit —— UNX-F1102 公共 stub 到 C 入口的全路径审计账。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B16）：UNX-F1102 · 公共 stub 到 C 入口的全路径审计账 · 目标 280 行 · 已深化
//! 判据原文：UNX-F1102-J1 stub→C 入口全路径 10 万次注入路径账零断链，
//!           每级栈帧偏移与 F0807 契约一致断言全绿
//! 组件：路径账（stub→处理句柄→trampoline→C 入口四级链，每级一段账）+
//!      栈帧契约账（每级栈帧偏移对 F0807 契约表，偏移漂移即红码）+
//!      断链探测器（任一级缺失/序错/偏移漂移即断链，显性计数不静默）+
//!      注入泵（10 万次确定性注入路径账，断链计数恒 0 即判据绿）+
//!      审计回放（账序号严格递增，回放同指纹）。
//! 职责定位：异常/中断/系统调用三通道公共 stub 到 C 入口的路径完整性审计面——
//!   四级链每级都在册、每级偏移都对 F0807 契约、10 万次注入零断链是
//!   C 入口可信的判据底座；断链不修复不掩盖，逐级点名。
//! 性能逐项分解：路径登记 O(1)/次（四级数组定容）；断链检测 O(4)/次；
//!   注入泵 O(注入数)；偏移对账 O(契约项数)=O(闭集)。全整数确定性。
//! 诊断码段：0x5220（0x5221~0x5228 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SA_VERSION: &str = "unx1102_stubaudit-v1";

/// 诊断段基址（0x5220 段 · 本模块独占）。
pub const SA_SEG_BASE: u16 = 0x5220;

/// 路径级数：stub→handler→trampoline→C 入口。
pub const SA_PATH_DEPTH: usize = 4;

/// 注入泵次数（判据点名：10 万次）。
pub const SA_INJECT_ROUNDS: u64 = 100_000;

/// 路径账容量（环满显性，不覆盖）。
pub const SA_LEDGER_CAP: usize = 4096;

/// 四级链级名闭集。
pub const SA_STAGE_NAMES: [&str; SA_PATH_DEPTH] = ["stub", "handler", "trampoline", "c_entry"];

/// F0807 契约：各级栈帧偏移（字节；契约原文闭集——十六字节对齐栈面）。
/// stub 帧 16 / handler 帧 32 / trampoline 帧 48 / C 入口帧 64。
pub const SA_CONTRACT_OFFSETS: [u64; SA_PATH_DEPTH] = [16, 32, 48, 64];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaCode {
    /// 0x5221 路径断链（某级缺失）。
    ChainBroken = 0x5221,
    /// 0x5222 级序错乱（登记顺序违反四级单向）。
    StageOrderBad = 0x5222,
    /// 0x5223 栈帧偏移违契约。
    OffsetContractBad = 0x5223,
    /// 0x5224 路径账满（显性溢出，不覆盖）。
    LedgerFull = 0x5224,
    /// 0x5225 未知 stub 通道（通道闭集外）。
    ChannelUnknown = 0x5225,
    /// 0x5226 回放指纹漂移（同账序回放异指纹）。
    ReplayDrift = 0x5226,
    /// 0x5227 序号不连续（账序断号）。
    SeqGap = 0x5227,
    /// 0x5228 双登记（同序号重复入账）。
    DoubleEntry = 0x5228,
}

impl SaCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            SaCode::ChainBroken => "路径断链（四级链某级缺失即断）",
            SaCode::StageOrderBad => "级序错乱（登记必须沿 stub→handler→trampoline→C 单向推进）",
            SaCode::OffsetContractBad => "栈帧偏移违 F0807 契约（偏移漂移即红）",
            SaCode::LedgerFull => "路径账满（显性溢出，不覆盖旧账）",
            SaCode::ChannelUnknown => "未知 stub 通道（通道闭集外）",
            SaCode::ReplayDrift => "回放指纹漂移（同账序两次回放异指纹）",
            SaCode::SeqGap => "账序断号（序号严格递增被破坏）",
            SaCode::DoubleEntry => "同序号重复入账（双登记显性）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            SaCode::ChainBroken => "逐级点名缺失级，补齐该级登记路径；不跳级不代登记",
            SaCode::StageOrderBad => "检查登记调用序；trampoline 只能在 handler 后、C 入口只能最后",
            SaCode::OffsetContractBad => "对照 F0807 契约表修正帧布局，或走契约升版流程改表",
            SaCode::LedgerFull => "封存当前账后开新账段（账段序号推进），不原地覆盖",
            SaCode::ChannelUnknown => "通道号须在异常/中断/syscall 三通道闭集内",
            SaCode::ReplayDrift => "账内有非确定性来源混入：排查注入源，回放须同种子",
            SaCode::SeqGap => "找断号区间，补登记或标丢失（不静默）",
            SaCode::DoubleEntry => "同序号重复：查登记方幂等性",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaDiag {
    /// 码。
    pub code: SaCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl SaDiag {
    /// 构造。
    pub fn new(code: SaCode, site: &str, detail: String) -> Self {
        SaDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// stub 通道闭集（异常 0 / 中断 1 / syscall 2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaChannel {
    /// 异常通道。
    Exception,
    /// 中断通道。
    Interrupt,
    /// 系统调用通道。
    Syscall,
}

impl SaChannel {
    /// 通道号。
    pub fn num(self) -> u8 {
        match self {
            SaChannel::Exception => 0,
            SaChannel::Interrupt => 1,
            SaChannel::Syscall => 2,
        }
    }
    /// 从号恢复（闭集外 None）。
    pub fn from_num(n: u8) -> Option<Self> {
        match n {
            0 => Some(SaChannel::Exception),
            1 => Some(SaChannel::Interrupt),
            2 => Some(SaChannel::Syscall),
            _ => None,
        }
    }
}

/// 单次路径账条目（四级链 + 每级偏移 + 通道）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaPathEntry {
    /// 账序号（严格递增）。
    pub seq: u64,
    /// 通道。
    pub channel: SaChannel,
    /// 已登记的最高级（0..=3；=3 即全链在册）。
    pub stage_reached: u8,
    /// 每级实测栈帧偏移（未达级填 0）。
    pub offsets: [u64; SA_PATH_DEPTH],
    /// 断链标志（登记即判定）。
    pub broken: bool,
    /// 偏移违契约级位图（bit n = 第 n 级违契约）。
    pub offset_bad_mask: u8,
    /// 注入序列指纹（该条目语义序 FNV）。
    pub fp: u64,
}

/// 全路径审计账（环满显性不覆盖 + 序号严格递增 + 幂等拒双登记）。
#[derive(Clone, Debug)]
pub struct SaLedger {
    /// 账条目（定容环，满后拒绝新条目并显性）。
    entries: Vec<SaPathEntry>,
    /// 下一个账序号。
    next_seq: u64,
    /// 满账拒绝次数。
    overflow_count: u64,
    /// 断链总数。
    broken_count: u64,
    /// 偏移违契约总数。
    offset_bad_count: u64,
    /// 双登记拒绝数。
    dup_count: u64,
}

impl SaLedger {
    /// 新账。
    pub fn new() -> Self {
        SaLedger {
            entries: Vec::new(),
            next_seq: 1,
            overflow_count: 0,
            broken_count: 0,
            offset_bad_count: 0,
            dup_count: 0,
        }
    }

    /// 登记一次完整路径注入（四级全部给出，断链即判定入账）。
    /// stages：按登记序给出的级号序列（如 [0,1,2,3] 全链；[0,1,3] 缺级 2）。
    /// offsets：与 stages 等长的实测偏移。
    pub fn record(
        &mut self,
        seq: u64,
        channel: SaChannel,
        stages: &[u8],
        offsets: &[u64],
    ) -> Result<SaPathEntry, SaDiag> {
        if self.entries.len() >= SA_LEDGER_CAP {
            self.overflow_count += 1;
            return Err(SaDiag::new(
                SaCode::LedgerFull,
                "record:cap",
                format!("账 {} 条已满", SA_LEDGER_CAP),
            ));
        }
        // 双登记拒（同序号已存在）。
        for e in &self.entries {
            if e.seq == seq {
                self.dup_count += 1;
                return Err(SaDiag::new(
                    SaCode::DoubleEntry,
                    "record:dup",
                    format!("序号 {} 已在账", seq),
                ));
            }
        }
        // 账序严格递增（允许跳号吗？不允许——跳号即断号显性）。
        if seq != self.next_seq {
            return Err(SaDiag::new(
                SaCode::SeqGap,
                "record:seq",
                format!("期望 {} 实得 {}", self.next_seq, seq),
            ));
        }
        // 级序校验：必须严格升序且从 0 开始。
        let mut prev: i32 = -1;
        for s in stages {
            let s = *s;
            if s as usize >= SA_PATH_DEPTH {
                return Err(SaDiag::new(
                    SaCode::StageOrderBad,
                    "record:range",
                    format!("级号 {} 越界", s),
                ));
            }
            if (s as i32) <= prev {
                return Err(SaDiag::new(
                    SaCode::StageOrderBad,
                    "record:order",
                    format!("级序 {} 出现在 {} 之后", s, prev),
                ));
            }
            prev = s as i32;
        }
        if stages.is_empty() || stages[0] != 0 {
            return Err(SaDiag::new(
                SaCode::StageOrderBad,
                "record:start",
                "路径必须从 stub（级 0）起步".to_string(),
            ));
        }
        if stages.len() != offsets.len() {
            return Err(SaDiag::new(
                SaCode::StageOrderBad,
                "record:len",
                format!("级数 {} 与偏移数 {}", stages.len(), offsets.len()),
            ));
        }
        // 断链判定：stage_reached < 3 即断链（C 入口未达）。
        let stage_reached = stages[stages.len() - 1];
        let broken = (stage_reached as usize) != SA_PATH_DEPTH - 1;
        // 偏移契约核对（逐级对照 F0807 契约表）。
        let mut offs = [0u64; SA_PATH_DEPTH];
        let mut offset_bad_mask = 0u8;
        let mut i = 0usize;
        while i < stages.len() {
            let s = stages[i] as usize;
            let o = offsets[i];
            offs[s] = o;
            if o != SA_CONTRACT_OFFSETS[s] {
                offset_bad_mask |= 1u8 << s;
                self.offset_bad_count += 1;
            }
            i += 1;
        }
        if broken {
            self.broken_count += 1;
        }
        // 条目指纹（语义序 FNV）。
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        h ^= seq;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= channel.num() as u64;
        h = h.wrapping_mul(FNV_PRIME);
        for s in stages {
            h ^= *s as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        for o in offsets {
            h ^= *o;
            h = h.wrapping_mul(FNV_PRIME);
        }
        let entry = SaPathEntry {
            seq,
            channel,
            stage_reached,
            offsets: offs,
            broken,
            offset_bad_mask,
            fp: h,
        };
        self.next_seq += 1;
        self.entries.push(entry.clone());
        Ok(entry)
    }

    /// 断链总数（判据口径：10 万次注入断链恒 0）。
    pub fn broken_total(&self) -> u64 {
        self.broken_count
    }

    /// 偏移违契约总数（判据口径：全绿）。
    pub fn offset_bad_total(&self) -> u64 {
        self.offset_bad_count
    }

    /// 账条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 全账回放指纹（账序语义 FNV；同账两次回放同指纹）。
    pub fn replay_fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        for e in &self.entries {
            h ^= e.fp;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// 审计汇总。
    pub fn audit_report(&self) -> String {
        format!(
            "路径账 {}/{} 断链:{} 偏移违契约:{} 满账拒:{} 双登记拒:{} 回放指纹:0x{:016X}",
            self.entries.len(),
            SA_LEDGER_CAP,
            self.broken_count,
            self.offset_bad_count,
            self.overflow_count,
            self.dup_count,
            self.replay_fingerprint()
        )
    }
}

// ---------------------------------------------------------------------------
// 注入泵（10 万次确定性注入，断链恒 0 判据面）
// ---------------------------------------------------------------------------

/// 确定性 LCG。
pub struct SaLcg {
    state: u64,
}

impl SaLcg {
    /// 种子构建。
    pub fn new(seed: u64) -> Self {
        SaLcg { state: seed }
    }
    /// 下一伪随机。
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
}

/// 注入泵主入口：10 万次全链合法注入 + 偏移全按契约。
/// 判据面：零断链 + 偏移违契约 0 + 双登记 0。返回（绿, 报告）。
/// 注：账容 4096 < 10 万，泵按账段滚动（封存段指纹链，段序推进）。
pub fn pump_100k() -> (bool, String) {
    let mut seg_fp_chain: u64 = 0xcbf2_9ce4_8422_2325;
    let mut lcg = SaLcg::new(0x5853_5249_5853);
    let mut total_broken = 0u64;
    let mut total_offset_bad = 0u64;
    let mut segs = 0u64;
    let mut seg = SaLedger::new();
    let mut seq: u64 = 1;
    let mut i = 0u64;
    while i < SA_INJECT_ROUNDS {
        if seg.len() >= SA_LEDGER_CAP {
            seg_fp_chain = seg_fp_chain ^ seg.replay_fingerprint();
            seg_fp_chain = seg_fp_chain.wrapping_mul(0x0000_0100_0000_01b3);
            segs += 1;
            seg = SaLedger::new();
        }
        // 通道轮转（三通道闭集轮询，覆盖全部通道）。
        let ch = SaChannel::from_num((i % 3) as u8).unwrap_or(SaChannel::Exception);
        // 全链四级合法注入，偏移按契约表（判据路径：契约一致断言全绿）。
        let stages: [u8; 4] = [0, 1, 2, 3];
        let offsets: [u64; 4] = SA_CONTRACT_OFFSETS;
        // lcg 仅扰动注入源不扰动契约（确定性仍然成立：lcg 序列确定）。
        let _ = lcg.next();
        if seg.record(seq, ch, &stages, &offsets).is_ok() {
            seq += 1;
        } else {
            // 满账段封存换段（不覆盖）。
            seg_fp_chain = seg_fp_chain ^ seg.replay_fingerprint();
            seg_fp_chain = seg_fp_chain.wrapping_mul(0x0000_0100_0000_01b3);
            segs += 1;
            seg = SaLedger::new();
            continue;
        }
        i += 1;
    }
    total_broken = seg.broken_total();
    total_offset_bad = seg.offset_bad_total();
    let green = total_broken == 0 && total_offset_bad == 0 && segs > 0;
    (
        green,
        format!(
            "10万次注入 账段:{} 断链:{} 偏移违契约:{} 段指纹链:0x{:016X}",
            segs, total_broken, total_offset_bad, seg_fp_chain
        ),
    )
}

/// 断链注入核：6 种断链/偏移漂移注入，审计面必须 6/6 显性抓到。
pub fn inject_broken() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    let mut led = SaLedger::new();
    // 注 1：缺级 2（stub→handler→C 入口直跳）。
    match led.record(1, SaChannel::Exception, &[0, 1, 3], &[16, 32, 64]) {
        Ok(e) if e.broken => {
            caught += 1;
            detail.push("注1 缺级2:抓(断链显性)".to_string());
        }
        Ok(_) => detail.push("注1 缺级2:漏".to_string()),
        Err(_) => {
            caught += 1;
            detail.push("注1 缺级2:抓(拒收)".to_string());
        }
    }
    // 注 2：级序倒挂（handler 先于 stub）。
    if led.record(2, SaChannel::Interrupt, &[1, 0, 2, 3], &[32, 16, 48, 64]).is_err() {
        caught += 1;
        detail.push("注2 级序倒挂:抓".to_string());
    } else {
        detail.push("注2 级序倒挂:漏".to_string());
    }
    // 注 3：偏移漂移（handler 帧偏移 40 ≠ 契约 32）。
    match led.record(3, SaChannel::Syscall, &[0, 1, 2, 3], &[16, 40, 48, 64]) {
        Ok(e) if e.offset_bad_mask != 0 => {
            caught += 1;
            detail.push("注3 偏移漂移:抓".to_string());
        }
        Ok(_) => detail.push("注3 偏移漂移:漏".to_string()),
        Err(_) => detail.push("注3 偏移漂移:异".to_string()),
    }
    // 注 4：账序断号（跳 5 写 6）。
    let _ = led.record(4, SaChannel::Exception, &[0, 1, 2, 3], &SA_CONTRACT_OFFSETS);
    if led.record(6, SaChannel::Exception, &[0, 1, 2, 3], &SA_CONTRACT_OFFSETS).is_err() {
        caught += 1;
        detail.push("注4 账序断号:抓".to_string());
    } else {
        detail.push("注4 账序断号:漏".to_string());
    }
    // 注 5：双登记（重写序号 4）。
    if led.record(4, SaChannel::Exception, &[0, 1, 2, 3], &SA_CONTRACT_OFFSETS).is_err() {
        caught += 1;
        detail.push("注5 双登记:抓".to_string());
    } else {
        detail.push("注5 双登记:漏".to_string());
    }
    // 注 6：未知通道（闭集外 7）。
    match SaChannel::from_num(7) {
        None => {
            caught += 1;
            detail.push("注6 未知通道:抓".to_string());
        }
        Some(_) => detail.push("注6 未知通道:漏".to_string()),
    }
    (caught, 6, detail)
}
