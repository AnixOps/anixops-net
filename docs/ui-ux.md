# 客户端界面规范

所有平台（Windows、Linux、macOS、iOS）的界面都遵守 AnixOps Design 的规范。颜色、字体、间距以
[AnixOps/anixops-design](https://github.com/AnixOps/anixops-design) 的 tokens 为准；按钮的层级和摆放以它的
[`guidelines/actions.md`](https://github.com/AnixOps/AnixOps-design/blob/main/guidelines/actions.md)（操作与层级，条文带编号，例如 A1、S3、D1）为准。本页只说明规范里的每一级在客户端里用哪个组件，以及提交界面改动前要自查什么。
规范本身只在 anixops-design 修改，这里不重复条文。
界面文字、文档、代码和测试数据的用词遵守 [`guidelines/terminology.md`](https://github.com/AnixOps/AnixOps-design/blob/main/guidelines/terminology.md)
（中性用语，T1–T8），CI 的 “Neutral terminology” 步骤检查。

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
8. 新加的操作是否能只用键盘完成，并且出现在菜单或命令面板里？（K1、K10）
9. 有没有给实时数据（新行、变化的数字）加动画？（M8）

## 深色模式的层级（Apple 端）

anixops-design 的颜色规则 6 和 layout.md「Elevation」：深色页面是纯黑 `#000`，弹层（sheet、对话框）落在 `bg-elevated` `#1C1C1E` 上，层与层靠亮度区分，不靠更重的阴影。

- 每个 `.sheet` 的内容都要加 `.brandSheet()`（`BrandPalette.swift`）：深色下把 `brand.bg` 换成 `bg-elevated`（`#1C1C1E`），`brand.surface`（卡片、输入框）换成叠在它上面的 `fill-1`（`brand.raised`，合成后约 `#2B2B2E`）；浅色不变。不加的话弹层和纯黑窗口融成一片。系统自己的弹层（分享面板）和全屏覆盖（首次启动、扫码）不加。
- `.brandSheet()` 要写在 `.environment(\.brand, brand)` 之前（离视图更近），否则会被外层的页面配色盖回去。
- 禁用的品牌按钮变成中性：`brand.raised` 底、`brand.text3` 字（label-3 用于禁用文字），不是把强调色调淡。
- macOS 的表单 sheet 高度跟内容走，不要写死高度留下空白。
- 新的弹层在 `mac-screenshots.sh` 里加浅色、深色各一张截图（例如 `import-light`、`import-dark`）。

## 动效、键盘、信息密度

三份规范同样只在 anixops-design 修改，条文带编号：
[`motion.md`](https://github.com/AnixOps/AnixOps-design/blob/main/guidelines/motion.md)（M）、
[`keyboard.md`](https://github.com/AnixOps/AnixOps-design/blob/main/guidelines/keyboard.md)（K）、
[`density.md`](https://github.com/AnixOps/AnixOps-design/blob/main/guidelines/density.md)（N）。这里只写客户端的对应关系。

### 每一页的密度档位（N1）

| 档位 | 页面 |
|---|---|
| 宽松：读和做决定的页面 | 概览、仪表盘、设置、扩展（改写）、出口、导入、首次启动、订阅详情 |
| 紧凑：扫读和比较的页面 | 代理（节点表）、规则、活动（请求、连接、日志）、请求详情里的头和正文 |

手机上紧凑档的行仍然不低于 44 pt，多出来的信息放在第二行（N5）。

macOS 的节点表（`MacNodeRow`）是紧凑表格的样板：行高 28，名称列占剩余宽度并截断、悬停显示全名（N9、N12）；
类型和延迟是定宽列，延迟右对齐、等宽数字、单位写在表头（N10）；没测过显示“—”，测不通显示“超时”“失败”这样的词而不是红色数字
（N13、N14）；延迟旁的色点按 `LatencyGrade` 的阈值（120 ms、250 ms）着色；表头就是排序控件，点一次按该列排序，
再点一次回到订阅顺序，选择会记住（N16）。

### 快捷键（K3、K9）

macOS 的快捷键只有一张表：`apps/ios/Sources/NetworkCoreBridge/AppShortcut.swift`。菜单从这张表绑定，
“键盘快捷键”窗口（⌘/）也从这张表画出来，所以两处不会不一致。新增快捷键先看 `keyboard.md` 第 3 节的保留表，
再加到这张表里；单元测试会拦住重复的和占用系统快捷键的。Windows 和 Linux 的原生客户端用同一批命令，
`⌘` 换成 `Ctrl`，不使用 `Ctrl+Alt`（K22）。

| 命令 | macOS | Windows、Linux |
|---|---|---|
| 命令面板 | `⌘K` | `Ctrl+K` |
| 键盘快捷键 | `⌘/` | `Ctrl+/` |
| 设置 | `⌘,` | `Ctrl+,` |
| 前往侧栏第 1–9 页 | `⌘1` … `⌘9` | `Ctrl+1` … `Ctrl+9` |
| 导入节点 | `⌘I` | `Ctrl+I` |
| 连接或断开 | `⇧⌘C` | `Ctrl+Shift+C` |
| 打开仪表盘 | `⌥⌘D` | 无 |
| 请求：暂停或继续刷新 | `⇧⌘P` | `Ctrl+Shift+P` |
| 请求：显示或隐藏详情 | `⌥⌘I` | `Ctrl+Shift+I` |

### 命令面板（K10–K16）

`⌘K` 打开，盖在主窗口上。里面有：当前模式的每一页、当前状态下适用的应用级命令（连接或断开、导入节点、
切换代理模式、更新全部订阅、打开仪表盘）、所有订阅的节点（选中即切换）。输入名称的一部分或拼音首字母
（“xg” 找到 “香港”）即可；不输入时先列最近用过的五条，再列页面和命令，节点要输入后才出现。
排序规则在 `CommandPalette.swift`，有单元测试。面板里的名称必须和按钮、菜单上的一致（K11）。

### 动效（M8–M10、M15）

- 请求、连接、日志这些实时列表的新行直接出现，数字直接替换，不加 `withAnimation`（M8、M23）。
- 切页、切标签、选中一行都是即时的（M9、M10）。
- 忙碌指示在等待 300 ms 之后才出现，出现后至少停留 400 ms（M15）。

## 入口不变量

截图对比只能发现“画面变了”，发现不了“该有的东西没了”：调试模式去掉概览时，iOS 的连接开关跟着没了，截图照样通过。
下面几条不随导航改动而变，每个平台都用测试守住，改导航时测试会先失败：

1. **连接开关**：代理模式在概览（首页）上；调试模式没有概览，开关在每一页的固定位置（iPhone、Android 是页面底部的连接条，
   iPad 是详情栏底部，macOS、Windows、Linux 是侧栏底部）。同一个位置只有一个。
2. **失败要在当前页说出来**：连接失败、VPN 授权被拒、保存设置失败这类提示，在用户正在看的页面上显示，不能只放在概览里。
   概览有自己的提示卡；其他页面用提示条、Snackbar 或侧栏；调试模式在连接条或侧栏里。
3. **开关不能用时说明原因并给出入口**：没有节点时不只显示一个灰掉的开关，而是写明“先导入”并给出导入入口。

| 平台 | 测试 |
|---|---|
| Windows、Linux | `screenshots/render.cjs` 每张截图都检查侧栏底部有且只有一个连接开关；`labels.test.ts` 检查失败原因和侧栏说明 |
| Android | `apps/android-native/app/src/test/java/io/anixops/net/ui/EntryTest.kt`：每个模式的每个页面检查连接开关和失败提示，没节点时首页给出导入按钮 |
| macOS | `apps/ios/UITests/NetworkCoreMac/MacEntryInvariantTests.swift`（XCUITest，CI 用演示数据运行）：两个模式的每一页检查侧栏底部有且只有一个连接开关（概览上的大开关另有标识 `home-connect-switch`），注入的失败提示在每一页都能看到 |
| iOS | `apps/ios/UITests/NetworkCoreApp/EntryInvariantTests.swift`（XCUITest，CI 的 iOS 模拟器上用演示数据运行）：每个模式的每个标签检查连接开关（`connect-switch`）和失败提示（`app-notice`） |

## 自动检查

UI/UX 问题不靠人截图指出。下面三层按顺序拦截，前两层是 CI 门槛，第三层每天自动跑。查出的现有问题直接修，不加豁免、不加白名单；规范确实不适用时先改 anixops-design 的规范，再改检查。

### 第一层：代码检查（`ui_lint.py`，所有平台）

CI 的 “UI lint” 步骤运行 `.github/scripts/ui_lint.py`（几秒钟，在路径分类 job 里和其他静态检查并行）。界面代码里的颜色只能来自
生成的令牌文件（`design_tokens.py` 从 `apps/ios/Design/tokens.json` 生成，AnixOps Design 已对每一对令牌算过对比度），所以：

| 规则 | 拦什么 | 改成什么 |
|---|---|---|
| U1 颜色字面量 | `#fff`、`rgb()`、`Color(red:)`、`Color.orange`、`.white`、`Color(0xFF…)`、XAML `"#AARRGGBB"` | 对应的令牌：`brand.*` / `Brand.*`、CSS 变量、`BrandTokens.kt`、`BrandTokens.xaml`、GTK `brand-*.css` |
| U2 派生颜色 | 给颜色 `.opacity()`、`color-mix()`、`.copy(alpha =)` | 浅色底用 `soft` / `successSoft` / `warningSoft` / `dangerSoft`，中性底用 `raised`（fill-1）或 `fill-2/3`，次级文字用 `text2` / `text3`，线用 `border` |
| U3 淡化禁用 | 禁用态把控件整体调淡（`opacity`） | 禁用文字 `text3`（label-3），底色 `raised`（fill-1）；规范 actions.md H10 |

常用对照：

- 深色模式里浮在卡片上的东西（按钮、菜单、提示、对话框、Toast）要比所在的面高一级：桌面端用 `--control`（半透明 fill，叠在什么上面就比它亮一级）
  和 `--overlay-surface`（bg-elevated 加一层 fill-1，再加 `--highlight` 一像素亮边）；Apple 端用 `.brandSheet()`。
- 板岩色主卡、小组件、实时活动永远是深色：文字 `Brand.line` / `lineMuted` / `lineFaint`，底 `lineFill` / `lineFill2`，线 `lineRule` / `lineRuleStrong`，
  外边 `lineEdge`，都是深色主题的令牌。
- 侧栏和设置行的彩色图标块用 `tile*`（浅色图表色，白色图形在两种主题下都不低于 3:1），图形颜色 `onTile`。
- 相机画面上的文字用 `Brand.line`，背后衬 `mediaScrim`；二维码底色 `qrPaper`（两种主题都是白）。

需要的颜色令牌里没有时，在 `apps/ios/Design/tokens.json` 里引用 AnixOps Design 已有的令牌加一个名字，再运行 `design_tokens.py` 生成；
AnixOps Design 里也没有，就先在 anixops-design 提 PR。`ui_lint.py --self-test` 是检查本身的测试，改规则时一起改。

### 第一层：iOS / macOS 文案本地化（`apple_strings.py`）

iOS、macOS 的界面文字用简体中文写在 Swift 里，由 SwiftUI / Foundation 到 `apps/ios/Sources/NetworkCoreApp/Resources/Localizable.xcstrings`
（App、Mac App、隧道扩展、小组件共用一份）查英文和繁体中文。工程由 XcodeGen 生成、目录手工维护，构建时不会自动抽取键，所以缺键或
没经过查找的文字会直接以简体中文显示给英文和繁体用户。CI 的 “Apple interface strings are localized” 步骤运行
`.github/scripts/apple_strings.py`，对 `apps/ios/Sources` 下每一个含中文的字符串字面量：

| 规则 | 拦什么 | 改成什么 |
|---|---|---|
| S1 缺键 | 目录里没有这条（插值按任意格式符匹配） | 在目录里加这条，en 和 zh-Hant 都写好；插值用编译器生成的格式符：Int `%lld`、UInt64 `%llu`、UInt32 `%u`、Int32 `%d`、String `%@`，语序变化时译文用 `%1$@` 这类位置参数 |
| S2 未翻译 | 目录里任何一条（用到没用到都算）缺 en / zh-Hant、状态不是 translated，或译文的格式符和键不一致 | 补译文 |
| S3 没查表 | 文字以 String 交给界面：参数声明为 String 的辅助函数、返回 String 的属性、String 状态、`Text(verbatim:)`、`joined(separator: "、")` | 参数和属性改成 `LocalizedStringKey` / `LocalizedStringResource` / `String.LocalizationValue`，或在赋值处写 `String(localized:)`；列表用 `.formatted(.list(type:))` |
| S4 看不出 | 检查跟不到的位置（闭包结果、没写类型的常量） | 写成查表可见的形式：`String(localized:)`、`LocalizedStringKey` 参数或属性 |

不是界面文字的中文（演示数据里的节点名、写进生成的插件文件里的文字、和中文 `TERMS.md` 匹配的正则）列在脚本的 `DATA` 里，
每条写明原因；命令行工具 `anixops` 的终端输出（`docs/macos-cli.md`）和 Siri / 快捷指令短语（属于单独的 AppShortcuts 表）不在检查范围内。
`apple_strings.py --self-test` 是检查本身的测试；`apps/ios/Tests/NetworkCoreBridgeTests/LocalizationCatalogueTests.swift`
另外确认目录每条都有 en、zh-Hant，以及经 String 显示的校验提示都有译文。

### 第二层：运行检查（截图任务里，各平台并行）

用各平台已经构建好的 App 跑无障碍检查，不另起构建，并且和截图分在并行的 job 里，不拉长 CI：桌面端用 axe（对比度、文字溢出截断、弹层与背景分层），
macOS / iOS 用 XCUITest 的 `performAccessibilityAudit`（对比度、文字被截断、动态字体），Android、Windows、Linux 原生端接各自的无障碍检查。
每一页浅色、深色都查。

| 平台 | 入口 | 查什么 |
|---|---|---|
| 桌面端（Windows、Linux 共用的界面） | `UI screenshots` 工作流的 “Draw and check the screenshots”：`screenshots/render.cjs` 每画一张就调 `screenshots/audit.cjs` | axe（WCAG 2.2 A/AA，serious、critical 为 0，含对比度）；文字被截断而没有省略号、文字出了窗口；对话框、菜单、弹出列表、提示、Toast 和下面的窗口分不开（截图上测：表面与外侧 ΔL* ≥ 4，或边缘 ΔL* ≥ 6）。结果和每个弹层的测量值在产物的 `after/audit.json` |
| macOS、iOS | 接入中：XCUITest 的 `performAccessibilityAudit(for: .all)`，两种模式的每一页 × 浅色、深色 × 中文、英文 | 第一次运行列出的问题（对比度、元素说明、点击区域、动态字体、文字截断等）修完后，作为阻断检查接入 iOS、macOS 的 UI 测试 job；不设豁免，只排除系统自带的 Touch Bar |
| Android、Windows 原生、Linux 原生 | 接入中 | |

截图清单要覆盖：每一页的浅色和深色、英文（更长的词会暴露截断）、窄窗口、每种弹层（对话框、菜单、弹出列表）的浅色和深色。
新页面、新对话框加进清单时，检查自动跟上。

### 第三层：每日评审

每天定时由 Claude 读 main 上最新的截图矩阵，按 anixops-design 的规范和本页自查清单逐页评审机器查不出的问题
（文案和功能是否相符、信息层级、按钮层级、空状态），能确定的直接开分支修、开 PR，CI 全绿后合并；需要产品判断的汇总给维护者。
评审发现的同类问题如果能写成规则，就补进第一层或第二层，让它以后在 PR 上就被拦住。

## 截图对比

界面细节靠截图对比把关，不靠人记得去看。

- **桌面端（Windows / Linux 共用的界面）**：`UI screenshots` 工作流用演示数据（`apps/windows-gui/ui/screenshots/mock.js`）逐页画出界面（`render.cjs` 里的清单），在 PR 上把基线提交用同样的方法再画一遍，逐张比对。时钟固定、动效关闭、没有随机数，同一份构建画多少次像素都一样，所以任何差异都是这个 PR 造成的。
- 有差异的截图必须在 PR 描述里确认，否则工作流失败。看过产物 `ui-screenshots-<sha>` 里 `report/<截图名>/` 下的 `before.png`、`after.png`、`diff.png`，确认是想要的变化后，在描述里加一行，改完描述工作流会自己重跑：

  ```
  界面变化：debug-requests-light, debug-inspector-*
  ```

  名字用逗号或空格隔开，`*` 代表任意一段字符。新增和删除的截图同样要列出。确认了却没有变化的名字会在报告里提示。
- 新页面或新状态要在 `render.cjs` 的清单里加一张截图；后端多了页面要用的命令，在 `mock.js` 里补上它的演示数据。
- **macOS**：CI 的 “macOS screenshots, UI tests and accessibility audit” 用 “iOS and macOS Xcode build” 构建好的调试版 App、演示数据逐页截图（`.github/scripts/mac-screenshots.sh`）。main 上每次改到 Apple 端的提交都会把截图存成 `mac-screenshots-<sha>`（保留 30 天）；PR 上拿自己的截图和基线提交往前最近的一份 main 截图逐张比对（`screenshot-compare.sh`），规则和桌面端一样，截图名前面加 `mac-`：

  ```
  界面变化：mac-proxies-*, mac-palette-light
  ```

  前后图和差异图在产物 `mac-screenshot-report-<sha>` 里。改完描述后重跑这个 job 即可，它会重新读描述。演示数据的时钟是固定的（请求时间、在线时长、到期日），同一份代码截多少次像素都一样。找不到 main 的截图（过期，或这台机器截不了图）时只给警告，不让 PR 失败。新页面或新状态在 `mac-screenshots.sh` 里加一行 `shoot`。
- **iOS**：“iOS screenshots” job（和 macOS 的并行）用 `.github/scripts/ios-screenshots.sh` 新建一台 iPhone 模拟器，装上模拟器版 App，用演示数据逐个标签页截图；状态栏固定为 9:41、满电。产物是 `ios-screenshots-<sha>`，比对方式和 macOS 完全一样，截图名前面加 `ios-`（例如 `ios-proxies-light`），报告在 `ios-screenshot-report-<sha>`。runner 换了新机型时所有截图尺寸都会变，报告里是“尺寸变了”，确认一次即可。
- 比对工具（`.github/scripts/screenshot-diff/diff.cjs`）只认两个目录的 PNG，不管截图怎么来的。macOS、iOS 模拟器和真实 Windows / Linux 窗口在不同缩放下的截图，都按“同样的方法拍两遍，再交给它比”接入。
