//! cga74_veiface —— CGPU-F0074 与 VE-D/F 域接口冻结。
//! 锚点（CGPU 施工书）：CGPU-F0074 · 与 VE-D/F 域接口冻结 · 目标 340 行
//! 判据摘录：跨域接口闭集（四通道：像素缓冲移交/诊断码透传/
//!   预算协商/错误回报）；通道版本冻结（各通道 v1 语义表逐条对
//!   拍）；诊断码透传不重编（CGPU 段码原样出境——重编 = 溯源断
//!   裂立案）；预算协商闭集（请求/授予/回收三态，授予 ≤ 请求，
//! 回收 ≤ 授予）；错误回报保序（FIFO——乱序回报立案）。
//! 组件：通道模型（四通道枚举）+ 版本冻结表 + 诊断码透传器 +
//!   预算三态协商机 + FIFO 回报账 + 冻结审计核。
//! 职责定位：跨域边界的「海关」——A 域（CGPU）与 D/F 域（渲染
//!   合成/事件）的每次交接都要过账；通道语义漂移（版本静默变更）
//!   是跨域联调的头号事故源，本模块把漂移变成账面立案。
//! 性能逐项分解：透传 O(1)；协商 O(1)；回报账 O(n)。全整数确
//!   定性。
//! 诊断码段：0xE820（0xE821~0xE829 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VI_VERSION: &str = "cga74_veiface-v1";

/// 诊断段基址（0xE820 段 · 本模块独占）。
pub const VI_SEG_BASE: u16 = 0xE820;

/// XcCode 闭集（0xE821~0xE829）。
pub const XC_CH_UNSUP: u32 = 0xE821;
pub const XC_VER_DRIFT: u32 = 0xE822;
pub const XC_CODE_REMAP: u32 = 0xE823;
pub const XC_GRANT_OVER: u32 = 0xE824;
pub const XC_RECLAIM_OVER: u32 = 0xE825;
pub const XC_FIFO_BREAK: u32 = 0xE826;
pub const XC_BUF_SHORT: u32 = 0xE827;
pub const XC_STATE_OOB: u32 = 0xE828;
pub const XC_NEG_ZERO: u32 = 0xE829;

/// 四通道闭集。
pub const CH_PIXEL: u8 = 0; // 像素缓冲移交
pub const CH_DIAG: u8 = 1; // 诊断码透传
pub const CH_BUDGET: u8 = 2; // 预算协商
pub const CH_ERRRET: u8 = 3; // 错误回报

/// 通道版本冻结表（v1 = 全通道基线）。
pub const IFACE_VER: u32 = 1;

pub fn channel_ver(ch: u8) -> Result<u32, u32> {
    match ch {
        CH_PIXEL | CH_DIAG | CH_BUDGET | CH_ERRRET => Ok(IFACE_VER),
        _ => Err(XC_CH_UNSUP),
    }
}

/// 版本冻结审计（通道版本表逐条 == 基线）。
pub fn ver_frozen_audit() -> Result<(), u32> {
    for ch in [CH_PIXEL, CH_DIAG, CH_BUDGET, CH_ERRRET].iter() {
        if channel_ver(*ch)? != IFACE_VER {
            return Err(XC_VER_DRIFT);
        }
    }
    Ok(())
}

/// 像素缓冲移交账（通道 0）：移交最小单元 = 宽×高×bpp 三元 +
/// 长度守恒（缓冲长 ≥ w×h×bpp）。
pub fn pixel_handoff(w: usize, h: usize, bpp: usize, buf_len: usize) -> Result<(), u32> {
    if w == 0 || h == 0 || bpp == 0 {
        return Err(XC_BUF_SHORT);
    }
    let need = w.checked_mul(h).ok_or(XC_BUF_SHORT)?.checked_mul(bpp).ok_or(XC_BUF_SHORT)?;
    if buf_len < need {
        return Err(XC_BUF_SHORT);
    }
    Ok(())
}

/// 诊断码透传器（通道 1）：段码原样出境（不重编——溯源账）。
/// 合法段 = 本域独占段闭集（0xE500~0xE6F0 的 A 域冻结段）。
pub fn diag_passthrough(code: u32) -> Result<u32, u32> {
    let seg = code & 0xFFF0;
    let in_a_domain = (0xE500..=0xE6F0).contains(&seg) && (code & 0xF) >= 1 && (code & 0xF) <= 9;
    if !in_a_domain {
        return Err(XC_CODE_REMAP);
    }
    Ok(code)
}

/// 预算协商三态机（通道 2）：请求 → 授予（≤ 请求）→ 回收（≤ 授予）。
pub struct BudgetNeg {
    pub state: u8, // 0=idle 1=requested 2=granted 3=settled
    pub requested: u64,
    pub granted: u64,
    pub reclaimed: u64,
}

pub const NS_IDLE: u8 = 0;
pub const NS_REQUESTED: u8 = 1;
pub const NS_GRANTED: u8 = 2;
pub const NS_SETTLED: u8 = 3;

impl BudgetNeg {
    pub fn new() -> BudgetNeg {
        BudgetNeg { state: NS_IDLE, requested: 0, granted: 0, reclaimed: 0 }
    }

    pub fn request(&mut self, n: u64) -> Result<(), u32> {
        if self.state != NS_IDLE {
            return Err(XC_STATE_OOB);
        }
        if n == 0 {
            return Err(XC_NEG_ZERO);
        }
        self.requested = n;
        self.state = NS_REQUESTED;
        Ok(())
    }

    pub fn grant(&mut self, n: u64) -> Result<(), u32> {
        if self.state != NS_REQUESTED {
            return Err(XC_STATE_OOB);
        }
        if n > self.requested {
            return Err(XC_GRANT_OVER);
        }
        self.granted = n;
        self.state = NS_GRANTED;
        Ok(())
    }

    pub fn reclaim(&mut self, n: u64) -> Result<(), u32> {
        if self.state != NS_GRANTED {
            return Err(XC_STATE_OOB);
        }
        if n > self.granted {
            return Err(XC_RECLAIM_OVER);
        }
        self.reclaimed = n;
        self.state = NS_SETTLED;
        Ok(())
    }
}

/// FIFO 回报账（通道 3）：错误回报严格先进先出（序号单调递增）。
pub struct RetLedger {
    next_seq: u64,
    pub entries: Vec<(u64, u32)>, // (seq, 诊断码)
}

impl RetLedger {
    pub fn new() -> RetLedger {
        RetLedger { next_seq: 1, entries: Vec::new() }
    }

    pub fn push(&mut self, seq: u64, code: u32) -> Result<(), u32> {
        if seq != self.next_seq {
            return Err(XC_FIFO_BREAK);
        }
        diag_passthrough(code)?;
        self.entries.push((seq, code));
        self.next_seq += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga74_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0074");
    // 族一：通道闭集 + 版本冻结。
    cs.add("四通道注册", channel_ver(CH_PIXEL).is_ok() && channel_ver(CH_DIAG).is_ok() && channel_ver(CH_BUDGET).is_ok() && channel_ver(CH_ERRRET).is_ok(), "闭集齐");
    cs.add("通道越界拒", channel_ver(9).is_err(), "CH_UNSUP");
    cs.add("版本冻结", ver_frozen_audit().is_ok(), "v1 基线");
    // 族二：像素移交守恒。
    cs.add("移交合法", pixel_handoff(8, 8, 4, 256).is_ok(), "256 恰足");
    cs.add("移交短缺拒", pixel_handoff(8, 8, 4, 255).is_err(), "BUF_SHORT");
    cs.add("移交零维拒", pixel_handoff(0, 8, 4, 256).is_err(), "BUF_SHORT");
    // 族三：诊断码透传（不重编）。
    cs.add("段内透传", matches!(diag_passthrough(0xE501), Ok(0xE501)), "原样出境");
    cs.add("段内高位", matches!(diag_passthrough(0xE6F9), Ok(0xE6F9)), "A 域末段");
    cs.add("外域码拒", diag_passthrough(0xE300).is_err(), "CODE_REMAP");
    cs.add("槽位越界拒", diag_passthrough(0xE50A).is_err(), "越 1~9 闭集");
    cs.add("零槽拒", diag_passthrough(0xE500).is_err(), "段基址非错误码");
    // 族四：预算三态协商。
    let mut n = BudgetNeg::new();
    let _ = n.request(100).map_err(|e| e as u16);
    cs.add("请求态", n.state == NS_REQUESTED && n.requested == 100, "入态 1");
    let _ = n.grant(80).map_err(|e| e as u16);
    cs.add("授予 ≤ 请求", n.granted == 80 && n.state == NS_GRANTED, "80 ≤ 100");
    let _ = n.reclaim(80).map_err(|e| e as u16);
    cs.add("回收 == 授予", n.reclaimed == 80 && n.state == NS_SETTLED, "清账");
    let mut n2 = BudgetNeg::new();
    let _ = n2.request(50).map_err(|e| e as u16);
    cs.add("授予超请求拒", n2.grant(60).is_err(), "GRANT_OVER");
    let _ = n2.grant(40).map_err(|e| e as u16);
    cs.add("回收超授予拒", n2.reclaim(50).is_err(), "RECLAIM_OVER");
    let mut n3 = BudgetNeg::new();
    cs.add("零请求拒", n3.request(0).is_err(), "NEG_ZERO");
    cs.add("跳态拒", n3.grant(1).is_err(), "STATE_OOB");
    // 族五：FIFO 回报账。
    let mut led = RetLedger::new();
    let _ = led.push(1, 0xE501).map_err(|e| e as u16);
    let _ = led.push(2, 0xE5B2).map_err(|e| e as u16);
    cs.add("FIFO 两笔", led.len() == 2, "序号 1→2");
    cs.add("跳序拒", led.push(9, 0xE501).is_err(), "FIFO_BREAK");
    cs.add("回序拒", led.push(2, 0xE501).is_err(), "FIFO_BREAK");
    cs.add("外域回报拒", led.push(3, 0xE300).is_err(), "透传闸前置");
    deepen_veif::merge_into(&mut cs);
    deepen_veif2::merge_into(&mut cs);
    merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga74_veiface）：真功能面 + 判据并入主核 =====
pub mod deepen_veif {
    use super::*;

    /// 扩展一：移交批账（多缓冲移交——Σ缓冲长守恒）。
    pub fn batch_handoff(specs: &[(usize, usize, usize)], buf_lens: &[usize]) -> Result<(), u32> {
        if specs.len() != buf_lens.len() {
            return Err(XC_BUF_SHORT);
        }
        for (k, &(w, h, bpp)) in specs.iter().enumerate() {
            pixel_handoff(w, h, bpp, buf_lens[k])?;
        }
        Ok(())
    }

    /// 扩展二：诊断码透传审计账（批透传——全过或全拒，不许部分
    /// 出境：批内一笔非法 → 整批拒绝）。
    pub fn diag_batch(codes: &[u32]) -> Result<(), u32> {
        for &c in codes.iter() {
            diag_passthrough(c)?;
        }
        Ok(())
    }

    /// 扩展三：协商周期账（完整周期计数——request→grant→reclaim
    /// 为一周期，账面可清点）。
    pub fn neg_cycles(seq: &[u8]) -> Result<usize, u32> {
        // 合法周期形：(0,1,2,3) 四态序列。
        let mut cycles = 0usize;
        let mut k = 0usize;
        while k + 3 < seq.len() {
            if seq[k] == NS_IDLE + 1 && seq[k + 1] == NS_GRANTED && seq[k + 2] == NS_SETTLED {
                cycles += 1;
                k += 3;
            } else {
                k += 1;
            }
        }
        Ok(cycles)
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("批移交", batch_handoff(&[(8, 8, 4), (16, 16, 1)], &[256, 256]).is_ok(), "双缓冲齐");
        cs.add("批移交短拒", batch_handoff(&[(8, 8, 4)], &[100]).is_err(), "短缺");
        cs.add("批移交失配拒", batch_handoff(&[(8, 8, 4), (8, 8, 4)], &[256]).is_err(), "LEN 失配");
        cs.add("批透传全过", diag_batch(&[0xE501, 0xE5B2, 0xE6F9]).is_ok(), "三笔出境");
        cs.add("批透传连坐", diag_batch(&[0xE501, 0xE300]).is_err(), "一笔非法全拒");
        cs.add("协商一周期", matches!(neg_cycles(&[1, 2, 3, 1, 2, 3]), Ok(2)), "双周期");
        cs.add("协商断周期", matches!(neg_cycles(&[1, 3, 2]), Ok(0)), "乱序不认");
    }

}

/// ===== 深化扩展块 2（cga74_veiface）=====
pub mod deepen_veif2 {
    use super::*;

    /// 扩展四：通道容量账（每通道在飞消息上限——背压面）。
    pub struct ChanCap {
        caps: [usize; 4],
        used: [usize; 4],
    }

    impl ChanCap {
        pub fn new(caps: [usize; 4]) -> Result<ChanCap, u32> {
            for c in caps.iter() {
                if *c == 0 {
                    return Err(XC_BUF_SHORT);
                }
            }
            Ok(ChanCap { caps, used: [0; 4] })
        }
        pub fn send(&mut self, ch: u8) -> Result<(), u32> {
            if ch > CH_ERRRET {
                return Err(XC_CH_UNSUP);
            }
            if self.used[ch as usize] >= self.caps[ch as usize] {
                return Err(XC_BUF_SHORT);
            }
            self.used[ch as usize] += 1;
            Ok(())
        }
        pub fn recv(&mut self, ch: u8) -> Result<(), u32> {
            if ch > CH_ERRRET {
                return Err(XC_CH_UNSUP);
            }
            if self.used[ch as usize] == 0 {
                return Err(XC_FIFO_BREAK);
            }
            self.used[ch as usize] -= 1;
            Ok(())
        }
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut cap = match ChanCap::new([2, 2, 2, 2]) {
            Ok(x) => x,
            Err(_) => return,
        };
        let _ = cap.send(CH_DIAG).map_err(|e| e as u16);
        let _ = cap.send(CH_DIAG).map_err(|e| e as u16);
        cs.add("容量满拒", cap.send(CH_DIAG).is_err(), "背压触发");
        let _ = cap.recv(CH_DIAG).map_err(|e| e as u16);
        cs.add("消费后可写", cap.send(CH_DIAG).is_ok(), "槽位回笼");
        cs.add("空收拒", cap.recv(CH_PIXEL).is_err(), "FIFO_BREAK");
        cs.add("通道越界拒", cap.send(9).is_err(), "CH_UNSUP");
        cs.add("零容量拒", ChanCap::new([2, 0, 2, 2]).is_err(), "BUF_SHORT");
    }

}

/// ===== 深化扩展块 3：移交面收尾 =====
/// 扩展五：通道语义摘要账（四通道语义一句话冻结——接口文档的
/// 机器可核形）。
pub fn channel_summary(ch: u8) -> Result<&'static str, u32> {
    match ch {
        CH_PIXEL => Ok("buffer-handoff"),
        CH_DIAG => Ok("diag-passthrough"),
        CH_BUDGET => Ok("budget-negotiation"),
        CH_ERRRET => Ok("error-fifo"),
        _ => Err(XC_CH_UNSUP),
    }
}

/// 扩展六：全通道巡检核（版本 + 摘要双账逐通道一致）。
pub fn channel_patrol() -> Result<(), u32> {
    for ch in [CH_PIXEL, CH_DIAG, CH_BUDGET, CH_ERRRET].iter() {
        if channel_ver(*ch)? != IFACE_VER {
            return Err(XC_VER_DRIFT);
        }
        channel_summary(*ch)?;
    }
    Ok(())
}

/// 深化判据并入主核。
pub fn merge_into(cs: &mut CheckSet) {
    cs.add("摘要四通道", channel_summary(CH_PIXEL).is_ok() && channel_summary(CH_DIAG).is_ok() && channel_summary(CH_BUDGET).is_ok() && channel_summary(CH_ERRRET).is_ok(), "语义冻结");
    cs.add("摘要越界拒", channel_summary(9).is_err(), "CH_UNSUP");
    cs.add("全通道巡检", channel_patrol().is_ok(), "版本+摘要双账");
}
