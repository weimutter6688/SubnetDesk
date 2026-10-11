# SubnetDesk 上游更新操作手册

适用于本仓库：<https://github.com/weimutter6688/SubnetDesk>。命令默认在 Windows PowerShell 中、项目根目录 `D:\code\SubnetDesk` 执行。GitHub 网页负责查看报告、审查 PR 和构建安装包。

推荐流程：**查看上游更新 → 在独立分支移植所需修复 → 审查并测试 → 合入主分支 → 使用新版本号发布**。每项修复分别提交，保留上游来源。

## 1. 先分清更新来源

| 来源 | 处理方式 |
| --- | --- |
| RustDesk 官方主仓 | 优先审查安全、认证、屏幕捕获、输入、文件传输和连接修复，再按提交移植。公网协调、中继和云账号相关功能需要判断是否适用。 |
| 原版 SubnetDesk | 检查你最初 fork 的仓库中的 LAN 功能和缺陷修复；添加 remote 前确认其地址，避免填成自己的仓库。 |
| `hbb_common` 共同库 | 已纳入本仓库，按源码移植；不要恢复成子模块或直接覆盖整个目录。 |

RustDesk 分叉点为 `5abf4e9724dc9a8c9b45dfe13e71af11616eb2c8`，详情见[上游基线](upstream-baseline.md)。这是历史分叉点，不代表后续安全修复已全部合入。JSON 中的 `upstream_head_observed_*` 只记录曾观察到的上游提交。

## 2. 查看每周上游报告

1. 打开 [Actions → Upstream security review queue](https://github.com/weimutter6688/SubnetDesk/actions/workflows/upstream-security.yml)。
2. 查看最近一次成功运行；需要立即检查时，点击 **Run workflow**，选择 `master`。
3. 在该次运行底部的 **Artifacts** 下载 `upstream-security-review`。
4. 打开里面的 Markdown 文件，审查安全候选提交、完整返回列表和公开安全公告。

该工作流每周一运行，目前只跟踪 **RustDesk 官方主仓**。它只生成报告，不自动合并代码，也不会更新已安装的软件。关键词和日期筛选可能漏掉修复；共同库、原版 SubnetDesk 和第三方依赖需要另行检查。API 失败或公告不可用不能当作“没有漏洞”。

也可以本地生成报告，需要 Python 3 和 GitHub 网络访问：

```powershell
Set-Location D:\code\SubnetDesk
python scripts/check_upstream_security.py --output build/upstream-review/security-review.md
```

无需在命令中粘贴 Token。遇到 GitHub API 限流时，可以使用 GitHub Actions 生成报告。

## 3. 添加上游 remote（首次执行）

先确认当前 remote：

```powershell
git remote -v
```

`origin` 应指向 `git@github.com:weimutter6688/SubnetDesk.git`，保留它用于推送自己的修改。另加 RustDesk 官方 remote：

```powershell
git remote add rustdesk-upstream https://github.com/rustdesk/rustdesk.git
git fetch rustdesk-upstream
```

如果提示 remote 已存在，先用 `git remote get-url rustdesk-upstream` 核对地址，确认正确后直接 `fetch`。

需要跟踪原版 SubnetDesk 时，再执行以下命令，并按提示输入原仓库的 Git URL：

```powershell
$subnetdeskUpstreamUrl = Read-Host '输入原版 SubnetDesk 的 Git URL，不要输入自己的 origin'
git remote add subnetdesk-upstream $subnetdeskUpstreamUrl
git fetch subnetdesk-upstream
git remote show subnetdesk-upstream
```

记下最后一条输出中的 **HEAD branch**。后面的示例使用 RustDesk 的 `master`；查看原版 SubnetDesk 时，把 remote 和分支名换成实际值。`fetch` 只下载提交，不会合并到工作分支。

## 4. 创建更新分支和备份

每条命令成功后再执行下一条：

```powershell
git status --short
```

应没有输出。若有未提交修改，先单独提交，或妥善保存后再开始上游更新。

```powershell
git switch master
git pull --ff-only origin master
git fetch rustdesk-upstream

$reviewId = Get-Date -Format 'yyyyMMdd-HHmmss'
$reviewBranch = "maintenance/upstream-$reviewId"
$backupBranch = "backup/before-upstream-$reviewId"
git branch $backupBranch
git switch -c $reviewBranch
```

`$backupBranch` 保留开始更新前的代码位置。若重新打开 PowerShell，需重新设置 `$reviewBranch` 为已创建的分支名。

如需独立保存已提交的 Git 历史，可另建 bundle；它不包括未提交文件、忽略的构建产物或应用配置：

```powershell
New-Item -ItemType Directory -Force build/upstream-review | Out-Null
git bundle create "build/upstream-review/before-$reviewId.bundle" --all
git bundle verify "build/upstream-review/before-$reviewId.bundle"
```

## 5. 挑选需要的提交

列出上游尚未以等价补丁合入的普通提交：

```powershell
git log --cherry-pick --right-only --no-merges --oneline master...rustdesk-upstream/master
```

此列表用于初筛：已被我们改写的修复仍可能出现，合并提交也未列出。需要完整审查时，从已记录的分叉点查看上游历史：

```powershell
git log --reverse --oneline 5abf4e9724dc9a8c9b45dfe13e71af11616eb2c8..rustdesk-upstream/master
```

选中候选提交后输入完整 SHA，先看差异：

```powershell
$fixSha = Read-Host '输入待审查的上游提交完整 SHA'
git show --stat $fixSha
git show $fixSha
```

导入前确认：

- 本 fork 是否使用被修复的代码路径；移除公网中继不等于共享认证、捕获或文件传输代码没有风险。
- 修复是否已由原版 SubnetDesk 或我们自己的提交实现。
- 是否依赖前面的接口、配置或协议修改；依赖提交应先处理。
- 是否涉及共同库 gitlink、依赖版本、生成文件或构建流程；这些可能需要单独移植。

审查完成后，普通提交可以这样导入：

```powershell
git cherry-pick -x $fixSha
```

`-x` 会在提交说明中记录来源 SHA。按依赖顺序逐项导入，不要把未经审查的整个提交范围一次性应用。上游合并提交需要判断实际改动和父提交，不能随意加 `-m 1`。

### 发生冲突时

```powershell
git status
git diff --name-only --diff-filter=U
```

编辑冲突文件，删除冲突标记并保留所需逻辑。不要整批采用“上游版本”或“当前版本”。每解决一个文件，输入其相对路径并暂存；多个文件重复执行：

```powershell
$resolvedPath = Read-Host '输入已解决冲突的文件相对路径'
git add -- $resolvedPath
```

确认冲突列表为空，再检查暂存内容并继续：

```powershell
git diff --name-only --diff-filter=U
git diff --cached
git diff --cached --check
git cherry-pick --continue
```

如决定放弃当前这一次导入：

```powershell
git cherry-pick --abort
```

若 Git 提示提交已为空，先确认修复确实存在，再执行 `git cherry-pick --skip`。这只跳过当前提交，不代表其依赖或整个上游版本已经处理完。

## 6. 处理已纳入主仓的共同库

`libs/hbb_common` 的初始来源是 `https://github.com/zibo-chen/hbb_common.git`，导入提交为 `64aab1c4585a9459c9849f0d0b53af5bbc40eacd`。详情见[共同库来源](common-library-provenance.md)。该目录之后还有本 fork 的修改，不能用上游文件整体替换。

上游主仓提交若只是更新子模块指针，需要进一步读取它使用的共同库 URL 和提交：

```powershell
git show "${fixSha}:.gitmodules"
git ls-tree $fixSha libs/hbb_common
```

模式 `160000` 表示子模块指针，不包含库的源码补丁。按该上游提交的 URL 和共同库 SHA 审查库内差异，将适用修改移植到本仓库的 `libs/hbb_common/`。不要把本仓库的普通目录恢复为 gitlink。

查看原共同库更新时，可以单独克隆到忽略的构建目录（首次执行）：

```powershell
New-Item -ItemType Directory -Force build/upstream-review | Out-Null
git clone https://github.com/zibo-chen/hbb_common.git build/upstream-review/hbb_common
```

已有该克隆时只需 `git -C build/upstream-review/hbb_common fetch origin`。比较一个确定的原共同库提交：

```powershell
$commonSha = Read-Host '输入原共同库中待审查的完整提交 SHA'
git -C build/upstream-review/hbb_common show $commonSha
git -C build/upstream-review/hbb_common diff 64aab1c4585a9459c9849f0d0b53af5bbc40eacd $commonSha -- src protos Cargo.toml
```

如果目标是 RustDesk 官方共同库，应使用目标主仓提交记录的库 URL 和 SHA 另行克隆，不能混用原共同库的上述基线。移植后单独提交，在提交说明或 PR 中记录库来源、SHA、适用范围和测试结果，并按需补充来源文档；保留原始导入记录和许可声明。

## 7. 审查必须保留的行为

| 项目 | 更新后的要求 |
| --- | --- |
| 自动更新与自定义配置信任 | 不恢复原作者更新通道及已移除的自定义配置签名信任入口。 |
| 认证与传输 | 保留 Argon2id LAN 认证、签名握手、方向区分的 nonce 和登录后的加密；不恢复旧单轮 SHA-256 密码路径或协议降级。 |
| 网络范围 | 保留 LAN/VPN 直连、来源白名单，不恢复公网设备 ID、信令、中继或云账号流程。 |
| 发现与 Web | 发现默认关闭；Web 网关默认关闭，默认或未知权限保持 view-only。 |
| 产品身份 | 产品、服务、数据目录、钥匙串和握手命名保持 SubnetDesk，不加入 GitHub 用户名；保留 MSI 升级身份。 |
| 构建输入 | 下载继续固定版本/提交并校验 SHA-256，保留 Actions 固定 revision 和质量门。 |
| 已有连接修复 | 保留回车连接、RDP 会话切换、首次连接和网络恢复方面的修复，并做回归验证。 |

上游新增加的调用即使没有 Git 冲突，也需要检查其运行行为。许可证、作者声明和仍在使用的 C ABI 符号不能靠全局替换 `rustdesk` 来处理。相关依据见[fork 安全复查](fork-security-review.md)和[命名与兼容性](fork-identity.md)。

## 8. 更新生成文件和记录

若修改 Rust/Flutter FFI 接口，按 [bridge.yml](../.github/workflows/bridge.yml) 使用锁定版本生成桥接文件，并保持 `--class-name Subnetdesk`。CI 会重新生成桥接并参与编译；不要手工改生成代码来掩盖接口不匹配。

若修改 protobuf 或 Web 客户端，使用项目锁定的 Bun 版本（当前 `1.3.4`）和已安装的 `protoc`，在项目根目录执行：

```powershell
Push-Location web
bun install --frozen-lockfile
bun run generate:proto
bun run build
bun test
bun run typecheck
Pop-Location
git diff -- web/src/generated web/dist
```

逐条确认成功后，再检查并提交发生变化的生成文件。CI 会重新生成并比较 `web/src/generated` 和 `web/dist`，只修改 `.proto` 而漏交生成产物会失败。协议不兼容时，应明确迁移方案，同时更新控制端、被控端及浏览器页面。

在对应提交或 PR 中记录：上游仓库和 SHA、解决的问题、LAN 模式适用性、依赖提交、冲突处理和验证结果。用户可见变更补充 `CHANGELOG.md`，行为或操作变化同步 README。

保留 `upstream-baseline.json` 的历史分叉点；选择性合入不能把某个最新上游 SHA 写成“已全部集成”。也不要在未完整审查对应时间段前推进 `monitor_since_utc`，否则后续报告可能漏掉未处理内容。

## 9. 推送更新分支并验证

先确认变更范围和工作区状态：

```powershell
git diff --stat master...HEAD
git diff --check master...HEAD
git status --short
git push -u origin $reviewBranch
```

移植、生成文件和文档变更应已分别提交，`git status --short` 应为空。在 GitHub 创建 PR，**base 选择 `master`，compare 选择更新分支**。

检查该 PR 的实际工作流结果：

- **SubnetDesk Preflight**：LAN 残留、命名、下载校验和打包契约。
- **SubnetDesk Rust CI**：Rust 构建/测试、共同库测试、fork 模块格式和 Clippy 门、Web 测试及生成文件检查。
- **SubnetDesk Installers CI**：安装包构建；依触发条件可能不会运行，不能把未运行当作通过。

质量门范围见 [Rust 质量门](rust-quality-gates.md)。不要直接在日常使用机器上运行会修改真实光标、分辨率或持久配置的全部测试；这些需要独立测试环境。

然后打开 [SubnetDesk Release Build](https://github.com/weimutter6688/SubnetDesk/actions/workflows/flutter-tag.yml)，手动构建测试包：

| 参数 | 测试时的值 |
| --- | --- |
| Branch | 当前更新分支 |
| `platform` | `all` |
| `architecture` | `all` |
| `release-tag` | 留空 |
| `publish-release` | **关闭 / false**，默认是开启，必须手动改 |
| `linux-extras` | `false`，除非本次需要验证额外格式 |

这样只生成 Artifacts；确认所有选中的平台成功后，下载测试包。记录运行链接、被测试的提交 SHA 和结果。已有正式版本号可以用于这次不发布的测试构建，正式发行需使用新版本号。

安装测试前备份应用配置和可信设备记录，确认实际运行的进程与服务来自待测试版本。至少完成以下与改动相关的实机检查：

- 正确/错误密码登录、回车连接、白名单允许与拒绝。
- 关闭 RDP 后首次连接、再次连接、锁屏/会话切换，以及连接恢复。
- 屏幕显示、键鼠输入、剪贴板、文件传输和本次修改影响的功能。
- 发现关闭时手动 IP 直连；需要时测试显式开启发现。
- Web 网关关闭状态；需要使用 Web 时测试只读档位和浏览器更新。
- 安装版升级与便携版启动，确认服务、设置和程序身份正确。

全平台构建成功只证明编译和打包通过，实际验证过哪些操作系统、架构和连接场景，应分别记录。分支后续又有修改时，重新运行受影响检查和测试包构建。

## 10. 合入并发布新版本

验证完成后合入 PR。需要保留“每项修复分别提交”时，选择 GitHub 的 **Create a merge commit**，不要将整批提交 Squash 成一个。

注意：本仓库向 `master` 推送会触发 Nightly 构建，成功后会更新 `nightly` 下载；因此应先在更新分支验证，再合入主分支。

同步本地：

```powershell
git switch master
git pull --ff-only origin master
```

然后创建发行分支，调整版本号和发布记录并提交 PR。`Cargo.toml` 的基础版本与 `flutter/pubspec.yaml` 中 `+` 前的版本需一致；每次正式发布增加 Flutter 构建号。例如 `1.3.0+87` 的下一次发布可用 `1.3.0+88`，对应 `v1.3.0.88`。若该版本已发布，继续使用更高构建号。

发行 PR 检查通过并合入后，再同步 `master`，选择以下一种方式发布：

- **网页方式**：在 Release Build 选择 `master`、全平台/全架构，开启 `publish-release`，`release-tag` 留空。确认该提交的版本正确后运行。
- **标签方式**：在版本变更提交上创建与源代码版本一致的新标签，再推送标签，自动触发全平台构建。

以源代码版本确实是 `1.3.0+88` 为例：

```powershell
git tag v1.3.0.88
git push origin v1.3.0.88
```

不要复用或强制移动已公开的正式标签。构建成功后核对 Release 的平台安装包、`SHA256SUMS` 和 `build-manifest.json`，再升级实际使用的设备。完整发行步骤见 [GitHub Release 操作说明](github-releases.md)。

## 11. 放弃更新或回滚

| 状态 | 操作 |
| --- | --- |
| 正在 cherry-pick，想放弃当前提交 | `git cherry-pick --abort`。 |
| 试验分支尚未合入 | 保存有价值的修改，确认工作区干净后 `git switch master`；保留试验分支方便复查。 |
| 单项修复已经合入 | 从最新 `master` 新建回滚分支，对本仓库中的修复提交执行 `git revert`，通过测试和 PR 合入。 |
| 整个更新 PR 用 merge commit 合入 | 确认第一父提交是原 `master` 后，在回滚分支执行 `git revert -m 1`，测试并通过 PR 合入。 |
| 新版程序已部署 | 按已保存的安装包和配置恢复计划处理；代码回滚应使用更高版本号发布修复包。MSI 降级和配置格式回退需要另行验证。 |

单项提交回滚示例，输入的是**本仓库实际合入后的提交 SHA**，不是 cherry-pick 前的上游 SHA：

```powershell
git switch master
git pull --ff-only origin master
$rollbackBranch = 'maintenance/revert-' + (Get-Date -Format 'yyyyMMdd-HHmmss')
git switch -c $rollbackBranch
$localFixSha = Read-Host '输入本仓库中待回滚的修复提交 SHA'
git revert $localFixSha
git push -u origin $rollbackBranch
```

对整批 merge commit 回滚时，先执行 `git show --no-patch --pretty=raw` 查看该提交的父提交，再用正确的主线编号；示例条件成立时使用 `git revert -m 1`。已公开历史用新的回滚提交处理，不用强推改写主分支。重新引入被整批回滚的更新时，需处理此前的 revert，不能假设再次 merge 就会恢复所有内容。

## 12. 如果确实需要整体合并上游

整体合并适用于计划中的大版本迁移，会涉及被移除的功能、共同库目录与 gitlink 冲突、工具链和 UI 接口变化。先完成备份，在独立更新分支、干净工作区执行：

```powershell
git merge --no-ff --no-commit rustdesk-upstream/master
```

检查所有冲突及自动合并的变化，完成共同库移植、功能审查和生成文件更新，再提交、走 PR 和第 9 节的验证。放弃正在进行的 merge 使用 `git merge --abort`。

GitHub 的 **Sync fork** 无法替代这些审查。对于本 fork 的日常维护，优先采用前面的逐项移植流程。
