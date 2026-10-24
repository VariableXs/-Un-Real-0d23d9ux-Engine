//! veb354_diagpanel —— VE-F0354 软渲诊断面板。
//! 锚点：VE-F0354 · 目标 320 行。
//! 判据原文：五组指标（瓦片统计 / 线程池曲线 / SIMD 命中矩
//!           阵 / 降质档位 / SoA 转换计数）、原子快照（发布
//!           序号引用）、缺测标注（单源失联该组缺测）、零侵
//!           入（数据只读不侵入渲染路径）、判据。
//! 组件：五组指标聚合 + 快照发布器 + 缺测标注。
//! 职责定位：诊断面板是「仪表盘」——指针只看不拧（只读零侵
//!           入）；某块表坏了就挂牌「缺测」，不许拿别家读数
//!           顶上；每格快照有编号，新旧一眼可辨。
//! 性能逐项分解：聚合 O(指标数)；发布 O(1) 引用；只读零侵
//!           入。
//! 诊断码段：0x32180（0x32181~0x32189 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 模块版本。
pub const DP_VERSION: &str = "veb354_diagpanel-v1";

/// 诊断段基址（0x32180 段 · 本模块独占）。
pub const DP_SEG_BASE: u32 = 0x32180;
pub const XC_NOSRC: u32 = 0x32181; // 单源失联（缺测标注）
pub const XC_SEQ: u32 = 0x32182; // 发布序号倒退
pub const XC_TIMEOUT: u32 = 0x32183; // 刷新超时（跳帧不堆积）

/// 五组指标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snap {
    /// 瓦片统计：分块数、负载均衡度（百分比）。
    pub tiles: (u32, u32),
    /// 线程池负载曲线（采样点）。
    pub pool: [u32; 4],
    /// SIMD 派发命中数（Simd 路径数）。
    pub simd_hit: u32,
    /// 降质档位（0~3）。
    pub deg_lvl: u32,
    /// SoA 转换计数。
    pub soa_convs: u32,
    /// 缺测位图（bit i = 第 i 组缺测）。
    pub na_mask: u32,
    /// 发布序号。
    pub seq: u32,
}

/// 快照发布器。
pub struct Panel {
    last: Option<Snap>,
    /// 发布次数。
    pub pubs: u32,
    /// 跳帧数。
    pub skips: u32,
}

impl Panel {
    /// 空面板。
    pub fn new() -> Panel {
        Panel {
            last: None,
            pubs: 0,
            skips: 0,
        }
    }

    /// 原子发布：序号必须递增；失败保留上版。
    pub fn publish(&mut self, s: Snap) -> Result<u32, u32> {
        if let Some(ref l) = self.last {
            if s.seq <= l.seq {
                self.skips += 1;
                return Err(XC_SEQ);
            }
        }
        self.last = Some(s);
        self.pubs += 1;
        Ok(s.seq)
    }

    /// 当前快照（只读）。
    pub fn read(&self) -> Option<Snap> {
        self.last
    }

    /// 缺测标注：源失联时置位 na_mask。
    pub fn mark_na(s: &mut Snap, group: u32) -> Result<(), u32> {
        if group > 4 {
            return Err(XC_NOSRC);
        }
        s.na_mask |= 1 << group;
        Ok(())
    }

    /// 缺测组数。
    pub fn na_count(s: &Snap) -> u32 {
        s.na_mask.count_ones()
    }

    /// 完整核：零缺测。
    pub fn complete(s: &Snap) -> bool {
        s.na_mask == 0
    }

    /// 文字档位语义（降质档可查不只靠视觉）。
    pub fn lvl_text(lvl: u32) -> &'static str {
        match lvl {
            0 => "满档",
            1 => "降分辨率",
            2 => "降效果",
            3 => "降帧率",
            _ => "未知档",
        }
    }

    /// 超时跳帧：超时后跳到最新序号（不堆积旧快照）。
    pub fn on_timeout(&mut self, latest: u32) -> u32 {
        if let Some(ref l) = self.last {
            let lost = latest.saturating_sub(l.seq).saturating_sub(1);
            self.skips += lost;
            return lost;
        }
        0
    }
}

pub fn run_veb354_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0354");
    // 族零：发布序号。
    let mut p = Panel::new();
    s.add("空面板无快照", p.read().is_none(), "未发布");
    let sn1 = Snap {
        tiles: (12, 90),
        pool: [10, 20, 30, 40],
        simd_hit: 64,
        deg_lvl: 0,
        soa_convs: 1,
        na_mask: 0,
        seq: 1,
    };
    s.add("首版过", p.publish(sn1) == Ok(1), "seq 1");
    s.add("可读", p.read().map(|x| x.seq) == Some(1), "只读");
    // 族一：倒退拒与保留。
    let mut sn0 = sn1;
    sn0.seq = 0;
    s.add("倒退拒", p.publish(sn0) == Err(XC_SEQ), "序号倒退");
    s.add("保留上版", p.read().map(|x| x.seq) == Some(1), "失败留旧");
    s.add("跳帧一", p.skips == 1, "跳过");
    s.add("同版拒", p.publish(sn1) == Err(XC_SEQ), "同版非原子更新");
    // 族二：五组指标。
    let sn2 = Snap {
        tiles: (16, 85),
        pool: [20, 40, 60, 80],
        simd_hit: 80,
        deg_lvl: 1,
        soa_convs: 2,
        na_mask: 0,
        seq: 2,
    };
    s.add("二版过", p.publish(sn2) == Ok(2), "seq 2");
    let cur = match p.read() {
        Some(x) => x,
        None => {
            s.add("读败", false, "不应到");
            return s;
        }
    };
    s.add("瓦片十六", cur.tiles.0 == 16, "分块数");
    s.add("均衡八五", cur.tiles.1 == 85, "均衡度");
    s.add("池四点", cur.pool[3] == 80, "曲线末点");
    s.add("SIMD 命中八十", cur.simd_hit == 80, "矩阵");
    s.add("降质一级", cur.deg_lvl == 1, "档位");
    s.add("SoA 二次", cur.soa_convs == 2, "转换计数");
    s.add("发布二", p.pubs == 2, "账二");
    // 族三：缺测标注。
    let mut sn3 = sn2;
    sn3.seq = 3;
    s.add("标缺测过", Panel::mark_na(&mut sn3, 1).is_ok(), "池曲线失联");
    s.add("缺测一", Panel::na_count(&sn3) == 1, "bit1");
    s.add("不完整", !Panel::complete(&sn3), "有缺测");
    s.add("标池失联过", Panel::mark_na(&mut sn3, 4).is_ok(), "SoA 失联");
    s.add("缺测二", Panel::na_count(&sn3) == 2, "bit4");
    s.add("坏组拒", Panel::mark_na(&mut sn3, 5) == Err(XC_NOSRC), ">4");
    s.add("三版过", p.publish(sn3) == Ok(3), "缺测也可发布");
    s.add("三版可读", p.read().map(|x| x.na_mask) == Some(0b10010), "标注在案");
    // 完整版覆盖。
    let sn4 = Snap {
        seq: 4,
        na_mask: 0,
        ..sn3
    };
    let _ = p.publish(sn4);
    s.add("复全", p.read().map(|x| Panel::complete(&x)) == Some(true), "恢复");
    // 族四：超时跳帧。
    s.add("超时跳二", p.on_timeout(6) == 1, "seq 4→6 丢 5");
    s.add("跳帧累计", p.skips >= 3, "不堆积");
    // 族五：文字档位。
    s.add("档 0 满档", Panel::lvl_text(0) == "满档", "文字可查");
    s.add("档 3 降帧", Panel::lvl_text(3) == "降帧率", "语义清晰");
    s.add("档 4 未知", Panel::lvl_text(4) == "未知档", "越界防御");
    s.add("魔数核", DP_SEG_BASE == 0x32180, "段基址");
    s
}
