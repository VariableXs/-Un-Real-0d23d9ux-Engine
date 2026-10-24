//! CGPU-F3374 · 验收与 P 域联动（vcv14_plink）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3374`
//!
//! **判据（锚点原文）**：数据入板、复用、一组、判据。
//!
//! **职责定位（锚点原文）**：联动：联动（验收×P 域（验收数据入遥
//! 测——联动复用；测试（联动一组）。
//!
//! # 一、「数据入板」的语义
//!
//! P 域是内核的可观测性底座（遥测/看板/指标管线）。V 域的验收结
//! 论不能只躺在验收矩阵里——必须**入 P 域板**：每次验收取证产出一
//! 条标准化遥测事件（[`PTEvent`]），由 P 域管线投递。复用纪律：事
//! 件结构遵循 P 域事件协议（域号 + 类别 + 键值 + 时间戳），不自造
//! 事件格式；V 域只负责「把验收事实翻译成 P 域事件」，翻译层
//! （[`PTBridge`]）是唯一的出域通道。
//!
//! # 二、桥的纪律
//!
//! 1. **事实先行**：只有已取证（Proven/Failed 终态）的结论才可入
//!    板——Pending 不是事实，入板就是造假（[`E_VV_NOFACT`]）；
//! 2. **不可变投递**：入板后事件只读，改历史结论走新事件（[`
//!    E_VV_IMMUT`]）——遥测板是事实流不是状态存储；
//! 3. **计量守恒**：桥的入板计数与验收矩阵的终态计数对账一致
//!    （[`bridge_reconcile`]）——少投=观测缺口，多投=重复事实。
//!
//! # 三、联动一组（判据）
//!
//! 判据组：事实先行（Pending 拒入板）+ 事件协议合法 + 不可变投递
//! + 对账守恒 + 声明非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PLINK_VERSION: &str = "V14-plink-v1";

/// 无事实入板（Pending 结论投遥测=造假）。
pub const E_VV_NOFACT: &str = "E_VV_NOFACT";

/// 试图改写已入板事件（遥测是事实流）。
pub const E_VV_IMMUT: &str = "E_VV_IMMUT";

/// V 域号（P 域事件协议的域标识——V=真机验收域）。
pub const P_DOMAIN_V: u8 = 0x56;

/// 事件类别（验收事实的三类）。
pub const P_EVENT_KINDS: [&str; 3] = ["criterion", "walkthrough", "matrix"];

// ---------------------------------------------------------------------------
// 二、P 域事件与桥
// ---------------------------------------------------------------------------

/// P 域标准化验收事件（结构遵循 P 域事件协议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PTEvent {
    /// 域号（恒为 P_DOMAIN_V）。
    pub domain: u8,
    /// 事件类别（必须三类之一）。
    pub kind: &'static str,
    /// 事实键（验收项标识）。
    pub key: String,
    /// 事实值（通过=1 / 失败=0）。
    pub val: i8,
    /// 证据指纹。
    pub fp: u32,
}

impl PTEvent {
    /// 构造并校验协议合法性（域号/类别/值域三关）。
    pub fn new(kind: &'static str, key: &str, val: i8, fp: u32) -> Result<PTEvent, String> {
        if !P_EVENT_KINDS.contains(&kind) {
            return Err(format!("{}：事件类别 {} 不在协议三类", E_VV_IMMUT, kind));
        }
        if val != 0 && val != 1 {
            return Err(format!("{}：事实值 {} 越值域(0/1)", E_VV_NOFACT, val));
        }
        Ok(PTEvent { domain: P_DOMAIN_V, kind, key: key.to_string(), val, fp })
    }
}

/// 验收结论态（与矩阵单元格态的桥接子集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactState {
    /// 未取证（不是事实，不许入板）。
    Pending,
    /// 取证通过。
    Proven,
    /// 取证失败。
    Failed,
}

/// P 域投递桥（V 域唯一的出遥测通道）。
pub struct PTBridge {
    board: Vec<PTEvent>,
    /// 遥测：拒投次数。
    pub rejects: u64,
}

impl PTBridge {
    /// 空桥。
    pub fn new() -> PTBridge {
        PTBridge { board: Vec::new(), rejects: 0 }
    }

    /// 入板：事实先行（Pending 拒）。
    pub fn publish(
        &mut self,
        state: FactState,
        kind: &'static str,
        key: &str,
        fp: u32,
    ) -> Result<(), String> {
        let val = match state {
            FactState::Pending => {
                self.rejects += 1;
                return Err(format!(
                    "{}：{} 未取证——Pending 入板=遥测造假",
                    E_VV_NOFACT, key
                ));
            }
            FactState::Proven => 1,
            FactState::Failed => 0,
        };
        let ev = PTEvent::new(kind, key, val, fp)?;
        self.board.push(ev);
        Ok(())
    }

    /// 板上事件数。
    pub fn len(&self) -> usize {
        self.board.len()
    }

    /// 板只读视图（改历史走新事件——返回的是副本）。
    pub fn snapshot(&self) -> Vec<PTEvent> {
        self.board.clone()
    }
}

/// 对账守恒：终态计数与板上计数一致。
pub fn bridge_reconcile(terminal: usize, bridge: &PTBridge) -> Result<(), String> {
    if terminal == bridge.len() {
        Ok(())
    } else {
        Err(format!(
            "{}：终态 {} 条 vs 板上 {} 条——观测缺口或重复事实",
            E_VV_IMMUT, terminal, bridge.len()
        ))
    }
}

/// 联动声明（复用纪律——非空即判据）。
pub const PLINK_DECLARATION: &str =
    "验收×P 域：验收事实翻译为 P 域事件入板，事实先行、投递不可变";

// ---------------------------------------------------------------------------
// 三、联动一组（判据）
// ---------------------------------------------------------------------------

/// CGPU-F3374 域自检：数据入板、复用、一组、判据。
pub fn run_vcv14_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv14_plink");

    // --- 事实先行：Pending 拒入板 ---
    let mut b = PTBridge::new();
    let r = b.publish(FactState::Pending, "criterion", "80帧P95", 0x11);
    cs.add(
        "Pending 入板拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_NOFACT),
        "未取证不是事实",
    );

    // --- 合法入板：Proven / Failed 两向 ---
    cs.add(
        "Proven 入板",
        b.publish(FactState::Proven, "criterion", "80帧P95", 0x11).is_ok(),
        "val=1",
    );
    cs.add(
        "Failed 入板",
        b.publish(FactState::Failed, "walkthrough", "菜单深度", 0x22).is_ok(),
        "val=0",
    );

    // --- 事件协议：类别闭集 ---
    let r = PTEvent::new("unknown", "x", 1, 0);
    cs.add(
        "类别闭集拒收",
        r.is_err(),
        "P 域事件三类：criterion/walkthrough/matrix",
    );

    // --- 事件协议：值域 0/1 ---
    let r = PTEvent::new("criterion", "x", 7, 0);
    cs.add("值域拒收", r.is_err() && r.unwrap_err().contains(E_VV_NOFACT), "事实值只许 0/1");

    // --- 事件协议：域号恒 V ---
    let ev = PTEvent::new("matrix", "dev0", 1, 3).expect("legal");
    cs.add(
        "域号恒 V",
        ev.domain == P_DOMAIN_V,
        "V 域事件域号钉死",
    );

    // --- 板只读：snapshot 改动不回写 ---
    let mut snap = b.snapshot();
    snap[0].val = 0;
    cs.add(
        "投递不可变",
        snap[0].val == 0 && b.snapshot()[0].val == 1,
        "改历史走新事件",
    );

    // --- 对账守恒 ---
    cs.add(
        "对账守恒通过",
        bridge_reconcile(2, &b).is_ok(),
        "终态数=板上数",
    );
    cs.add(
        "对账缺口拦截",
        bridge_reconcile(3, &b).is_err(),
        "少投=观测缺口",
    );

    // --- 声明非空 ---
    cs.add(
        "联动声明非空",
        !PLINK_DECLARATION.is_empty() && PLINK_DECLARATION.contains("P 域"),
        "复用纪律写入声明",
    );

    // --- 版本指纹 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for c in PLINK_VERSION.bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    cs.add("版本指纹非零", fp != 0, PLINK_VERSION);

    cs
}
