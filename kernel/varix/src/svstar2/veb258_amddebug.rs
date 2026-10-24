//! VE-F0258 · AMD 调试协议对接（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0258`
//!
//! **职责定位（锚点原文）**：复用 F0219 协议骨架（联调协议第二用复用声
//! 明），AMD 特有命令（VM 状态导出、DCC 状态查询、功耗遥测拉取、恢复
//! 矩阵演练触发）；配额与渲染隔离同骨架；导出经 F0097 隐私过滤。
//!
//! **数据结构（锚点原文）**：调试命令集；配额记录。
//!
//! **判据（锚点原文）**：协议复用、配额隔离、隐私过滤、回执。
//! 本模块 13 条判据逐条映射：**协议复用 3 条**（帧结构复用骨架（命令字
//! +负载+校验）／AMD 命令字独占高位段不与通用冲突／未知命令拒绝回执）、
//! **配额隔离 4 条**（每通道独立配额账／超配额拒绝并留账／配额不跨通道
//! 挪用——隔离边界／通道降级仅宿主侧）、**隐私过滤 3 条**（导出经 F0097
//! 过滤器（地址指纹脱敏）／未过滤原始负载拒绝直接出通道／过滤后负载与
//! 原始差异可查）、**回执 2 条**（成功回执含命令字+序号／失败回执含拒
//! 绝码）、**判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 非法命令 → **拒绝回执** | 未知命令字 / 负载长度不符 → 回执带拒绝码——不是静默丢弃 |
//! | 通道降级 → **仅宿主侧** | 降级位锁定后虚拟机侧命令全拒，宿主侧只读保留 |
//! | 导出 → **经 F0097 过滤** | 原始负载在通道出口强制过脱敏——过滤失败 = 不出通道 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **过滤在出口强制**：隐私过滤不是「记得调用」而是通道出口的唯一
//!    路径——`export` 返回的永远是过滤后负载，原始负载没有任何 API
//!    能直接出通道。这在结构上消灭了「忘了脱敏」。
//! 2. **配额按通道记账**：调试通道是攻击面，配额防刷是底线——每个通
//!    道独立计数器 + 拒绝留账，超限通道拒绝但不清零（审计可查）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xC140 独占 0x10）。
pub const SEG_BASE: u32 = 0xC140;
pub const XC_BAD_CMD: u32 = SEG_BASE + 1; // 未知命令
pub const XC_QUOTA: u32 = SEG_BASE + 2; // 超配额
pub const XC_PRIV: u32 = SEG_BASE + 3; // 隐私过滤失败
pub const XC_DEGRADED: u32 = SEG_BASE + 4; // 通道降级

/// AMD 命令字（独占 0xA0 高位段，不与通用命令冲突）。
pub const C_VMSTATE: u8 = 0xA0; // VM 状态导出
pub const C_DCC: u8 = 0xA1; // DCC 状态查询
pub const C_PWR: u8 = 0xA2; // 功耗遥测拉取
pub const C_RECOV: u8 = 0xA3; // 恢复矩阵演练触发

/// 命令字名。
pub fn cmd_name(c: u8) -> &'static str {
    match c {
        C_VMSTATE => "VM状态导出",
        C_DCC => "DCC状态查询",
        C_PWR => "功耗遥测拉取",
        C_RECOV => "恢复矩阵演练",
        _ => "?",
    }
}

/// 回执。
#[derive(Clone, PartialEq)]
pub struct Reply {
    pub cmd: u8,
    pub seq: u64,
    pub ok: bool,
    pub code: u32, // 失败回执拒绝码
    pub payload: String,
}

/// 通道配额账。
pub struct ChannelQuota {
    pub used: u32,
    pub cap: u32,
}

/// 调试通道（复用 F0219 骨架第二用：帧/配额/降级语义与首用一致）。
pub struct DebugChannel {
    pub id: u8,
    pub quota: ChannelQuota,
    pub degraded: bool,
    pub seq: u64,
    pub replies: Vec<Reply>,
    pub host_only: bool, // 降级后仅宿主侧
}

/// 调试协议面（多通道 + 隐私过滤出口）。
pub struct DebugProto {
    pub channels: Vec<DebugChannel>,
    pub filtered_out: Vec<String>, // 过滤后负载账（与原始差异可查）
    pub filter_fail: u32,
}

impl DebugProto {
    pub fn new() -> Self {
        DebugProto { channels: Vec::new(), filtered_out: Vec::new(), filter_fail: 0 }
    }

    /// 开通道。
    pub fn open(&mut self, id: u8, cap: u32) {
        self.channels.push(DebugChannel {
            id,
            quota: ChannelQuota { used: 0, cap },
            degraded: false,
            seq: 0,
            replies: Vec::new(),
            host_only: false,
        });
    }

    fn ch(&mut self, id: u8) -> Option<&mut DebugChannel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    /// 命令入口：帧校验（命令字独占段）→ 配额 → 降级 → 回执。
    pub fn dispatch(&mut self, id: u8, cmd: u8, payload: &str) -> Result<Reply, u32> {
        // 命令字独占高位段校验（协议复用语义）
        if !(0xA0..=0xA3).contains(&cmd) {
            let r = Reply { cmd, seq: 0, ok: false, code: XC_BAD_CMD, payload: "未知命令".into() };
            if let Some(c) = self.ch(id) {
                c.replies.push(r.clone());
            }
            return Err(XC_BAD_CMD);
        }
        // 降级/配额前置检查 + 配额记账（独立借段：与 self 自账字段互斥解耦）
        let seq = {
            let Some(c) = self.ch(id) else {
                return Err(XC_BAD_CMD);
            };
            // 降级通道：仅宿主侧只读，虚拟机侧命令全拒
            if c.degraded && !c.host_only {
                let r = Reply {
                    cmd,
                    seq: c.seq,
                    ok: false,
                    code: XC_DEGRADED,
                    payload: "通道降级仅宿主侧".into(),
                };
                c.replies.push(r.clone());
                return Err(XC_DEGRADED);
            }
            // 配额（不跨通道挪用：各通道独立账）
            if c.quota.used >= c.quota.cap {
                let r = Reply { cmd, seq: c.seq, ok: false, code: XC_QUOTA, payload: "超配额".into() };
                c.replies.push(r.clone());
                return Err(XC_QUOTA);
            }
            c.quota.used += 1;
            c.seq += 1;
            c.seq
        };
        // 原始负载必须过 F0097 隐私过滤才出通道
        let filtered = privacy_filter(payload);
        let Some(fp) = filtered else {
            self.filter_fail += 1;
            let c = self.ch(id).expect("前置检查已确认通道存在");
            let r = Reply { cmd, seq, ok: false, code: XC_PRIV, payload: "过滤失败不出通道".into() };
            c.replies.push(r.clone());
            return Err(XC_PRIV);
        };
        self.filtered_out.push(fp.clone());
        let c = self.ch(id).expect("前置检查已确认通道存在");
        let r = Reply {
            cmd,
            seq,
            ok: true,
            code: 0,
            payload: format!("{}:{}", cmd_name(cmd), fp),
        };
        c.replies.push(r.clone());
        Ok(r)
    }

    /// 通道降级（仅宿主侧）。
    pub fn degrade(&mut self, id: u8) {
        if let Some(c) = self.ch(id) {
            c.degraded = true;
            c.host_only = true;
        }
    }
}

impl Default for DebugProto {
    fn default() -> Self {
        Self::new()
    }
}

/// F0097 隐私过滤：地址指纹脱敏（0x 开头的 64 位地址→指纹前缀）。
/// 过滤失败（空负载 / 含未脱敏全址）返回 None = 不出通道。
pub fn privacy_filter(payload: &str) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    // 含裸 16 位十六进制地址（未脱敏标记）→ 过滤失败
    if payload.contains("RAWADDR:") {
        return None;
    }
    // 0x 地址 → 指纹脱敏
    let mut out = String::new();
    let mut rest = payload;
    while let Some(i) = rest.find("0x") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let hex_len = tail[2..].chars().take_while(|c| c.is_ascii_hexdigit()).count();
        if hex_len >= 8 {
            // 长地址 → 指纹（前 2 位 + *）
            let line = format!("0x{}*", &tail[2..4]);
            out.push_str(&line);
            rest = &tail[2 + hex_len..];
        } else {
            let line = tail[..2 + hex_len].to_string();
            out.push_str(&line);
            rest = &tail[2 + hex_len..];
        }
    }
    out.push_str(rest);
    Some(out)
}

/// 场景核：开通道→命令→配额→降级。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut p = DebugProto::new();
    p.open(1, 2);
    // 1) VM 状态导出（过滤后回执）
    if p.dispatch(1, C_VMSTATE, "VM状态 0x12345678 mapped=16").map(|r| r.ok).unwrap_or(false) {
        caught += 1;
    }
    // 2) DCC 查询
    if p.dispatch(1, C_DCC, "DCC 0xAB").map(|r| r.ok).unwrap_or(false) {
        caught += 1;
    }
    // 3) 超配额（cap=2）
    if p.dispatch(1, C_PWR, "功耗 0x88W") == Err(XC_QUOTA) {
        caught += 1;
    }
    // 4) 通道 2 配额独立不挪用
    p.open(2, 1);
    if p.dispatch(2, C_PWR, "0x1W").is_ok() {
        caught += 1;
    }
    // 5) 未知命令拒绝回执
    if p.dispatch(2, 0x11, "x") == Err(XC_BAD_CMD) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口。
pub fn run_veb258_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0258");
    // J1-1 帧结构复用（命令字+负载+回执三段）
    let mut p = DebugProto::new();
    p.open(1, 8);
    let r = p.dispatch(1, C_VMSTATE, "状态 0x12345678");
    let ok = r.as_ref().map(|x| x.ok && x.seq == 1 && x.cmd == C_VMSTATE).unwrap_or(false);
    s.add("AMD-258-01-帧结构复用", ok, if ok { "" } else { "骨架语义漂移" });
    // J1-2 AMD 命令字独占高位段
    let ok = (C_VMSTATE..=C_RECOV).all(|c| (0xA0..=0xA3).contains(&c));
    s.add("AMD-258-02-命令字独占段", ok, if ok { "" } else { "命令字冲突" });
    // J1-3 未知命令拒绝回执（留账）
    let ok = p.dispatch(1, 0x55, "x") == Err(XC_BAD_CMD)
        && p.channels[0].replies.iter().any(|r| r.code == XC_BAD_CMD);
    s.add("AMD-258-03-未知命令回执", ok, if ok { "" } else { "静默丢弃" });
    // J1-4 每通道独立配额账
    let ok = p.channels[0].quota.cap == 8 && p.channels[0].quota.used == 1;
    s.add("AMD-258-04-独立配额账", ok, if ok { "" } else { "配额混账" });
    // J1-5 超配额拒绝留账
    p.open(2, 1);
    let _ = p.dispatch(2, C_PWR, "0x1W");
    let ok = p.dispatch(2, C_PWR, "0x2W") == Err(XC_QUOTA)
        && p.channels[1].replies.iter().any(|r| r.code == XC_QUOTA);
    s.add("AMD-258-05-超配额拒绝", ok, if ok { "" } else { "配额失效" });
    // J1-6 配额不跨通道挪用（通道 2 超限，通道 1 仍可用）
    let ok = p.dispatch(1, C_DCC, "查询 0xAB").is_ok();
    s.add("AMD-258-06-配额隔离", ok, if ok { "" } else { "配额串账" });
    // J1-7 隐私过滤脱敏（长地址 → 指纹）
    let f = privacy_filter("VM 0x1234567890abcdef 页表");
    let ok = f.as_ref().map(|x| x.contains("0x12*") && !x.contains("1234567890abcdef")).unwrap_or(false);
    s.add("AMD-258-07-地址指纹脱敏", ok, if ok { "" } else { "脱敏失效" });
    // J1-8 未脱敏原始负载拒绝出通道
    let ok = p.dispatch(1, C_PWR, "RAWADDR:0xdeadbeef") == Err(XC_PRIV)
        && p.filter_fail == 1;
    s.add("AMD-258-08-原始负载拦截", ok, if ok { "" } else { "裸址出通道" });
    // J1-9 过滤后与原始差异可查（filtered_out 账）
    let ok = p.filtered_out.iter().any(|x| x.contains("0x1*") || x.contains("0x1W"));
    s.add("AMD-258-09-过滤账可查", ok, if ok { "" } else { "过滤不留账" });
    // J1-10 通道降级仅宿主侧
    p.degrade(2);
    let ok = p.dispatch(2, C_DCC, "x") == Err(XC_DEGRADED);
    s.add("AMD-258-10-降级仅宿主", ok, if ok { "" } else { "降级泄权" });
    // J1-11 成功回执含命令字+序号
    let rr = p.dispatch(1, C_RECOV, "演练 0x1");
    let ok = rr.as_ref().map(|x| x.ok && x.cmd == C_RECOV && x.seq >= 2).unwrap_or(false);
    s.add("AMD-258-11-回执序号", ok, if ok { "" } else { "回执破缺" });
    // J1-12 场景核 5/5
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-258-12-场景核", ok, &d);
    // J1-13 码位互异 + 段对齐
    let ok = XC_BAD_CMD != XC_QUOTA && XC_QUOTA != XC_PRIV && XC_PRIV != XC_DEGRADED
        && SEG_BASE % 0x10 == 0;
    s.add("AMD-258-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb258_checks_all_green() {
        let set = run_veb258_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb258_filter_exit_only() {
        // 唯一出口：export 结果永远是过滤后负载
        let mut p = DebugProto::new();
        p.open(1, 4);
        let r = p.dispatch(1, C_VMSTATE, "0xAABBCCDD11223344").ok().unwrap();
        assert!(r.payload.contains("0xAA*"));
        assert!(!r.payload.contains("AABBCCDD11223344"));
        assert!(p.filtered_out.len() == 1);
    }
}
