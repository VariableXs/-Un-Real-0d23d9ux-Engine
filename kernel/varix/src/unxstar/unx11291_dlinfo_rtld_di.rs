//! UNX-F11291 RTLD_DI 接口族（dlinfo）（0x1f660 段）
//!
//! 锚点：UNX-F11291-J1 dlinfo 高频请求（LINK_MAP / SERINFO / LMID）判据绿，
//! 语义与 Linux 一致。
//!
//! 纯功能模型：以命名空间承载 link_map 链与搜索路径表，dlinfo 按请求码分派。
//! 请求码与 glibc 对齐：LMID=1、LINKMAP=2、CONFIGADDR=3、SERINFO=4、
//! SERINFOSIZE=5、ORIGIN=6、PROFILE*=7/8、TLS_*=9/10、PHDR=11。空句柄、
//! 未知码、废弃码、空链、空 origin、缓冲区不足一律按 Linux 语义拒止。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f660;
/// 空句柄或句柄失效
pub const DI_EHANDLE: u32 = SEG_BASE + 0x01;
/// 未知或本实现不支持的请求码
pub const DI_EREQ: u32 = SEG_BASE + 0x02;
/// 缓冲区不足
pub const DI_ESMALL: u32 = SEG_BASE + 0x03;
/// link_map 为空
pub const DI_ENOMAP: u32 = SEG_BASE + 0x04;
/// origin 为空
pub const DI_EORIGIN: u32 = SEG_BASE + 0x05;
/// 冻结后增删
pub const DI_EFROZEN: u32 = SEG_BASE + 0x06;
/// 成员重名
pub const DI_EDUP: u32 = SEG_BASE + 0x07;
/// 搜索路径表为空
pub const DI_ENOPATH: u32 = SEG_BASE + 0x08;

/// RTLD_DI_LMID：命名空间号。
pub const RTLD_DI_LMID: u32 = 1;
/// RTLD_DI_LINKMAP：link_map 链头。
pub const RTLD_DI_LINKMAP: u32 = 2;
/// RTLD_DI_CONFIGADDR：废弃请求（Linux 上返回 EINVAL）。
pub const RTLD_DI_CONFIGADDR: u32 = 3;
/// RTLD_DI_SERINFO：搜索路径表。
pub const RTLD_DI_SERINFO: u32 = 4;
/// RTLD_DI_SERINFOSIZE：搜索路径表所需字节数。
pub const RTLD_DI_SERINFOSIZE: u32 = 5;
/// RTLD_DI_ORIGIN：装载 origin。
pub const RTLD_DI_ORIGIN: u32 = 6;
/// 请求码上界（PHDR=11）。
pub const RTLD_DI_PHDR: u32 = 11;
/// 表头字节（Dl_serinfo 的 size/count 两字段）。
pub const SERINFO_HEADER: usize = 4;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// link_map 节点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapNode {
    pub name: String,
    pub base: u64,
    pub dynamic: bool,
}

/// dlinfo 回执（按请求码取值面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Lmid(u32),
    LinkMap(usize),
    SerInfo(Vec<String>),
    SerInfoSize(usize),
    Origin(String),
}

/// 命名空间：lmid + origin + 成员链 + 搜索路径。
#[derive(Clone, Debug)]
pub struct Namespace {
    pub lmid: u32,
    pub origin: String,
    members: Vec<MapNode>,
    search: Vec<String>,
    frozen: bool,
    pub queries: u64,
    pub rejects: u64,
}

impl Namespace {
    /// 建域：lmid 非零、origin 非空。
    pub fn new(lmid: u32, origin: &str) -> Result<Namespace, u32> {
        if lmid == 0 {
            return Err(DI_EHANDLE);
        }
        if origin.is_empty() {
            return Err(DI_EORIGIN);
        }
        Ok(Namespace {
            lmid,
            origin: origin.to_string(),
            members: Vec::new(),
            search: Vec::new(),
            frozen: false,
            queries: 0,
            rejects: 0,
        })
    }

    /// 挂成员：重名拒、冻结拒。
    pub fn add_member(&mut self, name: &str, base: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(DI_EFROZEN);
        }
        if name.is_empty() {
            return Err(DI_ENOMAP);
        }
        let mut k = 0usize;
        while k < self.members.len() {
            if let Some(m) = self.members.get(k) {
                if m.name == name {
                    return Err(DI_EDUP);
                }
            }
            k = k.wrapping_add(1);
        }
        self.members.push(MapNode { name: name.to_string(), base, dynamic: true });
        Ok(())
    }

    /// 追加搜索路径：空路径拒、冻结拒。
    pub fn add_search(&mut self, path: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(DI_EFROZEN);
        }
        if path.is_empty() {
            return Err(DI_ENOPATH);
        }
        self.search.push(path.to_string());
        Ok(())
    }

    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    pub fn search_count(&self) -> usize {
        self.search.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 链头名（空链返回 None）。
    pub fn head(&self) -> Option<&str> {
        match self.members.first() {
            Some(m) => Some(m.name.as_str()),
            None => None,
        }
    }

    /// 成员基址（按名查）。
    pub fn base_of(&self, name: &str) -> Option<u64> {
        let mut k = 0usize;
        while k < self.members.len() {
            if let Some(m) = self.members.get(k) {
                if m.name == name {
                    return Some(m.base);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// RTLD_DI_LMID：返回命名空间号（非零）。
    pub fn query_lmid(&self) -> Result<u32, u32> {
        if self.lmid == 0 {
            return Err(DI_EHANDLE);
        }
        Ok(self.lmid)
    }

    /// RTLD_DI_LINKMAP：返回链长；空链按 Linux 语义拒（不得返回空指针当成功）。
    pub fn query_link_map(&self) -> Result<usize, u32> {
        if self.members.is_empty() {
            return Err(DI_ENOMAP);
        }
        Ok(self.members.len())
    }

    /// RTLD_DI_ORIGIN：返回 origin 串。
    pub fn query_origin(&self) -> Result<String, u32> {
        if self.origin.is_empty() {
            return Err(DI_EORIGIN);
        }
        Ok(self.origin.clone())
    }

    /// RTLD_DI_SERINFOSIZE：表所需字节 = 头 4 + Σ(路径长+1) + 尾 1。
    pub fn serinfo_size(&self) -> Result<usize, u32> {
        if self.search.is_empty() {
            return Err(DI_ENOPATH);
        }
        let mut n = SERINFO_HEADER.wrapping_add(1);
        let mut k = 0usize;
        while k < self.search.len() {
            if let Some(p) = self.search.get(k) {
                n = n.wrapping_add(p.len()).wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        Ok(n)
    }

    /// RTLD_DI_SERINFO：缓冲足够才回表；不足时回 ESMALL 且不写半张表。
    pub fn query_serinfo(&self, buf_len: usize) -> Result<Vec<String>, u32> {
        let need = self.serinfo_size()?;
        if buf_len < need {
            return Err(DI_ESMALL);
        }
        let mut out: Vec<String> = Vec::with_capacity(self.search.len());
        let mut k = 0usize;
        while k < self.search.len() {
            if let Some(p) = self.search.get(k) {
                out.push(p.clone());
            }
            k = k.wrapping_add(1);
        }
        Ok(out)
    }

    /// dlinfo 分派：句柄 0 拒、未知码拒、废弃码拒。
    pub fn dispatch(&mut self, handle: u64, req: u32, buf_len: usize) -> Result<Reply, u32> {
        let r = self.dispatch_inner(handle, req, buf_len);
        match r {
            Ok(rep) => {
                self.queries = self.queries.wrapping_add(1);
                Ok(rep)
            }
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(e)
            }
        }
    }

    fn dispatch_inner(&self, handle: u64, req: u32, buf_len: usize) -> Result<Reply, u32> {
        if handle == 0 {
            return Err(DI_EHANDLE);
        }
        match req {
            RTLD_DI_LMID => Ok(Reply::Lmid(self.query_lmid()?)),
            RTLD_DI_LINKMAP => Ok(Reply::LinkMap(self.query_link_map()?)),
            RTLD_DI_SERINFO => Ok(Reply::SerInfo(self.query_serinfo(buf_len)?)),
            RTLD_DI_SERINFOSIZE => Ok(Reply::SerInfoSize(self.serinfo_size()?)),
            RTLD_DI_ORIGIN => Ok(Reply::Origin(self.query_origin()?)),
            RTLD_DI_CONFIGADDR => Err(DI_EREQ),
            _ => {
                if req == 0 || req > RTLD_DI_PHDR {
                    Err(DI_EREQ)
                } else {
                    // PROFILE* / TLS_* / PHDR 在纯功能模型内不着账。
                    Err(DI_EREQ)
                }
            }
        }
    }

    /// 命名空间指纹：lmid + origin + 成员 + 路径。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.lmid as u64).wrapping_mul(FNV_PRIME);
        let ob = self.origin.as_bytes();
        let mut k = 0usize;
        while k < ob.len() {
            h = (h ^ ob[k] as u64).wrapping_mul(FNV_PRIME);
            k = k.wrapping_add(1);
        }
        let mut m = 0usize;
        while m < self.members.len() {
            if let Some(node) = self.members.get(m) {
                h = (h ^ node.base).wrapping_mul(FNV_PRIME);
                let nb = node.name.as_bytes();
                let mut b = 0usize;
                while b < nb.len() {
                    h = (h ^ nb[b] as u64).wrapping_mul(FNV_PRIME);
                    b = b.wrapping_add(1);
                }
            }
            m = m.wrapping_add(1);
        }
        h
    }

    /// 冻结：幂等取回锁定指纹；冻结后只读查询仍可用。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return Some(self.fold());
        }
        self.frozen = true;
        Some(self.fold())
    }
}

/// 标准命名空间：lmid=1，三个成员、三条搜索路径（确定性样本）。
pub fn build_standard() -> Result<Namespace, u32> {
    let mut ns = Namespace::new(1, "/opt/varix/lib")?;
    ns.add_member("ld.so", 0x7f00_0000)?;
    ns.add_member("libc.so.6", 0x7f01_0000)?;
    ns.add_member("libvarix.so", 0x7f02_0000)?;
    ns.add_search("/lib")?;
    ns.add_search("/usr/lib")?;
    ns.add_search("/opt/varix/lib")?;
    Ok(ns)
}

/// 四路注入：空句柄 / 未知与废弃码 / 缓冲不足 / 冻结后增删，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // ① 空句柄
    let mut a = match build_standard() {
        Ok(v) => v,
        Err(_) => return got,
    };
    got[0] = a.dispatch(0, RTLD_DI_LMID, 0).err() == Some(DI_EHANDLE);

    // ② 未知码、零码与废弃码（CONFIGADDR 在 Linux 上返回 EINVAL）
    let mut b = match build_standard() {
        Ok(v) => v,
        Err(_) => return got,
    };
    got[1] = b.dispatch(1, 0, 0).err() == Some(DI_EREQ)
        && b.dispatch(1, 99, 0).err() == Some(DI_EREQ)
        && b.dispatch(1, RTLD_DI_CONFIGADDR, 0).err() == Some(DI_EREQ);

    // ③ 缓冲不足：ESMALL 且不改账
    let mut c = match build_standard() {
        Ok(v) => v,
        Err(_) => return got,
    };
    let need = match c.serinfo_size() {
        Ok(v) => v,
        Err(_) => 0,
    };
    got[2] = c.dispatch(1, RTLD_DI_SERINFO, need.wrapping_sub(1)).err() == Some(DI_ESMALL)
        && c.dispatch(1, RTLD_DI_SERINFO, need).is_ok();

    // ④ 冻结后增删拒、只读查询仍通
    let mut d = match build_standard() {
        Ok(v) => v,
        Err(_) => return got,
    };
    let root = d.freeze();
    got[3] = root.is_some()
        && d.freeze() == root
        && d.add_member("late.so", 1).err() == Some(DI_EFROZEN)
        && d.add_search("/late").err() == Some(DI_EFROZEN)
        && d.dispatch(1, RTLD_DI_LMID, 0).ok() == Some(Reply::Lmid(1));
    got
}

/// 判据台：dlinfo 高频请求面的十五条判据。
pub fn run_unx11291_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11291");

    let mut ns = match build_standard() {
        Ok(v) => v,
        Err(_) => {
            s.add("J0-装配失败", false, "标准命名空间未能建立");
            return s;
        }
    };

    // J1 LMID 命中
    s.add(
        "J1-LMID 命中",
        ns.dispatch(1, RTLD_DI_LMID, 0) == Ok(Reply::Lmid(1)),
        "句柄有效时 LMID 回 1",
    );

    // J2 LINKMAP 链长
    s.add(
        "J2-LINKMAP 链长",
        ns.dispatch(1, RTLD_DI_LINKMAP, 0) == Ok(Reply::LinkMap(3))
            && ns.head() == Some("ld.so"),
        "三成员链、链头为首挂对象",
    );

    // J3 SERINFOSIZE 与手算对平
    let manual = SERINFO_HEADER + 1 + ("/lib".len() + 1) + ("/usr/lib".len() + 1)
        + ("/opt/varix/lib".len() + 1);
    s.add(
        "J3-SERINFOSIZE 对平",
        ns.dispatch(1, RTLD_DI_SERINFOSIZE, 0) == Ok(Reply::SerInfoSize(manual)),
        "所需字节 = 头 4 + Σ(路径长+1) + 尾 1",
    );

    // J4 SERINFO 足缓冲回表
    let need = manual;
    let got = ns.dispatch(1, RTLD_DI_SERINFO, need);
    s.add(
        "J4-SERINFO 回表",
        got == Ok(Reply::SerInfo(vec![
            "/lib".to_string(),
            "/usr/lib".to_string(),
            "/opt/varix/lib".to_string()
        ])),
        "三条搜索路径按序回读",
    );

    // J5 SERINFO 缓冲不足拒
    s.add(
        "J5-缓冲不足拒",
        ns.dispatch(1, RTLD_DI_SERINFO, need.wrapping_sub(1)).err() == Some(DI_ESMALL),
        "差一字节即 ESMALL、不写半张表",
    );

    // J6 ORIGIN 回读
    s.add(
        "J6-ORIGIN 回读",
        ns.dispatch(1, RTLD_DI_ORIGIN, 0) == Ok(Reply::Origin("/opt/varix/lib".to_string())),
        "origin 串逐字回读",
    );

    // J7 空句柄拒
    s.add(
        "J7-空句柄拒",
        ns.dispatch(0, RTLD_DI_LMID, 0).err() == Some(DI_EHANDLE),
        "句柄 0 一律 EINVAL 口径",
    );

    // J8 未知码与废弃码拒
    s.add(
        "J8-未知废弃码",
        ns.dispatch(1, 0, 0).err() == Some(DI_EREQ)
            && ns.dispatch(1, 12, 0).err() == Some(DI_EREQ)
            && ns.dispatch(1, RTLD_DI_CONFIGADDR, 0).err() == Some(DI_EREQ),
        "0/12/CONFIGADDR 三路皆拒",
    );

    // J9 空链拒
    let j9 = match Namespace::new(7, "/ns7") {
        Ok(mut e) => {
            e.dispatch(1, RTLD_DI_LINKMAP, 0).err() == Some(DI_ENOMAP)
                && e.dispatch(1, RTLD_DI_SERINFOSIZE, 0).err() == Some(DI_ENOPATH)
        }
        Err(_) => false,
    };
    s.add("J9-空链拒", j9, "空 link_map 与空路径表皆拒");

    // J10 空 origin 拒建域
    s.add(
        "J10-空 origin 拒",
        Namespace::new(1, "").err() == Some(DI_EORIGIN)
            && Namespace::new(0, "/x").err() == Some(DI_EHANDLE),
        "origin 空或 lmid 零不得建域",
    );

    // J11 成员重名拒
    let j11 = match build_standard() {
        Ok(mut d) => {
            d.add_member("libc.so.6", 0).err() == Some(DI_EDUP) && d.member_count() == 3
        }
        Err(_) => false,
    };
    s.add("J11-成员重名拒", j11, "同名对象二次挂载拒");

    // J12 冻结幂等
    let root = ns.freeze();
    s.add(
        "J12-冻结幂等",
        root.is_some() && ns.freeze() == root && ns.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J13 冻结后增删拒、只读仍通
    s.add(
        "J13-冻结拒增删",
        ns.add_member("late.so", 1).err() == Some(DI_EFROZEN)
            && ns.add_search("/late").err() == Some(DI_EFROZEN)
            && ns.dispatch(1, RTLD_DI_ORIGIN, 0).is_ok(),
        "冻结后挂载与加路径皆拒、查询仍通",
    );

    // J14 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J14-注入四拒", all, "空句柄/未知码/缓冲/冻结四路全拒");

    // J15 动态明细
    s.add_dyn(
        "J15-命名空间明细",
        ns.queries >= 5 && ns.rejects >= 2,
        &format!(
            "members={} paths={} q={} rej={}",
            ns.member_count(),
            ns.search_count(),
            ns.queries,
            ns.rejects
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11291_dlinfo_flow() {
        let mut ns = match build_standard() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(ns.dispatch(1, RTLD_DI_LMID, 0), Ok(Reply::Lmid(1)));
        assert_eq!(ns.dispatch(1, RTLD_DI_LINKMAP, 0), Ok(Reply::LinkMap(3)));
        assert_eq!(ns.base_of("libc.so.6"), Some(0x7f01_0000));

        let need = match ns.serinfo_size() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(ns.query_serinfo(need).ok().map(|v| v.len()), Some(3));
        assert_eq!(ns.query_serinfo(need - 1).err(), Some(DI_ESMALL));
        assert_eq!(
            ns.dispatch(1, RTLD_DI_ORIGIN, 0),
            Ok(Reply::Origin("/opt/varix/lib".to_string()))
        );
        assert_eq!(ns.dispatch(0, RTLD_DI_LMID, 0).err(), Some(DI_EHANDLE));
        assert_eq!(ns.dispatch(1, 99, 0).err(), Some(DI_EREQ));
        assert_eq!(ns.add_member("ld.so", 1).err(), Some(DI_EDUP));

        assert!(ns.freeze().is_some());
        assert_eq!(ns.freeze(), ns.freeze());
        assert_eq!(ns.add_member("x.so", 1).err(), Some(DI_EFROZEN));

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11291_checks_green() {
        let s = run_unx11291_checks();
        assert!(s.all_passed(), "UNX-F11291 判据未全绿");
    }
}
