//! unx0269_bootcmd —— UNX-F0269 boot 命令（BootManifest 全视图）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0269 · boot 全视图 · 目标 240 行
//! 判据原文：UNX-F0269-J1 boot 命令一次吐 BootManifest 全视图：
//!           介质序 + 介质内条目序 + 启动优先级位；视图与账面
//!           diff 零漂移；默认启动项必须指向存活条目（死项作
//!           默认 = 引导必失败）。
//! 组件：启动条目（介质 + 槽位 + 存活位 + 权重）+ BootManifest
//!       （条目集 + 默认项 + 全视图序列化）+ 一致性核（默认项
//!       存活 / 介质槽位唯一 / 权重和界）。
//! 职责定位：全视图是「引导态的一面镜子」——视图必须与账面
//!   零漂移（看了假视图改配置等于闭眼开车）；默认项存活是
//!   引导可用性的最低线，死项默认在构造时当场拒。
//! 性能逐项分解：视图构造 O(条目数)；一致性核 O(条目数²)（≤16 条）。
//! 诊断码段：0x20280（0x20281~0x20289 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BC_VERSION: &str = "unx0269_bootcmd-v1";

/// 诊断段基址（0x20280 段 · 本模块独占）。
pub const BC_SEG_BASE: u32 = 0x20280;
pub const XC_DEAD_DEFAULT: u32 = 0x20281; // 默认项死
pub const XC_SLOT_DUP: u32 = 0x20282; // 介质内槽位重
pub const XC_MEDIA_OOR: u32 = 0x20283; // 介质号越域
pub const XC_WEIGHT_SUM: u32 = 0x20284; // 权重和越域
pub const XC_EMPTY_VIEW: u32 = 0x20285; // 空视图拒
pub const XC_DRIFT: u32 = 0x20286; // 视图/账面漂移
pub const XC_UNKNOWN_DEFAULT: u32 = 0x20287; // 默认项不存在
pub const XC_MAGIC: u32 = 0x20288; // 序列化魔数破
pub const XC_LEN_BAD: u32 = 0x20289; // 视图长不合

/// 介质闭集。
pub const MEDIA_MAX: u32 = 8;
/// 条目上限。
pub const ENTRY_MAX: u32 = 16;
/// 权重和上限（千分）。
pub const WEIGHT_SUM_MAX: u32 = 1000;

/// 启动条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BootEntry {
    /// 介质号 1~8。
    pub media: u32,
    /// 介质内槽位 0~7。
    pub slot: u32,
    /// 存活位（探测可引导）。
    pub alive: bool,
    /// 优先权重 0~500（千分）。
    pub weight: u32,
    /// 条目标签（介质×槽 唯一键的展示名）。
    pub tag: u32,
}

impl BootEntry {
    fn valid(&self) -> Result<(), u32> {
        if self.media == 0 || self.media > MEDIA_MAX {
            return Err(XC_MEDIA_OOR);
        }
        if self.slot > 7 {
            return Err(XC_MEDIA_OOR);
        }
        if self.weight > 500 {
            return Err(XC_WEIGHT_SUM);
        }
        Ok(())
    }
}

/// BootManifest 全视图。
pub struct BootManifest {
    entries: Vec<BootEntry>,
    /// 默认启动项 tag。
    default_tag: u32,
}

impl BootManifest {
    pub fn new(entries: &[BootEntry], default_tag: u32) -> Result<BootManifest, u32> {
        if entries.is_empty() || entries.len() as u32 > ENTRY_MAX {
            return Err(XC_EMPTY_VIEW);
        }
        let mut wsum: u32 = 0;
        for e in entries.iter() {
            e.valid()?;
            wsum = wsum.saturating_add(e.weight);
            // 介质内槽位唯一。
            if entries.iter().filter(|x| x.media == e.media && x.slot == e.slot).count() > 1 {
                return Err(XC_SLOT_DUP);
            }
        }
        if wsum >= WEIGHT_SUM_MAX {
            return Err(XC_WEIGHT_SUM);
        }
        // 默认项必须存在且存活。
        let d = entries.iter().find(|e| e.tag == default_tag).ok_or(XC_UNKNOWN_DEFAULT)?;
        if !d.alive {
            return Err(XC_DEAD_DEFAULT);
        }
        Ok(BootManifest { entries: entries.to_vec(), default_tag })
    }

    /// 条目数。
    pub fn len(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 默认项 tag。
    pub fn default_tag(&self) -> u32 {
        self.default_tag
    }

    /// 全视图行：条目按（介质, 槽位）升序 + 默认项标记。
    pub fn view(&self) -> Vec<(u32, u32, u32, bool)> {
        let mut rows: Vec<(u32, u32, u32, bool)> = self
            .entries
            .iter()
            .map(|e| (e.media, e.slot, e.tag, e.tag == self.default_tag))
            .collect();
        rows.sort_unstable();
        rows
    }

    /// 视图与账面 diff：视图行集合 == 账面派生集合（零漂移恒真，
    /// 序列化复跑时用于对拍外部账）。
    pub fn diff_ledger(&self, ledger: &[(u32, u32, u32, bool)]) -> Result<(), u32> {
        let v = self.view();
        if v.len() != ledger.len() {
            return Err(XC_DRIFT);
        }
        for (a, b) in v.iter().zip(ledger.iter()) {
            if a != b {
                return Err(XC_DRIFT);
            }
        }
        Ok(())
    }

    /// 序列化：魔数 + 默认项 + 逐条 4 词。
    pub fn serialize(&self) -> Vec<u32> {
        let mut words = vec![0x424F4F54u32, self.default_tag, self.entries.len() as u32]; // "BOOT"
        for e in self.entries.iter() {
            words.push(e.media | (e.slot << 8) | ((e.weight & 0x3FF) << 16));
            words.push(e.tag);
            words.push(e.alive as u32);
            words.push(0);
        }
        words
    }

    /// 反序列化 + 全约束复验。
    pub fn deserialize(words: &[u32]) -> Result<BootManifest, u32> {
        if words.len() < 3 || words[0] != 0x424F4F54 {
            return Err(XC_MAGIC);
        }
        let n = words[2] as usize;
        if words.len() != 3 + n * 4 {
            return Err(XC_LEN_BAD);
        }
        let mut entries = Vec::new();
        for i in 0..n {
            let w = words[3 + i * 4];
            entries.push(BootEntry {
                media: w & 0xFF,
                slot: (w >> 8) & 0xFF,
                weight: (w >> 16) & 0x3FF,
                tag: words[4 + i * 4],
                alive: words[5 + i * 4] != 0,
            });
        }
        BootManifest::new(&entries, words[1])
    }
}

pub fn run_unx0269_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0269_bootcmd");
    // 族一：视图构造。
    let entries = vec![
        BootEntry { media: 2, slot: 0, alive: true, weight: 400, tag: 100 },
        BootEntry { media: 1, slot: 3, alive: true, weight: 300, tag: 200 },
        BootEntry { media: 2, slot: 1, alive: false, weight: 100, tag: 300 },
    ];
    let man = match BootManifest::new(&entries, 100) {
        Ok(m) => m,
        Err(_) => {
            cs.add("视图构造", false, "被拒");
            return cs;
        }
    };
    cs.add("条目数", man.len() == 3, "三条目");
    cs.add("默认项", man.default_tag() == 100, "指向存活项");
    // 族二：全视图排序与标记。
    let v = man.view();
    cs.add("介质槽位序", v[0] == (1, 3, 200, false) && v[1] == (2, 0, 100, true), "升序");
    cs.add("默认标记", v[1].3, "100 号默认");
    cs.add("零漂移", man.diff_ledger(&v).is_ok(), "视图=账面派生");
    // 族三：默认项约束。
    cs.add("死项默认拒", matches!(BootManifest::new(&entries, 300), Err(XC_DEAD_DEFAULT)), "DEAD");
    cs.add("未知默认拒", matches!(BootManifest::new(&entries, 999), Err(XC_UNKNOWN_DEFAULT)), "UNKNOWN");
    // 族四：槽位/介质/权重约束。
    cs.add("槽位重拒", matches!(BootManifest::new(&[
        BootEntry { media: 1, slot: 0, alive: true, weight: 100, tag: 1 },
        BootEntry { media: 1, slot: 0, alive: true, weight: 100, tag: 2 },
    ], 1), Err(XC_SLOT_DUP)), "SLOT_DUP");
    cs.add("介质越域拒", BootManifest::new(&[
        BootEntry { media: 9, slot: 0, alive: true, weight: 100, tag: 1 },
    ], 1).is_err(), "MEDIA");
    cs.add("槽位越域拒", BootManifest::new(&[
        BootEntry { media: 1, slot: 8, alive: true, weight: 100, tag: 1 },
    ], 1).is_err(), "SLOT");
    cs.add("权重和越域拒", BootManifest::new(&[
        BootEntry { media: 1, slot: 0, alive: true, weight: 500, tag: 1 },
        BootEntry { media: 2, slot: 0, alive: true, weight: 500, tag: 2 },
    ], 1).is_err(), "和 1000 > 999 拦");
    // 族五：序列化往返。
    let words = man.serialize();
    cs.add("词位长", words.len() == 3 + 3 * 4, "头 3 + 3×4");
    let back = BootManifest::deserialize(&words);
    cs.add("往返零漂", match back {
        Ok(m2) => m2.view() == man.view() && m2.default_tag() == 100,
        Err(_) => false,
    }, "视图与默认双对");
    // 族六：篡改检出。
    let mut t = words.clone();
    t[0] = 0x41414141;
    cs.add("魔数拒", matches!(BootManifest::deserialize(&t), Err(XC_MAGIC)), "MAGIC");
    let mut t2 = words.clone();
    t2[2] = 9; // 例数谎报
    cs.add("长度拒", matches!(BootManifest::deserialize(&t2), Err(XC_LEN_BAD)), "LEN");
    cs.add("空视图拒", BootManifest::new(&[], 1).is_err(), "EMPTY");
    cs
}
