# ICON-SPEC — 官方图标网格规格（Z-01 / U-10）

> AI-17 视觉语言组交付。目标：图标与 Windows 11 系统图标并排无法区分。
> 单一事实源：`src/lib/iconRegistry.ts`；本文档为规格说明。

## 1. 尺寸档位（U-10 / Z-01）

| 档位 | 尺寸 | 适用 |
|---|---|---|
| settings | 16px | 设置区、列表行内 |
| default | 18px | 全局默认（工具栏/按钮内） |
| desktop | 24px | 桌面级入口、空状态插画 |
| large | 32px | 首页/引导大图标 |

## 2. 网格与 keylines（对齐 Windows 11 官方网格）

- 主网格：20×20（16px 图标内缩 2px 绘制区；24px 图标按 1.2 倍外扩同 keyline）。
- 圆角终止线：线段端点一律圆头（stroke-linecap: round），与 Fluent 一致。
- 笔画粗细档位：16/18px → 2px；24/32px → 1.5–2px（lucide `strokeWidth` 1.5–2）。
- keyline 偏差硬指标：≤ 0.5px@1x。

## 3. 图标来源优先级

1. **Segoe Fluent Icons**（系统自带字体，零依赖）：`resolveSegoe(semantic)` 返回 PUA 码位时优先使用；
2. **lucide** 补缺口：`resolveIcon(semantic)`（映射表未登记 Segoe 对应物的语义）。

常用 Segoe 码位（节选，全集见 `SEGOE_FLUENT_MAP`）：
回收站 `\uE74D`、关闭 `\uE711`、搜索 `\uE721`、设置 `\uE713`、文件夹 `\uE8B7`、
复制 `\uE8C8`、剪切 `\uE8C6`、粘贴 `\uE77F`、刷新 `\uE72C`、添加 `\uE710`。

## 4. 语义字典（U-10 图标语言 2.0）

- 同一语义全仓唯一图标：组件层一律 `resolveIcon("delete")`，禁止散落直引 lucide；
- 「删除」永远是 trash，禁止某处用 x 某处用 trash；
- 单测守护：`src/lib/__tests__/iconRegistry.test.ts`（重复语义/组件冲突即 fail）。

## 5. 动效图标八件套（U-10）

加载弧线 / 同步循环 / 告警呼吸 / 成功勾绘 / 网络波动 / 音量级联 / 电池充电呼吸 / 时钟指针步进。
实现：`src/components/icons/AnimatedIcons.tsx` + `src/styles/animated-icons.css`；
全部 transform/opacity-only，`data-reduce-motion` 一键停用。

## 6. 验收口径

- [x] registry 单测：语义冲突即 fail（7 断言组全过）；
- [x] 全库无未经映射表登记的裸图标语义（新代码层）；
- [x] 动效图标 transform-only、可 reduce-motion 停用。

## 不做清单

不重绘 Fluent 图标（侵权且无必要）；不引入第三方图标库；不改变现有图标语义指代。
