//! CGPU-F3372 · 验收与 T 域对接联动（vcv12_tlink）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3372`
//!
//! **判据（锚点原文）**：协议验收、复用、一组、判据。
//!
//! **职责定位（锚点原文）**：联动：联动（验收×T 域（对接协议验收项
//! ——联动复用；测试（联动一组）。
//!
//! # 一、T 域对接协议为什么需要「验收项」
//!
//! T 域（对接/协议域）定义了本内核与外部系统的握手协议；V 域的真
//! 机验收要覆盖这些协议项——不是重新发明一套协议测试，而是**复用
//! T 域的协议项清单**，把每一条协议义务映射成一条验收项（[`TLink`]）。
//! 复用的纪律：协议项 ID 直接引用 T 域登记号（不自造编号），验收
//! 结论回写时带原 ID——T 域看得到「我定义的每条义务在真机上验到
//! 什么程度」，V 域看得到「我验的每一条是谁的义务」。双向对账
//! （[`reconcile_tlinks`]）保证两边清单永不漂移。
//!
//! # 二、映射三态
//!
//! 每条协议义务在 V 域有三种状态：**已映射**（有验收项且已取证）、
//! **待验**（有验收项未取证）、**无验收项**（义务裸奔——拒绝发布）。
//! [`reconcile_tlinks`] 对无验收项的义务报错（[`E_VV_BARE`]）——
//! 协议义务没有验收项=发布后靠用户当测试员。
//!
//! # 三、联动一组（判据）
//!
//! 判据组：映射齐备（每条义务有验收项）+ ID 保真（引用不改号）+
//! 裸奔拦截（无验收项即拒）+ 回写带 ID + 对账守恒（两侧计数一致）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const TLINK_VERSION: &str = "V12-tlink-v1";

/// 协议义务无验收项（义务裸奔——拒绝发布）。
pub const E_VV_BARE: &str = "E_VV_BARE";

/// 协议项 ID 不在 T 域登记表（自造编号——复用纪律破坏）。
pub const E_VV_TID: &str = "E_VV_TID";

/// T 域协议义务登记表（复用 T 域清单——本表即 T 域侧真源的投影）。
pub const T_OBLIGATIONS: [&str; 8] = [
    "T-P01 握手序列",
    "T-P02 帧同步",
    "T-P03 断链恢复",
    "T-P04 背压协商",
    "T-P05 超时语义",
    "T-P06 错误码透传",
    "T-P07 乱序容忍",
    "T-P08 心跳保活",
];

/// 协议项验收态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// 已映射且已取证（附证据指纹）。
    Proven(u32),
    /// 有验收项未取证。
    Pending,
}

/// T 域义务 → V 域验收项的映射。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TLink {
    /// T 域协议项 ID（原样引用，不改号）。
    pub tid: &'static str,
    /// 验收项名。
    pub item: String,
    /// 验收态。
    pub state: LinkState,
}

impl TLink {
    /// 构造映射：ID 必须在 T 域登记表内（自造编号即拒）。
    pub fn new(tid: &'static str, item: &str) -> Result<TLink, String> {
        if !T_OBLIGATIONS.contains(&tid) {
            return Err(format!(
                "{}：{} 不在 T 域登记表——验收项不许自造协议编号",
                E_VV_TID, tid
            ));
        }
        Ok(TLink { tid, item: item.to_string(), state: LinkState::Pending })
    }

    /// 取证：Pending → Proven（重复取证幂等允许，指纹更新）。
    pub fn prove(&mut self, fp: u32) {
        self.state = LinkState::Proven(fp);
    }
}

// ---------------------------------------------------------------------------
// 二、对账
// ---------------------------------------------------------------------------

/// 双向对账：每条 T 域义务必须恰好有一条映射（无映射=裸奔拒收）。
pub fn reconcile_tlinks(links: &[TLink]) -> Result<(), String> {
    for ob in T_OBLIGATIONS.iter() {
        let hits = links.iter().filter(|l| l.tid == *ob).count();
        if hits == 0 {
            return Err(format!(
                "{}：协议义务 {} 无验收项——发布后靠用户当测试员",
                E_VV_BARE, ob
            ));
        }
        if hits > 1 {
            return Err(format!(
                "{}：协议义务 {} 有 {} 条映射——口径分裂",
                E_VV_TID, ob, hits
            ));
        }
    }
    for l in links.iter() {
        if !T_OBLIGATIONS.contains(&l.tid) {
            return Err(format!("{}：{} 越表", E_VV_TID, l.tid));
        }
    }
    Ok(())
}

/// 取证进度：已取证数 / 总义务数。
pub fn proven_ratio(links: &[TLink]) -> (usize, usize) {
    let p = links
        .iter()
        .filter(|l| matches!(l.state, LinkState::Proven(_)))
        .count();
    (p, T_OBLIGATIONS.len())
}

/// 回写串（T 域可读的对账行——必须带原 ID）。
pub fn report_line(l: &TLink) -> String {
    let st = match l.state {
        LinkState::Proven(fp) => format!("已取证({:#x})", fp),
        LinkState::Pending => "待验".to_string(),
    };
    format!("[{}] {}", l.tid, st)
}

/// 联动声明（复用纪律——非空即判据）。
pub const TLINK_DECLARATION: &str =
    "验收×T 域：协议义务全量映射为验收项，ID 原样引用双向对账";

// ---------------------------------------------------------------------------
// 三、联动一组（判据）
// ---------------------------------------------------------------------------

/// CGPU-F3372 域自检：协议验收、复用、一组、判据。
pub fn run_vcv12_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv12_tlink");

    // --- 全量映射可构造（8 条义务全部映射成功） ---
    let mut links: Vec<TLink> = Vec::new();
    let mut all_ok = true;
    for (i, ob) in T_OBLIGATIONS.iter().enumerate() {
        match TLink::new(ob, &format!("验收项-{}", i + 1)) {
            Ok(l) => links.push(l),
            Err(_) => all_ok = false,
        }
    }
    cs.add(
        "八条义务全量映射",
        all_ok && links.len() == T_OBLIGATIONS.len(),
        "锚点：对接协议验收项",
    );

    // --- 自造编号拒收 ---
    let r = TLink::new("T-P99 自造项", "假验收项");
    cs.add(
        "自造协议编号拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_TID),
        "复用纪律：ID 原样引用不改号",
    );

    // --- 对账：全量映射通过 ---
    cs.add("对账全量通过", reconcile_tlinks(&links).is_ok(), "无裸奔无分裂");

    // --- 裸奔拦截：抽走一条即拒 ---
    let shorted: Vec<TLink> = links.iter().skip(1).cloned().collect();
    let r = reconcile_tlinks(&shorted);
    cs.add(
        "裸奔义务拦截",
        r.is_err() && r.unwrap_err().contains(E_VV_BARE),
        "协议义务无验收项=用户当测试员",
    );

    // --- 重复映射拦截（口径分裂） ---
    let mut dup = links.clone();
    dup.push(TLink::new(T_OBLIGATIONS[0], "重复验收项").expect("legal tid"));
    cs.add(
        "重复映射拦截",
        reconcile_tlinks(&dup).is_err(),
        "一条义务一条验收项",
    );

    // --- 取证流转 + 进度 ---
    links[0].prove(0x1F);
    let (p, t) = proven_ratio(&links);
    cs.add(
        "取证进度口径",
        p == 1 && t == 8,
        "已取证/总义务",
    );
    cs.add(
        "回写带原 ID",
        report_line(&links[0]).starts_with("[T-P01") && report_line(&links[1]).contains("待验"),
        "T 域可读对账行",
    );

    // --- 声明非空 ---
    cs.add(
        "联动声明非空",
        !TLINK_DECLARATION.is_empty() && TLINK_DECLARATION.contains("T 域"),
        "复用纪律写入声明",
    );

    // --- 版本指纹 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in TLINK_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    cs.add("版本指纹非零", fp != 0, TLINK_VERSION);

    cs
}
