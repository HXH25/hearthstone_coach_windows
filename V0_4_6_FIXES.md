# HearthCoach V0.4.6 — Trinket Names + Rendering/Input Correctness

本版针对 V0.4.5 实战截图修复四个问题：

1. **饰品规划显示真实候选名称**：ChoiceUpdated 记录实际饰品候选的 CardId/名称/文字；候选稳定后 DeepSeek 只在真实候选内排序，Overlay 直接显示“分数 + 饰品名 + 作用/理由”。模型不可用时仍显示全部实际名称。
2. **顶部卡图错误/拉伸**：不再递归扫描整个 HDT 目录；只搜索显式 card-art 目录，并拒绝极窄/极宽 sprite。绘制时保持纵横比并中心裁切。找不到可信图片时退回文字卡。
3. **右下角横向截断**：动作、路线、错误、饰品理由改为按 GDI 实际像素宽度换行，不再用固定字符数粗暴截断。
4. **AI 对话输入被刷新覆盖**：聊天父窗口只在聊天历史/状态/尺寸发生变化时重绘；原生 EDIT 子控件最后显示，因此持续输入不会被 Overlay 120ms 刷新覆盖。

验证：`powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_4_6.ps1`。
