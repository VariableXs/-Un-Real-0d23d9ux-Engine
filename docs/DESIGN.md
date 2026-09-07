# Variable 设计语言（A-1 Design Tokens 冻结）

单一事实源：[`src/design/tokens.css`](../src/design/tokens.css)（在 `src/styles/global.css` 顶部引入）。
判断标准：任何界面放大 400% 不糊、任何动效逐帧看都顺、任何状态（空/错/加载）都被设计过。

## 语义层（三层结构）

```
语义 token（--bg-canvas / --accent / --danger …）
        ↑ 覆盖
亮度层（:root dark 默认 / [data-theme=paper] light / [data-theme=high-contrast]）
```

- 色彩全部走 **OKLCH**，保证跨主题感知一致；新增颜色必须先入 token 再引用。
- 语义层 → 既有变量（--bg/--panel/--text…）由「兼容期映射」桥接，迁移完成后移除。

## 字阶 / 间距 / 圆角

| 类别 | 档位 |
| --- | --- |
| 字阶 | 12 / 13 / 15 / 17 / 20 / 24 / 32（1.25 比例，`--fs-*`） |
| 间距 | 4px 基数 8 档（`--sp-1..8`） |
| 圆角 | 窗口 16 / 卡片 12 / 控件 8 / 小件 4（`--r-*`） |

## 阴影（elevation 0-5）

贴合壁纸层的半透明 OKLCH 阴影，**不用纯黑**：`--elev-1..5`（1 浮起卡片 → 5 模态/浮窗）。

## 动效（A-2）

- 曲线：`--ease-standard`（常规）/ `--ease-emphasized`（强调）/ `--ease-spring`（窗口弹入）。
- 时长六档：80 / 120 / 170 / 200 / 240 / 320ms（红绿灯 170/200ms 等既有数值全部收编为 `--dur-*`）。
- 原则：动效只表达**空间关系与因果**（窗口从哪来、到哪去、什么被打开）；装饰性动画仅限壁纸层。
- 降级：`data-reduce-motion="true"`（设置 reduceMotion / 性能低档位）下全部动效降级为 80ms 淡入淡出——**动效是增益不是依赖**。
- 性能：动画只用 transform/opacity（GPU 合成层）。

## 声音（A-4）

- 精选 **6 音**（`src/lib/sounds.ts`，Web Audio 程序化合成，零资产文件、永不报错）：
  启动落定 / 通知横幅 / 闹钟 / 错误 / 贴靠吸附 / 回收站清空，全部 ≤ 400ms。
- 音量独立可控（`settings.soundVolume`）+ 全局静音（`settings.soundMuted`）；勿扰模式自动静音（闹钟除外）。

## 无障碍（F-7 交界）

- 全局 `:focus-visible` 焦点环（2px accent）；`forced-colors: active` 尊重宿主高对比度。
- 高对比度主题：黑底 + 高亮 accent + 强对比描边。

## CI 门禁

- `tools/audit.cjs`：i18n 键完整性（缺失即 fail）+ A-1 裸色值计数（迁移期信息输出，迁移完成后可升级为门禁）。

## 迁移策略

现有硬编码样式逐模块机械替换为 token；每次改 UI 跑 `node tools/audit.cjs` 观察裸值计数不上升。
