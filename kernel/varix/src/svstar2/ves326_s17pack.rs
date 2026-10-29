//! ves326_s17pack — VE-F4086 · T05 语言资源打包
//! 按语种打语言包（文本翻译+字体子集+本地化资产四类归一包）+ manifest 五字段冻结（语种/版本/体积/依赖/校验和）+ 增量包 diff + 校验和错拒装 + 依赖缺补包提示 + 版本乱序拒绝降级安装。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9440;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PkCode {
    Ok = 0,
    ChecksumBad = SEG_BASE + 0x01,
    DepMissing = SEG_BASE + 0x02,
    VersionOrder = SEG_BASE + 0x03,
    ManifestFrozen = SEG_BASE + 0x04,
    A11yIncomplete = SEG_BASE + 0x05,
}

/// 四类归一资产项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackKind { Text = 0, FontSubset = 1, Image = 2, AudioVideo = 3 }

impl PackKind {
    pub const ALL: [PackKind; 4] = [
        PackKind::Text, PackKind::FontSubset, PackKind::Image, PackKind::AudioVideo];
}

/// 打包项。
#[derive(Clone)]
pub struct PackItem { pub kind: PackKind, pub id: &'static str, pub size_kb: u32 }

/// manifest：五字段冻结——语种/版本/体积/依赖/校验和；另有无障碍资产完成度字段（域本色）。
#[derive(Clone)]
pub struct Manifest {
    pub lang: &'static str,
    pub version: u32,
    pub total_kb: u32,
    pub deps: Vec<&'static str>,
    pub checksum: u32,
    /// 无障碍资产完成度（0~10000 = 万分比）。
    pub a11y_complete: u32,
}

pub const FNV_SEED: u32 = 0x811C_9DC5;

pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = FNV_SEED;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// u32 十进制串化（no_std 手写，免 itoa）。
pub fn u32_str(mut v: u32) -> String {
    if v == 0 { return String::from("0"); }
    let mut buf = Vec::new();
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap_or_default()
}

/// 打包器：四类归一 → manifest（校验和 = 对清单内容 FNV）。
pub struct Packer { pub items: Vec<PackItem> }

impl Packer {
    pub fn new() -> Self { Packer { items: Vec::new() } }

    pub fn push(&mut self, it: PackItem) { self.items.push(it); }

    pub fn total_kb(&self) -> u32 { self.items.iter().map(|i| i.size_kb).sum() }

    /// 四类齐备判定：归一包必须四类都有。
    pub fn all_kinds(&self) -> bool {
        PackKind::ALL.iter().all(|k| self.items.iter().any(|i| i.kind == *k))
    }

    /// 冻结产出 manifest：校验和覆盖语种+版本+全部清单 id。
    pub fn freeze(&self, lang: &'static str, version: u32, deps: Vec<&'static str>, a11y_complete: u32) -> Manifest {
        let mut buf = String::new();
        buf.push_str(lang);
        buf.push_str(&u32_str(version));
        for it in self.items.iter() { buf.push_str(it.id); }
        let checksum = fnv1a(buf.as_bytes());
        Manifest { lang, version, total_kb: self.total_kb(), deps, checksum, a11y_complete }
    }
}

/// 安装器：五检前的安装门——校验和/依赖/版本序/完成度。
pub struct Installer {
    pub installed: Vec<Manifest>,
    pub available: Vec<&'static str>, // 可补的依赖包语种
}

impl Installer {
    pub fn new(available: Vec<&'static str>) -> Self { Installer { installed: Vec::new(), available } }

    /// 校验和复算：对给定清单重新打包应得同 checksum。
    pub fn verify_checksum(&self, m: &Manifest, items: &[PackItem]) -> bool {
        let mut p = Packer::new();
        for it in items.iter() { p.push(it.clone()); }
        let m2 = p.freeze(m.lang, m.version, m.deps.clone(), m.a11y_complete);
        m2.checksum == m.checksum
    }

    /// 安装门。
    pub fn install(&mut self, m: &Manifest, items: &[PackItem]) -> Result<(), PkCode> {
        if !self.verify_checksum(m, items) { return Err(PkCode::ChecksumBad); }
        if m.a11y_complete < 10_000 { return Err(PkCode::A11yIncomplete); }
        for d in m.deps.iter() {
            if !self.installed.iter().any(|x| x.lang == *d) && !self.available.contains(d) {
                return Err(PkCode::DepMissing);
            }
        }
        if let Some(prev) = self.installed.iter().find(|x| x.lang == m.lang) {
            if m.version <= prev.version { return Err(PkCode::VersionOrder); }
        }
        self.installed.push(m.clone());
        Ok(())
    }
}

/// 增量包 diff：对两版清单做变更集（新增/删除 id）。
#[derive(Clone)]
pub struct DiffItem { pub added: Vec<&'static str>, pub removed: Vec<&'static str> }

pub fn diff_lists(prev: &[&'static str], next: &[&'static str]) -> DiffItem {
    let added = next.iter().filter(|n| !prev.contains(n)).map(|n| *n).collect();
    let removed = prev.iter().filter(|p| !next.contains(p)).map(|p| *p).collect();
    DiffItem { added, removed }
}

impl DiffItem {
    pub fn is_empty(&self) -> bool { self.added.is_empty() && self.removed.is_empty() }
    /// 增量包体积 = 变更项体积（此处按每项固定 12KB 口径计）。
    pub fn size_kb(&self, both: &[(&'static str, u32)]) -> u32 {
        let mut kb = 0;
        for (id, sz) in both.iter() {
            if self.added.contains(id) { kb += sz; }
            if self.removed.contains(id) { kb += sz / 2; } // 删除需 tombstone，半体积口径
        }
        kb
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves326_checks() -> CheckSet {
    let mut set = CheckSet::new("ves326");
    let mut p = Packer::new();

    // 夹具：四类各一条。
    p.push(PackItem { kind: PackKind::Text, id: "txt-zh-9001", size_kb: 300 });
    p.push(PackItem { kind: PackKind::FontSubset, id: "fnt-zh-sub", size_kb: 2400 });
    p.push(PackItem { kind: PackKind::Image, id: "img-zh-hero", size_kb: 120 });
    p.push(PackItem { kind: PackKind::AudioVideo, id: "aud-zh-cue", size_kb: 80 });

    // C4086-01 四类归一：全部类别齐备。
    chk(&mut set, "C4086-01", p.all_kinds() && p.total_kb() == 2900, "四类归一 2900KB");

    // C4086-02 manifest 五字段冻结：校验和确定性（同内容同校验）。
    let m1 = p.freeze("zh", 9001, vec!["en"], 10_000);
    let m2 = p.freeze("zh", 9001, vec!["en"], 10_000);
    chk(&mut set, "C4086-02", m1.checksum == m2.checksum && m1.total_kb == 2900, "冻结确定性 + 体积一致");

    // C4086-03 校验和敏感：内容变 → 校验和变。
    let mut p2 = Packer::new();
    for it in p.items.iter() { p2.push(it.clone()); }
    p2.items[0].id = "txt-zh-9002";
    let m3 = p2.freeze("zh", 9001, vec!["en"], 10_000);
    chk(&mut set, "C4086-03", m3.checksum != m1.checksum, "清单变 => 校验和变");

    // C4086-04 校验和错拒装。
    let mut inst = Installer::new(vec![]);
    let mut forged = m1.clone();
    forged.checksum ^= 1;
    chk(&mut set, "C4086-04", inst.install(&forged, &p.items) == Err(PkCode::ChecksumBad), "校验和不符 => 拒装");

    // C4086-05 依赖缺 → 补包提示（依赖未装且不可补）。
    let mut nodep = Installer::new(vec![]);
    chk(&mut set, "C4086-05", nodep.install(&m1, &p.items) == Err(PkCode::DepMissing), "缺 en 依赖 => DepMissing");

    // C4086-06 完成度不足拒装。
    let mut part = p.freeze("zh", 9001, vec![], 9_000);
    let mut inst2 = Installer::new(vec![]);
    chk(&mut set, "C4086-06", inst2.install(&part, &p.items) == Err(PkCode::A11yIncomplete), "完成度 9000/10000 => 拒");

    // C4086-07 合法安装：依赖可补 + 校验和过。
    part.a11y_complete = 10_000;
    let mut inst3 = Installer::new(vec!["en"]);
    chk(&mut set, "C4086-07", inst3.install(&part, &p.items).is_ok(), "依赖可补 + 校验过 => 安装");

    // C4086-08 版本乱序拒绝降级安装。
    let older = p.freeze("zh", 9000, vec![], 10_000);
    chk(&mut set, "C4086-08", inst3.install(&older, &p.items) == Err(PkCode::VersionOrder), "9000 <= 9001 => 拒降级");

    // C4086-09 增量包 diff：新增+删除识别。
    let prev = ["txt-zh-9001", "fnt-zh-sub", "img-zh-hero"];
    let next = ["txt-zh-9002", "fnt-zh-sub", "img-zh-hero", "aud-zh-cue"];
    let d = diff_lists(&prev, &next);
    chk(&mut set, "C4086-09", d.added.len() == 2 && d.removed.len() == 1, "diff 增 2 删 1");

    // C4086-10 增量体积远小于全量。
    let both = [("txt-zh-9002", 300u32), ("aud-zh-cue", 80), ("txt-zh-9001", 300)];
    let dk = d.size_kb(&both);
    chk(&mut set, "C4086-10", dk == 530 && dk < 2900, "增量 530KB < 全量 2900KB");

    // C4086-11 空变更 = 无增量包。
    let d0 = diff_lists(&next, &next);
    chk(&mut set, "C4086-11", d0.is_empty(), "同版 diff 空");

    // C4086-12 校验和 FNV 实现锚点：空数据 = 偏移基。
    chk(&mut set, "C4086-12", fnv1a(b"") == FNV_SEED, "FNV 空数据恒等偏移基");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves326_smoke() {
        let set = run_ves326_checks();
        assert!(set.failed_count() == 0, "ves326 red: {}", set.red_items_text());
    }

    #[test]
    fn ves326_fnv_known() {
        // FNV-1a 32 已知向量：空串=2166136261，"a"=0xE40C292C。
        assert_eq!(fnv1a(b""), 0x811C_9DC5);
        assert_eq!(fnv1a(b"a"), 0xE40C_292C);
    }
}
