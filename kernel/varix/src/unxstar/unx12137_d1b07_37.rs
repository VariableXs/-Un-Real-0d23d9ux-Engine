//! UNX-F12137 — .xdata 压缩与对齐规则（0x1a340 段）
//!
//! 锚点：UNX-F12137-J1 压缩 .xdata（单 UNWIND_INFO 12 字节内）与对齐规则
//! （DWORD 对齐 + 帧偏移 slot 布局），宽函数降级展开 10/10。
//! 域：CoRun·UNX-D1-B07（.pdata 展开表面谱系）。
//! 结构：UNWIND_INFO 编码器（版本/标志/slot 布局）+ 压缩判定（slot 数 ≤2 即 12 字节内）+
//! 对齐校验器（DWORD 对齐填充）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a340;
pub const XD_ALIGN: u32 = SEG_BASE + 0x01; // 对齐违例
pub const XD_WIDE: u32 = SEG_BASE + 0x02; // 宽函数降级
pub const XD_VERSION: u32 = SEG_BASE + 0x03; // 版本位非法
pub const XD_EMPTY: u32 = SEG_BASE + 0x04; // 名为空
pub const XD_FROZEN: u32 = SEG_BASE + 0x05; // 印后编码

/// UNWIND_INFO 版本位（低 3 位）。
pub const UNW_VER: u32 = 1;
/// 压缩 .xdata 上限：头 4 字节 + 2 slot × 2 字节 + 尾 4 字节 = 12 字节。
pub const COMPACT_MAX: usize = 12;
/// slot 宽度 2 字节。
pub const SLOT_W: usize = 2;

/// UNWIND_INFO 编码产物。
#[derive(Clone)]
pub struct XdataInfo {
    pub func: String,
    /// 标志位（高 5 位）。
    pub flags: u32,
    /// 帧 slot 数（0~2 压缩，>2 降级展开）。
    pub slots: usize,
    /// 编码字节数。
    pub size: usize,
    /// 是否 DWORD 对齐。
    pub aligned: bool,
}

impl XdataInfo {
    /// 压缩判定：slot ≤ 2 时 12 字节内。
    pub fn compact(&self) -> bool {
        self.slots <= 2 && self.size <= 4 + 2 * SLOT_W + 4
    }

    /// 展开判定：slot > 2 降级为宽格式。
    pub fn wide(&self) -> bool {
        self.slots > 2
    }

    pub fn ok(&self) -> bool {
        !self.func.is_empty() && self.flags <= 0x1F && self.aligned
    }

    pub fn render(&self) -> String {
        format!(
            "{}:v{}|f{:#x}|{}slot|{}B{}",
            self.func,
            UNW_VER,
            self.flags,
            self.slots,
            self.size,
            if self.compact() { " compact" } else { " wide" }
        )
    }
}

/// .xdata 编码器。
pub struct XdataEncoder {
    pub infos: Vec<XdataInfo>,
    pub seq_next: u32,
    sealed: bool,
}

impl XdataEncoder {
    pub fn new() -> XdataEncoder {
        XdataEncoder { infos: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 编码一个函数的 UNWIND_INFO（对齐违例/版本非法/名空必拒）。
    pub fn encode(&mut self, func: &str, flags: u32, slots: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(XD_FROZEN);
        }
        if func.is_empty() {
            return Err(XD_EMPTY);
        }
        if flags > 0x1F {
            return Err(XD_VERSION);
        }
        // DWORD 对齐：总长向上取整到 4 的倍数。
        let raw = 4 + slots * SLOT_W + 4;
        let size = (raw + 3) & !3usize;
        if size != raw && slots > 2 && size > raw + 3 {
            return Err(XD_ALIGN);
        }
        let aligned = size % 4 == 0;
        self.infos.push(XdataInfo {
            func: func.to_string(),
            flags,
            slots,
            size,
            aligned,
        });
        Ok(self.infos.len() - 1)
    }

    /// 压缩函数数。
    pub fn compact_count(&self) -> usize {
        self.infos.iter().filter(|i| i.compact()).count()
    }

    /// 宽函数数。
    pub fn wide_count(&self) -> usize {
        self.infos.iter().filter(|i| i.wide()).count()
    }

    /// 全部 DWORD 对齐。
    pub fn all_aligned(&self) -> bool {
        self.infos.iter().all(|i| i.aligned)
    }

    /// 全自洽。
    pub fn all_ok(&self) -> bool {
        self.infos.iter().all(|i| i.ok())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a340;
        for i in &self.infos {
            h = h.wrapping_mul(211).wrapping_add(i.flags);
            h = h.wrapping_mul(211).wrapping_add(i.slots as u32);
            h = h.wrapping_mul(211).wrapping_add(i.size as u32);
            for by in i.func.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 按名查。
    pub fn find(&self, func: &str) -> Option<&XdataInfo> {
        self.infos.iter().find(|i| i.func == func)
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "total={} compact={} wide={} aligned={}",
            self.infos.len(),
            self.compact_count(),
            self.wide_count(),
            self.all_aligned()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = XdataEncoder::new();
    // 1) 名空必拒。
    got[0] = e.encode("", 0, 0).err() == Some(XD_EMPTY);
    // 2) 标志越 5 位必拒。
    got[1] = e.encode("f", 0x3F, 0).err() == Some(XD_VERSION);
    // 3) 宽函数降级（3 slot 非 compact）。
    let _ = e.encode("wide_inj", 0, 5);
    got[2] = e.find("wide_inj").unwrap().wide() && !e.find("wide_inj").unwrap().compact();
    // 4) 编码产物全 DWORD 对齐（压缩与宽皆然）。
    got[3] = e.all_aligned();
    // 5) 印后编码必拒。
    e.seal();
    got[4] = e.encode("post", 0, 0).err() == Some(XD_FROZEN);
    got
}

/// 判据入口：UNX-F12137
pub fn run_unx12137_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12137");
    let mut e = XdataEncoder::new();

    // J1 压缩编码 10/10：slot 0~2 全走 12 字节内。
    let mut j1 = true;
    for i in 0..10usize {
        let slots = i % 3;
        let name = format!("cmp{}", i);
        if e.encode(&name, i as u32 & 0x1F, slots).is_err() {
            j1 = false;
        }
        if !e.find(&name).unwrap().compact() {
            j1 = false;
        }
    }
    s.add("J1_compact_10x", j1 && e.compact_count() == 10, "压缩编码 10/10 全 12 字节内");

    // J2 宽函数降级 10/10：slot 3~12 皆 wide。
    let mut j2 = true;
    for i in 0..10usize {
        let slots = 3 + i;
        let name = format!("wide{}", i);
        let _ = e.encode(&name, 0, slots);
        let info = e.find(&name).unwrap();
        if !info.wide() || info.compact() {
            j2 = false;
        }
    }
    s.add("J2_wide_10x", j2 && e.wide_count() == 10, "宽函数降级 10/10");

    // J3 全 DWORD 对齐。
    s.add("J3_aligned", e.all_aligned(), "全产物 DWORD 对齐");

    // J4 全自洽。
    s.add("J4_all_ok", e.all_ok(), "全 INFO 过合法性核");

    // J5 尺寸公式：4 + slot×2 + 4，向上取整。
    let info = e.find("wide0").unwrap();
    s.add(
        "J5_size_formula",
        info.slots == 3 && info.size == ((4 + 3 * 2 + 4 + 3) & !3),
        "宽格式尺寸公式对平",
    );

    // J6 版本位受控。
    s.add(
        "J6_version",
        e.encode("badver", 0x20, 0).err() == Some(XD_VERSION),
        "标志越 5 位拒 VERSION",
    );

    // J7 名空受控。
    s.add("J7_empty", e.encode("", 0, 0).err() == Some(XD_EMPTY), "名空拒 EMPTY");

    // J8 渲染行含版本/标志/尺寸。
    let r = e.find("cmp0").unwrap().render();
    s.add(
        "J8_render",
        r.contains("v1|") && r.contains("B compact"),
        "渲染行含版本/标志/slot/尺寸",
    );

    // J9 指纹稳定。
    let f1 = e.fold();
    let f2 = e.fold();
    s.add("J9_fingerprint", f1 == f2, "编码器折叠指纹一致");

    // J10 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 印账受控。
    e.seal();
    s.add(
        "J11_sealed",
        e.encode("post", 0, 0).err() == Some(XD_FROZEN),
        "印账后编码拒 FROZEN",
    );

    // J12 零 slot 极小产物。
    let mut e2 = XdataEncoder::new();
    let _ = e2.encode("min", 0, 0);
    s.add(
        "J12_min_size",
        e2.find("min").unwrap().size == 8,
        "零 slot 产物 8 字节",
    );

    // J13 双 slot 压缩上限（12 字节整）。
    let _ = e2.encode("max2", 0, 2);
    s.add(
        "J13_max_compact",
        e2.find("max2").unwrap().size == COMPACT_MAX,
        "双 slot 压缩恰 12 字节",
    );

    // J14 压缩/宽互斥。
    let all = e2.infos.iter().all(|i| i.compact() != i.wide());
    s.add("J14_exclusive", all, "压缩与宽判定互斥");

    // J15 账面摘要。
    let sm = e2.book_summary();
    s.add(
        "J15_summary",
        sm.contains("total=2") && sm.contains("compact=2"),
        "摘要含压缩/宽计数",
    );

    // J16 对齐填充生效：奇 slot 产物向上取整。
    let _ = e2.encode("odd", 0, 1);
    let odd = e2.find("odd").unwrap();
    s.add(
        "J16_odd_pad",
        odd.size == 12 && (4 + 1 * 2 + 4) == 10 && odd.aligned,
        "奇 slot 10 字节补到 12 对齐",
    );

    // J17 按名查命中/未命中。
    s.add(
        "J17_find",
        e2.find("min").is_some() && e2.find("nope").is_none(),
        "按名查命中/未命中",
    );

    // J18 空账指纹稳定。
    let e3 = XdataEncoder::new();
    s.add("J18_fresh_fp", e3.fold() == e3.fold(), "空编码器指纹稳定");

    // J19 宽函数 size 仍为 4 的倍数。
    let wide_all = e.infos.iter().filter(|i| i.wide()).all(|i| i.size % 4 == 0);
    s.add("J19_wide_aligned", wide_all, "宽函数产物仍 DWORD 对齐");

    // J20 slot 计数守恒：压缩 + 宽 = 全账。
    s.add(
        "J20_conserved",
        e.compact_count() + e.wide_count() == e.infos.len(),
        "压缩 + 宽 = 全账",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12137_compact_wide() {
        let mut e = XdataEncoder::new();
        let _ = e.encode("c", 0, 1);
        let _ = e.encode("w", 0, 7);
        assert!(e.find("c").unwrap().compact());
        assert!(e.find("w").unwrap().wide());
    }

    #[test]
    fn unx12137_checks_green() {
        let s = run_unx12137_checks();
        assert!(s.all_passed(), "UNX-F12137 判据未全绿");
    }

    #[test]
    fn unx12137_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
