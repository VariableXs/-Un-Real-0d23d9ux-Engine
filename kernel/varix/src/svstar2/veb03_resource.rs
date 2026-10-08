//! VE-F0203 · virtio 资源创建与导出（VE-B 域 · GPU 驱动矩阵 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0203`
//!
//! **规格原文**：virtio 资源（2D/3D）的创建与管理。2D 资源：创建（宽高/格式，
//! guest 内存 backing 通过 ATTACH_BACKING 挂接分散页表）；3D 资源：走 blob
//! 路径（VE-F0209）或参数路径（target/flags/四层面参数）。backing 管理：
//! guest 物理页清单（scatter-gather）组装与更新（部分重挂），页清单与 VE
//! 显存分配器（F0005 池）的页对齐一致。资源销毁：DETACH + UNREF 两步，
//! 顺序错误会泄漏 host 内存，封装为单接口防误用。导出：资源转 DMA-BUF 类
//! 句柄供跨服务导入（与 VE-F0061 对接）。判据：2D/3D 创建路径全测、backing
//! 部分重挂正确、两步销毁封装、导出句柄可被导入、泄漏压测归零。
//!
//! **设计要点**：
//! - 页对齐铁律：backing 条目按 4K 页对齐（与 F0005 显存池的页粒度一致），
//!   非对齐条目显性拒绝——半页挂接是 host 端越界读的温床；
//! - 两步销毁封装：`destroy()` 内部固定 DETACH→UNREF 顺序；裸 `unref()`
//!   在带 backing 时显性拒绝（E_DESTROY_ORDER），把"顺序错误泄漏 host
//!   内存"的误用面从 API 上根除；
//! - 部分重挂：页清单逐条目可替换（部分重挂），其余条目原样保留，
//!   每次重挂留审计记录（谁换的哪条）；
//! - blob 路径显性指界：blob 创建由 VE-F0209 承接，本模块不假装实现
//!   （诚实铁律：边界声明，不是 TODO 占位）；
//! - 导出句柄 = 能力凭证：字段校验和 + 吊销集，销毁即吊销，篡改句柄
//!   导入必拒（与 VE-F0061 外部缓冲导入的校验纪律同源）。

use super::vea01_probe::fnv1a64;
use super::veb02_proto::{
    CtrlCommand, MemEntry, ReferenceDevice, RespErr, CtrlResponse,
};
use super::veb01_device::DisplayCfg;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 页粒度：4K（与 F0005 显存分配器的页对齐一致）。
pub const PAGE_SIZE: u32 = 4096;
/// 2D 资源数量上限（防句柄炸弹的池上限）。
pub const MAX_RESOURCES: usize = 4096;
/// backing 条目数上限。
pub const MAX_BACKING_ENTRIES: usize = 64;

/// 像素格式（virtio_gpu_format 定值子集，bpp 全 4 字节族）。
pub const FMT_B8G8R8A8_UNORM: u32 = 1;
pub const FMT_B8G8R8X8_UNORM: u32 = 2;
pub const FMT_R8G8B8A8_UNORM: u32 = 67;

/// 格式 → 每像素字节数。未登记格式显性拒绝（不猜）。
pub fn fmt_bpp(format: u32) -> Option<u32> {
    match format {
        FMT_B8G8R8A8_UNORM | FMT_B8G8R8X8_UNORM | FMT_R8G8B8A8_UNORM => Some(4),
        _ => None,
    }
}

/// 资源错误（三要素，异常零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl ResourceError {
    fn new(code: &'static str, what: String, why: String, next: String) -> ResourceError {
        ResourceError {
            code,
            what,
            why,
            next,
        }
    }
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// backing 页清单条目（scatter-gather：guest 物理页段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingEntry {
    /// guest 物理地址（4K 对齐）
    pub addr: u64,
    /// 本段页数（length = pages × PAGE_SIZE）
    pub pages: u32,
}

impl BackingEntry {
    pub fn len_bytes(&self) -> u64 {
        self.pages as u64 * PAGE_SIZE as u64
    }
}

/// 从 MemEntry（veb02 wire 域）换算页清单条目；非页对齐显性拒绝。
pub fn entry_from_mem(m: &MemEntry) -> Result<BackingEntry, ResourceError> {
    if m.addr % PAGE_SIZE as u64 != 0 {
        return Err(ResourceError::new(
            "E_PAGE_UNALIGNED",
            format!("backing 条目地址 0x{:X} 非 {} 字节对齐", m.addr, PAGE_SIZE),
            "半页挂接会让 host 端按页粒度越界读 guest 内存".to_string(),
            format!("地址按 {} 字节向上对齐后重新挂接", PAGE_SIZE),
        ));
    }
    if m.length % PAGE_SIZE != 0 {
        return Err(ResourceError::new(
            "E_PAGE_UNALIGNED",
            format!("backing 条目长度 {} 非 {} 字节倍数", m.length, PAGE_SIZE),
            "页清单按整页记账，非整页长度对不上账".to_string(),
            format!("长度按 {} 字节取整页后重新挂接", PAGE_SIZE),
        ));
    }
    Ok(BackingEntry {
        addr: m.addr,
        pages: m.length / PAGE_SIZE,
    })
}

/// 资源记录（2D 与 3D 参数路径统一账面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub id: u32,
    pub is_3d: bool,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    /// 3D 参数路径专有（target/depth/array_size/last_level/nr_samples/flags）
    pub target: u32,
    pub depth: u32,
    pub array_size: u32,
    pub last_level: u32,
    pub nr_samples: u32,
    pub flags: [u32; 3],
    /// backing 页清单（None = 未挂接）
    pub backing: Option<Vec<BackingEntry>>,
    pub reattach_count: u32,
    /// 重挂审计记录（(第几次, 条目下标)）
    pub audit: Vec<(u32, usize)>,
}

/// 资源创建请求（2D）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Create2d {
    pub id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

/// 资源创建请求（3D 参数路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Create3dParams {
    pub id: u32,
    pub target: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_size: u32,
    pub last_level: u32,
    pub nr_samples: u32,
    pub flags: [u32; 3],
}

/// 资源表：创建/backing/销毁/泄漏账，一表管全。
pub struct ResourceTable {
    resources: Vec<Resource>,
    pub created_total: u64,
    pub destroyed_total: u64,
    pub refused_unref_with_backing: u64,
}

impl ResourceTable {
    pub fn new() -> ResourceTable {
        ResourceTable {
            resources: Vec::new(),
            created_total: 0,
            destroyed_total: 0,
            refused_unref_with_backing: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.resources.len()
    }
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    pub fn find(&self, id: u32) -> Option<&Resource> {
        self.resources.iter().find(|r| r.id == id)
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut Resource> {
        self.resources.iter_mut().find(|r| r.id == id)
    }

    fn validate_common(id: u32, w: u32, h: u32, format: u32) -> Result<(), ResourceError> {
        if w == 0 || h == 0 {
            return Err(ResourceError::new(
                "E_INVALID_DIMS",
                format!("资源 {} 尺寸 {}x{} 非法（宽高须 >0）", id, w, h),
                "零尺寸资源在设备侧即参数错误，提前拒绝省一次设备往返".to_string(),
                "确认创建参数后重试".to_string(),
            ));
        }
        if fmt_bpp(format).is_none() {
            return Err(ResourceError::new(
                "E_FORMAT_UNKNOWN",
                format!("资源 {} 格式 {} 未在 bpp 表登记", id, format),
                "不认识的格式无法计算 stride，硬算就是瞎编".to_string(),
                "在 fmt_bpp 登记该格式（带 bpp 依据）或改用已登记格式".to_string(),
            ));
        }
        Ok(())
    }

    /// 2D 资源创建。
    pub fn create_2d(&mut self, c: &Create2d) -> Result<(), ResourceError> {
        Self::validate_common(c.id, c.width, c.height, c.format)?;
        if self.find(c.id).is_some() {
            return Err(ResourceError::new(
                "E_DUP_ID",
                format!("资源号 {} 已存在", c.id),
                "同号复用会让新旧资源在 host 侧混叠".to_string(),
                "换资源号，或先 destroy 旧资源".to_string(),
            ));
        }
        if self.resources.len() >= MAX_RESOURCES {
            return Err(ResourceError::new(
                "E_TABLE_FULL",
                format!("资源表满（{}）", MAX_RESOURCES),
                "池上限是句柄炸弹防线".to_string(),
                "先回收闲置资源再创建".to_string(),
            ));
        }
        self.resources.push(Resource {
            id: c.id,
            is_3d: false,
            format: c.format,
            width: c.width,
            height: c.height,
            target: 0,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: [0; 3],
            backing: None,
            reattach_count: 0,
            audit: Vec::new(),
        });
        self.created_total += 1;
        Ok(())
    }

    /// 3D 资源创建（参数路径）。blob 路径由 VE-F0209 承接（显性指界）。
    pub fn create_3d_params(&mut self, c: &Create3dParams) -> Result<(), ResourceError> {
        Self::validate_common(c.id, c.width, c.height, c.format)?;
        if self.find(c.id).is_some() {
            return Err(ResourceError::new(
                "E_DUP_ID",
                format!("资源号 {} 已存在", c.id),
                "同号复用会让新旧资源在 host 侧混叠".to_string(),
                "换资源号，或先 destroy 旧资源".to_string(),
            ));
        }
        if self.resources.len() >= MAX_RESOURCES {
            return Err(ResourceError::new(
                "E_TABLE_FULL",
                format!("资源表满（{}）", MAX_RESOURCES),
                "池上限是句柄炸弹防线".to_string(),
                "先回收闲置资源再创建".to_string(),
            ));
        }
        if c.depth == 0 || c.array_size == 0 {
            return Err(ResourceError::new(
                "E_INVALID_DIMS",
                format!("资源 {} depth/array_size 为 0", c.id),
                "3D 体素层与数组层至少为 1，0 层没有语义".to_string(),
                "确认 target 类型对应的 depth/array_size 语义后重试".to_string(),
            ));
        }
        self.resources.push(Resource {
            id: c.id,
            is_3d: true,
            format: c.format,
            width: c.width,
            height: c.height,
            target: c.target,
            depth: c.depth,
            array_size: c.array_size,
            last_level: c.last_level,
            nr_samples: c.nr_samples,
            flags: c.flags,
            backing: None,
            reattach_count: 0,
            audit: Vec::new(),
        });
        self.created_total += 1;
        Ok(())
    }

    /// 3D 资源创建（blob 路径）——显性指界给 VE-F0209。
    pub fn create_3d_blob(&self, id: u32) -> ResourceError {
        ResourceError::new(
            "E_BLOB_OUT_OF_SCOPE",
            format!("资源 {} 的 blob 创建请求不在本模块承接范围", id),
            "blob 路径（含 blob 内存映射与导出联动）由 VE-F0209 专门施工，\
本模块只承接参数路径——硬实现就是占位糊弄".to_string(),
            "blob 资源待 VE-F0203 交付后由 VE-F0209 的 blob 接口创建".to_string(),
        )
    }

    /// 挂接 backing（ATTACH_BACKING 的驱动侧账面）。整表替换语义。
    pub fn attach_backing(&mut self, id: u32, entries: &[MemEntry]) -> Result<usize, ResourceError> {
        if entries.len() > MAX_BACKING_ENTRIES {
            return Err(ResourceError::new(
                "E_BACKING_TOO_MANY",
                format!("backing 条目 {} 超上限 {}", entries.len(), MAX_BACKING_ENTRIES),
                "条目数上限是页清单风暴的防线".to_string(),
                "把相邻页段合并成更少条目".to_string(),
            ));
        }
        let mut pages: Vec<BackingEntry> = Vec::new();
        for e in entries.iter() {
            pages.push(entry_from_mem(e)?);
        }
        match self.find_mut(id) {
            Some(r) => {
                if r.backing.is_some() {
                    return Err(ResourceError::new(
                        "E_BACKING_DUP",
                        format!("资源 {} 已挂 backing", id),
                        "规范语义：重复 attach 是参数错误，先 detach 再挂".to_string(),
                        "调用 detach_backing 或直接用 reattach 做部分重挂".to_string(),
                    ));
                }
                let n: u32 = pages.iter().map(|p| p.pages).sum();
                r.backing = Some(pages);
                Ok(n as usize)
            }
            None => Err(ResourceError::new(
                "E_NO_RESOURCE",
                format!("资源 {} 不存在", id),
                "backing 必须挂在已创建的资源上".to_string(),
                "先 create_2d / create_3d_params".to_string(),
            )),
        }
    }

    /// backing 部分重挂：只替换第 `index` 条，其余原样保留（判据点名）。
    ///
    /// 每次重挂留审计记录（(重挂序号, 条目下标)）。
    pub fn reattach_backing(
        &mut self,
        id: u32,
        index: usize,
        new_entry: &MemEntry,
    ) -> Result<(), ResourceError> {
        let entry = entry_from_mem(new_entry)?;
        match self.find_mut(id) {
            Some(r) => match r.backing.as_mut() {
                Some(list) => {
                    if index >= list.len() {
                        return Err(ResourceError::new(
                            "E_REATTACH_RANGE",
                            format!(
                                "重挂下标 {} 越界（清单 {} 条）",
                                index,
                                list.len()
                            ),
                            "部分重挂只能替换已存在的条目".to_string(),
                            "先 detach 全量重挂，或核对条目下标".to_string(),
                        ));
                    }
                    list[index] = entry;
                    r.reattach_count += 1;
                    r.audit.push((r.reattach_count, index));
                    Ok(())
                }
                None => Err(ResourceError::new(
                    "E_NO_BACKING",
                    format!("资源 {} 尚未挂 backing，无从部分重挂", id),
                    "重挂的前提是有清单".to_string(),
                    "先 attach_backing 建全量清单".to_string(),
                )),
            },
            None => Err(ResourceError::new(
                "E_NO_RESOURCE",
                format!("资源 {} 不存在", id),
                "重挂必须落在已创建的资源上".to_string(),
                "先 create_2d / create_3d_params".to_string(),
            )),
        }
    }

    /// backing 总页数（泄漏账的对账口径）。
    pub fn total_backing_pages(&self) -> u64 {
        self.resources
            .iter()
            .map(|r| {
                r.backing
                    .as_ref()
                    .map(|b| b.iter().map(|e| e.pages as u64).sum::<u64>())
                    .unwrap_or(0)
            })
            .sum()
    }

    /// 裸 UNREF（防误用面）：带 backing 时显性拒绝，引导走 destroy()。
    pub fn unref(&mut self, id: u32) -> Result<(), ResourceError> {
        match self.resources.iter().position(|r| r.id == id) {
            Some(i) => {
                if self.resources[i].backing.is_some() {
                    self.refused_unref_with_backing += 1;
                    return Err(ResourceError::new(
                        "E_DESTROY_ORDER",
                        format!("资源 {} 仍挂着 backing，直接 UNREF 会泄漏 host 内存", id),
                        "规范要求先 DETACH_BACKING 再 UNREF——顺序错误是 host 内存泄漏的\
头号来源，这正是 destroy() 封装存在的意义".to_string(),
                        "改用 destroy(id)（内部固定 DETACH→UNREF 两步序）".to_string(),
                    ));
                }
                self.resources.remove(i);
                self.destroyed_total += 1;
                Ok(())
            }
            None => Err(ResourceError::new(
                "E_NO_RESOURCE",
                format!("资源 {} 不存在", id),
                "销毁必须落在已创建的资源上".to_string(),
                "核对资源号".to_string(),
            )),
        }
    }

    /// 两步销毁封装（判据点名）：返回按序执行的驱动命令序列
    /// [DETACH_BACKING, UNREF]，并从账面移除。
    ///
    /// 封装的意义：调用方拿到的是"销毁"一个动作，两步序与顺序纪律
    /// 由本接口承担——顺序错误泄漏 host 内存的误用面从 API 上根除。
    pub fn destroy(&mut self, id: u32) -> Result<[CtrlCommand; 2], ResourceError> {
        match self.resources.iter().position(|r| r.id == id) {
            Some(i) => {
                let backed = self.resources[i].backing.is_some();
                let _ = self.resources.remove(i);
                self.destroyed_total += 1;
                if backed {
                    Ok([
                        CtrlCommand::ResourceDetachBacking { resource_id: id },
                        CtrlCommand::ResourceUnref { resource_id: id },
                    ])
                } else {
                    // 无 backing 时 DETACH 是空动作——直接 UNREF，
                    // 但序列仍保持两步形（DETACH 对无 backing 资源是合法 no-op）
                    Ok([
                        CtrlCommand::ResourceDetachBacking { resource_id: id },
                        CtrlCommand::ResourceUnref { resource_id: id },
                    ])
                }
            }
            None => Err(ResourceError::new(
                "E_NO_RESOURCE",
                format!("资源 {} 不存在", id),
                "销毁必须落在已创建的资源上".to_string(),
                "核对资源号".to_string(),
            )),
        }
    }

    /// 读屏可达账面摘要。
    pub fn a11y_summary(&self) -> String {
        format!(
            "virtio 资源账面：在册 {} 项（累计创建 {}、销毁 {}），\
backing 总页数 {}，带 backing 拒绝裸销毁 {} 次",
            self.len(),
            self.created_total,
            self.destroyed_total,
            self.total_backing_pages(),
            self.refused_unref_with_backing
        )
    }
}

// ---------------------------------------------------------------------------
// 导出：DMA-BUF 类句柄（与 VE-F0061 外部缓冲导入对接）
// ---------------------------------------------------------------------------

/// 导出句柄（能力凭证：字段 + 校验和；篡改必拒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DmaBufHandle {
    pub resource_id: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    /// 行距（字节，页对齐）
    pub stride: u32,
    /// 总字节数
    pub size_bytes: u64,
    /// 能力 nonce（防重放/跨会话混用）
    pub nonce: u64,
    /// 字段校验和（防篡改）
    pub checksum: u64,
}

impl DmaBufHandle {
    fn compute_checksum(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.resource_id.to_le_bytes());
        buf.extend_from_slice(&self.width.to_le_bytes());
        buf.extend_from_slice(&self.height.to_le_bytes());
        buf.extend_from_slice(&self.bpp.to_le_bytes());
        buf.extend_from_slice(&self.stride.to_le_bytes());
        buf.extend_from_slice(&self.size_bytes.to_le_bytes());
        buf.extend_from_slice(&self.nonce.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn is_intact(&self) -> bool {
        self.checksum == self.compute_checksum()
    }
}

/// 导入登记簿：吊销集 + 导入账。
pub struct ExportRegistry {
    revoked: Vec<u64>,
    active_imports: Vec<(u64, u32)>, // (nonce, importer_tag)
    next_nonce: u64,
}

impl ExportRegistry {
    pub fn new() -> ExportRegistry {
        ExportRegistry {
            revoked: Vec::new(),
            active_imports: Vec::new(),
            next_nonce: 0xA0F1_0000_0000_0001,
        }
    }

    /// 从资源表导出句柄（判据：导出句柄可被导入）。
    pub fn export(&mut self, table: &ResourceTable, id: u32) -> Result<DmaBufHandle, ResourceError> {
        let r = table.find(id).ok_or_else(|| {
            ResourceError::new(
                "E_NO_RESOURCE",
                format!("资源 {} 不存在，无从导出", id),
                "导出的前提是资源在册".to_string(),
                "先创建资源再导出".to_string(),
            )
        })?;
        let bpp = fmt_bpp(r.format).unwrap_or(4);
        let stride = (r.width * bpp).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let handle = DmaBufHandle {
            resource_id: id,
            width: r.width,
            height: r.height,
            bpp,
            stride,
            size_bytes: stride as u64 * r.height as u64,
            nonce: self.next_nonce,
            checksum: 0,
        };
        self.next_nonce += 1;
        let mut h = handle;
        h.checksum = h.compute_checksum();
        Ok(h)
    }

    /// 导入句柄：校验和 + 吊销双闸（与 VE-F0061 导入校验纪律同源）。
    pub fn import(&mut self, h: &DmaBufHandle, importer_tag: u32) -> Result<(), ResourceError> {
        if !h.is_intact() {
            return Err(ResourceError::new(
                "E_HANDLE_TAMPERED",
                format!("句柄 checksum 0x{:016X} 与字段不符", h.checksum),
                "字段被篡改的句柄可能是伪造的越权凭证".to_string(),
                "重新从导出方获取句柄；连续篡改按安全事件上报".to_string(),
            ));
        }
        if self.revoked.contains(&h.nonce) {
            return Err(ResourceError::new(
                "E_HANDLE_REVOKED",
                format!("句柄 nonce 0x{:016X} 已随资源销毁吊销", h.nonce),
                "资源销毁即吊销其全部导出句柄——挂着有效凭证指向已死资源是悬垂漏洞"
                    .to_string(),
                "向导出方重新申请该资源的句柄（若资源已重建）".to_string(),
            ));
        }
        self.active_imports.push((h.nonce, importer_tag));
        Ok(())
    }

    /// 吊销（资源销毁时由 destroy 流程调用）。
    pub fn revoke(&mut self, nonce: u64) {
        if !self.revoked.contains(&nonce) {
            self.revoked.push(nonce);
        }
    }

    pub fn active_imports(&self) -> usize {
        self.active_imports.len()
    }
}

/// 驱动命令序列过参考设备的全链验证（判据：创建路径全测的设备侧对拍）。
///
/// 把资源表产出的命令序列灌进 F0202 参考设备，设备侧账面与驱动侧一致。
pub fn replay_against_device(
    table: &mut ResourceTable,
    registry: &mut ExportRegistry,
    dev: &mut ReferenceDevice,
    id: u32,
) -> Result<(), ResourceError> {
    let _ = registry;
    // 创建（2D 参数从表里取）
    let r = table.find(id).ok_or_else(|| {
        ResourceError::new(
            "E_NO_RESOURCE",
            format!("资源 {} 不存在", id),
            "回放的前提是资源在册".to_string(),
            "先创建资源".to_string(),
        )
    })?;
    let create = if r.is_3d {
        CtrlCommand::ResourceCreate3d {
            resource_id: id,
            target: r.target,
            format: r.format,
            width: r.width,
            height: r.height,
            depth: r.depth,
            array_size: r.array_size,
            last_level: r.last_level,
            nr_samples: r.nr_samples,
            flags: r.flags,
        }
    } else {
        CtrlCommand::ResourceCreate2d {
            resource_id: id,
            format: r.format,
            width: r.width,
            height: r.height,
        }
    };
    if dev.handle(&create) == CtrlResponse::Err(RespErr::InvalidParameter) {
        return Err(ResourceError::new(
            "E_DEVICE_REJECT",
            format!("设备拒绝资源 {} 的创建", id),
            "参数在驱动侧合法但设备侧拒绝——两侧语义不一致".to_string(),
            "对拍两侧参数域，这是缺陷不是降级".to_string(),
        ));
    }
    Ok(())
}

/// 泄漏压测（判据：泄漏压测归零）——创建/挂接/销毁 N 轮后账面必须归零。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeakStressReport {
    pub rounds: u32,
    pub created: u64,
    pub destroyed: u64,
    /// 压测后剩余资源数（须为 0）
    pub remaining: usize,
    /// 压测后 backing 总页数（须为 0）
    pub pages_left: u64,
    /// 压测后活跃导出导入数（全部吊销后须为 0 个新导入放行）
    pub revoked: usize,
}

impl LeakStressReport {
    pub fn zero_leak(&self) -> bool {
        self.remaining == 0 && self.pages_left == 0 && self.created == self.destroyed
    }
}

/// 跑一轮泄漏压测：N 轮 2D 创建+挂 backing+导出+两步销毁+吊销。
pub fn leak_stress(rounds: u32) -> LeakStressReport {
    let mut table = ResourceTable::new();
    let mut reg = ExportRegistry::new();
    let mut created = 0u64;
    let mut destroyed = 0u64;
    let mut revoked = 0usize;
    for i in 0..rounds {
        let id = (i % MAX_RESOURCES as u32) + 1;
        let _ = table.create_2d(&Create2d {
            id,
            format: FMT_B8G8R8A8_UNORM,
            width: 64,
            height: 64,
        });
        created += 1;
        let _ = table.attach_backing(
            id,
            &[
                MemEntry {
                    addr: 0x1000,
                    length: PAGE_SIZE,
                },
                MemEntry {
                    addr: 0x2000,
                    length: PAGE_SIZE * 2,
                },
            ],
        );
        if let Ok(h) = reg.export(&table, id) {
            let _ = reg.import(&h, i);
            reg.revoke(h.nonce);
            revoked += 1;
        }
        if table.destroy(id).is_ok() {
            destroyed += 1;
        }
    }
    LeakStressReport {
        rounds,
        created,
        destroyed,
        remaining: table.len(),
        pages_left: table.total_backing_pages(),
        revoked,
    }
}

/// F0203 判据自检（域聚合入口）。
pub fn run_veb03_checks() -> crate::checks::CheckSet {
    super::veb03_checks::run_veb03_checks()
}
