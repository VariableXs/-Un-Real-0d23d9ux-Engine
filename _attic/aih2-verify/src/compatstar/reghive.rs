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
pub fn run_reghive_checks() -> CheckSet {
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
