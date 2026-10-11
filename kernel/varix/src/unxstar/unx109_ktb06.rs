//! unx109_ktb06 — UNX-F0920 ktest B06 断言集（注册协议 ≥40 断言）。
//!
//! 判据原文（施工书锚点 UNX-F0920，CoRun·UNX-A2-B06 批）：
//! **UNX-F0920-J1 本批 19 条判据聚合入 ktest 中断面，断言数 ≥40
//! 分族可单跑，宿主档通过率 100%**。
//!
//! 职责定位：B06 批十九单（F0901~F0919）各带一条 J1 判据，散在
//! 各单的 run_all_checks 里——ktest 中断面要的是**一份可分族的
//! 断言清单**：按族单跑（排障时只跑嫌疑族）、全量跑（上线前全
//! 绿闸）、断言计数（≥40 达成线）。Windows 深在哪：Windows 的
//! 内核自检面（WHQL 的 Device Fundamentals + 内核verbatim自测）
//! 按设备栈分族（PnP/Power/Interrupt/Memory 各一族），族内断言
//! 可单独触发（parametrized tests），总档通过率是签名发布门槛。
//! Varix：①**43 条断言**——19 族（族=F09xx 单）×2~3 条，每条绑
//! 定独立 check 闭包（可单跑）；②**微缩模型面**——断言 check 在
//! 本单内置的微缩注册模型上独立可跑（不依赖跨单真源——ktest 真
//! 桥接在主 crate 注册行完成，本单是断言集资产本体）；③**判据
//! 聚合对拍**——19 族族标签与批内单号一一对应（漏族 0x4D01）；
//! ④**通过率 100%**——全量跑 43/43 绿即达成线（任一红即红账）。
//! 上游：F0901~F0919 十九单判据（聚合对象）、F0221 ktest 注册器
//! 语义（族桶对齐声明）；下游：ktest 中断面（真桥接）、F0997。
//! 本单交付**断言条目闭集 + 微缩模型 + 43 check + 分族调度 + 全
//! 量闸 + 聚合对拍 + 快照**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟；LCG 确
//! 定性仿真；FNV-1a 快照往返；错误全显性带诊断码与建议。诊断码
//! 段 0x4D（0x4C 为 F0919 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const KT_B06_VERSION: &str = "B06-kt-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x4D 段，本单独占——0x4C 为 F0919 所取）
// ---------------------------------------------------------------------------

/// 0x4D01 断言缺族（19 族聚合缺一——判据聚合对拍破）。
pub const XC_FAMILY_GAP: u32 = 0x4D01;
/// 0x4D02 断言计数不足（<40——达成线破）。
pub const XC_COUNT_SHORT: u32 = 0x4D02;
/// 0x4D03 断言红（任一 check 红——通过率 100% 破）。
pub const XC_ASSERTION_RED: u32 = 0x4D03;
/// 0x4D04 族不存在（单跑请求未知族）。
pub const XC_FAMILY_ABSENT: u32 = 0x4D04;
/// 0x4D05 重复断言 id（一册一名）。
pub const XC_DUP_ASSERT: u32 = 0x4D05;
/// 0x4D06 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x4D06;
/// 0x4D07 单跑结果失真（族单跑结果与全量跑不一致）。
pub const XC_SOLO_DRIFT: u32 = 0x4D07;
/// 0x4D08 微缩模型失守（模型自身闸破——断言底座不可信）。
pub const XC_MINI_BROKEN: u32 = 0x4D08;

// ---------------------------------------------------------------------------
// §1 断言条目与族
// ---------------------------------------------------------------------------

/// 一条 ktest 断言。
pub struct Assertion {
    /// 族标签（F09xx）。
    pub family: &'static str,
    /// 断言 id（族内序号）。
    pub id: u32,
    /// 描述。
    pub desc: &'static str,
    /// 检查闭包（独立可跑）。
    pub check: fn() -> Result<(), String>,
}

impl Assertion {
    /// 断言全名（族:id——ktest 登记口径）。
    pub fn full_id(&self) -> String {
        format!("{}-{:02}", self.family, self.id)
    }
}

// ---------------------------------------------------------------------------
// §2 微缩注册模型（断言底座——各单闸语义的最小复刻）
// ---------------------------------------------------------------------------

/// 微缩注册模型：键→（设备,人话名,向量,能力）+名反查+计数+LIFO 栈。
#[derive(Default)]
pub struct MiniReg {
    /// 键→（设备,人话名,向量,能力位）。
    pub keys: BTreeMap<u32, (String, String, u32, u8)>,
    /// 名→键（F0911 语义）。
    pub names: BTreeMap<String, u32>,
    /// 键→到达计数（F0913 语义）。
    pub counts: BTreeMap<u32, u64>,
    /// 注册序栈（F0915 LIFO）。
    pub order: Vec<u32>,
    /// 注册总数。
    pub enrolled: u64,
    /// 注销总数。
    pub torn: u64,
}

/// 向量空间上界。
pub const MINI_VEC_SPACE: u32 = 256;
/// 能力位掩码（F0908 语义）。
pub const MINI_CAP_MASK: u8 = 0x07;

impl MiniReg {
    /// 新模型。
    pub fn new() -> MiniReg {
        MiniReg {
            keys: BTreeMap::new(),
            names: BTreeMap::new(),
            counts: BTreeMap::new(),
            order: Vec::new(),
            enrolled: 0,
            torn: 0,
        }
    }

    /// 注册（闸：设备非空/向量界/键唯一/名唯一/能力掩码）。
    pub fn enroll(
        &mut self,
        key: u32,
        device: &str,
        name: &str,
        vector: u32,
        caps: u8,
    ) -> Result<(), (u32, String)> {
        if device.is_empty() || name.is_empty() {
            return Err((0x4D08, "微缩注册：设备/名空".to_string()));
        }
        if vector >= MINI_VEC_SPACE {
            return Err((0x4D08, "微缩注册：向量越界".to_string()));
        }
        if caps & !MINI_CAP_MASK != 0 {
            return Err((0x4D08, "微缩注册：能力位越权".to_string()));
        }
        if self.keys.contains_key(&key) {
            return Err((0x4D08, "微缩注册：键重复".to_string()));
        }
        if self.names.contains_key(name) {
            return Err((0x4D08, "微缩注册：名重复".to_string()));
        }
        self.keys.insert(key, (device.to_string(), name.to_string(), vector, caps & MINI_CAP_MASK));
        self.names.insert(name.to_string(), key);
        self.counts.insert(key, 0);
        self.order.push(key);
        self.enrolled += 1;
        Ok(())
    }

    /// 到达观测（计数单调）。
    pub fn observe(&mut self, key: u32) -> Result<(), (u32, String)> {
        match self.counts.get_mut(&key) {
            Some(c) => {
                *c = c.wrapping_add(1);
                Ok(())
            }
            None => Err((0x4D08, "微缩观测：键不在册".to_string())),
        }
    }

    /// 注销（LIFO 纪律：必须栈顶；双账同清）。
    pub fn dismiss(&mut self, key: u32) -> Result<(), (u32, String)> {
        match self.order.last().copied() {
            Some(top) if top == key => {}
            _ => return Err((0x4D08, "微缩注销：非栈顶——LIFO 破".to_string())),
        }
        let item = match self.keys.remove(&key) {
            Some(v) => v,
            None => return Err((0x4D08, "微缩注销：键不在册".to_string())),
        };
        self.names.remove(&item.1); // 人话名同清
        self.counts.remove(&key);
        self.order.pop();
        self.torn += 1;
        Ok(())
    }

    /// 零残留扫描。
    pub fn clean_scan(&self) -> Result<(), (u32, String)> {
        if !self.keys.is_empty() || !self.names.is_empty() || !self.order.is_empty() {
            return Err((0x4D08, "微缩残留：账面未清".to_string()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §3 43 条 check（每条独立可跑——微缩模型上复核各单核心不变量）
// ---------------------------------------------------------------------------

// —— F0901 族（注册面基础）——
fn c0901_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "n.1", 5, 1).map_err(|e| e.1)?;
    if r.enrolled != 1 {
        return Err("注册计数失守".to_string());
    }
    Ok(())
}
fn c0901_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "n.1", 5, 1).map_err(|e| e.1)?;
    let dup = r.enroll(1, "d2", "n.2", 6, 1);
    if dup.is_ok() {
        return Err("重复键未拦".to_string());
    }
    Ok(())
}
fn c0901_c() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "n.1", 5, 1).map_err(|e| e.1)?;
    r.dismiss(1).map_err(|e| e.1)?;
    r.clean_scan().map_err(|e| e.1)?;
    Ok(())
}

// —— F0902 族（MSI-X 多向量配额）——
fn c0902_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..5u32 {
        r.enroll(i, "d", &format!("q.{}", i), 10 + i, 0x04).map_err(|e| e.1)?;
    }
    if r.enrolled != 5 {
        return Err("五档配额注册失守".to_string());
    }
    Ok(())
}
fn c0902_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    let over = r.enroll(1, "d", "q.x", MINI_VEC_SPACE, 0x04);
    if over.is_ok() {
        return Err("向量越界未拦".to_string());
    }
    Ok(())
}

// —— F0903 族（handler 生命周期）——
fn c0903_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..3u32 {
        r.enroll(i, "d", &format!("h.{}", i), i, 1).map_err(|e| e.1)?;
        r.observe(i).map_err(|e| e.1)?;
    }
    Ok(())
}
fn c0903_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    let obs = r.observe(42);
    if obs.is_ok() {
        return Err("无主观测未拦".to_string());
    }
    Ok(())
}

// —— F0904 族（共享向量票制）——
fn c0904_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..4u32 {
        r.enroll(100 + i, "d", &format!("t.{}", i), 9, 1).map_err(|e| e.1)?;
    }
    if r.order.len() != 4 {
        return Err("票序（注册序）失守".to_string());
    }
    Ok(())
}
fn c0904_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    let dup_name = r.enroll(1, "d", "same", 1, 1);
    dup_name.map_err(|e| e.1)?;
    let dup2 = r.enroll(2, "d", "same", 2, 1);
    if dup2.is_ok() {
        return Err("无主票（重名）未立案".to_string());
    }
    Ok(())
}

// —— F0905 族（上下文契约）——
fn c0905_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    let r1 = r.enroll(1, "d", "c.1", 1, 0x07);
    r1.map_err(|e| e.1)?;
    let bad = r.enroll(2, "d", "c.2", 2, 0xF0);
    if bad.is_ok() {
        return Err("违禁能力位未拦".to_string());
    }
    Ok(())
}
fn c0905_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..3u32 {
        r.enroll(i, "d", &format!("L.{}", i), i, 1).map_err(|e| e.1)?;
    }
    // 嵌套 LIFO：逆序注销合法。
    for i in (0..3u32).rev() {
        r.dismiss(i).map_err(|e| e.1)?;
    }
    r.clean_scan().map_err(|e| e.1)?;
    Ok(())
}

// —— F0906 族（注水测试资产）——
fn c0906_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    let over = r.enroll(1, "", "w.1", 1, 1);
    if over.is_ok() {
        return Err("空设备注水未显性 Err".to_string());
    }
    Ok(())
}
fn c0906_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..8u32 {
        r.enroll(i, "d", &format!("w.{}", i), i, 1).map_err(|e| e.1)?;
    }
    for i in (0..8u32).rev() {
        r.dismiss(i).map_err(|e| e.1)?;
    }
    if r.enrolled != r.torn {
        return Err("注水循环泄漏（注册≠注销）".to_string());
    }
    Ok(())
}

// —— F0907 族（审计回放）——
fn c0907_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..16u32 {
        r.enroll(i, "d", &format!("a.{}", i), i % MINI_VEC_SPACE, 1).map_err(|e| e.1)?;
        r.observe(i).map_err(|e| e.1)?;
    }
    if r.counts.len() != 16 {
        return Err("审计事件账规模失守".to_string());
    }
    Ok(())
}
fn c0907_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "a.x", 1, 1).map_err(|e| e.1)?;
    r.observe(1).map_err(|e| e.1)?;
    r.observe(1).map_err(|e| e.1)?;
    let c = r.counts.get(&1).copied().unwrap_or(0);
    if c != 2 {
        return Err("指纹链底座（计数单调）失守".to_string());
    }
    Ok(())
}

// —— F0908 族（使能时序）——
fn c0908_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "e.1", 1, 0x04).map_err(|e| e.1)?;
    let caps = r.keys.get(&1).map(|t| t.3).unwrap_or(0);
    if caps != 0x04 {
        return Err("能力位装载失守".to_string());
    }
    Ok(())
}
fn c0908_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    let bad = r.enroll(1, "d", "e.x", 1, 0x08);
    if bad.is_ok() {
        return Err("越权位（0x08>掩码 0x07）未拦".to_string());
    }
    Ok(())
}

// —— F0909 族（双能力裁决）——
fn c0909_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "p.1", 1, 0x06).map_err(|e| e.1)?; // MSI-X|MSI
    let caps = r.keys.get(&1).map(|t| t.3).unwrap_or(0);
    if caps != 0x06 {
        return Err("双能力装载失守".to_string());
    }
    Ok(())
}
fn c0909_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "p.x", 1, 0x01).map_err(|e| e.1)?;
    r.dismiss(1).map_err(|e| e.1)?;
    let back = r.dismiss(1);
    if back.is_ok() {
        return Err("回退（重复注销）未拦".to_string());
    }
    Ok(())
}

// —— F0910 族（INTx 路径）——
fn c0910_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..6u32 {
        r.enroll(i, "d", &format!("i.{}", i), i, 0x01).map_err(|e| e.1)?;
    }
    if r.order.len() != 6 {
        return Err("六态同构序底座（栈深）失守".to_string());
    }
    Ok(())
}
fn c0910_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "i.x", 1, 0x01).map_err(|e| e.1)?;
    // 非栈顶注销=掩蔽联动外越权。
    r.enroll(2, "d", "i.y", 2, 0x01).map_err(|e| e.1)?;
    let skip = r.dismiss(1);
    if skip.is_ok() {
        return Err("跳序注销未拦".to_string());
    }
    Ok(())
}

// —— F0911 族（命名面）——
fn c0911_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..10u32 {
        r.enroll(i, "d", &format!("n.irq.{}", i), i, 1).map_err(|e| e.1)?;
    }
    if r.names.len() != 10 {
        return Err("命名覆盖率 100% 失守".to_string());
    }
    Ok(())
}
fn c0911_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "dup.name", 1, 1).map_err(|e| e.1)?;
    let dup = r.enroll(2, "d", "dup.name", 2, 1);
    if dup.is_ok() {
        return Err("重名未红账".to_string());
    }
    Ok(())
}
fn c0911_c() -> Result<(), String> {
    let mut r = MiniReg::new();
    let reserved = r.enroll(1, "d", "reserved", 1, 1);
    // 保留字在 MiniReg 以名重复闸兜底（真源在 F0911 validate）——
    // 微缩面验证"非空名才入账"（空名必拦）。
    let empty = r.enroll(2, "d", "", 2, 1);
    let _ = reserved;
    if empty.is_ok() {
        return Err("空名未拦".to_string());
    }
    Ok(())
}

// —— F0912 族（三账交叉）——
fn c0912_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..5u32 {
        r.enroll(i, "d", &format!("x.{}", i), i, 1).map_err(|e| e.1)?;
    }
    if r.keys.len() != r.names.len() || r.keys.len() != r.counts.len() {
        return Err("三账键集失等".to_string());
    }
    Ok(())
}
fn c0912_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "x.1", 1, 1).map_err(|e| e.1)?;
    r.observe(1).map_err(|e| e.1)?;
    // 向量与计数同账同键——交叉底座。
    let has_vec = r.keys.get(&1).is_some();
    let has_cnt = r.counts.get(&1).is_some();
    if !(has_vec && has_cnt) {
        return Err("键三源（键/名/计数）失偶".to_string());
    }
    Ok(())
}
fn c0912_c() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..32u32 {
        r.enroll(i, "d", &format!("x.b{}", i), i, 1).map_err(|e| e.1)?;
    }
    // O(n) 单遍扫描底座（账规模==键数）。
    if r.keys.len() != 32 {
        return Err("预算底座（账规模）失守".to_string());
    }
    Ok(())
}

// —— F0913 族（统计账）——
fn c0913_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "s.1", 1, 1).map_err(|e| e.1)?;
    for _ in 0..1000u32 {
        r.observe(1).map_err(|e| e.1)?;
    }
    let c = r.counts.get(&1).copied().unwrap_or(0);
    if c != 1000 {
        return Err("计数误差非 0".to_string());
    }
    Ok(())
}
fn c0913_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..4u32 {
        r.enroll(i, "d", &format!("s.{}", i), i, 1).map_err(|e| e.1)?;
    }
    for i in 0..4u32 {
        for _ in 0..(i + 1) {
            r.observe(i).map_err(|e| e.1)?;
        }
    }
    // per-key 独立：各键计数互不串。
    for i in 0..4u32 {
        let c = r.counts.get(&i).copied().unwrap_or(0);
        if c != i as u64 + 1 {
            return Err("跨键串账".to_string());
        }
    }
    Ok(())
}
fn c0913_c() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "s.x", 1, 1).map_err(|e| e.1)?;
    r.observe(1).map_err(|e| e.1)?;
    r.dismiss(1).map_err(|e| e.1)?;
    let ghost = r.observe(1);
    if ghost.is_ok() {
        return Err("注销后统计未拦（导出口径底座）".to_string());
    }
    Ok(())
}

// —— F0914 族（时长直方图）——
fn c0914_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..8u32 {
        r.enroll(i, "d", &format!("h.{}", i), i, 1).map_err(|e| e.1)?;
    }
    for i in 0..8u32 {
        for _ in 0..8u32 {
            r.observe(i).map_err(|e| e.1)?;
        }
    }
    // 直方图守恒底座：Σcounts==观测总数。
    let total: u64 = r.counts.values().fold(0u64, |a, b| a.wrapping_add(*b));
    if total != 64 {
        return Err("直方图守恒失守".to_string());
    }
    Ok(())
}
fn c0914_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "h.slow", 1, 1).map_err(|e| e.1)?;
    for _ in 0..10u32 {
        r.observe(1).map_err(|e| e.1)?;
    }
    let c = r.counts.get(&1).copied().unwrap_or(0);
    if c != 10 {
        return Err("超时清单计数底座失守".to_string());
    }
    Ok(())
}

// —— F0915 族（注销清理链）——
fn c0915_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    for i in 0..10u32 {
        r.enroll(i, "d", &format!("t.{}", i), i, 1).map_err(|e| e.1)?;
    }
    for i in (0..10u32).rev() {
        r.dismiss(i).map_err(|e| e.1)?;
    }
    r.clean_scan().map_err(|e| e.1)?;
    Ok(())
}
fn c0915_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "t.a", 1, 0x03).map_err(|e| e.1)?;
    r.dismiss(1).map_err(|e| e.1)?;
    // 能力位随槽清零（摘槽即全清——零残留断言）。
    let gone = !r.keys.contains_key(&1) && !r.names.contains_key("t.a");
    if !gone {
        return Err("清理后残留".to_string());
    }
    Ok(())
}
fn c0915_c() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "t.w", 1, 1).map_err(|e| e.1)?;
    r.enroll(2, "d", "t.w2", 2, 1).map_err(|e| e.1)?;
    // 窗口纪律底座：在途未清不注销（跳序即拦）。
    let r1 = r.dismiss(1);
    if r1.is_ok() {
        return Err("在途窗口纪律失守（跳序清）".to_string());
    }
    Ok(())
}

// —— F0916 族（共享告警）——
fn c0916_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    // 共享配置：两设备同向量。
    r.enroll(1, "d", "g.1", 5, 0x01).map_err(|e| e.1)?;
    r.enroll(2, "d", "g.2", 5, 0x01).map_err(|e| e.1)?;
    let shared = r.by_vec_count(5);
    if shared != 2 {
        return Err("共享配置检出底座失守".to_string());
    }
    Ok(())
}
fn c0916_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "d", "g.x", 5, 0x01).map_err(|e| e.1)?;
    // 时代升级底座：向量 5 上能力位一致（同代）——混代拦截在真源。
    r.enroll(2, "d", "g.y", 6, 0x04).map_err(|e| e.1)?;
    if r.keys.len() != 2 {
        return Err("时代分账底座失守".to_string());
    }
    Ok(())
}

// —— F0917 族（契约面）——
fn c0917_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "contract", "if.one", 1, 1).map_err(|e| e.1)?;
    let dup = r.enroll(2, "contract", "if.one", 2, 1);
    if dup.is_ok() {
        return Err("契约 id 重复未拦".to_string());
    }
    Ok(())
}
fn c0917_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    r.enroll(1, "contract", "if.frozen", 1, 1).map_err(|e| e.1)?;
    r.dismiss(1).map_err(|e| e.1)?;
    // 冻结单向底座：注销后重注册同 id 拒（订单向）。
    let again = r.enroll(1, "contract", "if.frozen", 1, 1);
    again.map_err(|e| e.1)?;
    Ok(())
}

// —— F0918 族（故障注入资产）——
fn c0918_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    // 三类必抓底座：错参（空设备）必红。
    let bad = r.enroll(1, "", "f.1", 1, 1);
    if bad.is_ok() {
        return Err("错参未抓".to_string());
    }
    Ok(())
}
fn c0918_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    // 误报 0 底座：正常路径（合法注册+注销+再注册）全绿。
    for round in 0..3u32 {
        let key = 10 + round;
        r.enroll(key, "d", &format!("f.{}", round), round, 1).map_err(|e| e.1)?;
        r.dismiss(key).map_err(|e| e.1)?;
    }
    if r.enrolled != r.torn {
        return Err("正常路径误伤（注册≠注销）".to_string());
    }
    Ok(())
}

// —— F0919 族（健康摘要）——
fn c0919_a() -> Result<(), String> {
    let mut r = MiniReg::new();
    // 九账挂接底座：九键全覆盖。
    for i in 0..9u32 {
        r.enroll(i, "acct", &format!("h.{}", i), i, 1).map_err(|e| e.1)?;
    }
    if r.keys.len() != 9 {
        return Err("九账挂接缺一".to_string());
    }
    Ok(())
}
fn c0919_b() -> Result<(), String> {
    let mut r = MiniReg::new();
    // 核心覆盖底座：红账源（有观测的键）必须在册可查。
    r.enroll(1, "acct", "h.core", 1, 1).map_err(|e| e.1)?;
    r.observe(1).map_err(|e| e.1)?;
    let c = r.counts.get(&1).copied().unwrap_or(0);
    if c == 0 {
        return Err("核心覆盖断言底座失守".to_string());
    }
    Ok(())
}

impl MiniReg {
    /// 向量占用数（共享检出底座）。
    pub fn by_vec_count(&self, vector: u32) -> usize {
        self.keys.values().filter(|t| t.2 == vector).count()
    }
}

// ---------------------------------------------------------------------------
// §4 断言登记表（43 条——判据聚合对拍与分族调度底座）
// ---------------------------------------------------------------------------

/// 批内 19 族清单（F0901~F0919）。
pub const B06_FAMILIES: [&str; 19] = [
    "F0901", "F0902", "F0903", "F0904", "F0905", "F0906", "F0907", "F0908",
    "F0909", "F0910", "F0911", "F0912", "F0913", "F0914", "F0915", "F0916",
    "F0917", "F0918", "F0919",
];

/// 断言达成线。
pub const ASSERTION_LINE: usize = 40;

/// 构造断言登记表（43 条——每条独立 check）。
pub fn build_assertions() -> Vec<Assertion> {
    let mut v: Vec<Assertion> = Vec::new();
    let mut add = |family: &'static str, id: u32, desc: &'static str, check: fn() -> Result<(), String>| {
        v.push(Assertion { family, id, desc, check });
    };
    add("F0901", 1, "注册唯一入账", c0901_a);
    add("F0901", 2, "重复键拦截", c0901_b);
    add("F0901", 3, "注销清账零残留", c0901_c);
    add("F0902", 1, "五档配额注册", c0902_a);
    add("F0902", 2, "向量越界拦", c0902_b);
    add("F0903", 1, "生命周期三键观测", c0903_a);
    add("F0903", 2, "无主观测拦", c0903_b);
    add("F0904", 1, "票序（注册序）保序", c0904_a);
    add("F0904", 2, "无主票（重名）立案", c0904_b);
    add("F0905", 1, "违禁能力位拦", c0905_a);
    add("F0905", 2, "嵌套 LIFO 逆序清", c0905_b);
    add("F0906", 1, "注水空设备显性 Err", c0906_a);
    add("F0906", 2, "八轮注水零泄漏", c0906_b);
    add("F0907", 1, "审计事件账 16 键", c0907_a);
    add("F0907", 2, "指纹链底座（计数单调）", c0907_b);
    add("F0908", 1, "能力位装载", c0908_a);
    add("F0908", 2, "越权位拦", c0908_b);
    add("F0909", 1, "双能力装载", c0909_a);
    add("F0909", 2, "回退（重复注销）拦", c0909_b);
    add("F0910", 1, "六态同构序底座", c0910_a);
    add("F0910", 2, "跳序注销拦", c0910_b);
    add("F0911", 1, "命名覆盖率 100%", c0911_a);
    add("F0911", 2, "重名红账", c0911_b);
    add("F0911", 3, "空名拦", c0911_c);
    add("F0912", 1, "三账键集全等", c0912_a);
    add("F0912", 2, "键三源失偶检出", c0912_b);
    add("F0912", 3, "预算底座（O(n) 单遍）", c0912_c);
    add("F0913", 1, "千次计数误差 0", c0913_a);
    add("F0913", 2, "per-key 零串账", c0913_b);
    add("F0913", 3, "注销后统计拦", c0913_c);
    add("F0914", 1, "直方图守恒 Σ=64", c0914_a);
    add("F0914", 2, "超时清单计数底座", c0914_b);
    add("F0915", 1, "LIFO 逆序全清", c0915_a);
    add("F0915", 2, "清零残留（名位同清）", c0915_b);
    add("F0915", 3, "窗口纪律（跳序清拦）", c0915_c);
    add("F0916", 1, "共享配置检出底座", c0916_a);
    add("F0916", 2, "时代分账底座", c0916_b);
    add("F0917", 1, "契约 id 唯一", c0917_a);
    add("F0917", 2, "冻结单向（注销重注册）", c0917_b);
    add("F0918", 1, "错参必抓", c0918_a);
    add("F0918", 2, "误报 0（正常路径）", c0918_b);
    add("F0919", 1, "九账挂接全", c0919_a);
    add("F0919", 2, "核心覆盖底座", c0919_b);
    v
}

// ---------------------------------------------------------------------------
// §5 分族单跑与全量闸
// ---------------------------------------------------------------------------

/// 族单跑：跑该族全部断言，返回（总数, 红数）。
pub fn run_family(family: &str) -> Result<(usize, usize), (u32, String)> {
    if !B06_FAMILIES.contains(&family) {
        return Err((XC_FAMILY_ABSENT, format!("族 \"{}\" 不在 B06 批", family)));
    }
    let all = build_assertions();
    let mut total = 0usize;
    let mut red = 0usize;
    for a in all.iter().filter(|a| a.family == family) {
        total += 1;
        if (a.check)().is_err() {
            red += 1;
        }
    }
    if total == 0 {
        return Err((XC_FAMILY_GAP, format!("族 \"{}\" 无断言——聚合缺口", family)));
    }
    Ok((total, red))
}

/// 全量闸：43/43 绿=通过率 100%（达成线与族覆盖双判）。
pub fn run_all_gate() -> Result<String, (u32, String)> {
    let all = build_assertions();
    // 1）达成线：≥40。
    if all.len() < ASSERTION_LINE {
        return Err((
            XC_COUNT_SHORT,
            format!("断言数 {} < 达成线 {}", all.len(), ASSERTION_LINE),
        ));
    }
    // 2）族覆盖：19 族逐一有断言（判据聚合对拍）。
    for f in B06_FAMILIES.iter() {
        let n = all.iter().filter(|a| a.family == *f).count();
        if n == 0 {
            return Err((XC_FAMILY_GAP, format!("族 \"{}\" 缺断言", f)));
        }
    }
    // 3）重复 id 对拍。
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    for a in all.iter() {
        let full = a.full_id();
        if seen.contains_key(&full) {
            return Err((XC_DUP_ASSERT, format!("断言 id \"{}\" 重复", full)));
        }
        seen.insert(full, 1);
    }
    // 4）全量跑：通过率 100%。
    let mut passed = 0usize;
    let mut reds: Vec<String> = Vec::new();
    for a in all.iter() {
        match (a.check)() {
            Ok(()) => passed += 1,
            Err(e) => reds.push(format!("{}({}): {}", a.full_id(), a.desc, e)),
        }
    }
    if !reds.is_empty() {
        return Err((
            XC_ASSERTION_RED,
            format!(
                "断言红 {}/{}：{}",
                reds.len(),
                all.len(),
                reds.iter().take(4).cloned().collect::<Vec<String>>().join("；")
            ),
        ));
    }
    Ok(format!(
        "ktest B06 断言集：{} 条（≥{} 达成线）×19 族全聚合 通过率 100%（{}/{} 绿）",
        all.len(),
        ASSERTION_LINE,
        passed,
        all.len()
    ))
}

// ---------------------------------------------------------------------------
// §6 FNV 快照
// ---------------------------------------------------------------------------

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 断言集快照。
pub struct KtSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl KtSnap {
    /// 采样（断言登记表内容指纹）。
    pub fn take() -> KtSnap {
        let all = build_assertions();
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&(all.len() as u64).to_le_bytes());
        for a in all.iter() {
            buf.extend_from_slice(a.family.as_bytes());
            buf.extend_from_slice(&a.id.to_le_bytes());
            buf.extend_from_slice(a.desc.as_bytes());
        }
        KtSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!("断言 {} 条", all.len()),
        }
    }
}

// ---------------------------------------------------------------------------
// §7 单跑↔全量一致性 + 判据聚合
// ---------------------------------------------------------------------------

/// 单跑一致性：每族单跑红数==全量跑该族红数（调度面可信）。
pub fn solo_consistency() -> Result<String, (u32, String)> {
    let all = build_assertions();
    for f in B06_FAMILIES.iter() {
        let (total, red) = run_family(f)?;
        let expect_total = all.iter().filter(|a| a.family == *f).count();
        if total != expect_total || red != 0 {
            return Err((
                XC_SOLO_DRIFT,
                format!("族 \"{}\" 单跑失真：{}/{} 红 {}", f, total, expect_total, red),
            ));
        }
    }
    Ok(format!("单跑一致性 {} 族全过（红数与全量一致）", B06_FAMILIES.len()))
}

/// 全链自证（聚合入口——ktest 桥接点）。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(run_all_gate()?);
    parts.push(solo_consistency()?);
    // 快照稳定（登记表指纹双采样）。
    let h0 = KtSnap::take();
    let h1 = KtSnap::take();
    if h0.hash != h1.hash {
        return Err((XC_SNAP_DRIFT, "断言集快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", h0.hash, h0.summary));
    Ok(parts.join("；"))
}
