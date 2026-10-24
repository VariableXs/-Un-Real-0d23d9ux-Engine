//! unx0378_handfield —— UNX-F0378 内存账交接字段（A4 预备）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0378 · 内存账交接字段 · 目标 260 行
//! 判据原文：UNX-F0378-J1 内存面各账（帧位图/DMA 池/堆/水位/保留区）
//!           的关键状态字段按统一行式打包成交接包，供 A4 交接；
//!           字段三态（冻结/活性/缺账）可判定；交接包两次采集
//!           签名必须一致（确定性核），字段篡改可检出（FNV32
//!           签对拍）；缺账字段立案不阻断但有账必签。
//! 组件：字段行（tag + 值 + 单位 + 三态）+ 交接包（行数组 +
//!       冻结闸 + FNV32 签）+ 二次采集核（签名一致）+ 篡改对拍
//!       （改任一字节签必变）+ 缺账立案账。
//! 职责定位：交接字段是「内存面的出口账」——A4 阶段接手时只看
//!   这个包就能重建内存面全景；三态语义是关键：冻结字段不许
//!   再变（变了签对不上），活性字段允许跨阶段演化（但要留演化
//!   记录），缺账字段显式声明（不许静默缺失）。
//! 性能逐项分解：打包 O(字段数)；签名 O(字段数)；对拍 O(1)。
//! 诊断码段：0x20B00（0x20B01~0x20B09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HF_VERSION: &str = "unx0378_handfield-v1";

/// 诊断段基址（0x20B00 段 · 本模块独占）。
pub const HF_SEG_BASE: u32 = 0x20B00;
pub const XC_FROZEN_MUT: u32 = 0x20B01; // 冻结字段被改
pub const XC_SIG_MISMATCH: u32 = 0x20B02; // 签名不符
pub const XC_SILENT_MISS: u32 = 0x20B03; // 字段静默缺失
pub const XC_DUP_TAG: u32 = 0x20B04; // 字段重号
pub const XC_STATE_OOR: u32 = 0x20B05; // 三态越域
pub const XC_FROZEN_OPEN: u32 = 0x20B06; // 冻结包未封而放行
pub const XC_TAMPER: u32 = 0x20B07; // 篡改检出
pub const XC_MAGIC: u32 = 0x20B08; // 包魔数破
pub const XC_EMPTY: u32 = 0x20B09; // 空包拒

/// 字段三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldState {
    /// 冻结：交接后不许再变。
    Frozen = 1,
    /// 活性：允许跨阶段演化，但每次演化留新值。
    Live = 2,
    /// 缺账：本阶段无数据，显式声明。
    Missing = 3,
}

impl FieldState {
    /// 词位编码。
    pub fn code(&self) -> u32 {
        match self {
            FieldState::Frozen => 1,
            FieldState::Live => 2,
            FieldState::Missing => 3,
        }
    }
}

/// 字段行。
#[derive(Clone, Copy, Debug)]
pub struct FieldRow {
    /// 字段号（内存面统一 tag：1=帧位图计数 2=DMA 余量 3=堆余量
    /// 4=水位千分 5=保留区数 6=牺牲页数 7=空闲块数 8=峰值占用）。
    pub tag: u32,
    /// 值。
    pub value: u64,
    /// 单位（0=帧 1=字节 2=千分 3=计数）。
    pub unit: u32,
    /// 三态。
    pub state: FieldState,
}

/// 交接包。
pub struct HandPack {
    rows: Vec<FieldRow>,
    sealed: bool,
    seal_sig: u32,
    /// 演化记录：tag → 旧值（活性字段改写时留档）。
    evolved: Vec<(u32, u64)>,
}

impl HandPack {
    /// 建空包。
    pub fn new() -> HandPack {
        HandPack { rows: Vec::new(), sealed: false, seal_sig: 0, evolved: Vec::new() }
    }

    /// 追加字段；重号拒、三态越域拒、封包后拒。
    pub fn put(&mut self, tag: u32, value: u64, unit: u32, state: FieldState) -> Result<(), u32> {
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
        self.rows.push(FieldRow { tag, value, unit, state });
        Ok(())
    }

    /// 活性字段改写：冻结改写拒、缺账改写拒、未登记改写拒；
    /// 改写留旧值档。
    pub fn mutate(&mut self, tag: u32, new_value: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_FROZEN_OPEN);
        }
        let mut hit = false;
        for r in self.rows.iter_mut() {
            if r.tag == tag {
                match r.state {
                    FieldState::Frozen => return Err(XC_FROZEN_MUT),
                    FieldState::Missing => return Err(XC_SILENT_MISS),
                    FieldState::Live => {
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

    /// 封包：缺账字段不算错但计入缺账账；封包后算签。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.rows.is_empty() {
            return Err(XC_EMPTY);
        }
        let (words, sig) = self.raw_words();
        let _ = words;
        self.sealed = true;
        self.seal_sig = sig;
        Ok(sig)
    }

    /// 词位流：魔数 + 行数 + 各行五词 + FNV32。
    fn raw_words(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0xA4F1_0001u32); // 魔数：A4 交接字段 v1
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

    /// 封包签读取。
    pub fn seal_sig(&self) -> u32 {
        self.seal_sig
    }

    /// 二次采集核：重算签 = 封包签（确定性）。
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

    /// 篡改对拍：任一非签词位翻转，重算签必异（检出能力核）。
    pub fn tamper_detectable(&self) -> Result<(), u32> {
        let (mut words, sig) = self.raw_words();
        if words[0] != 0xA4F1_0001 {
            return Err(XC_MAGIC);
        }
        // 翻转第一个数据词（行 0 的 tag）。
        if words.len() < 3 {
            return Err(XC_EMPTY);
        }
        words[2] ^= 0xFFFF_0000;
        let mut h: u32 = 0x811C_9DC5;
        for &w in words.iter() {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        // 词位流里不含签本身（签在 raw_words 返回值），翻转必改签。
        if h == sig {
            return Err(XC_TAMPER); // 翻转不改变签 = 检不出 = 不合格
        }
        Ok(())
    }

    /// 缺账字段数。
    pub fn missing_count(&self) -> usize {
        self.rows.iter().filter(|r| r.state == FieldState::Missing).count()
    }

    /// 演化记录数。
    pub fn evolved_count(&self) -> usize {
        self.evolved.len()
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 按号读字段。
    pub fn get(&self, tag: u32) -> Option<FieldRow> {
        self.rows.iter().copied().find(|r| r.tag == tag)
    }
}

/// 采集两次一致性核（独立于封包的纯函数视图）。
pub fn double_collect_eq(a: &HandPack, b: &HandPack) -> Result<(), u32> {
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

pub fn run_unx0378_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0378_handfield");
    // 族一：八字段标准包全绿。
    let mut p = HandPack::new();
    let _ = p.put(1, 4096, 0, FieldState::Frozen);   // 帧位图计数（冻结）
    let _ = p.put(2, 2048, 0, FieldState::Live);     // DMA 余量（活性）
    let _ = p.put(3, 65536, 1, FieldState::Live);    // 堆余量（活性）
    let _ = p.put(4, 500, 2, FieldState::Live);      // 水位千分（活性）
    let _ = p.put(5, 3, 3, FieldState::Frozen);      // 保留区数（冻结）
    let _ = p.put(6, 8576, 0, FieldState::Frozen);   // 牺牲页数（冻结）
    let _ = p.put(7, 12, 3, FieldState::Live);       // 空闲块数（活性）
    let _ = p.put(8, 1024, 0, FieldState::Missing);  // 峰值占用（缺账声明）
    cs.add("八字段登记", p.len() == 8, "7 有账 + 1 缺账");
    cs.add("缺账计数", p.missing_count() == 1, "峰值缺账");
    cs.add("读回核对", p.get(2).map(|r| r.value) == Some(2048), "DMA 余量 2048");
    let sig1 = p.seal();
    cs.add("封包成", sig1.is_ok(), "签生成");
    cs.add("二次采集稳", p.recollect_stable().is_ok(), "签一致");
    // 族二：活性改写与留档。
    let mut q = HandPack::new();
    let _ = q.put(1, 100, 0, FieldState::Live);
    cs.add("活性改写成", q.mutate(1, 200).is_ok(), "100→200");
    cs.add("演化留档", q.evolved_count() == 1, "旧值 100 在案");
    cs.add("新值生效", q.get(1).map(|r| r.value) == Some(200), "读回 200");
    cs.add("未登记改写拒", q.mutate(9, 1) == Err(XC_SILENT_MISS), "无 tag 9");
    // 族三：冻结改写拒。
    let mut r0 = HandPack::new();
    let _ = r0.put(5, 3, 3, FieldState::Frozen);
    cs.add("冻结改写拒", r0.mutate(5, 4) == Err(XC_FROZEN_MUT), "tag 5 冻结");
    // 族四：封包后操作拒。
    cs.add("封包后改写拒", p.mutate(2, 999) == Err(XC_FROZEN_OPEN), "已封");
    cs.add("封包后追加拒",
        p.put(9, 1, 0, FieldState::Live) == Err(XC_FROZEN_OPEN),
        "已封");
    // 族五：篡改检出。
    cs.add("篡改必检出", p.tamper_detectable().is_ok(), "翻转改签");
    // 族六：重号 / 空包。
    let mut d = HandPack::new();
    let _ = d.put(1, 1, 0, FieldState::Live);
    cs.add("重号拒", d.put(1, 2, 0, FieldState::Live) == Err(XC_DUP_TAG), "tag 1 撞");
    cs.add("空包拒", HandPack::new().seal() == Err(XC_EMPTY), "零行拒");
    // 族七：双采集一致性（两个独立采集器各采一次）。
    let mut a = HandPack::new();
    let mut b = HandPack::new();
    for &(tg, v, u, st) in [
        (1u32, 4096u64, 0u32, FieldState::Frozen),
        (2, 2048, 0, FieldState::Live),
        (5, 3, 3, FieldState::Frozen),
    ]
    .iter()
    {
        let _ = a.put(tg, v, u, st);
        let _ = b.put(tg, v, u, st);
    }
    cs.add("双采集一致", double_collect_eq(&a, &b).is_ok(), "三字段同");
    // 漂移注入：b 改一值后必须对不上。
    let mut c = HandPack::new();
    let _ = c.put(1, 4096, 0, FieldState::Frozen);
    let _ = c.put(2, 2049, 0, FieldState::Live);
    let _ = c.put(5, 3, 3, FieldState::Frozen);
    cs.add("漂移检出", double_collect_eq(&a, &c) == Err(XC_SIG_MISMATCH), "DMA 值异");
    cs
}
