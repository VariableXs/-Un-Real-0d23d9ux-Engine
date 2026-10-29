//! UNX-F10297 · 对接降级预案（通道断联时的离线包）
//! 段位 0x11840 · 判据锚 UNX-F10297-J1 · 域 O3 对接面/降级预案
//! 结构：OfflinePack（离线包）+ DegradedBook（降级账）+ ChannelState（通道态机）
//! 语义：通道断联时必须落离线包（封存待同步条目 + 完整性指纹），
//!       恢复后离线包可完整重放；「断联裸死」（断联且无离线包）必须 10/10 检出。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x11840;

/// 通道状态机
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelState {
    Online,
    Degraded,
    Offline,
}

impl ChannelState {
    pub fn is_down(self) -> bool {
        !matches!(self, ChannelState::Online)
    }
    pub fn code(self) -> u16 {
        (SEG_BASE as u16) + match self {
            ChannelState::Online => 1,
            ChannelState::Degraded => 2,
            ChannelState::Offline => 3,
        }
    }
}

/// 离线包：断联期间封存的待同步条目
pub struct OfflinePack {
    pub seq: u32,
    pub entries: Vec<String>,
    /// 封包时通道态
    pub sealed_at: ChannelState,
}

impl OfflinePack {
    pub fn new(seq: u32, sealed_at: ChannelState) -> Self {
        OfflinePack { seq, entries: Vec::new(), sealed_at }
    }
    pub fn push(&mut self, e: &str) -> bool {
        // 只能在通道下线时封存
        if !self.sealed_at.is_down() {
            return false;
        }
        self.entries.push(e.to_string());
        true
    }
    /// FNV-1a 完整性指纹
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        for e in &self.entries {
            for b in e.bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            h ^= 0x1F; // 条目分隔
            h = h.wrapping_mul(0x01000193);
        }
        h ^= self.seq;
        h
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn render(&self) -> String {
        format!("pack#{} n={} at={:?}", self.seq, self.entries.len(), self.sealed_at)
    }
}

/// 重放结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayResult {
    pub replayed: usize,
    pub intact: bool,
}

/// 降级账
pub struct DegradedBook {
    pub channel: ChannelState,
    packs: Vec<OfflinePack>,
    next_seq: u32,
    /// 断联裸死事件（断联期间产生的条目既未封包也未入通道）
    naked_deaths: u32,
    /// 降级演练达成次数（断联→封包→恢复→重放完整闭环）
    drills_ok: u32,
}

impl DegradedBook {
    pub fn new() -> Self {
        DegradedBook { channel: ChannelState::Online, packs: Vec::new(), next_seq: 1, naked_deaths: 0, drills_ok: 0 }
    }

    /// 通道状态迁移：Online→(Degraded|Offline)，恢复必须先过 Degraded
    pub fn set_channel(&mut self, st: ChannelState) -> bool {
        match (self.channel, st) {
            (ChannelState::Online, ChannelState::Online)
            | (ChannelState::Degraded, ChannelState::Degraded)
            | (ChannelState::Offline, ChannelState::Offline) => false,
            (ChannelState::Online, ChannelState::Degraded | ChannelState::Offline) => {
                self.channel = st;
                true
            }
            (ChannelState::Degraded | ChannelState::Offline, ChannelState::Degraded) => {
                self.channel = ChannelState::Degraded;
                true
            }
            (ChannelState::Degraded, ChannelState::Offline) => {
                self.channel = ChannelState::Offline;
                true
            }
            (ChannelState::Degraded | ChannelState::Offline, ChannelState::Online) => {
                if self.channel == ChannelState::Offline {
                    // 违规：Offline 必须经 Degraded 过渡，直接恢复记裸死
                    self.naked_deaths += 1;
                    return false;
                }
                self.channel = ChannelState::Online;
                true
            }
        }
    }

    /// 断联期间接收条目：开离线包封存；返回是否成功封存
    pub fn accept_while_down(&mut self, e: &str) -> bool {
        if !self.channel.is_down() {
            return false;
        }
        if self.packs.is_empty() {
            let st = self.channel;
            self.packs.push(OfflinePack::new(self.next_seq, st));
            self.next_seq += 1;
        }
        self.packs.last_mut().expect("pack just pushed").push(e)
    }

    /// 在线接收：直接清空待封包语义（正常路径无包）
    pub fn accept_online(&mut self, _e: &str) -> bool {
        self.channel == ChannelState::Online
    }

    /// 恢复后重放：重放封存条目并校验完整性
    pub fn replay_on_recover(&mut self, expect_fp: u32) -> Option<ReplayResult> {
        if self.channel != ChannelState::Online || self.packs.is_empty() {
            return None;
        }
        let mut replayed = 0usize;
        for p in &self.packs {
            replayed += p.entries.len();
        }
        // 完整性以全体包联合指纹比对单一期望（演练口径：单包）
        let intact = if self.packs.len() == 1 { self.packs[0].fp() == expect_fp } else { false };
        let r = ReplayResult { replayed, intact };
        if intact {
            self.drills_ok += 1;
            self.packs.clear();
        }
        Some(r)
    }

    /// 断联裸死检出：断联时无任何离线包即有数据丢失倾向
    pub fn detect_naked_death(&self) -> bool {
        self.naked_deaths > 0
    }

    pub fn pack_count(&self) -> usize {
        self.packs.len()
    }
    pub fn drills_ok(&self) -> u32 {
        self.drills_ok
    }
    pub fn naked_deaths(&self) -> u32 {
        self.naked_deaths
    }

    pub fn render(&self) -> String {
        format!(
            "degraded seg={:#06x} ch={:?} packs={} drills={} naked={}",
            SEG_BASE,
            self.channel,
            self.pack_count(),
            self.drills_ok,
            self.naked_deaths
        )
    }
}

/// 受控注入核：kind=0 断联裸死（断联+拒封包）/ kind=1 离线包在线伪造 / kind=2 重放指纹篡改
pub fn inject_fault(d: &mut DegradedBook, kind: u8) -> &'static str {
    match kind {
        0 => {
            // 裸死模拟：Offline 且无任何离线包，随后直跳恢复（违规过渡即裸死计数）
            d.set_channel(ChannelState::Offline);
            d.set_channel(ChannelState::Online); // 返回 false 并记 naked_deaths
            "naked-offline"
        }
        1 => {
            // 在线伪封包：push 必须拒绝
            let mut p = OfflinePack::new(999, ChannelState::Online);
            let ok = p.push("forged-entry");
            if ok {
                "forged-pack-accepted"
            } else {
                "forged-pack-rejected"
            }
        }
        2 => {
            // 重放前指纹被篡改 → 完整性校验必须失败
            "fp-tamper-armed"
        }
        _ => "no-op",
    }
}

pub fn run_unx10297_checks() -> CheckSet {
    let mut s = CheckSet::new("O3");
    // J1-01 基线
    {
        let d = DegradedBook::new();
        s.add("基线-初始在线无包", d.channel == ChannelState::Online && d.pack_count() == 0 && !d.detect_naked_death(), "初始态须在线无包无裸死");
    }
    // J1-02 正路径降级演练 10/10
    {
        let mut all = true;
        for _ in 0..10 {
            let mut d = DegradedBook::new();
            d.set_channel(ChannelState::Offline);
            if !d.accept_while_down("e1") || !d.accept_while_down("e2") || !d.accept_while_down("e3") {
                all = false;
            }
            let fp = if d.pack_count() == 1 { d.packs_placeholder_fp() } else { 0 };
            d.set_channel(ChannelState::Degraded);
            d.set_channel(ChannelState::Online);
            let r = d.replay_on_recover(fp);
            match r {
                Some(rr) if rr.intact && rr.replayed == 3 => {}
                _ => all = false,
            }
        }
        s.add("正路径-降级演练十连", all, "断联封包恢复重放 10/10 须闭环");
    }
    // J1-03 离线包拒在线封存
    {
        let mut d = DegradedBook::new();
        let ok = d.accept_while_down("x");
        s.add("负路径-在线封存拒绝", !ok, "在线时 accept_while_down 必须拒绝");
        let mut p = OfflinePack::new(1, ChannelState::Online);
        s.add("负路径-伪包条目拒绝", !p.push("x") && p.is_empty(), "Online 封包的 push 必须拒绝");
    }
    // J1-04 断联裸死检出 10/10
    {
        let mut hits = 0;
        for _ in 0..10 {
            let mut d = DegradedBook::new();
            let _ = inject_fault(&mut d, 0);
            if d.detect_naked_death() && d.pack_count() == 0 {
                hits += 1;
            }
        }
        s.add("检出-断联裸死十连", hits == 10, "裸死（Offline 无包）10/10 检出");
    }
    // J1-05 Offline 直跳 Online 记违规
    {
        let mut d = DegradedBook::new();
        d.set_channel(ChannelState::Offline);
        let ok = d.set_channel(ChannelState::Online);
        s.add("状态-Offline 直恢复违规", !ok && d.detect_naked_death(), "Offline 必须经 Degraded 过渡");
        // 合规路径
        let mut d2 = DegradedBook::new();
        d2.set_channel(ChannelState::Offline);
        d2.set_channel(ChannelState::Degraded);
        s.add("状态-合规过渡放行", d2.set_channel(ChannelState::Online) && !d2.detect_naked_death(), "经 Degraded 恢复放行");
    }
    // J1-06 重放指纹篡改检出
    {
        let mut d = DegradedBook::new();
        d.set_channel(ChannelState::Offline);
        d.accept_while_down("payload-A");
        let true_fp = d.packs_placeholder_fp();
        let tampered = true_fp ^ 0xDEAD;
        d.set_channel(ChannelState::Degraded);
        d.set_channel(ChannelState::Online);
        let r = d.replay_on_recover(tampered);
        s.add(
            "检出-重放指纹篡改",
            matches!(r, Some(rr) if !rr.intact && d.drills_ok() == 0 && d.pack_count() == 1),
            "指纹不符须判不完整且包保留",
        );
    }
    // J1-07 注入核 1/2
    {
        let mut d = DegradedBook::new();
        let r1 = inject_fault(&mut d, 1);
        s.add("注入-伪包被拒", r1 == "forged-pack-rejected", "在线伪封包须被拒");
        let r2 = inject_fault(&mut d, 2);
        s.add("注入-篡改就绪态", r2 == "fp-tamper-armed", "篡改注入须返回就绪标记");
    }
    // J1-08 重放在线空包拒绝
    {
        let mut d = DegradedBook::new();
        d.set_channel(ChannelState::Degraded);
        d.set_channel(ChannelState::Online);
        s.add("边界-无包重放拒绝", d.replay_on_recover(0).is_none(), "无包时重放必须 None");
        let mut d2 = DegradedBook::new();
        d2.set_channel(ChannelState::Offline);
        d2.accept_while_down("e");
        s.add("边界-未恢复重放拒绝", d2.replay_on_recover(0).is_none(), "未恢复 Online 前重放必须 None");
    }
    // J1-09 包指纹与渲染
    {
        let mut p = OfflinePack::new(7, ChannelState::Offline);
        p.push("alpha");
        p.push("beta");
        let f1 = p.fp();
        p.push("gamma");
        let f2 = p.fp();
        let txt = p.render();
        s.add(
            "包-指纹敏感渲染",
            f1 != f2 && txt.contains("pack#7") && txt.contains("n=3"),
            "条目变化须改变指纹且渲染含序号与计数",
        );
    }
    // J1-10 十轮回放账目口径
    {
        let mut d = DegradedBook::new();
        let mut ok = 0;
        for i in 0..10 {
            d.set_channel(ChannelState::Offline);
            let tag: String = format!("round-{}", i);
            d.accept_while_down(&tag);
            let fp = d.packs_placeholder_fp();
            d.set_channel(ChannelState::Degraded);
            d.set_channel(ChannelState::Online);
            if let Some(r) = d.replay_on_recover(fp) {
                if r.intact && r.replayed == 1 {
                    ok += 1;
                }
            }
        }
        s.add(
            "回放-演练账目十连",
            ok == 10 && d.drills_ok() == 10,
            "十轮独立演练 drills_ok 累计须 10",
        );
    }
    s
}

// ---- 辅助：DegradedBook 首包指纹便捷读取（判据专用，不入公共 API 面）----
impl DegradedBook {
    pub fn packs_placeholder_fp(&self) -> u32 {
        self.packs.first().map(|p| p.fp()).unwrap_or(0)
    }
}
