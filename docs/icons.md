# 应用图标

应用采用现代、简约扁平、蓝色科技风格：一个圆角六边形管理核心连接三个圆形服务节点，代表统一编排多个服务。主图使用蔚蓝 `#0A84FF`、连接蓝 `#3B82F6` 和青色 `#67E8F9`，背景透明，不含文字、阴影或三维效果。

本次使用内置图像生成工具生成概念稿，保存在 `ui/assets/icon-generated.png`。再根据现有项目的矢量图标流程，将概念稿的几何轮廓规范化为 `ui/assets/icon.svg`，去除生成稿的轻微色彩变化，使图标保持准确的纯色和清晰边缘。`ui/assets/icon-small.svg` 是相同轮廓的小尺寸版本，统一为蔚蓝并加粗连接线。

## 生成提示词（中文对应规格）

> 为桌面服务管理器 rpmm 生成一个现代极简扁平应用图标，方形、透明背景。中心为一个较大的蔚蓝圆角六边形，通过三条干净、笔直、圆头的连接线，连接上方、左下方、右下方三个较小的圆形服务节点，构成平衡的中心辐射式树形拓扑，象征一个管理器编排三个服务。使用纯色、清晰几何边缘和充分留白，适合缩小至 16 像素。主色为 `#0A84FF`，连接线为 `#3B82F6`，节点为 `#67E8F9`。仅输出一个独立标志，不含边框、文字、字母、数字、水印、签名、三维、斜面、阴影、网格渐变、写实纹理、复杂细节、光晕、紫色、红色或绿色。

原始生成稿为 1280×1280；规范化的透明 PNG 主图为 `src-tauri/icons/source.png`，尺寸为 1024×1024。

## 资源与用途

| 资源 | 用途 |
| --- | --- |
| `ui/assets/icon.svg` | 界面品牌标志、标准矢量源 |
| `ui/assets/icon-small.svg` | 页签、小尺寸矢量源 |
| `src-tauri/icons/source.png`、`1024x1024.png` | 1024 像素透明 PNG 主图 |
| `src-tauri/icons/icon.ico` | 桌面可执行文件、窗口、安装器、卸载器、独立 CLI |
| `src-tauri/icons/16x16.png`、`24x24.png`、`32x32.png`、`48x48.png` | 单色、加粗的小尺寸图标，其中 32 像素用于托盘 |
| `src-tauri/icons/64x64.png`、`128x128.png`、`256x256.png`、`128x128@2x.png`、`icon.png` | 标准彩色桌面 PNG |
| `src-tauri/icons/icon.icns` | macOS 图标资源 |
| `src-tauri/icons/Square*Logo.png`、`StoreLogo.png` | Windows 商店图标资源 |

ICO 带有 16、24、32、48、64、128、256 像素七帧，每帧保留透明通道；其中 16–48 像素使用单色加粗版，64 像素及以上使用彩色主图。30 和 44 像素商店资源也使用单色版。

## 重新生成

```powershell
npm run icons
npm run tauri build
cargo build -p rpmm --release
```

`scripts/build-icons.mjs` 使用项目现有 Tauri CLI 转换 SVG，再将七档 PNG 封装为 ICO，不新增图像处理依赖。中间文件保存在 `temp/icon-build/`。桌面构建沿用 `tauri-build` 的图标嵌入流程；NSIS 安装器和卸载器均显式指定 `icons/icon.ico`；独立 CLI 通过 `resources/rpmm.rc` 和 `embed-resource` 注入相同图标。

该项目管理当前用户会话中的子进程，不注册 SCM 服务；图标覆盖可执行文件、窗口、托盘、页签及安装包。
