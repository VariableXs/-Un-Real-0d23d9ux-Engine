//! VE 域模块登记（AI-MIMO-1022 批开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。
//!
//! | 模块 | 任务号 | 核心交付 |
//! |------|--------|----------|
//! | [`ve174_minimizer`] | VE-F0174 | 崩溃复现最小化器（delta debugging 二分/三分/层级裁剪） |
//! | [`ve175_cmdcompress`] | VE-F0175 | 命令流压缩差分存储（增量编码+字典+零拷贝读取） |
//! | [`ve176_logsampler`] | VE-F0176 | 日志采样器（自适应采样率+突发保留+统计无偏） |
//! | [`ve177_perfcnt`] | VE-F0177 | 性能计数器注册表（五要素注册+版本化+冲突拒绝） |
//! | [`ve178_cnt2diag`] | VE-F0178 | 计数器到诊断中心通路（档位汇聚+配额限流+失联检测） |
//! | [`ve179_knobpanel`] | VE-F0179 | 实时调参面板（旋钮注册+即时应用+失败回滚+一键默认） |
//! | [`ve180_knobaudit`] | VE-F0180 | 旋钮变更审计（五元组+只增不改+历史回放+分流独立） |
//! | [`ve181_dbgmatrix`] | VE-F0181 | 调试分级能力矩阵（三档矩阵+越权拒绝+保守未知） |
//! | [`ve182_trap`] | VE-F0182 | 渲染陷阱系统（主动陷阱+release剔除+现场快照+去重限频） |
//! | [`ve183_timetravel`] | VE-F0183 | 时间旅行调试增强（双向状态快照+确定性回放+检查点） |
//! | [`ve184_multidbg`] | VE-F0184 | 多实例并发调试（ID路由+通道隔离+跨例对照+生命周期） |
//! | [`ve185_frameanno`] | VE-F0185 | 帧标注系统（随流存档+回放对齐+检索+类型化） |
//! | [`ve186_domselfcheck`] | VE-F0186 | VE-A 域聚合自检（源头模板+增维声明+证据引用制） |
//! | [`ve187_apifreeze`] | VE-F0187 | 域内接口冻结清单（四大族+语义版本+公告机制） |
//! | [`ve188_perfbaseline`] | VE-F0188 | 域性能基线总账（分档诚实+样本如实+版本化） |
//! | [`ve189_memledger`] | VE-F0189 | 域内存占用总账（常驻上限+超限逐出+对账补登） |
//! | [`ve190_errcodes`] | VE-F0190 | 域错误码段分配总表（连续段位+不回收+三要素齐） |
//! | [`ve191_docgen`] | VE-F0191 | 域接口文档自动生成（同源生成+四要素模板+注释隔离） |
//! | [`ve192_dedup`] | VE-F0192 | 域查重总对账（语义查重+限期改写+误报复核） |
//! | [`ve193_acceptance`] | VE-F0193 | 域验收矩阵（20维逐项+证据裁决+阻断机制） |
//! | [`ve194_depgraph`] | VE-F0194 | 域间依赖图（三列表+循环阻断+违规登记+升版流程） |

// ---------------------------------------------------------------------------
// AI-MIMO-1022 批（VE-F0174~F0194 · VE-A 调试/遥测域 · 纯功能）
// ---------------------------------------------------------------------------
// ve174_minimizer ↔ VE-F0174 崩溃复现最小化器
pub mod ve174_minimizer;
// ve175_cmdcompress ↔ VE-F0175 命令流压缩差分存储
pub mod ve175_cmdcompress;
// ve176_logsampler ↔ VE-F0176 日志采样器
pub mod ve176_logsampler;
// ve177_perfcnt ↔ VE-F0177 性能计数器注册表
pub mod ve177_perfcnt;
// ve178_cnt2diag ↔ VE-F0178 计数器到诊断中心通路
pub mod ve178_cnt2diag;
// ve179_knobpanel ↔ VE-F0179 实时调参面板
pub mod ve179_knobpanel;
// ve180_knobaudit ↔ VE-F0180 旋钮变更审计
pub mod ve180_knobaudit;
// ve181_dbgmatrix ↔ VE-F0181 调试分级能力矩阵
pub mod ve181_dbgmatrix;
// ve182_trap ↔ VE-F0182 渲染陷阱系统
pub mod ve182_trap;
// ve183_timetravel ↔ VE-F0183 时间旅行调试增强
pub mod ve183_timetravel;
// ve184_multidbg ↔ VE-F0184 多实例并发调试
pub mod ve184_multidbg;
// ve185_frameanno ↔ VE-F0185 帧标注系统
pub mod ve185_frameanno;
// ve186_domselfcheck ↔ VE-F0186 VE-A 域聚合自检
pub mod ve186_domselfcheck;
// ve187_apifreeze ↔ VE-F0187 域内接口冻结清单与版本化
pub mod ve187_apifreeze;
// ve188_perfbaseline ↔ VE-F0188 域性能基线总账
pub mod ve188_perfbaseline;
// ve189_memledger ↔ VE-F0189 域内存占用总账
pub mod ve189_memledger;
// ve190_errcodes ↔ VE-F0190 域错误码段分配总表
pub mod ve190_errcodes;
// ve191_docgen ↔ VE-F0191 域接口文档自动生成
pub mod ve191_docgen;
// ve192_dedup ↔ VE-F0192 域查重总对账
pub mod ve192_dedup;
// ve193_acceptance ↔ VE-F0193 域验收矩阵（20 维度逐项过）
pub mod ve193_acceptance;
// ve194_depgraph ↔ VE-F0194 域间依赖图
pub mod ve194_depgraph;
