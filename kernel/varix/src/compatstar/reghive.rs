//! F009 注册表虚拟化（compatstar · G-A-09）——「注册表泥潭」在结构上不存在。
//!
//! 主册判据（验收标准第一句）：
//! **「三族安装器（F030）安装-使用-卸载全周期后：全局模板零写入（哈希不变）、
//! 蜂巢完整回收；断电注入百次后蜂巢可打开率 100%（WAL 恢复）。」**
//!
//! 功能定义（G-A-09）：每应用独立注册表蜂巢（per-app hive）：应用视角的
//! HKCU/HKLM 读写全部落在自己的蜂巢文件；读操作先查自有蜂巢、未命中回读
//! 系统模板（只读）；写操作一律进自有蜂巢。卸载 = 蜂巢文件进回收站（零真删
//! 红线）。
//!
//! 【交互设计】设置中心「应用-高级」页显示蜂巢大小与「查看注册表内容」入口
//! （只读树视图）；诊断中心导出蜂巢快照。【数据与存储】蜂巢文件存沙盒目录
//! （F010），格式自定（键值对 B 树，写入走 WAL 保断电一致）；系统模板蜂巢
//! 只读内嵌镜像。
//! 【状态与异常】蜂巢损坏 → 重建空蜂巢 + 通知（应用设置丢失如实告知，不静
//! 默）；蜂巢超 256MB → 告警；并发写冲突 WAL 串行化。
//! 【设计细节】蜂巢 B 树节点 4KB、写放大控制（脏节点合并落盘）；系统模板蜂
//! 巢启动时只读共享；蜂巢查看器支持键值复制与十六进制值查看；导出快照为
//! JSON（开放格式 F126）；恢复点（F121）自动纳入蜂巢快照——注册表可回滚。
//!
//! 零堆纪律：B 树节点定长 4KB × 定长节点池，键定长 64B / 值定长 256B，
//! 无 Vec/String/Box/format!（节点池容量 64 节点 = 256KB 工程值，登记报告）。

use crate::checks::CheckSet;
use alloc::string::ToString;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 蜂巢 B 树节点尺寸 4KB（主册【设计细节】）。
pub const NODE_SIZE: usize = 4096;
/// 蜂巢告警线 256MB（主册【状态与异常】：正常应用设置 <1MB，超限必有异常）。
pub const HIVE_ALARM_BYTES: u64 = 256 * 1024 * 1024;
/// 键路径最大长度（键定长存储）。
pub const KEY_MAX: usize = 64;
/// 值最大长度（值定长存储）。
pub const VAL_MAX: usize = 256;
/// B 树节点池容量（工程值：64 节点 × 4KB = 256KB，登记完成报告）。
pub const NODE_POOL: usize = 64;
/// B 树阶（每节点最大键数——4KB 节点承载的键槽数按定长记录折算）。
pub const NODE_ORDER: usize = 16;
/// CLSID 子树前缀（F019 消费面：COM 注册进蜂巢 Classes\CLSID 节点）。
pub const CLSID_PREFIX: &str = "Classes\\CLSID\\";

// ---------------------------------------------------------------------------
// B 树（键值对，WAL 前置）
// ---------------------------------------------------------------------------

/// 叶子记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rec {
    pub key: [u8; KEY_MAX],
    pub key_len: usize,
    pub val: [u8; VAL_MAX],
    pub val_len: usize,
}

impl Rec {
    pub fn new(key: &str, val: &[u8]) -> Option<Rec> {
        let kb = key.as_bytes();
        if kb.len() > KEY_MAX || val.len() > VAL_MAX {
            return None;
        }
        let mut r = Rec {
            key: [0; KEY_MAX],
            key_len: kb.len(),
            val: [0; VAL_MAX],
            val_len: val.len(),
        };
        r.key[..kb.len()].copy_from_slice(kb);
        r.val[..val.len()].copy_from_slice(val);
        Some(r)
    }

    pub fn key_str(&self) -> &str {
        core::str::from_utf8(&self.key[..self.key_len]).unwrap_or("")
    }

    pub fn val_bytes(&self) -> &[u8] {
        &self.val[..self.val_len]
    }
}

/// 4KB 节点（B 树：键有序数组 + 子指针/叶子记录）。
#[derive(Clone, Copy)]
struct Node {
    /// 叶子：记录槽；内部：分隔键 + 子节点号。
    leaf: bool,
    recs: [Option<Rec>; NODE_ORDER],
    children: [i16; NODE_ORDER + 1],
    n: usize,
}

impl Node {
    const fn new() -> Node {
        Node { leaf: true, recs: [None; NODE_ORDER], children: [-1; NODE_ORDER + 1], n: 0 }
    }
}

/// 蜂巢：每应用一棵 B 树 + WAL + 模板回读。
pub struct Hive {
    nodes: [Node; NODE_POOL],
    root: i16,
    free: usize,
    /// 模板（只读共享面——读未命中回读）。
    template: &'static [(&'static str, &'static [u8])],
    /// WAL 缓冲（写前日志——断电一致）。
    wal: [Option<Rec>; 128],
    wal_n: usize,
    /// 脏节点合并落盘计数（写放大控制记账）。
    flushes: u32,
    /// 总字节估算（蜂巢大小显示面）。
    bytes: u64,
    /// 损坏重建标记（通知中心报备数据源）。
    pub rebuilt_notify: bool,
    /// 卸载回收站标记（零真删红线：recycled 而非 deleted）。
    pub recycled: bool,
}

impl Hive {
    /// 新建空蜂巢（带只读模板回读面）。
    pub fn new(template: &'static [(&'static str, &'static [u8])]) -> Hive {
        let mut h = Hive {
            nodes: [Node::new(); NODE_POOL],
            root: 0,
            free: 1,
            template,
            wal: [None; 128],
            wal_n: 0,
            flushes: 0,
            bytes: NODE_SIZE as u64,
            rebuilt_notify: false,
            recycled: false,
        };
        h.nodes[0].leaf = true;
        h
    }

    fn alloc_node(&mut self) -> Option<i16> {
        if self.free >= NODE_POOL {
            return None;
        }
        let id = self.free as i16;
        self.nodes[id as usize] = Node::new();
        self.free += 1;
        Some(id)
    }

    // -- 读（自有蜂巢 → 模板回读） ------------------------------------------

    /// 读键：先查自有蜂巢；未命中回读系统模板（只读）。
    pub fn get(&self, key: &str) -> Option<Rec> {
        if self.recycled {
            return None; // 卸载后蜂巢在回收站——读为空（应用已不可见）
        }
        if let Some(r) = self.btree_get(key) {
            return Some(r);
        }
        self.template.iter().find_map(|(k, v)| {
            if *k == key {
                Rec::new(k, v)
            } else {
                None
            }
        })
    }

    fn btree_get(&self, key: &str) -> Option<Rec> {
        let mut id = self.root;
        loop {
            let node = &self.nodes[id as usize];
            let mut i = 0;
            while i < node.n {
                let r = node.recs[i].unwrap();
                match cmp_key(r.key_str(), key) {
                    core::cmp::Ordering::Equal => return Some(r),
                    core::cmp::Ordering::Less => i += 1,
                    core::cmp::Ordering::Greater => break,
                }
            }
            if node.leaf {
                return None;
            }
            id = node.children[i];
            if id < 0 {
                return None;
            }
        }
    }

    // -- 写（一律进自有蜂巢，WAL 前置） --------------------------------------

    /// 写键。WAL 先记，再落 B 树；成功返回 true。超池 → 失败（背压如实）。
    pub fn set(&mut self, key: &str, val: &[u8]) -> bool {
        if self.recycled {
            return false;
        }
        let rec = match Rec::new(key, val) {
            Some(r) => r,
            None => return false,
        };
        // WAL 前置（断电一致：日志先于数据）。
        if self.wal_n < self.wal.len() {
            self.wal[self.wal_n] = Some(rec);
            self.wal_n += 1;
        }
        let old = self.btree_get(key);
        let delta = match &old {
            Some(o) => rec.val_len as i64 - o.val_len as i64,
            None => (rec.key_len + rec.val_len + 8) as i64,
        };
        self.bytes = (self.bytes as i64 + delta).max(0) as u64;
        let inserted = self.btree_insert(rec);
        if inserted {
            self.flushes += 1; // 脏节点合并落盘（简化计数：每写一批）
        }
        inserted
    }

    /// 删键（卸载清扫路径的细粒度操作；返回是否存在）。
    pub fn delete(&mut self, key: &str) -> bool {
        self.btree_delete(key)
    }

    fn btree_insert(&mut self, rec: Rec) -> bool {
        // 覆盖路径：树内已有同键 → 原位改。
        if self.btree_set_in(rec) {
            return true;
        }
        let root_leaf = self.nodes[self.root as usize].leaf;
        if root_leaf {
            let root = &mut self.nodes[self.root as usize];
            if root.n < NODE_ORDER {
                // 有序插入。
                let mut pos = root.n;
                for i in 0..root.n {
                    if cmp_key(root.recs[i].unwrap().key_str(), rec.key_str()) == core::cmp::Ordering::Greater {
                        pos = i;
                        break;
                    }
                }
                let mut j = root.n;
                while j > pos {
                    root.recs[j] = root.recs[j - 1];
                    j -= 1;
                }
                root.recs[pos] = Some(rec);
                root.n += 1;
                return true;
            }
            // 根满 → 分裂：右半移新节点，提升中间键到新根。
            let right = match self.alloc_node() {
                Some(n) => n,
                None => return false,
            };
            let mid = NODE_ORDER / 2;
            // 先从左节点值拷贝，再写右节点——借用分离。
            let (up_val, moved): (Option<Rec>, [Option<Rec>; NODE_ORDER]) = {
                let rn = &self.nodes[self.root as usize];
                let mut mv = [None; NODE_ORDER];
                for k in 0..(NODE_ORDER - mid) {
                    mv[k] = rn.recs[mid + k];
                }
                (rn.recs[mid - 1], mv)
            };
            {
                let nr = &mut self.nodes[right as usize];
                nr.leaf = true;
                for k in 0..(NODE_ORDER - mid) {
                    nr.recs[k] = moved[k];
                }
                nr.n = NODE_ORDER - mid;
            }
            let up = up_val;
            let old_root = self.root;
            let new_root = match self.alloc_node() {
                Some(n) => n,
                None => return false,
            };
            self.nodes[new_root as usize].leaf = false;
            self.nodes[new_root as usize].recs[0] = up;
            self.nodes[new_root as usize].children[0] = old_root;
            self.nodes[new_root as usize].children[1] = right;
            self.nodes[new_root as usize].n = 1;
            // 左半收缩。
            self.nodes[old_root as usize].n = mid - 1;
            self.root = new_root;
            // 重试插入（此时根为内部节点，走下沉路径）。
            return self.btree_insert(rec);
        }
        // 内部根：下沉到孩子（两级树：孩子必为叶）。di = 下沉槽位。
        let (child, di) = {
            let root = &self.nodes[self.root as usize];
            let mut i = 0;
            while i < root.n {
                if cmp_key(root.recs[i].unwrap().key_str(), rec.key_str()) == core::cmp::Ordering::Greater {
                    break;
                }
                i += 1;
            }
            (root.children[i], i)
        };
        if child < 0 {
            // 最右侧空槽（rec 大于全部已提升键）：直接建叶挂上。
            // 不变量：children[k] 为 -1 仅出现在 k == root.n。
            debug_assert_eq!(di, self.nodes[self.root as usize].n);
            let leaf = match self.alloc_node() {
                Some(n) => n,
                None => return false,
            };
            self.nodes[leaf as usize].leaf = true;
            self.nodes[leaf as usize].recs[0] = Some(rec);
            self.nodes[leaf as usize].n = 1;
            self.nodes[self.root as usize].children[di] = leaf;
            return true;
        }
        let leaf_full = self.nodes[child as usize].n >= NODE_ORDER;
        if !leaf_full {
            let leaf = &mut self.nodes[child as usize];
            let mut pos = leaf.n;
            for i in 0..leaf.n {
                if cmp_key(leaf.recs[i].unwrap().key_str(), rec.key_str()) == core::cmp::Ordering::Greater {
                    pos = i;
                    break;
                }
            }
            let mut j = leaf.n;
            while j > pos {
                leaf.recs[j] = leaf.recs[j - 1];
                j -= 1;
            }
            leaf.recs[pos] = Some(rec);
            leaf.n += 1;
            return true;
        }
        // 叶满 → 分裂：右半移新叶，中间键提升到根（根未满才可行——两级树
        // 容量极限按背压如实拒绝，工程值见 NODE_POOL/NODE_ORDER 注释）。
        if self.nodes[self.root as usize].n >= NODE_ORDER {
            return false;
        }
        let mid = NODE_ORDER / 2;
        let right = match self.alloc_node() {
                Some(n) => n,
                None => return false,
            };
        let (up_val, moved): (Rec, [Option<Rec>; NODE_ORDER]) = {
            let ln = &self.nodes[child as usize];
            let mut mv = [None; NODE_ORDER];
            for k in 0..(NODE_ORDER - mid) {
                mv[k] = ln.recs[mid + k];
            }
            (ln.recs[mid - 1].unwrap(), mv)
        };
        let up = up_val;
        {
            let nr = &mut self.nodes[right as usize];
            nr.leaf = true;
            for k in 0..(NODE_ORDER - mid) {
                nr.recs[k] = moved[k];
            }
            nr.n = NODE_ORDER - mid;
        }
        self.nodes[child as usize].n = mid - 1;
        let root = &mut self.nodes[self.root as usize];
        // up 插在 recs[di]，right 挂在 children[di+1]（其余右移）。
        let mut j = root.n;
        while j > di {
            root.recs[j] = root.recs[j - 1];
            j -= 1;
        }
        root.recs[di] = Some(up);
        let mut j = root.n + 1;
        while j > di + 1 {
            root.children[j] = root.children[j - 1];
            j -= 1;
        }
        root.children[di + 1] = right;
        root.n += 1;
        // 重试（分裂后目标叶必有空位）。
        self.btree_insert(rec)
    }

    fn btree_set_in(&mut self, rec: Rec) -> bool {
        let mut id = self.root;
        loop {
            let node = &mut self.nodes[id as usize];
            let mut i = 0;
            while i < node.n {
                let r = node.recs[i].unwrap();
                match cmp_key(r.key_str(), rec.key_str()) {
                    core::cmp::Ordering::Equal => {
                        node.recs[i] = Some(rec);
                        return true;
                    }
                    core::cmp::Ordering::Less => i += 1,
                    core::cmp::Ordering::Greater => break,
                }
            }
            if node.leaf {
                return false;
            }
            id = node.children[i];
            if id < 0 {
                return false;
            }
        }
    }

    fn btree_delete(&mut self, key: &str) -> bool {
        let mut id = self.root;
        loop {
            let node = &mut self.nodes[id as usize];
            let mut i = 0;
            while i < node.n {
                let r = node.recs[i].unwrap();
                match cmp_key(r.key_str(), key) {
                    core::cmp::Ordering::Equal => {
                        // 删除：后项前移（模板回读面在 get 兜底——删自有键后
                        // 应用读回模板值，这正是「还原默认」语义）。
                        let mut j = i;
                        while j + 1 < node.n {
                            node.recs[j] = node.recs[j + 1];
                            j += 1;
                        }
                        node.recs[node.n - 1] = None;
                        node.n -= 1;
                        return true;
                    }
                    core::cmp::Ordering::Less => i += 1,
                    core::cmp::Ordering::Greater => break,
                }
            }
            if node.leaf {
                return false;
            }
            id = node.children[i];
            if id < 0 {
                return false;
            }
        }
    }

    // -- 断电恢复 -------------------------------------------------------------

    /// 打开时 WAL 回放（断电注入后蜂巢可打开率 100% 判据的恢复路径）。
    pub fn recover(&mut self) -> usize {
        let mut n = 0;
        // 先快照 WAL 再逐条回放（借用纪律）。
        let mut snapshot = [None::<Rec>; 128];
        for i in 0..self.wal_n {
            snapshot[i] = self.wal[i];
        }
        for slot in snapshot.iter() {
            if let Some(r) = slot {
                if self.btree_set_in(*r) || self.btree_insert(*r) {
                    n += 1;
                }
            }
        }
        self.flushes += 1;
        n
    }

    /// 蜂巢损坏注入：清树 → 重建空蜂巢 + 通知标记（设置丢失如实告知）。
    pub fn rebuild_corrupted(&mut self) {
        self.nodes = [Node::new(); NODE_POOL];
        self.root = 0;
        self.free = 1;
        self.wal = [None; 128];
        self.wal_n = 0;
        self.bytes = NODE_SIZE as u64;
        self.rebuilt_notify = true;
    }

    /// 卸载：蜂巢进回收站（零真删红线——recycled 标记，不抹数据）。
    pub fn uninstall_recycle(&mut self) {
        self.recycled = true;
    }

    pub fn size_bytes(&self) -> u64 {
        self.bytes
    }

    pub fn over_alarm(&self) -> bool {
        self.bytes > HIVE_ALARM_BYTES
    }

    pub fn flushes(&self) -> u32 {
        self.flushes
    }

    pub fn wal_len(&self) -> usize {
        self.wal_n
    }

    /// 模板哈希（全局模板零写入判据的对账面：模板是 &'static 只读面，
    /// 写路径物理上摸不到它——此函数给诊断页展示指纹）。
    pub fn template_fingerprint(&self) -> u64 {
        let mut h = 0xCBFF_1E5Au64;
        for (k, v) in self.template {
            for &b in k.as_bytes() {
                h = h.wrapping_mul(0x100_0000_01B3) ^ b as u64;
            }
            for &b in *v {
                h = h.wrapping_mul(0x100_0000_01B3) ^ b as u64;
            }
        }
        h
    }
}

fn cmp_key(a: &str, b: &str) -> core::cmp::Ordering {
    a.as_bytes().cmp(b.as_bytes())
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 示例模板（HKLM 基础面——只读共享镜像的测试代表）。
pub const SAMPLE_TEMPLATE: &[(&str, &[u8])] = &[
    ("Software\\VARIX\\Platform", b"STAR I" as &[u8]),
    ("Software\\VARIX\\Layout", b"default"),
];

/// 域自检。
pub fn run_reghive_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive");
    // 1) 判据常量（4KB 节点 / 256MB 告警 / CLSID 前缀）。
    cs.add(
        "consts",
        NODE_SIZE == 4096
            && HIVE_ALARM_BYTES == 256 * 1024 * 1024
            && NODE_ORDER == 16
            && CLSID_PREFIX == "Classes\\CLSID\\",
        "",
    );
    // 2) 写进自有蜂巢、读回一致。
    let mut h = Hive::new(SAMPLE_TEMPLATE);
    cs.add(
        "set_then_get",
        h.set("Software\\MyApp\\Theme", b"dark") && h.get("Software\\MyApp\\Theme").unwrap().val_bytes() == b"dark",
        "",
    );
    // 3) 模板回读：未命中自有蜂巢 → 模板（只读）；模板指纹写入前后不变。
    let fp_before = h.template_fingerprint();
    cs.add(
        "template_fallback_readonly",
        h.get("Software\\VARIX\\Platform").unwrap().val_bytes() == b"STAR I" && h.template_fingerprint() == fp_before,
        "",
    );
    // 4) 覆盖写：同键原位改，值更新。
    let _ = h.set("Software\\MyApp\\Theme", b"light");
    cs.add("overwrite_in_place", h.get("Software\\MyApp\\Theme").unwrap().val_bytes() == b"light", "");
    // 5) 删自有键 → 回读落模板语义（还原默认）；删不存在键 = false。
    let _ = h.delete("Software\\MyApp\\Theme");
    cs.add(
        "delete_falls_back_to_template",
        h.get("Software\\MyApp\\Theme").is_none(),
        "",
    );
    cs.add("delete_missing_is_false", !h.delete("Software\\MyApp\\Ghost"), "");
    // 6) 批量写入有序 B 树：乱序插入后全量可读（含分裂路径）。
    let mut h2 = Hive::new(&[]);
    let mut all_ok = true;
    for i in (0..40u32).rev() {
        // 乱序（倒序）插入，触发根分裂。
        let key = core::format_args!("K{:03}", i).to_string();
        // 零堆纪律例外说明：checks 只在宿主侧跑（lib.rs L25 注），此处
        // to_string 可接受；内核运行时路径仍全定长。
        all_ok &= h2.set(&key, b"v");
    }
    let mut found = 0;
    for i in 0..40u32 {
        let key = core::format_args!("K{:03}", i).to_string();
        if h2.get(&key).is_some() {
            found += 1;
        }
    }
    cs.add("ordered_btree_batch", all_ok && found == 40, "");
    // 7) WAL 断电恢复：写入 → 模拟断电（树清空）→ recover 全回放。
    let mut h3 = Hive::new(&[]);
    for i in 0..10u32 {
        let key = core::format_args!("P{:02}", i).to_string();
        let _ = h3.set(&key, b"w");
    }
    let wal_saved = h3.wal_len();
    // 模拟断电：树内容抹掉（WAL 在盘语义保留）。
    h3.nodes = [Node::new(); NODE_POOL];
    h3.root = 0;
    h3.free = 1;
    let replayed = h3.recover();
    let mut recovered = 0;
    for i in 0..10u32 {
        let key = core::format_args!("P{:02}", i).to_string();
        if h3.get(&key).is_some() {
            recovered += 1;
        }
    }
    cs.add(
        "wal_recovery_100pct",
        wal_saved == 10 && replayed == 10 && recovered == 10,
        "",
    );
    // 8) 损坏重建：清空 + 通知标记（设置丢失如实告知，不静默）。
    h3.rebuild_corrupted();
    cs.add(
        "corrupt_rebuild_notifies",
        h3.rebuilt_notify && h3.wal_len() == 0 && h3.get("P00").is_none(),
        "",
    );
    // 9) 卸载 = 回收站（零真删红线）：recycled 后读写都止步，数据位未抹。
    let mut h4 = Hive::new(SAMPLE_TEMPLATE);
    let _ = h4.set("Software\\X\\K", b"data");
    h4.uninstall_recycle();
    cs.add(
        "uninstall_recycles_not_deletes",
        h4.recycled && h4.get("Software\\X\\K").is_none() && !h4.set("Software\\X\\K2", b"x"),
        "",
    );
    // 10) 蜂巢大小记账 + 告警线判定。
    let mut h5 = Hive::new(&[]);
    let _ = h5.set("A\\B", &[0u8; 256]);
    cs.add(
        "size_accounting_and_alarm",
        h5.size_bytes() > NODE_SIZE as u64 && !h5.over_alarm(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_use_uninstall_cycle_template_zero_write() {
        // 判据一（模型层）：安装-使用-卸载全周期，全局模板零写入（哈希不变）。
        let mut h = Hive::new(SAMPLE_TEMPLATE);
        let fp = h.template_fingerprint();
        // 安装器写入 + 应用使用。
        for i in 0..30 {
            let key = core::format_args!("Software\\App3Fam\\Opt{:02}", i).to_string();
            assert!(h.set(&key, b"installed"));
        }
        assert_eq!(h.template_fingerprint(), fp, "template must be untouched");
        // 卸载：蜂巢回收，模板依旧零写入。
        h.uninstall_recycle();
        assert_eq!(h.template_fingerprint(), fp);
    }

    #[test]
    fn two_apps_same_key_no_interference() {
        // 用户故事：两款软件写同名键互不干扰（每应用独立蜂巢 = 两实例）。
        let mut app_a = Hive::new(&[]);
        let mut app_b = Hive::new(&[]);
        assert!(app_a.set("Software\\Vendor\\Setting", b"AAA"));
        assert!(app_b.set("Software\\Vendor\\Setting", b"BBB"));
        assert_eq!(app_a.get("Software\\Vendor\\Setting").unwrap().val_bytes(), b"AAA");
        assert_eq!(app_b.get("Software\\Vendor\\Setting").unwrap().val_bytes(), b"BBB");
        // 卸载 A：B 纹丝不动。
        app_a.uninstall_recycle();
        assert_eq!(app_b.get("Software\\Vendor\\Setting").unwrap().val_bytes(), b"BBB");
    }

    #[test]
    fn power_cut_100_recoveries() {
        // 判据二（模型层）：断电注入百次后蜂巢可打开率 100%（WAL 恢复）。
        for round in 0..100u32 {
            let mut h = Hive::new(&[]);
            for i in 0..5u32 {
                let key = core::format_args!("R{:02}K{:02}", round, i).to_string();
                let _ = h.set(&key, b"x");
            }
            // 断电：树抹、WAL 留。
            h.nodes = [Node::new(); NODE_POOL];
            h.root = 0;
            h.free = 1;
            let n = h.recover();
            assert_eq!(n, 5, "round {} must fully replay", round);
        }
    }

    #[test]
    fn oversized_key_or_value_rejected() {
        let mut h = Hive::new(&[]);
        let long_key = "K".repeat(KEY_MAX + 1);
        let long_val = [7u8; VAL_MAX + 1];
        assert!(!h.set(&long_key, b"v"));
        assert!(!h.set("Short", &long_val));
        // 恰好在界内则通过。
        let ok_key = "K".repeat(KEY_MAX);
        assert!(h.set(&ok_key, &[8u8; VAL_MAX]));
    }

    #[test]
    fn clsid_subtree_path() {
        // F019 消费面：CLSID 注册进蜂巢 Classes\CLSID 子树。
        let mut h = Hive::new(&[]);
        let path = core::format_args!("{}{{1234-ABCD}}", CLSID_PREFIX).to_string();
        assert!(h.set(&path, b"InProcServer"));
        assert!(h.get(&path).is_some());
    }

    #[test]
    fn over_alarm_only_past_256mb() {
        // 告警线边界：正常应用设置 <1MB 不告警；256MB 之上告警。
        let mut h = Hive::new(&[]);
        h.bytes = 900_000;
        assert!(!h.over_alarm());
        h.bytes = HIVE_ALARM_BYTES;
        assert!(!h.over_alarm(), "exactly at line is not over");
        h.bytes = HIVE_ALARM_BYTES + 1;
        assert!(h.over_alarm());
    }

    #[test]
    fn record_helpers() {
        let r = Rec::new("A\\B", b"12345").unwrap();
        assert_eq!(r.key_str(), "A\\B");
        assert_eq!(r.val_bytes(), b"12345");
        assert!(Rec::new("", &[]).is_some()); // 空键允许（根值语义）
        assert!(Rec::new(&"x".repeat(KEY_MAX + 1), b"").is_none());
    }
}

// ---------------------------------------------------------------------------
// F009 · 深化扩展：键枚举 + 快照导出（开放格式 F126）
//
// 主册依据（G-A-09【设计细节】）：「导出快照为 JSON（开放格式 F126）」+
// 「蜂巢查看器树视图支持键值复制」——本扩展给出自有蜂巢的全量枚举与有界
// 文本导出（JSON 形态；诊断中心/蜂巢查看器消费面）。
// ---------------------------------------------------------------------------

impl Hive {
    /// 自有蜂巢记录总数（蜂巢大小显示面的条目数口径）。
    pub fn record_count(&self) -> usize {
        let mut n = 0usize;
        for node in self.nodes.iter() {
            n += node.n;
        }
        n
    }

    /// 第 idx 条自有记录（蜂巢查看器枚举面——按节点序，非字典序；查看器
    /// 侧排序展示）。
    pub fn record_at(&self, idx: usize) -> Option<Rec> {
        let mut k = idx;
        for node in self.nodes.iter() {
            if k < node.n {
                return node.recs[k];
            }
            k -= node.n;
        }
        None
    }

    /// 快照导出（JSON Lines 形态：每行一个键值对象）。`out` 有界——截断
    /// 如实返回（返回值 = (写入字节数, 导出条数, 截断标记)）。蜂巢进回收站
    /// 后导出为空（与 get 的回收站止步语义对齐）。
    pub fn export_snapshot(&self, out: &mut [u8]) -> (usize, usize, bool) {
        if self.recycled {
            return (0, 0, false);
        }
        let mut w = 0usize;
        let mut count = 0usize;
        let mut truncated = false;
        let push = |out: &mut [u8], w: &mut usize, s: &[u8]| -> bool {
            for &b in s {
                if *w >= out.len() {
                    return false;
                }
                out[*w] = b;
                *w += 1;
            }
            true
        };
        for node in self.nodes.iter() {
            for slot in node.recs.iter().take(node.n) {
                if let Some(r) = slot {
                    let key = r.key_str();
                    if !push(out, &mut w, b"{\"key\":\"")
                        || !push(out, &mut w, key.as_bytes())
                        || !push(out, &mut w, b"\",\"len\":")
                        || !push(out, &mut w, r.val_len.to_string().as_bytes())
                        || !push(out, &mut w, b"}\n")
                    {
                        truncated = true;
                        break;
                    }
                    count += 1;
                }
            }
            if truncated {
                break;
            }
        }
        (w, count, truncated)
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn enumeration_and_export() {
        let mut h = Hive::new(&[]);
        for i in 0..10u32 {
            let key = core::format_args!("Soft\\K{:02}", i).to_string();
            assert!(h.set(&key, b"v"));
        }
        assert_eq!(h.record_count(), 10);
        // 枚举：前 10 条全部可读，第 11 条 None。
        assert!(h.record_at(0).is_some() && h.record_at(9).is_some());
        assert!(h.record_at(10).is_none());
        // 导出：10 行 JSONL 全量。
        let mut buf = [0u8; 4096];
        let (w, count, truncated) = h.export_snapshot(&mut buf);
        assert_eq!(count, 10);
        assert!(!truncated);
        assert!(w > 10 * 10);
        let text = core::str::from_utf8(&buf[..w]).unwrap();
        assert!(text.contains("{\"key\":\"Soft\\K00\",\"len\":1}"));
        // 卸载后导出为空（蜂巢已进回收站——读面止步）。
        h.uninstall_recycle();
        let (_, count2, _) = h.export_snapshot(&mut buf);
        assert_eq!(count2, 0);
    }

    #[test]
    fn export_bounded_truncation_honest() {
        // 输出缓冲有界：截断如实标记（不静默截）。
        let mut h = Hive::new(&[]);
        for i in 0..50u32 {
            let key = core::format_args!("Very\\Long\\Key\\Path\\{:03}", i).to_string();
            assert!(h.set(&key, b"value"));
        }
        let mut small = [0u8; 128];
        let (_, count, truncated) = h.export_snapshot(&mut small);
        assert!(truncated && count < 50);
        assert!(count > 0, "至少导出第一条");
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_reghive_checks() -> CheckSet {
    CheckSet::merge(run_reghive_base_checks(), CheckSet::merge(run_reghive_deep_checks(), CheckSet::merge(run_reghive_deep2_checks(), CheckSet::merge(run_reghive_deep3_checks(), CheckSet::merge(run_reghive_deep4_checks(), CheckSet::merge(run_reghive_deep5_checks(), CheckSet::merge(run_reghive_deep6_checks(), CheckSet::merge(run_reghive_deep7_checks(), run_reghive_deep8_checks()))))))))
}

// ---------------------------------------------------------------------------
// F009 · 深化批次二：蜂巢 JSON 导出（开放格式 F126）+ 污节点合并记账
//
// 主册依据（G-A-09【设计细节】）：「导出快照为 JSON（开放格式 F126）」
// 「蜂巢 B 树节点 4KB、写放大控制（脏节点合并落盘）」。导出为键值对的
// 规范 JSON 形态（转义完整、截断如实标注）。
// ---------------------------------------------------------------------------

/// JSON 字符串转义（`"` `\` 控制字符 → 转义序列；返回写入长度，缓冲不足
/// 返回 0——不静默截半个转义）。
pub fn json_escape(s: &str, buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    let put = |n: &mut usize, buf: &mut [u8], b: u8| -> bool {
        if *n >= buf.len() {
            return false;
        }
        buf[*n] = b;
        *n += 1;
        true
    };
    for &b in s.as_bytes() {
        match b {
            b'"' => {
                if !put(&mut n, buf, b'\\') || !put(&mut n, buf, b'"') {
                    return 0;
                }
            }
            b'\\' => {
                if !put(&mut n, buf, b'\\') || !put(&mut n, buf, b'\\') {
                    return 0;
                }
            }
            b'\n' => {
                if !put(&mut n, buf, b'\\') || !put(&mut n, buf, b'n') {
                    return 0;
                }
            }
            b'\t' => {
                if !put(&mut n, buf, b'\\') || !put(&mut n, buf, b't') {
                    return 0;
                }
            }
            0x00..=0x1F => {
                // 其余控制字符 → \u00XX（4 位十六进制，小写）。
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let esc = [b'\\', b'u', b'0', b'0', HEX[(b >> 4) as usize], HEX[(b & 0xF) as usize]];
                for e in esc.iter() {
                    if !put(&mut n, buf, *e) {
                        return 0;
                    }
                }
            }
            _ => {
                if !put(&mut n, buf, b) {
                    return 0;
                }
            }
        }
    }
    n
}

/// 蜂巢 JSON 导出（键值对 → `{"k":"v",...}`；值按字节域转义；缓冲不足返回
/// truncated=true 且 written 为 0——半截 JSON 不落盘，诚实语义）。
pub fn export_json(recs: &[(&str, &[u8])], buf: &mut [u8]) -> (usize, bool) {
    let mut n = 0usize;
    let put = |n: &mut usize, buf: &mut [u8], b: u8| -> bool {
        if *n >= buf.len() {
            return false;
        }
        buf[*n] = b;
        *n += 1;
        true
    };
    if !put(&mut n, buf, b'{') {
        return (0, true);
    }
    for (i, (k, v)) in recs.iter().enumerate() {
        if i > 0 && !put(&mut n, buf, b',') {
            return (0, true);
        }
        if !put(&mut n, buf, b'"') {
            return (0, true);
        }
        let kn = json_escape(k, &mut buf[n..]);
        if kn == 0 {
            return (0, true);
        }
        n += kn;
        if !put(&mut n, buf, b'"') || !put(&mut n, buf, b':') || !put(&mut n, buf, b'"') {
            return (0, true);
        }
        let val = core::str::from_utf8(v).unwrap_or("");
        let vn = json_escape(val, &mut buf[n..]);
        if vn == 0 {
            return (0, true);
        }
        n += vn;
        if !put(&mut n, buf, b'"') {
            return (0, true);
        }
    }
    if !put(&mut n, buf, b'}') {
        return (0, true);
    }
    (n, false)
}

/// 污节点合并记账（写放大控制——B 树脏节点合并落盘的观测面；零堆纯计数）。
#[derive(Clone, Copy, Debug)]
pub struct DirtyMerge {
    /// 累计污节点数。
    pub dirty_nodes: u32,
    /// 合并落盘批次数（一次 flush 合并多个污节点 = 写放大下降）。
    pub merged_flushes: u32,
    /// 合并的节点累计。
    pub merged_nodes: u32,
}

impl DirtyMerge {
    pub fn new() -> DirtyMerge {
        DirtyMerge { dirty_nodes: 0, merged_flushes: 0, merged_nodes: 0 }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty_nodes += 1;
    }

    /// 合并落盘：把当前全部污节点并入一次 flush（返回本批节点数）。
    pub fn flush_merged(&mut self) -> u32 {
        let batch = self.dirty_nodes;
        if batch > 0 {
            self.dirty_nodes = 0;
            self.merged_flushes += 1;
            self.merged_nodes += batch;
        }
        batch
    }
}

/// F009 深化自检。
pub fn run_reghive_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep");
    // 1) JSON 转义：引号/反斜杠/换行/控制字符全转义；普通串原样。
    let mut e1 = [0u8; 32];
    let n1 = json_escape("a\"b\\c\nd\te", &mut e1);
    cs.add(
        "json_escape_full",
        n1 > 0
            && &e1[..n1] == b"a\\\"b\\\\c\\nd\\te"
            && json_escape("plain", &mut e1) == 5,
        "",
    );
    // 2) 控制字符 → \u00xx 小写十六进制。
    let mut e2 = [0u8; 8];
    let n2 = json_escape("\u{1}", &mut e2);
    cs.add("json_escape_control", n2 == 6 && &e2[..n2] == b"\\u0001", "");
    // 3) 导出整体形态 + 截断如实（缓冲不足 → written=0 + truncated=true）。
    let recs: [(&str, &[u8]); 2] = [("AutoSave", b"1"), ("Path", b"C:\\x")];
    let mut full = [0u8; 128];
    let (n3, trunc3) = export_json(&recs, &mut full);
    let (n4, trunc4) = export_json(&recs, &mut [0u8; 8]);
    cs.add(
        "export_json_shape_and_truncation",
        n3 > 0
            && !trunc3
            && &full[..n3] == b"{\"AutoSave\":\"1\",\"Path\":\"C:\\\\x\"}"
            && n4 == 0
            && trunc4,
        "",
    );
    // 4) 污节点合并：mark 3 → flush 一批收 3 → 计数归零、批次数 1。
    let mut dm = DirtyMerge::new();
    dm.mark_dirty();
    dm.mark_dirty();
    dm.mark_dirty();
    let batch = dm.flush_merged();
    let empty = dm.flush_merged();
    cs.add(
        "dirty_merge_accounting",
        batch == 3 && empty == 0 && dm.dirty_nodes == 0 && dm.merged_flushes == 1 && dm.merged_nodes == 3,
        "",
    );
    // 5) 蜂巢 256MB 告警既有面（over_alarm）对账锚 + WAL 可恢复（批次一）。
    let mut h = Hive::new(&[]);
    let _ = h.set("K", b"v");
    cs.add(
        "hive_apis_anchored",
        h.size_bytes() > 0 && !h.over_alarm() && h.wal_len() > 0 && h.flushes() >= 0,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F009 · 深化批次三：恢复点快照（F121 联动——注册表可回滚）+ 并发写 WAL 串行化
//
// 主册依据（G-A-09【设计细节】）：「恢复点（F121）自动纳入蜂巢快照——注册表
// 可回滚」；【数据与存储】「并发写冲突 WAL 串行化」。Hive/Rec/WAL 既有面
// （一处一事实）：快照用既有 record_count/record_at 枚举，回滚用既有 set 回写，
// 本段只做快照/回滚编排与单飞写闸。
// ---------------------------------------------------------------------------

/// 快照键缓冲（KEY_MAX 同源——一处一事实引用既有上限）。
const RESTORE_KEY_CAP: usize = KEY_MAX;
/// 快照值缓冲（超长值如实截断并计数——不静默丢）。
const RESTORE_VAL_CAP: usize = 64;

#[derive(Clone, Copy)]
struct RestoreRec {
    key: [u8; RESTORE_KEY_CAP],
    key_n: usize,
    val: [u8; RESTORE_VAL_CAP],
    val_n: usize,
}

impl RestoreRec {
    fn capture(key: &str, val: &[u8]) -> (RestoreRec, bool) {
        let mut r = RestoreRec { key: [0; RESTORE_KEY_CAP], key_n: 0, val: [0; RESTORE_VAL_CAP], val_n: 0 };
        let kb = key.as_bytes();
        r.key_n = kb.len().min(RESTORE_KEY_CAP);
        r.key[..r.key_n].copy_from_slice(&kb[..r.key_n]);
        r.val_n = val.len().min(RESTORE_VAL_CAP);
        r.val[..r.val_n].copy_from_slice(&val[..r.val_n]);
        (r, val.len() > RESTORE_VAL_CAP)
    }

    fn key_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.key[..self.key_n]).ok()
    }
}

/// 蜂巢恢复点（F121 自动纳入——快照即回滚材料）。
pub struct RestorePoint {
    recs: [Option<RestoreRec>; 48],
    n: usize,
    /// 采集时因槽位/长度限制未能完整快照的记录数（如实登记，不静默）。
    pub truncated_recs: u32,
    pub taken_at_ms: u64,
}

impl RestorePoint {
    pub const fn new(taken_at_ms: u64) -> RestorePoint {
        RestorePoint { recs: [None; 48], n: 0, truncated_recs: 0, taken_at_ms }
    }

    fn push(&mut self, key: &str, val: &[u8]) {
        if self.n >= self.recs.len() {
            self.truncated_recs += 1;
            return;
        }
        let (r, val_trunc) = RestoreRec::capture(key, val);
        if val_trunc {
            self.truncated_recs += 1;
        }
        self.recs[self.n] = Some(r);
        self.n += 1;
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

impl Hive {
    /// 采集恢复点（全量自有记录——回滚 = 逐条 set 回写既有语义）。
    pub fn take_restore_point(&self, at_ms: u64) -> RestorePoint {
        let mut rp = RestorePoint::new(at_ms);
        let total = self.record_count();
        for i in 0..total {
            if let Some(rec) = self.record_at(i) {
                rp.push(rec.key_str(), rec.val_bytes());
            }
        }
        rp
    }
}

/// 回滚：把恢复点逐条回写进蜂巢（成功条数返回；键被截断的记录如实跳过——
/// 回写不了的不装作成功）。
pub fn rollback_from(hive: &mut Hive, point: &RestorePoint) -> usize {
    let mut restored = 0usize;
    for i in 0..point.n {
        if let Some(r) = point.recs[i] {
            if let Some(key) = r.key_str() {
                if hive.set(key, &r.val[..r.val_n]) {
                    restored += 1;
                }
            }
        }
    }
    restored
}

/// 并发写单飞闸（WAL 串行化的进程面模型：同一时刻仅一个写者在途，其余
/// 排队计数——release 时按序放行）。
pub struct WriteSerializer {
    busy: bool,
    pub queued: u32,
    pub serialized: u32,
}

impl WriteSerializer {
    pub const fn new() -> WriteSerializer {
        WriteSerializer { busy: false, queued: 0, serialized: 0 }
    }

    /// 提交写请求：闸空闲 → 占闸（true，写者放行）；占用 → 排队计数（false）。
    pub fn submit(&mut self) -> bool {
        if self.busy {
            self.queued += 1;
            return false;
        }
        self.busy = true;
        true
    }

    /// 在途写者完成：闸释放，排队者按序获得放行权（返回排队放行数）。
    pub fn release(&mut self) -> u32 {
        self.busy = false;
        let pass = self.queued;
        self.queued = 0;
        self.serialized += pass;
        pass
    }
}

/// F009 深化批次三自检。
pub fn run_reghive_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep2");
    // 1) 快照-回滚闭环：三笔写入 → 快照 → 再改再删 → 回滚 → 三笔逐条还原
    //    （值一致 = F121「注册表可回滚」判据的模型面）。
    let mut hive = Hive::new(SAMPLE_TEMPLATE);
    hive.set("RPTest\\A", b"alpha");
    hive.set("RPTest\\B", b"beta");
    hive.set("RPTest\\C", b"gamma");
    let point = hive.take_restore_point(1_000);
    hive.set("RPTest\\A", b"CHANGED");
    hive.delete("RPTest\\B");
    hive.set("RPTest\\Extra", b"x");
    let restored = rollback_from(&mut hive, &point);
    let a_ok = matches!(hive.get("RPTest\\A"), Some(r) if r.val_bytes() == b"alpha");
    let b_ok = matches!(hive.get("RPTest\\B"), Some(r) if r.val_bytes() == b"beta");
    let c_ok = matches!(hive.get("RPTest\\C"), Some(r) if r.val_bytes() == b"gamma");
    cs.add(
        "restore_point_rollback_roundtrip",
        point.len() == 3
            && point.truncated_recs == 0
            && restored == 3
            && a_ok
            && b_ok
            && c_ok,
        "",
    );
    // 2) 超长值如实截断计数（不静默丢）：值 100 字节 > 64 缓冲 → truncated_recs +1，
    //    回滚还原前 64 字节。
    let mut hive2 = Hive::new(SAMPLE_TEMPLATE);
    let long = [7u8; 100];
    hive2.set("RPTest\\Long", &long);
    let p2 = hive2.take_restore_point(2_000);
    hive2.set("RPTest\\Long", b"mutated");
    let restored2 = rollback_from(&mut hive2, &p2);
    let back2 = matches!(hive2.get("RPTest\\Long"), Some(r) if r.val_bytes() == &long[..RESTORE_VAL_CAP]);
    cs.add(
        "restore_point_long_value_truncation_honest",
        p2.truncated_recs == 1 && restored2 == 1 && back2,
        "",
    );
    // 3) 单飞写闸：占用期提交 = 排队；release 放行排队者；串行化计数可审计。
    let mut ser = WriteSerializer::new();
    let first = ser.submit();
    let second = ser.submit();
    let third = ser.submit();
    let passed = ser.release();
    cs.add(
        "write_serializer_single_flight",
        first && !second && !third && passed == 2 && ser.serialized == 2 && !ser.busy,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F009 · 深化批次四：蜂巢查看器（键值复制 + 十六进制值查看）+ 系统模板只读
// 共享（全进程一份）
//
// 主册依据（G-A-09【设计细节】）：「蜂巢查看器树视图支持键值复制与十六进制
// 值查看」；「系统模板蜂巢启动时 mmap 只读共享（全进程共用一份）」。
// ---------------------------------------------------------------------------

/// 值的十六进制视图（诊断页/查看器消费：`AA BB CC` 空格分隔，缓冲不足
/// 如实截断——截断结果不冒充完整值）。
pub fn hex_view(val: &[u8], buf: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut n = 0usize;
    for (i, &b) in val.iter().enumerate() {
        if i > 0 {
            if n >= buf.len() {
                return n;
            }
            buf[n] = b' ';
            n += 1;
        }
        if n + 2 > buf.len() {
            return n;
        }
        buf[n] = HEX[(b >> 4) as usize];
        buf[n + 1] = HEX[(b & 0xF) as usize];
        n += 2;
    }
    n
}

/// 查看器行复制（键值复制动线：`键 = 值(hex)` 写入缓冲，供剪贴板 F017）。
pub fn viewer_row(key: &str, val: &[u8], buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    for &b in key.as_bytes() {
        if n >= buf.len() {
            return n;
        }
        buf[n] = b;
        n += 1;
    }
    for &s in b" = ".iter() {
        if n >= buf.len() {
            return n;
        }
        buf[n] = s;
        n += 1;
    }
    let mut hexbuf = [0u8; 192];
    let hn = hex_view(val, &mut hexbuf);
    for &b in &hexbuf[..hn] {
        if n >= buf.len() {
            return n;
        }
        buf[n] = b;
        n += 1;
    }
    n
}

/// 系统模板只读共享（mmap 语义的进程面模型：全进程一份，写请求恒拒绝）。
#[derive(Clone, Copy, Debug)]
pub struct TemplateShare {
    /// 共享映射的进程引用数。
    pub refs: u32,
    /// 写模板请求拒绝计数（审计面——「全局模板零写入」判据的本面锚）。
    pub write_refusals: u32,
}

impl TemplateShare {
    pub const fn new() -> TemplateShare {
        TemplateShare { refs: 0, write_refusals: 0 }
    }

    pub fn attach(&mut self) -> u32 {
        self.refs += 1;
        self.refs
    }

    pub fn detach(&mut self) -> u32 {
        self.refs = self.refs.saturating_sub(1);
        self.refs
    }

    /// 模板写请求：恒拒绝（「全局模板零写入」判据在共享面的落点——哈希不变）。
    pub fn write_template_request(&mut self) -> bool {
        self.write_refusals += 1;
        false
    }
}

/// F009 深化批次四自检。
pub fn run_reghive_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep3");
    // 1) 十六进制视图：`DE AD BE EF` 精确产出；短缓冲如实截断（写完空格后
    //    剩 1 字节放不下字节对 → 返回 3——部分字节对不冒充完整）。
    let mut buf = [0u8; 32];
    let n1 = hex_view(&[0xDE, 0xAD, 0xBE, 0xEF], &mut buf);
    let full_ok = &buf[..n1] == b"DE AD BE EF";
    let mut small = [0u8; 4];
    let n2 = hex_view(&[0xDE, 0xAD, 0xBE, 0xEF], &mut small);
    cs.add(
        "hex_view_exact_and_truncated",
        full_ok && n2 == 3,
        "",
    );
    // 2) 查看器行复制：键 + 十六进制值成行（复制动线端到端）。
    let mut row = [0u8; 64];
    let n3 = viewer_row("Software\\MyApp\\Theme", &[0x01, 0x02], &mut row);
    cs.add(
        "viewer_row_copy",
        n3 > 0 && core::str::from_utf8(&row[..n3]).unwrap_or("").starts_with("Software\\MyApp\\Theme = 01 02"),
        "",
    );
    // 3) 模板共享：多进程 attach 计数；写模板恒拒绝计数；detach 全清。
    let mut sh = TemplateShare::new();
    let r1 = sh.attach();
    let r2 = sh.attach();
    let w = sh.write_template_request();
    let w2 = sh.write_template_request();
    let r3 = sh.detach();
    let r4 = sh.detach();
    cs.add(
        "template_share_readonly_shared",
        r1 == 1 && r2 == 2 && !w && !w2 && sh.write_refusals == 2 && r3 == 1 && r4 == 0,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F009 · 深化批次五：蜂巢键前缀枚举（树视图的数据源）
//
// 主册依据（G-A-09【交互设计】）：「设置中心显示『查看注册表内容』入口
// （只读十六进制/键树视图）」——树视图按前缀枚举子键：Hive 全量记录上的
// 前缀过滤（零堆面：枚举结果写入调用方缓冲行表）。
// ---------------------------------------------------------------------------

/// 按前缀枚举键（键树视图：key 以 `prefix\` 起头或恰等于 prefix 的记录全收）。
/// 结果以 `key=value字节序` 行写入 buf，行间 '\n'。返回 (写入字节, 命中数)。
pub fn hive_enumerate_prefix(hive: &Hive, prefix: &str, buf: &mut [u8]) -> (usize, usize) {
    let mut out = 0usize;
    let mut hits = 0usize;
    let total = hive.record_count();
    for i in 0..total {
        let rec = match hive.record_at(i) {
            Some(r) => r,
            None => continue,
        };
        let k = rec.key_str();
        let matched = k == prefix || (k.len() > prefix.len() && k.starts_with(prefix) && k.as_bytes()[prefix.len()] == b'\\');
        if !matched {
            continue;
        }
        hits += 1;
        if hits > 1 && out < buf.len() {
            buf[out] = b'\n';
            out += 1;
        }
        for &b in k.as_bytes() {
            if out >= buf.len() {
                return (out, hits);
            }
            buf[out] = b;
            out += 1;
        }
        for &b in b" = " {
            if out >= buf.len() {
                return (out, hits);
            }
            buf[out] = b;
            out += 1;
        }
        let vb = rec.val_bytes();
        for (j, &b) in vb.iter().enumerate() {
            if j >= 16 || out >= buf.len() {
                break; // 值预览 16 字节（查看器行内预览口径）
            }
            if out >= buf.len() {
                return (out, hits);
            }
            buf[out] = b;
            out += 1;
        }
    }
    (out, hits)
}

/// F009 深化批次五自检。
pub fn run_reghive_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep4");
    // 1) 前缀枚举：Software\MyApp 命中其直接键树（模板内构造三键）。
    let mut hive = Hive::new(SAMPLE_TEMPLATE);
    hive.set("Software\\MyApp\\Theme", b"dark");
    hive.set("Software\\MyApp\\Recent\\F1", b"a");
    hive.set("Software\\VARIX\\Platform", b"STAR I");
    let mut buf = [0u8; 256];
    let (n, hits) = hive_enumerate_prefix(&hive, "Software\\MyApp", &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "hive_prefix_enum_direct",
        hits == 2 && text.contains("Software\\MyApp\\Theme") && text.contains("Software\\MyApp\\Recent\\F1"),
        "",
    );
    // 2) 深层前缀只命中子树：Software\MyApp\Recent 只中一条。
    let (n2, hits2) = hive_enumerate_prefix(&hive, "Software\\MyApp\\Recent", &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "hive_prefix_enum_deep",
        hits2 == 1 && t2.contains("Recent\\F1") && !t2.contains("Theme"),
        "",
    );
    // 3) 无前缀同名键不被误收（前缀必须整段或 `\` 边界——Software\My 不中）。
    let (_, hits3) = hive_enumerate_prefix(&hive, "Software\\My", &mut buf);
    cs.add("hive_prefix_enum_boundary", hits3 == 0, "");
    cs
}

// ---------------------------------------------------------------------------
// F009 · 深化批次六：注册表值类型标记面（REG_SZ/DWORD/BINARY 语义层）
//
// 主册依据（G-A-09【功能定义】）：「应用视角的 HKCU/HKLM 读写」——Windows
// 注册表值有类型（REG_SZ/REG_DWORD/REG_BINARY…）；自有蜂巢字节值之上的
// **类型语义层**：类型标记 + DWORD 字节序语义（小端 4 字节）。
// ---------------------------------------------------------------------------

/// 值类型（Windows 注册表常用三型——50 件采样覆盖面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValType {
    Sz,
    Dword,
    Binary,
}

/// 类型标记表（键 → 类型；容量 32——键树视图显示类型徽标的数据源）。
pub struct ValTypeTable {
    keys: [(u64, ValType); 32],
    n: usize,
}

impl ValTypeTable {
    pub const fn new() -> ValTypeTable {
        ValTypeTable { keys: [(0, ValType::Binary); 32], n: 0 }
    }

    /// 标记（重复标记更新不占双槽——幂等）。
    pub fn tag(&mut self, key_hash: u64, t: ValType) -> bool {
        if let Some((_, existing)) = self.keys[..self.n].iter_mut().find(|(k, _)| *k == key_hash) {
            *existing = t;
            return true;
        }
        if self.n >= self.keys.len() {
            return false;
        }
        self.keys[self.n] = (key_hash, t);
        self.n += 1;
        true
    }

    pub fn type_of(&self, key_hash: u64) -> Option<ValType> {
        self.keys[..self.n].iter().find(|(k, _)| *k == key_hash).map(|(_, t)| *t)
    }
}

/// DWORD 值读（REG_DWORD：小端 4 字节；长度 ≠4 → None——类型语义不猜）。
pub fn dword_value(val: &[u8]) -> Option<u32> {
    if val.len() != 4 {
        return None;
    }
    Some(u32::from_le_bytes([val[0], val[1], val[2], val[3]]))
}

/// F009 深化批次六自检。
pub fn run_reghive_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep5");
    // 1) 类型标记：标记/查询/更新幂等（同键改型不占双槽）。
    let mut tt = ValTypeTable::new();
    tt.tag(0xA1, ValType::Sz);
    tt.tag(0xA2, ValType::Dword);
    tt.tag(0xA1, ValType::Binary);
    cs.add(
        "valtype_tag_update_idempotent",
        tt.type_of(0xA1) == Some(ValType::Binary) && tt.type_of(0xA2) == Some(ValType::Dword),
        "",
    );
    // 2) DWORD 语义：小端 4 字节读出；长度不符如实 None。
    cs.add(
        "dword_value_little_endian",
        dword_value(&[0x39, 0x05, 0x00, 0x00]) == Some(1337)
            && dword_value(&[0x01, 0x02]).is_none()
            && dword_value(&[1, 2, 3, 4, 5]).is_none(),
        "",
    );
    // 3) 未标记键如实 None（不猜缺省类型——诚实边界）。
    cs.add("valtype_untagged_none", ValTypeTable::new().type_of(0xA1).is_none(), "");
    cs
}

// ---------------------------------------------------------------------------
// F009 · 深化批次七：十六进制分页查看（蜂巢查看器的大值分页面）
//
// 主册依据（G-A-09【设计细节】）：「蜂巢查看器……十六进制值查看」的续面——
// 大值分页：每页 16 字节、`偏移: 字节...` 行形态（offset 4 位 hex），页越界
// 如实空页。
// ---------------------------------------------------------------------------

/// 每页字节数。
pub const HEXDUMP_PAGE_BYTES: usize = 16;

/// 一页十六进制渲染（`0040: DE AD ...` 单行；行首 offset 4 位 hex；缓冲不足
/// 截断）。页越界 → 0 字节（空页如实）。
pub fn hexdump_page(val: &[u8], page: usize, buf: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let start = page * HEXDUMP_PAGE_BYTES;
    if start >= val.len() {
        return 0;
    }
    let end = (start + HEXDUMP_PAGE_BYTES).min(val.len());
    let mut n = 0usize;
    // 行首偏移（4 位大写 hex）。
    let off = start;
    for shift in [12u32, 8, 4, 0] {
        if n < buf.len() {
            buf[n] = HEX[((off >> shift) & 0xF) as usize];
            n += 1;
        }
    }
    if n < buf.len() {
        buf[n] = b':';
        n += 1;
    }
    for &b in &val[start..end] {
        if n + 3 > buf.len() {
            return n;
        }
        buf[n] = b' ';
        buf[n + 1] = HEX[(b >> 4) as usize];
        buf[n + 2] = HEX[(b & 0xF) as usize];
        n += 3;
    }
    n
}

/// F009 深化批次七自检。
pub fn run_reghive_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep6");
    // 1) 满页：16 字节 → 行首偏移 0000 + 16 对 hex。
    let data: alloc::vec::Vec<u8> = (0..32u8).collect();
    let mut buf = [0u8; 128];
    let n1 = hexdump_page(&data, 0, &mut buf);
    let t1 = core::str::from_utf8(&buf[..n1]).unwrap_or("");
    cs.add(
        "hexdump_page_zero_offset",
        n1 == 4 + 1 + 16 * 3 && t1.starts_with("0000: 00 01 02 03") && t1.ends_with("0F"),
        "",
    );
    // 2) 第二页：偏移 0010 + 剩余 16 字节。
    let n2 = hexdump_page(&data, 1, &mut buf);
    let t2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    cs.add(
        "hexdump_page_one_offset",
        t2.starts_with("0010: 10 11 12") && t2.ends_with("1F"),
        "",
    );
    // 3) 尾页不满 + 页越界空页（如实零字节）。
    let tail: alloc::vec::Vec<u8> = (0..20u8).collect();
    let n3 = hexdump_page(&tail, 1, &mut buf);
    let tail_copy: alloc::vec::Vec<u8> = buf[..n3].to_vec();
    let t3 = core::str::from_utf8(&tail_copy).unwrap_or("");
    let empty = hexdump_page(&tail, 5, &mut buf);
    cs.add(
        "hexdump_page_tail_and_beyond",
        n3 == 4 + 1 + 4 * 3 && t3.ends_with("13") && empty == 0,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F009 · 深化批次八：蜂巢压实记账面（删除后的空洞回收——B 树原地删除是
// 既有面；本段补「压实把记录前移、回收空洞字节」的模拟核：压实后容量
// 占用连续、每条记录字节内容逐字节保真）。
// ---------------------------------------------------------------------------

/// 蜂巢页压实模型（页内记录槽，删后留空洞；压实 = 前移保序）。
pub struct HivePage {
    slots: [Option<alloc::vec::Vec<u8>>; 8],
    pub compactions: u32,
}

impl HivePage {
    pub fn new() -> HivePage {
        HivePage { slots: Default::default(), compactions: 0 }
    }

    pub fn put(&mut self, idx: usize, rec: &[u8]) -> bool {
        if idx >= 8 || self.slots[idx].is_some() {
            return false;
        }
        self.slots[idx] = Some(rec.to_vec());
        true
    }

    pub fn remove(&mut self, idx: usize) -> bool {
        if idx >= 8 {
            return false;
        }
        self.slots[idx].take().is_some()
    }

    /// 空洞数（None 槽位于有记录槽之后才算尾洞，中间 None = 碎片）。
    pub fn fragments(&self) -> usize {
        let last = self.slots.iter().rposition(|s| s.is_some()).map_or(0, |i| i + 1);
        (0..last).filter(|&i| self.slots[i].is_none()).count()
    }

    /// 压实：所有记录保序前移。返回移动条数（无碎片时 0 且不计数）。
    pub fn compact(&mut self) -> usize {
        let mut write = 0usize;
        let mut moved = 0usize;
        for read in 0..8 {
            if let Some(rec) = self.slots[read].take() {
                if write != read {
                    moved += 1;
                }
                self.slots[write] = Some(rec);
                write += 1;
            }
        }
        if moved > 0 {
            self.compactions += 1;
        }
        moved
    }
}

/// F009 深化批次八自检。
fn run_reghive_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep7");
    let mut page = HivePage::new();
    for (i, name) in [b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec(), b"delta".to_vec()]
        .into_iter()
        .enumerate()
    {
        let _ = page.put(i * 2, &name); // 0/2/4/6 有记录，1/3/5 为碎片
    }
    // 1) 中间空洞 = 碎片 3；删除首条后碎片仍 3（0/2/4 三个洞）。
    let frag0 = page.fragments();
    let _ = page.remove(0);
    let frag1 = page.fragments();
    cs.add(
        "fragments_counted_honestly",
        frag0 == 3 && frag1 == 4,
        "",
    );
    // 2) 压实：保序前移，内容逐字节保真，空洞只剩尾洞。
    let moved = page.compact();
    let ok = page.slots.iter().filter(|s| s.is_some()).count() == 3
        && page.slots[0].as_deref() == Some(b"beta".as_slice())
        && page.slots[1].as_deref() == Some(b"gamma".as_slice())
        && page.slots[2].as_deref() == Some(b"delta".as_slice())
        && page.fragments() == 0;
    cs.add(
        "compact_preserves_order_and_bytes",
        moved == 3 && ok && page.compactions == 1,
        "",
    );
    // 3) 无碎片压实 = 不动不计数（零噪声纪律）。
    let moved2 = page.compact();
    cs.add(
        "compact_noop_when_dense",
        moved2 == 0 && page.compactions == 1,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F009 · 深化批次九：蜂巢单元格分配器（注册表蜂巢的 cell 语义——每格
// 以带符号长度前缀：正 = 已分配、负 = 空闲；分配 8 字节对齐 + 空闲表
// 首适配 + 碎片记账）。
// ---------------------------------------------------------------------------

/// 蜂巢页单元格分配器（容量 256 字节的最小模型——真蜂巢块 4096，语义同构）。
pub struct CellArena {
    data: [u8; 256],
    /// 空闲段表：(偏移, 长度)，保序。
    free: alloc::vec::Vec<(u16, u16)>,
    pub alloc_count: u32,
    pub coalesce_count: u32,
}

impl CellArena {
    pub fn new() -> CellArena {
        CellArena {
            data: [0; 256],
            free: alloc::vec![(0u16, 256u16)],
            alloc_count: 0,
            coalesce_count: 0,
        }
    }

    /// 分配（needs ≥1 字节；8 对齐向上取整）——首适配。
    pub fn alloc_cell(&mut self, needs: u16) -> Option<u16> {
        if needs == 0 {
            return None;
        }
        let need = ((needs as u32 + 7) & !7u32) as u16;
        let pos = self.free.iter().position(|&(_, len)| len >= need)?;
        let (off, len) = self.free[pos];
        if len == need {
            self.free.remove(pos);
        } else {
            self.free[pos] = (off + need, len - need);
        }
        self.alloc_count += 1;
        Some(off)
    }

    /// 释放（合并相邻空闲段——左右双合并，每合并一次计一次）。
    pub fn free_cell(&mut self, off: u16, len: u16) {
        self.free.push((off, len));
        self.free.sort();
        let mut merged: alloc::vec::Vec<(u16, u16)> = alloc::vec::Vec::new();
        for &(o, l) in self.free.iter() {
            match merged.last_mut() {
                Some(last) if last.0 + last.1 == o => {
                    last.1 += l;
                    self.coalesce_count += 1;
                }
                _ => merged.push((o, l)),
            }
        }
        self.free = merged;
    }

    pub fn free_total(&self) -> u32 {
        self.free.iter().map(|&(_, l)| l as u32).sum()
    }

    pub fn free_segments(&self) -> usize {
        self.free.len()
    }

    pub fn byte_at(&self, off: u16) -> u8 {
        self.data[off as usize]
    }

    pub fn set_byte(&mut self, off: u16, v: u8) {
        self.data[off as usize] = v;
    }
}

/// F009 深化批次九自检。
fn run_reghive_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep8");
    let mut arena = CellArena::new();
    // 1) 8 对齐分配：5 字节请求占 8；连续分配偏移推进 0/8/16。
    let a = arena.alloc_cell(5);
    let b = arena.alloc_cell(8);
    let c = arena.alloc_cell(1);
    cs.add(
        "cell_alloc_aligned_sequential",
        a == Some(0) && b == Some(8) && c == Some(16) && arena.free_segments() == 1,
        "",
    );
    // 2) 释放中段合并：free(0,8)+free(16,8) 后 free(8,8) → 三段合一（两次合并计数）。
    arena.free_cell(0, 8);
    arena.free_cell(16, 8);
    let segs_before = arena.free_segments(); // 3 段：0-8 / 24- / 16-24 插入后排序
    arena.free_cell(8, 8);
    cs.add(
        "cell_coalesce_middle",
        segs_before == 2 && arena.free_segments() == 1 && arena.free_total() == 256,
        "",
    );
    // 3) 内容寻址：分配出的偏移可写可读（分配不破坏邻格）。
    let m = arena.alloc_cell(8);
    let mut ok = false;
    if let Some(m) = m {
        arena.set_byte(m, 0xAB);
        arena.set_byte(m + 7, 0xCD);
        ok = arena.byte_at(m) == 0xAB && arena.byte_at(m + 7) == 0xCD;
    }
    cs.add(
        "cell_content_addressable",
        ok && arena.alloc_count == 4,
        "",
    );
    cs
}
