# 客户端界面规范

所有平台（Windows、Linux、macOS、iOS）的界面都遵守 AnixOps Design 的规范。颜色、字体、间距以
[AnixOps/anixops-design](https://github.com/AnixOps/anixops-design) 的 tokens 为准；按钮的层级和摆放以它的
[`guidelines/actions.md`](https://github.com/AnixOps/AnixOps-design/blob/main/guidelines/actions.md)（操作与层级，条文带编号，例如 A1、S3、D1）为准。本页只说明规范里的每一级在客户端里用哪个组件，以及提交界面改动前要自查什么。
规范本身只在 anixops-design 修改，这里不重复条文。

## 一句话原则

一个界面里一眼能看出：哪个是主操作，每个按钮会影响什么范围，哪些操作有破坏性。重要程度不同、作用范围不同的按钮不摆在同一组里。

## 强调级别对应的组件

| 级别 | 含义 | 桌面端（`apps/windows-gui/ui`） | Apple 平台（SwiftUI） |
|---|---|---|---|
| E1 主操作 | 这个界面就是为它存在，每个界面最多一个 | `button.primary` | `.borderedProminent`，概览里是连接开关 |
| E2 次操作 | 主操作之外有用的选择 | `button.secondary`（行内加 `compact`） | `.bordered` |
| E3 轻操作 | 常用但次要：刷新、复制、打开、“全部 ›” | `button.link-button`、`IconButton` | `.link`、`.plain`、`.borderless` |
| E4 破坏性 | 删除数据、断开连接、移除系统组件 | `MoreMenu` 里的 `danger` 项，或 `ConfirmButton` | `Menu` 里的 `Button(role: .destructive)` 加确认 |

即时生效的开关用 `Switch` / `Toggle`，二到四个互斥模式用 `Segmented` / 分段 `Picker`，都不用按钮代替。

## 桌面端的控件和字体

桌面端（Windows、Linux，`apps/windows-gui/ui`）的外观自己画，行为交给 React Aria Components，统一封装在 `src/controls.tsx`：

| 要做的事 | 用哪个组件 | 不要用 |
|---|---|---|
| 从几个值里选一个 | `Select` | 浏览器的 `<select>` |
| 行或分区的“更多”操作 | `MoreMenu` | 自己定位的浮层 |
| 图标按钮和它的悬停说明 | `IconButton`（内含 `Tip`） | `title` 属性 |
| 盖住整个窗口的对话框 | `ModalDialog`，标题用 `<Heading slot="title">` | 自己写 `role="dialog"` 的遮罩 |

这些组件自带方向键、首字母定位、Esc 关闭、焦点圈定与归还、弹层不出窗口。新控件先看 React Aria Components 有没有，再考虑手写。

字体用各平台的系统界面字体（Windows 是 Segoe UI Variable 配微软雅黑 UI，Linux 跟随桌面设置），不加载设计仓库附带的 Inter；
字体栈在 `styles.css` 的 `--font-sans`、`--font-mono`。字号、字重仍以 AnixOps Design 的 tokens 为准。

## 作用范围对应的位置

| 范围 | 影响什么 | 桌面端 | macOS |
|---|---|---|---|
| S1 应用 | 整个客户端：连接、导入、代理模式 | 侧栏底部的连接开关、概览的主卡 | 侧栏底部的连接开关、侧栏工具栏的导入、菜单栏 |
| S2 页面 | 这一页的全部内容：搜索、全部测速 | `PageTools`（标题旁） | 页面自己的 `.toolbar` |
| S3 分区 | 一张卡片或一个列表 | `Card` 的 `actions`、`Panel` 的 `action` | 卡片标题行右侧 |
| S4 行 | 一条记录 | 行尾 | 行尾 |

- 页面工具栏只放这一页的操作。导入、连接这类应用级操作在每一页都待在固定位置，不进页面工具栏。
- 一行最多显示一个带文字的操作和一个图标操作，其余收进“更多”菜单（`MoreMenu` / `Menu`）。
- 菜单里日常项在前，分隔线之后才是破坏性项，红色，放最后。

## 只显示当前适用的操作

按状态决定显示哪个按钮，不把安装、启动、停止并排摆出来。例如一个可安装的组件：未安装时只有“安装”，已停止时只有“启动”，运行中只有“停止”。
正在使用的订阅不再显示“使用”按钮，只显示“使用中”。

## 破坏性操作

- 不和主操作相邻，不做成一组里最显眼的那个。
- 执行前要确认，确认文字写出对象（“确认删除 Example”），确认按钮重复动词，不写“确定”。
- 改动系统的操作（驱动、服务、根证书、系统代理）在卡片说明里写清会改什么。

## 提交前自查

界面改动的 PR 描述里逐条回答：

1. 每个界面（页面、对话框、卡片）里填充色按钮是否最多一个？
2. 同一组按钮是否只相差一级，并且只有一个最强？
3. 每个工具栏、标题行、行尾是否只放同一范围的操作？
4. 有没有哪一行显示了超过一个文字操作加一个图标操作？
5. 破坏性操作是否在菜单里或明显分开，红色，并且按名字确认？
6. 是否只显示了当前状态下适用的操作？
7. 页面标题、分区标题、行内容、提示这四级文字能否一眼分清？

## 截图对比

界面细节靠截图对比把关，不靠人记得去看。

- **桌面端（Windows / Linux 共用的界面）**：`UI screenshots` 工作流用演示数据（`apps/windows-gui/ui/screenshots/mock.js`）逐页画出界面（`render.cjs` 里的清单），在 PR 上把基线提交用同样的方法再画一遍，逐张比对。时钟固定、动效关闭、没有随机数，同一份构建画多少次像素都一样，所以任何差异都是这个 PR 造成的。
- 有差异的截图必须在 PR 描述里确认，否则工作流失败。看过产物 `ui-screenshots-<sha>` 里 `report/<截图名>/` 下的 `before.png`、`after.png`、`diff.png`，确认是想要的变化后，在描述里加一行，改完描述工作流会自己重跑：

  ```
  界面变化：debug-requests-light, debug-inspector-*
  ```

  名字用逗号或空格隔开，`*` 代表任意一段字符。新增和删除的截图同样要列出。确认了却没有变化的名字会在报告里提示。
- 新页面或新状态要在 `render.cjs` 的清单里加一张截图；后端多了页面要用的命令，在 `mock.js` 里补上它的演示数据。
- **macOS**：CI 的 `mac-screenshots-<sha>` 目前只上传截图，不自动比对，改动后要人工核对。
- 比对工具（`.github/scripts/screenshot-diff/diff.cjs`）只认两个目录的 PNG，不管截图怎么来的。macOS、iOS 模拟器和真实 Windows / Linux 窗口在不同缩放下的截图，都按“同样的方法拍两遍，再交给它比”接入。
