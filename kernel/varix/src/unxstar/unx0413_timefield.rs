//! unx0413_timefield —— UNX-F0413 计时交接字段（A2 预备）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0413 · 计时交接字段 · 目标 240 行
//! 判据原文：UNX-F0413-J1 计时面各账（墙钟偏移/主时钟源/定时
//!           队列/抖动限/跳变案）的关键字段按统一行式打包成
//!           A2 交接包；字段三态（冻结/活性/缺账）可判定；
//!           交接包两次采集签名必须一致（确定性核）；字段篡改
//!           可检出（FNV32 签对拍）；冻结字段改写拒绝、缺账
//!           字段显式声明。
//! 组件：字段行（tag + 值 + 单位 + 三态）+ 交接包（行数组 +
//!       封包签 + FNV32）+ 二次采集核 + 篡改对拍 + 演化留档。
//! 职责定位：计时交接字段是「计时面的出口账」——与内存面交
//!   接（UNX-F0378）同构，覆盖对象换成计时面五账；三态语义
//!   一致：冻结字段（主时钟源名）不许再变，活性字段（偏移、
//!   定时数）允许演化留档，缺账字段（未探测的源）显式声明。
//! 性能逐项分解：打包 O(字段数)；签名 O(字段数)；对拍 O(1)。
//! 诊断码段：0x210C0（0x210C1~0x210C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TF_VERSION: &str = "unx0413_timefield-v1";

/// 诊断段基址（0x210C0 段 · 本模块独占）。
pub const TF_SEG_BASE: u32 = 0x210C0;
pub const XC_FROZEN_MUT: u32 = 0x210C1; // 冻结字段被改
pub const XC_SIG_MISMATCH: u32 = 0x210C2; // 签名不符
pub const XC_SILENT_MISS: u32 = 0x210C3; // 字段静默缺失
pub const XC_DUP_TAG: u32 = 0x210C4; // 字段重号
pub const XC_STATE_OOR: u32 = 0x210C5; // 三态越域
pub const XC_FROZEN_OPEN: u32 = 0x210C6; // 封包后操作
pub const XC_TAMPER: u32 = 0x210C7; // 篡改检不出
pub const XC_MAGIC: u32 = 0x210C8; // 包魔数破
pub const XC_EMPTY: u32 = 0x210C9; // 空包拒

/// 字段三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TfState {
    /// 冻结。
    Frozen = 1,
    /// 活性。
    Live = 2,
    /// 缺账。
    Missing = 3,
}

impl TfState {
    /// 编码。
    pub fn code(&self) -> u32 {
        match self {
            TfState::Frozen => 1,
            TfState::Live => 2,
            TfState::Missing => 3,
        }
    }
}

/// 字段行（计时面统一 tag：1=墙钟偏移 2=主时钟源号 3=定时在册数
/// 4=抖动限 5=跳变案数 6=RTC 底值 7=TSC 频率 8=deadline 当前值）。
#[derive(Clone, Copy, Debug)]
pub struct TfRow {
    pub tag: u32,
    pub value: u64,
    /// 单位（0=计数 1=Hz 2=秒 3=纳秒）。
    pub unit: u32,
    pub state: TfState,
}

/// 计时交接包。
pub struct TimePack {
    rows: Vec<TfRow>,
    sealed: bool,
    seal_sig: u32,
    evolved: Vec<(u32, u64)>,
}

impl TimePack {
    /// 建空包。
    pub fn new() -> TimePack {
        TimePack { rows: Vec::new(), sealed: false, seal_sig: 0, evolved: Vec::new() }
    }

    /// 追加字段。
    pub fn put(&mut self, tag: u32, value: u64, unit: u32, state: TfState) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_FROZEN_OPEN);
        }
        if state.code() < 1 || state.code() > 3 {
            return Err(XC_STATE_OOR);
        }
        for r in self.rows.iter() {
            if r.tag == tag {
                return Err(XC_DUP_TAG);
            }
        }
        self.rows.push(TfRow { tag, value, unit, state });
        Ok(())
    }

    /// 活性改写留档。
    pub fn mutate(&mut self, tag: u32, new_value: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_FROZEN_OPEN);
        }
        let mut hit = false;
        for r in self.rows.iter_mut() {
            if r.tag == tag {
                match r.state {
                    TfState::Frozen => return Err(XC_FROZEN_MUT),
                    TfState::Missing => return Err(XC_SILENT_MISS),
                    TfState::Live => {
                        self.evolved.push((tag, r.value));
                        r.value = new_value;
                        hit = true;
                    }
                }
                break;
            }
        }
        if !hit {
            return Err(XC_SILENT_MISS);
        }
        Ok(())
    }

    /// 封包。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.rows.is_empty() {
            return Err(XC_EMPTY);
        }
        let (_, sig) = self.raw_words();
        self.sealed = true;
        self.seal_sig = sig;
        Ok(sig)
    }

    /// 词位流。
    fn raw_words(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0xA2D1_0001u32); // 魔数：A2 计时交接 v1
        words.push(self.rows.len() as u32);
        for r in self.rows.iter() {
            words.push(r.tag);
            words.push(r.value as u32);
            words.push((r.value >> 32) as u32);
            words.push(r.unit);
            words.push(r.state.code());
        }
        let mut h: u32 = 0x811C_9DC5;
        for &w in words.iter() {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        (words, h)
    }

    /// 二次采集核。
    pub fn recollect_stable(&self) -> Result<(), u32> {
        if !self.sealed {
            return Err(XC_FROZEN_OPEN);
        }
        let (_, sig) = self.raw_words();
        if sig != self.seal_sig {
            return Err(XC_SIG_MISMATCH);
        }
        Ok(())
    }

    /// 篡改检出核。
    pub fn tamper_detectable(&self) -> Result<(), u32> {
        let (mut words, sig) = self.raw_words();
        if words[0] != 0xA2D1_0001 {
            return Err(XC_MAGIC);
        }
        if words.len() < 3 {
            return Err(XC_EMPTY);
        }
        words[2] ^= 0xFFFF_0000;
        let mut h: u32 = 0x811C_9DC5;
        for &w in words.iter() {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        if h == sig {
            return Err(XC_TAMPER);
        }
        Ok(())
    }

    /// 缺账数。
    pub fn missing_count(&self) -> usize {
        self.rows.iter().filter(|r| r.state == TfState::Missing).count()
    }

    /// 演化记录数。
    pub fn evolved_count(&self) -> usize {
        self.evolved.len()
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 读字段。
    pub fn get(&self, tag: u32) -> Option<TfRow> {
        self.rows.iter().copied().find(|r| r.tag == tag)
    }
}

/// 双采集一致性核。
pub fn double_collect_eq(a: &TimePack, b: &TimePack) -> Result<(), u32> {
    if a.len() != b.len() {
        return Err(XC_SIG_MISMATCH);
    }
    for i in 0..a.len() {
        let ra = match a.rows.get(i) {
            Some(r) => *r,
            None => return Err(XC_SILENT_MISS),
        };
        let rb = match b.rows.get(i) {
            Some(r) => *r,
            None => return Err(XC_SILENT_MISS),
        };
        if ra.tag != rb.tag || ra.value != rb.value || ra.state != rb.state {
            return Err(XC_SIG_MISMATCH);
        }
    }
    Ok(())
}

pub fn run_unx0413_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0413_timefield");
    // 族一：八字段标准包。
    let mut p = TimePack::new();
    let _ = p.put(1, 3601, 2, TfState::Live);      // 墙钟偏移
    let _ = p.put(2, 1, 0, TfState::Frozen);       // 主时钟源号
    let _ = p.put(3, 12, 0, TfState::Live);        // 定时在册数
    let _ = p.put(4, 20, 3, TfState::Frozen);      // 抖动限
    let _ = p.put(5, 0, 0, TfState::Live);         // 跳变案数
    let _ = p.put(6, 1_700_000_000, 2, TfState::Frozen); // RTC 底值
    let _ = p.put(7, 2_400_000_000, 1, TfState::Live);   // TSC 频率
    let _ = p.put(8, 0, 3, TfState::Missing);      // deadline 缺账
    cs.add("八字段登记", p.len() == 8, "7 有账 + 1 缺账");
    cs.add("缺账计数", p.missing_count() == 1, "deadline 缺账");
    cs.add("读回核对", p.get(7).map(|r| r.value) == Some(2_400_000_000), "频率回读");
    let sig1 = p.seal();
    cs.add("封包成", sig1.is_ok(), "签生成");
    cs.add("二次采集稳", p.recollect_stable().is_ok(), "签一致");
    cs.add("篡改必检出", p.tamper_detectable().is_ok(), "翻转改签");
    // 族二：活性改写留档。
    let mut q = TimePack::new();
    let _ = q.put(1, 3601, 2, TfState::Live);
    cs.add("活性改写成", q.mutate(1, 3661).is_ok(), "3601→3661");
    cs.add("演化留档", q.evolved_count() == 1, "旧值在案");
    cs.add("新值生效", q.get(1).map(|r| r.value) == Some(3661), "读回 3661");
    cs.add("未登记改写拒", q.mutate(9, 1) == Err(XC_SILENT_MISS), "无 tag 9");
    // 族三：冻结改写拒。
    let mut r0 = TimePack::new();
    let _ = r0.put(2, 1, 0, TfState::Frozen);
    cs.add("冻结改写拒", r0.mutate(2, 2) == Err(XC_FROZEN_MUT), "主源冻结");
    // 族四：封包后操作拒。
    cs.add("封包后改写拒", p.mutate(3, 99) == Err(XC_FROZEN_OPEN), "已封");
    cs.add("封包后追加拒", p.put(9, 1, 0, TfState::Live) == Err(XC_FROZEN_OPEN), "已封");
    // 族五：登记拒绝。
    let mut d = TimePack::new();
    let _ = d.put(1, 1, 0, TfState::Live);
    cs.add("重号拒", d.put(1, 2, 0, TfState::Live) == Err(XC_DUP_TAG), "tag 1 撞");
    cs.add("空包拒", TimePack::new().seal() == Err(XC_EMPTY), "零行拒");
    // 族六：双采集一致 + 漂移检出。
    let mut a = TimePack::new();
    let mut b = TimePack::new();
    for &(tg, v, u, st) in [
        (1u32, 3601u64, 2u32, TfState::Live),
        (2, 1, 0, TfState::Frozen),
        (4, 20, 3, TfState::Frozen),
    ]
    .iter()
    {
        let _ = a.put(tg, v, u, st);
        let _ = b.put(tg, v, u, st);
    }
    cs.add("双采集一致", double_collect_eq(&a, &b).is_ok(), "三字段同");
    let mut c = TimePack::new();
    let _ = c.put(1, 3602, 2, TfState::Live);
    let _ = c.put(2, 1, 0, TfState::Frozen);
    let _ = c.put(4, 20, 3, TfState::Frozen);
    cs.add("漂移检出", double_collect_eq(&a, &c) == Err(XC_SIG_MISMATCH), "偏移值异");
    cs
}
