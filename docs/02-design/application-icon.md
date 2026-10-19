# Tiny Proxy 软件图标

状态：Approved

最后更新：2026-10-14

关联文档：[视觉系统](visual-system.md)、[文档索引](../README.md)、[项目约定](../../AGENTS.md)

## 设计

名称：**中继之门 / Relay Gate**。

以蓝色 T 和白色 P 组成紧凑的门形，中央负空间向右打开，表达请求经过个人网关转发。图形采用粗笔画和克制的圆角，保留小尺寸下的整体轮廓。石墨色底板延续项目的 hashicorp 桌面视觉风格，蓝色呼应现有焦点与链接色。没有使用参考品牌的标志。

2026-10-13 作为设计提案交付；2026-10-14 用户明确要求界面使用本项目已有图标，因此品牌采用这里的“中继之门”素材。本轮不重新设计图案，不以通用网络图标或生图临时标记替代。

设计目标色：底板 `#15181e`、主体 `#ffffff`、蓝色 `#2b89ff`、外缘 `#3b3d45`。实际交付为生成式栅格图，边缘抗锯齿及轻微明暗变化不保证逐像素等于目标色；不是 SVG 矢量源文件。

## 交付文件

| 文件 | 内容与用途 |
|---|---|
| [tiny-proxy.ico](../../assets/icons/tiny-proxy.ico) | Windows 多尺寸图标，9 帧：16、20、24、32、40、48、64、128、256 px，32 位 RGBA，PNG 压缩帧 |
| [tiny-proxy.png](../../assets/icons/tiny-proxy.png) | 1024 × 1024 透明 PNG，用于应用品牌展示及文档 |
| [tiny-proxy-master.png](../../assets/icons/tiny-proxy-master.png) | 1254 × 1254 原始生成母版，保留原始 alpha |
| [png/](../../assets/icons/png/) | 独立 PNG：16、20、24、32、40、48、64、128、256、512 px |

外部留白与圆角区域具有真实透明像素。使用时保持正方形比例，避免拉伸、再次裁切或添加背景方块。底板是图标本身的一部分。蓝色是品牌识别色，不代表服务正在运行；后续运行状态应另加明确文字或独立状态标记。

当前已有 Windows EXE 资源和托盘接入代码；用户实际截图的标题栏显示本项目图标。本轮补充内嵌 PNG 图片源用于设置“关于”等内部品牌位，不依赖素材文件路径。此次未编译或运行，PNG 实际绘制仍待验；下方记录保留为早期素材验证事实。

## 验证记录

2026-10-13 在 Windows 本机使用 .NET System.Drawing 执行格式验证：

- 12 个 PNG 均可解码、为正方形、左上角 alpha 为 0。
- ICO 头与目录包含预期的 9 帧，每帧的数据范围、解码尺寸及透明角均检查通过。
- Windows System.Drawing.Icon 成功载入 ICO 的 32 px 图标。
- 已查看 16、24、32、48、64、128、256 px 在浅色和深色背景下的实际尺寸对照；24 px 以上主体与蓝白分区清晰，16 px 用于轮廓识别，不依赖其辨读完整字母。
- 对照预览保存在本地忽略目录 `artifacts/icon-review.png`，属于可重新生成的检查产物。
- 未验证 Windows Explorer、实际任务栏、程序窗口或托盘的显示；未将文件解码视为已完成系统接入。
- 本次只新增图像资产与说明，没有修改 Rust 代码或依赖，因此未运行 Cargo 测试与构建。

## 生成来源与提示词

使用内置 **image_gen** 工具生成，`transparent_background=true`，未使用 CLI/API fallback。原始生成结果复制到项目后，使用 .NET System.Drawing 等比例缩放导出 PNG，并将 PNG 帧封装为 ICO；未重画生成图案。

以下为实际生成提示词。提示词指定的目标分辨率是 1024 × 1024，工具实际返回 1254 × 1254，因此同时保留母版和 1024 px 标准导出。

```text
Use case: logo-brand
Asset type: finished Windows desktop application icon for Tiny Proxy, a small personal native desktop model API gateway connecting the user's devices and AI agents.
Create ONE polished bespoke app icon, not a presentation sheet. Design concept: "Relay Gate". A bold memorable architectural TP monogram that simultaneously feels like a compact open portal and a signal being relayed through it. The silhouette must be exceptionally simple and recognizable at 24px: one substantial white angular folded gate/ribbon suggesting a P, with a single vivid blue #2b89ff short segment or block completing the routing gesture and subtly evoking the T. Make the geometry original, balanced and optically centered; meaningful negative space and broad consistent strokes, no tiny details. Let the designer resolve the elegant monogram rather than setting plain typed letters.
Style: refined flat vector-like brand mark, engineered precision, confident quiet developer utility, crisp edges, flat solid colors. A graphite #15181e rounded-square app tile with restrained 18% corner radius, a very subtle #3b3d45 edge to separate it on dark desktops. White #ffffff is the main mark; blue #2b89ff is only 15-25% of the mark. Tile occupies about 90% of a square canvas with genuinely transparent exterior and transparent corners. Mark occupies about 62% of full canvas so it has ample internal padding yet reads at small sizes. Front orthographic view, no perspective.
Output a clean high-resolution 1024 by 1024 RGBA icon suitable for production PNG and ICO export. No words or lettering outside the custom monogram, no caption, no watermark. No glow, no gradient, no drop shadow, no 3D bevel, no glossy material, no neural network nodes, no circuit traces, no shields, no cloud, no sparkles, no robot, no arrows scattered around the icon, no existing company logos, no mockup environment. Genuine transparent pixels outside the tile; do not draw a checkerboard.
```

