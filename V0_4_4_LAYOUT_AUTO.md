# HearthCoach V0.4.4 — Auto Guide + Movable Decision Panel

## 1. 阵容指南自动切换阶段

顶部横向阵容指南不再提供手动“前期 / 中期 / 后期”覆盖。

它始终直接消费 `PublicState.current_stage`：

```text
Round 1-4  -> early / 前期
Round 5-8  -> mid   / 中期
Round 9+   -> late  / 后期
```

进入新的阶段后，下一次 Overlay 刷新就会自动换成对应 Watchlist。顶部会显示：

```text
自动阶段 · 前期/中期/后期
```

## 2. HearthCoach 决策框自由拖动

按住右下角决策框最上方约 30 px 的标题栏拖动即可移动。

位置会以 Hearthstone client rect 的归一化比例保存到 `hearthcoach_demo.json`：

```json
"panel_x_ratio": 0.62,
"panel_y_ratio": 0.54
```

因此下一次启动仍保持上一次位置，并能随分辨率变化按比例恢复。

## 3. 自由缩放

拖动决策框右下角 `↘` 区域即可改变宽高。

宽高仍写回：

```json
"panel_width_ratio": 0.235,
"panel_height_ratio": 0.30
```

为避免框被缩到完全不可操作，保留最小可读尺寸；最大尺寸限制在 Hearthstone client 内。

## 4. 一键复位

标题栏右上角新增 `复位`。点击后恢复 V0.4.3 的紧凑右下角默认位置和尺寸，并清空自定义 `panel_x_ratio/panel_y_ratio`。

## Config migration

旧 V0.4.3 配置无需修改。缺失：

```json
"panel_x_ratio": null,
"panel_y_ratio": null
```

时继续使用原来的右下角 anchor。第一次拖动后才写入自定义位置。
