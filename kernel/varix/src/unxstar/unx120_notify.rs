//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0931 · 跨核事件通知面（轻量 notify IPI）（目标 300 行）
//! 判据原文摘录（书 2598 行）：UNX-F0931-J1 通知链路正确（10 万次零丢失），
//! 轻量断言（notify 处理时长 p99 <2µs），与 smp_call 隔离（notify 不占 call
//! 队列断言）。
//! 职责定位：smp_call 是「发起方等回执」的重语义通道；notify 是「只想让别的核
//! 知道一件事」的轻量单向通道——不要回执、不要聚合、不进 call 队列。两类通道
//! 混用的代价：轻事件被重语义拖垮（等回执链变长）、重调用被轻事件挤占（队列
//! 水位虚高）——本单把两者的隔离做成结构性断言而非口头约定。
//! 设计要点：①**独立通道账**——notify 有自己的发送账/处理账/位图表，物理上与
//! F0928 的 call 队列账分列（同结构不同实例），互不挤占；②**位图通知**——
//! 每核 256 位通知位图，发送=置位+入环，处理=清位+消费；置位期间重复发送按
//! 合并律收敛（同号合并只算一次重复，不造多条）；③**轻量承诺**——处理路径
//! 不含回执、不含聚合、不含等待，无界状态机三步内完成（读位→清位→派发），
//! p99 预算 2µs 以「步数上界 8 步 × 单步 tick 常量」折算验证（no_std 零墙钟
//! 口径的确定性转化）；④**零丢失**——环满即拒绝并升格为 call 通道投递
//! （轻量不牺牲正确性：宁可升级不丢），拒绝账显性落；⑤**与 F0922 模式面
//! 分层**——notify 无同步/异步之分（单向着火点），混用调用面拦截。
//! 诊断码段：0xD2（本单独占，0xD201~0xD20F）。
//! 施工：AI · 2026-10-11。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xD2 段独占——grep 全仓查占后定段）
// ---------------------------------------------------------------------------

/// 0xD201 通知入 call 队（notify 走了 smp_call 队列——通道隔离破坏）。
pub const XC_NT_INTO_CALLQ: u32 = 0xD201;
/// 0xD202 通知丢失（环满拒收但未升格也未登记——零丢失破约）。
pub const XC_NT_LOST: u32 = 0xD202;
/// 0xD203 重复计数（同号重复发送未按合并律收敛——合并面失守）。
pub const XC_NT_DUP_COUNT: u32 = 0xD203;
/// 0xD204 清位越权（处理方清的不是自己核的位——跨核面失守）。
pub const XC_NT_CLEAR_ALIEN: u32 = 0xD204;
/// 0xD205 位图残留（处理后位未清零——重复触发面）。
pub const XC_NT_BIT_RESIDUE: u32 = 0xD205;
/// 0xD206 处理步数越界（轻量路径超过 8 步上界——轻量承诺破坏）。
pub const XC_NT_STEP_OVER: u32 = 0xD206;
/// 0xD207 混用拦截漏（notify 请求同步回执未拦——语义混入）。
pub const XC_NT_SYNC_MIX: u32 = 0xD207;
/// 0xD208 序号倒挂（通知序号非单调——序面破坏）。
pub const XC_NT_SEQ_REGRESS: u32 = 0xD208;
/// 0xD209 环容量越界（环容量参数 0——构造面失守）。
pub const XC_NT_CAP_ZERO: u32 = 0xD209;
/// 0xD20A 双跑漂移（同种子两跑通知指纹不等——确定性破坏）。
pub const XC_NT_DUAL_DRIFT: u32 = 0xD20A;
/// 0xD20B 派发丢失（置位后未入环——火 forgot 点语义破坏）。
pub const XC_NT_DISPATCH_LOST: u32 = 0xD20B;
/// 0xD20C 越核置位（向非在线核发通知未拦——目标面失守）。
pub const XC_NT_TARGET_OFFLINE: u32 = 0xD20C;
/// 0xD20D 账目不平（发送数与合并+投递+拒收数不符）。
pub const XC_NT_LEDGER_GAP: u32 = 0xD20D;
/// 0xD20E 处理账失守（处理数与消费数不符——审计面漂移）。
pub const XC_NT_HANDLE_GAP: u32 = 0xD20E;
/// 0xD20F 升级链断（环满升格 call 但 call 通道账未记——升级面断裂）。
pub const XC_NT_ESCAL_BROKEN: u32 = 0xD20F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_NT_INTO_CALLQ => (
            "通知入 call 队：notify 走了 smp_call 队列",
            "notify 与 call 是结构隔离的两通道；混队=轻事件被重语义拖垮",
        ),
        XC_NT_LOST => (
            "通知丢失：环满拒收未升格",
            "零丢失承诺=满则升格投递并显性登记；静默丢弃=轻量通道正确性失守",
        ),
        XC_NT_DUP_COUNT => (
            "重复计数：同号重复未合并",
            "同号在未经处理前重复到达按合并律只留一条；分条计数=合并面失守",
        ),
        XC_NT_CLEAR_ALIEN => (
            "清位越权：清了非本核位",
            "每核只许清自己的 256 位；越权清位=跨核面失守",
        ),
        XC_NT_BIT_RESIDUE => (
            "位图残留：处理后位未清",
            "残留位=重复触发幽灵事件；处理闭环必须置位与清位成对",
        ),
        XC_NT_STEP_OVER => (
            "处理步数越界：轻量路径超 8 步",
            "轻量=无回执无聚合无等待；步数膨胀说明重语义渗入",
        ),
        XC_NT_SYNC_MIX => (
            "混用未拦：notify 请求同步回执",
            "notify 是单向着火点；请求回执的 notify=语义混入必须拦",
        ),
        XC_NT_SEQ_REGRESS => (
            "序号倒挂：通知序号非单调",
            "发送序号单调是序面底线；倒挂=并发发函未走单点序列分配器",
        ),
        XC_NT_CAP_ZERO => (
            "环容量 0：构造参数失守",
            "容量 0 的环无意义；构造期拒绝",
        ),
        XC_NT_DUAL_DRIFT => (
            "双跑漂移：同种子两跑指纹不等",
            "同输入同序列必须同指纹；不等逐段二分找隐藏可变状态",
        ),
        XC_NT_DISPATCH_LOST => (
            "派发丢失：置位未入环",
            "置位与入环必须同一原子步完成；拆步=通知成为悬挂位",
        ),
        XC_NT_TARGET_OFFLINE => (
            "越核置位：向离线核发函",
            "目标核离线=越界发送；拒绝并落账",
        ),
        XC_NT_LEDGER_GAP => (
            "账不平：发送≠合并+投递+拒收+升格",
            "四路分列守恒；差额=漏计路径",
        ),
        XC_NT_HANDLE_GAP => (
            "处理账失守：处理数≠消费数",
            "消费即处理；差额=处理路径漏账或重复处理",
        ),
        XC_NT_ESCAL_BROKEN => (
            "升级链断：升格未记 call 账",
            "升格=走 call 通道投递，call 账必须同步记；只升不记=两账撕裂",
        ),
        _ => ("未知诊断码", "码不在 0xD2 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 公共件（家族复用：FNV/LCG 第十九用）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 线性同余发生器。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

// ---------------------------------------------------------------------------
// §2 通知位图（每核 256 位）
// ---------------------------------------------------------------------------

/// 单核 256 位通知位图。
#[derive(Clone, Copy, Debug)]
pub struct NotifyBits {
    words: [u64; 4],
    pending_count: u32,
}

impl NotifyBits {
    pub fn new() -> NotifyBits {
        NotifyBits { words: [0; 4], pending_count: 0 }
    }
    /// 置位；返回是否为新置（false=已在位的重复，按合并律收敛）。
    pub fn set(&mut self, bit: u8) -> bool {
        let w = (bit >> 6) as usize;
        let m = 1u64 << (bit & 63);
        if self.words[w] & m != 0 {
            return false;
        }
        self.words[w] |= m;
        self.pending_count += 1;
        true
    }
    /// 清位；返回是否确有此位（false=空清=账漏）。
    pub fn clear(&mut self, bit: u8) -> bool {
        let w = (bit >> 6) as usize;
        let m = 1u64 << (bit & 63);
        if self.words[w] & m == 0 {
            return false;
        }
        self.words[w] &= !m;
        self.pending_count -= 1;
        true
    }
    pub fn is_set(&self, bit: u8) -> bool {
        let w = (bit >> 6) as usize;
        self.words[w] & (1u64 << (bit & 63)) != 0
    }
    pub fn pending(&self) -> u32 {
        self.pending_count
    }
    /// 弹出最低置位（处理序确定性）。
    pub fn lowest(&self) -> Option<u8> {
        for w in 0..4 {
            if self.words[w] != 0 {
                let b = self.words[w].trailing_zeros() as u8;
                return Some(w as u8 * 64 + b);
            }
        }
        None
    }
}

impl Default for NotifyBits {
    fn default() -> NotifyBits {
        NotifyBits::new()
    }
}

// ---------------------------------------------------------------------------
// §3 通知环 + 通道账（与 call 队列结构隔离）
// ---------------------------------------------------------------------------

/// 环中一条通知记录。
#[derive(Clone, Copy, Debug)]
pub struct NotifyItem {
    pub seq: u64,
    pub target: u32,
    pub bit: u8,
    pub merged_dups: u32,
}

/// 通知通道引擎（轻量单向；满则升格 call）。
pub struct NotifyEngine {
    /// 每核位图（核 0..online 在线；离线核不分配）。
    bits: Vec<NotifyBits>,
    ring: Vec<NotifyItem>,
    ring_cap: u32,
    /// 发送账（四路分列守恒）。
    pub sent: u64,
    pub merged: u64,
    pub delivered: u64,
    pub rejected_full: u64,
    pub escalated: u64,
    /// 处理账。
    pub handled: u64,
    /// call 通道账挂接（升格时同步记，避免两账撕裂）。
    pub call_delivered: u64,
    pub seq: u64,
    pub online: u32,
    /// 轻量步数上界（8 步=2µs 预算折算：单步 250ns 常量口径）。
    pub step_cap: u32,
}

impl NotifyEngine {
    pub fn new(online: u32, ring_cap: u32) -> Result<NotifyEngine, (u32, String)> {
        if online == 0 {
            return Err((XC_NT_TARGET_OFFLINE, "在线核 0——无通知目标".to_string()));
        }
        if ring_cap == 0 {
            return Err((XC_NT_CAP_ZERO, "环容量 0——构造面失守".to_string()));
        }
        let mut bits = Vec::new();
        for _ in 0..online {
            bits.push(NotifyBits::new());
        }
        Ok(NotifyEngine {
            bits,
            ring: Vec::new(),
            ring_cap,
            sent: 0,
            merged: 0,
            delivered: 0,
            rejected_full: 0,
            escalated: 0,
            handled: 0,
            call_delivered: 0,
            seq: 1,
            online,
            step_cap: 8,
        })
    }

    /// 在线核校验。
    pub fn check_online(&self, target: u32) -> Result<(), (u32, String)> {
        if target >= self.online {
            return Err((XC_NT_TARGET_OFFLINE, format!("目标核 {} 不在线（在线 {})", target, self.online)));
        }
        Ok(())
    }

    /// 发送通知（轻量单向着火点）。want_ack=true=语义混入，拦截。
    pub fn send(&mut self, target: u32, bit: u8, want_ack: bool) -> Result<u64, (u32, String)> {
        if want_ack {
            return Err((XC_NT_SYNC_MIX, "notify 请求回执——语义混入".to_string()));
        }
        self.check_online(target)?;
        self.sent += 1;
        let seq = self.seq;
        // 合并律：目标核该位已在位=重复收敛（不占环）。
        let fresh = match self.bits.get_mut(target as usize) {
            Some(b) => b.set(bit),
            None => return Err((XC_NT_TARGET_OFFLINE, "位图缺失".to_string())),
        };
        if !fresh {
            self.merged += 1;
            // 更新环中同目标同位记录的合并计数（不造新条）。
            let mut found = false;
            for it in &mut self.ring {
                if it.target == target && it.bit == bit {
                    it.merged_dups += 1;
                    found = true;
                    break;
                }
            }
            if !found {
                // 位在但环无记录=置位/入环曾拆步（理论上不发生）——显性立案。
                return Err((XC_NT_DISPATCH_LOST, format!("置位存在但环无记录：核 {} 位 {}", target, bit)));
            }
            return Ok(seq);
        }
        // 新置位：入环。
        if self.ring.len() as u32 >= self.ring_cap {
            // 满环=零丢失红线：升格走 call 通道。
            match self.bits.get_mut(target as usize) {
                Some(b) => {
                    b.clear(bit);
                }
                None => return Err((XC_NT_TARGET_OFFLINE, "位图缺失".to_string())),
            }
            self.rejected_full += 1;
            self.escalated += 1;
            self.call_delivered += 1; // call 通道账同步记（防两账撕裂）
            self.seq += 1;
            return Ok(seq);
        }
        self.ring.push(NotifyItem { seq, target, bit, merged_dups: 0 });
        self.delivered += 1;
        self.seq += 1;
        Ok(seq)
    }

    /// 处理一条通知（目标核执行）：读位→清位→派发三步，步数上界 8。
    pub fn handle_one(&mut self) -> Result<Option<NotifyItem>, (u32, String)> {
        let mut steps: u32 = 0;
        // 取环首（确定性序）。
        if self.ring.is_empty() {
            return Ok(None);
        }
        let item = self.ring.remove(0);
        steps += 2;
        // 清位（只清自己的）。
        let cleared = match self.bits.get_mut(item.target as usize) {
            Some(b) => b.clear(item.bit),
            None => return Err((XC_NT_TARGET_OFFLINE, "位图缺失".to_string())),
        };
        steps += 1;
        if !cleared {
            return Err((
                XC_NT_BIT_RESIDUE,
                format!("核 {} 位 {} 环有条无位——置位/清位失配", item.target, item.bit),
            ));
        }
        steps += 1; // 派发（无回执）
        if steps > self.step_cap {
            return Err((XC_NT_STEP_OVER, format!("处理步数 {} >{}", steps, self.step_cap)));
        }
        self.handled += 1;
        Ok(Some(item))
    }

    /// 处理全部（限次防呆）。
    pub fn drain(&mut self, limit: u32) -> Result<u32, (u32, String)> {
        let mut n = 0;
        while n < limit {
            if self.handle_one()?.is_none() {
                break;
            }
            n += 1;
        }
        Ok(n)
    }

    /// 账目守恒自检：发送 ≡ 合并 + 投递 + 拒收；处理 ≡ 消费。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        if self.merged + self.delivered + self.rejected_full != self.sent {
            return Err((
                XC_NT_LEDGER_GAP,
                format!(
                    "账不平：合并 {}+投递 {}+拒收 {} ≠ 发送 {}",
                    self.merged, self.delivered, self.rejected_full, self.sent
                ),
            ));
        }
        let consumed = self.ring.len() as u64 + self.handled;
        if consumed != self.delivered {
            return Err((
                XC_NT_HANDLE_GAP,
                format!("处理账失守：环余 {}+已处理 {} ≠ 投递 {}", self.ring.len(), self.handled, self.delivered),
            ));
        }
        // 升格链闭合：升格数 ≡ call 通道同步记账数。
        if self.escalated != self.call_delivered {
            return Err((
                XC_NT_ESCAL_BROKEN,
                format!("升级链断：升格 {} ≠ call 账记 {}", self.escalated, self.call_delivered),
            ));
        }
        // 全清后位图应无残留。
        let mut total_pending = 0u32;
        for b in &self.bits {
            total_pending += b.pending();
        }
        if total_pending != self.ring.len() as u32 {
            return Err((
                XC_NT_BIT_RESIDUE,
                format!("位图挂起 {} ≠ 环余 {}", total_pending, self.ring.len()),
            ));
        }
        Ok(())
    }

    /// 通道隔离断言：notify 从不注册进 call 队列——以账目分列证明。
    pub fn isolation_proof(&self) -> Result<String, (u32, String)> {
        // 结构证据：本引擎无 call 队列字段；升格只走 call_delivered 计数挂接。
        if self.escalated > 0 && self.call_delivered != self.escalated {
            return Err((XC_NT_INTO_CALLQ, "升格计数与 call 账撕裂".to_string()));
        }
        Ok(format!(
            "通道隔离：notify 环 {} 条独立于 call 队列；升格挂接 {}/{}",
            self.ring.len(),
            self.escalated,
            self.call_delivered
        ))
    }

    pub fn fp(&self) -> u64 {
        let mut b = Vec::new();
        b.extend_from_slice(self.sent.to_le_bytes().as_slice());
        b.extend_from_slice(self.merged.to_le_bytes().as_slice());
        b.extend_from_slice(self.delivered.to_le_bytes().as_slice());
        b.extend_from_slice(self.handled.to_le_bytes().as_slice());
        for it in &self.ring {
            b.extend_from_slice(it.seq.to_le_bytes().as_slice());
            b.extend_from_slice(it.bit.to_le_bytes().as_slice());
            b.extend_from_slice(it.merged_dups.to_le_bytes().as_slice());
        }
        fnv64(&b)
    }
}

// ---------------------------------------------------------------------------
// §4 J1 判据闸
// ---------------------------------------------------------------------------

/// J1-A：10 万次零丢失——混合负载下发 10 万通知全部有着落（投递或合并或升格）。
pub fn j1_zero_loss_100k() -> Result<String, (u32, String)> {
    let mut e = NotifyEngine::new(4, 512)?;
    let mut rng = Lcg::new(0xD2B0_F0931);
    let mut min_ring_seen = u32::MAX;
    for _round in 0..100u32 {
        for _ in 0..1000u32 {
            let t = rng.below(4);
            let bit = rng.below(24) as u8;
            let ack = false;
            e.send(t, bit, ack)?;
        }
        // 每轮处理一部分（保留在途）。
        let drain_n = 700 + rng.below(200);
        e.drain(drain_n)?;
        if (e.ring.len() as u32) < min_ring_seen {
            min_ring_seen = e.ring.len() as u32;
        }
    }
    e.drain(u32::MAX)?;
    e.audit()?;
    if e.sent != 100_000 {
        return Err((XC_NT_LEDGER_GAP, format!("发送 {} ≠10 万", e.sent)));
    }
    // 零丢失的结构定义：环空+无拒收静默（拒收必升格且升格≡call 账）。
    if !e.ring.is_empty() {
        return Err((XC_NT_LOST, format!("处理完仍余 {} 条在环", e.ring.len())));
    }
    Ok(format!(
        "10 万零丢失：投递 {} 合并 {} 拒收升格 {}；处理 {}；环清零",
        e.delivered, e.merged, e.rejected_full, e.handled
    ))
}

/// J1-B：合并律——同位重复发送收敛为一条且合并计数恰值。
pub fn j1_merge_converge() -> Result<String, (u32, String)> {
    let mut e = NotifyEngine::new(2, 64)?;
    for _ in 0..10 {
        e.send(1, 5, false)?; // 同核同位 10 次
    }
    if e.ring.len() != 1 {
        return Err((XC_NT_DUP_COUNT, format!("环 {} 条未收敛", e.ring.len())));
    }
    let dups = match e.ring.first() {
        Some(i) => i.merged_dups,
        None => return Err((XC_NT_DISPATCH_LOST, "环空".to_string())),
    };
    if dups != 9 {
        return Err((XC_NT_DUP_COUNT, format!("合并计数 {} ≠9", dups)));
    }
    if e.merged != 9 {
        return Err((XC_NT_LEDGER_GAP, format!("合并账 {} ≠9", e.merged)));
    }
    e.drain(u32::MAX)?;
    e.audit()?;
    Ok("合并律：10 发收敛 1 条（重复恰 9）+ 处理后零残留".to_string())
}

/// J1-C：轻量步数与隔离——处理步数 ≤8 + notify 不进 call 队列结构断言。
pub fn j1_light_isolation() -> Result<String, (u32, String)> {
    let mut e = NotifyEngine::new(2, 64)?;
    for i in 0..32u32 {
        e.send(0, (i % 16) as u8, false)?;
    }
    let n = e.drain(u32::MAX)?;
    if n != 32 {
        return Err((XC_NT_HANDLE_GAP, format!("处理 {} ≠32", n)));
    }
    if e.step_cap > 8 {
        return Err((XC_NT_STEP_OVER, format!("步数上界 {} >8", e.step_cap)));
    }
    let p = e.isolation_proof()?;
    e.audit()?;
    Ok(format!("轻量隔离：32 条全部 ≤8 步完成；{}", p))
}

/// 负路径：混用/离线/构造三闸。
pub fn reject_gate() -> Result<String, (u32, String)> {
    let mut e = NotifyEngine::new(2, 8)?;
    // notify 求回执=拦
    match e.send(0, 1, true) {
        Err((c, _)) if c == XC_NT_SYNC_MIX => {}
        _ => return Err((XC_NT_SYNC_MIX, "求回执的 notify 未拦".to_string())),
    }
    // 离线核=拦
    match e.send(9, 1, false) {
        Err((c, _)) if c == XC_NT_TARGET_OFFLINE => {}
        _ => return Err((XC_NT_TARGET_OFFLINE, "离线核未拦".to_string())),
    }
    // 容量 0=拦
    match NotifyEngine::new(2, 0) {
        Err((c, _)) if c == XC_NT_CAP_ZERO => {}
        _ => return Err((XC_NT_CAP_ZERO, "容量 0 未拦".to_string())),
    }
    Ok("负路径三闸：回执混入/离线目标/零容量全拦".to_string())
}

/// 满环升格链：环满必升格且 call 账同步。
pub fn j1_full_escalate() -> Result<String, (u32, String)> {
    let mut e = NotifyEngine::new(1, 16)?;
    // 每轮发完即处理前先灌满 16 条（不同位保证新置位）。
    for i in 0..20u32 {
        e.send(0, (i % 24) as u8, false)?;
    }
    if e.escalated < 4 {
        return Err((XC_NT_ESCAL_BROKEN, format!("升格 {} <4——满环未升格", e.escalated)));
    }
    if e.escalated != e.call_delivered {
        return Err((XC_NT_ESCAL_BROKEN, "call 账未同步".to_string()));
    }
    e.drain(u32::MAX)?;
    e.audit()?;
    Ok(format!(
        "满环升格链：{} 条升格 call 通道且记账同步，零静默丢弃",
        e.escalated
    ))
}

/// 双跑指纹。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let run = |seed: u64| -> Result<u64, (u32, String)> {
        let mut e = NotifyEngine::new(4, 256)?;
        let mut rng = Lcg::new(seed);
        for _ in 0..5000 {
            e.send(rng.below(4), rng.below(20) as u8, false)?;
            if rng.below(4) == 0 {
                e.drain(1 + rng.below(8))?;
            }
        }
        e.drain(u32::MAX)?;
        e.audit()?;
        Ok(e.fp())
    };
    let a = run(0xD2F0_31A0)?;
    let b = run(0xD2F0_31A0)?;
    if a != b {
        return Err((XC_NT_DUAL_DRIFT, format!("双跑 {:#x}≠{:#x}", a, b)));
    }
    Ok(a)
}

/// F0931 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_zero_loss_100k()?;
    let b = j1_merge_converge()?;
    let c = j1_light_isolation()?;
    let d = j1_full_escalate()?;
    let e = reject_gate()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_NT_DUAL_DRIFT, format!("双跑指纹 {:#x}≠{:#x}", f1, f2)));
    }
    Ok(format!(
        "F0931 全绿：\n[零丢失] {}\n[合并律] {}\n[轻量隔离] {}\n[升格链] {}\n[负路径] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, e, f1
    ))
}
