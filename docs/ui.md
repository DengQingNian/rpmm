# 界面说明

界面按 Material Design 3（Material You）重写，只使用本仓库内的组件与 CSS 令牌，不依赖第三方组件库。

## 设计令牌

- `ui/tokens.css`：颜色、字体、形状、海拔、动效与 8dp 间距令牌。颜色取自以深松绿为种子的色调色板，浅色与深色两套方案由 `<html data-theme="light|dark">` 切换。
- `ui/style.css`：基础元素、布局骨架与各页面的组件样式，全部引用语义令牌，不写死颜色。
- `ui/theme.ts`：跟随系统 / 浅色 / 深色三种模式，偏好保存在 `localStorage` 的 `rpmm.theme`。

## 组件

通用组件位于 `ui/components`，统一以 `Md` 前缀命名：

| 组件 | 用途 |
| --- | --- |
| `MdButton` / `MdIconButton` | 填充、色调、描边、文本、海拔五种按钮与 40dp 图标按钮 |
| `MdTextField` | 轮廓文本框，支持标签缺口、前后缀图标、清除、多行与错误态 |
| `MdSelect` | 暴露式下拉菜单，支持键盘上下移动、回车选择与 Esc 关闭 |
| `MdSwitch` / `MdCheckbox` / `MdChipSet` | 开关、复选框与多选过滤标签 |
| `MdTabs` | 带滑动指示器的主标签，支持左右方向键切换 |
| `MdDialog` / `FeedbackHost` | 基础对话框、确认对话与底部提示（snackbar） |
| `MdAlert` / `MdBadge` / `MdEmpty` | 提示条、状态徽章与空状态 |
| `MdGauge` / `MdLinearProgress` | 环形与线性进度指示 |
| `StatCard` / `MetricCard` | 概览统计与指标卡片 |

页面组件（`DesktopShell`、`DashboardPage`、`MonitorPage`、`ConfigPage`、`SettingsPage` 及各详情面板）只负责业务排版。

## 布局

- 展开式导航抽屉在 ≥840px 常驻，可收起为 96px 图标导航栏；更窄时改为顶部菜单按钮唤出的模态抽屉。
- 顶栏为小标题应用栏，聚合同步时间、主题切换、重载与新建入口。
- 内容区最大宽度 1440px，面板之间使用 16/24dp 间距；表格、日志与配置正文使用等宽字体。
- 窗口尺寸分类断点：1440 / 1240 / 1120 / 960 / 840 / 600px。

## 无障碍与动效

- 所有交互元素保留 40dp 以上触控区与可见焦点环；图标按钮必须提供无障碍名称。
- 动效使用 Material 动效令牌（emphasized / standard 缓动），并遵循 `prefers-reduced-motion`。
