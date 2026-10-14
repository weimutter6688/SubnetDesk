<p align="center">
  <img src="res/subnetdesk-icon.svg" alt="SubnetDesk 图标" width="120">
</p>

<h1 align="center">SubnetDesk</h1>

<p align="center">
  <strong>面向局域网的远程桌面：连接直接、部署简单、数据路径可控。</strong>
</p>

<p align="center">
  基于 <a href="https://github.com/rustdesk/rustdesk">RustDesk</a> 独立维护，适用于局域网、可路由私网与 VPN。
</p>

<p align="center">
  <a href="https://github.com/weimutter6688/SubnetDesk/releases/latest"><img src="https://img.shields.io/github/v/release/weimutter6688/SubnetDesk?style=flat-square&logo=github&color=2563eb" alt="最新版本"></a>
  <a href="https://github.com/weimutter6688/SubnetDesk/releases"><img src="https://img.shields.io/github/downloads/weimutter6688/SubnetDesk/total?style=flat-square&logo=github&color=0ea5e9" alt="累计下载量"></a>
  <a href="https://github.com/weimutter6688/SubnetDesk/stargazers"><img src="https://img.shields.io/github/stars/weimutter6688/SubnetDesk?style=flat-square&logo=github&color=f59e0b" alt="GitHub Stars"></a>
  <a href="https://github.com/weimutter6688/SubnetDesk/network/members"><img src="https://img.shields.io/github/forks/weimutter6688/SubnetDesk?style=flat-square&logo=github" alt="GitHub Forks"></a>
  <a href="https://github.com/weimutter6688/SubnetDesk/actions/workflows/subnetdesk-preflight.yml"><img src="https://img.shields.io/github/actions/workflow/status/weimutter6688/SubnetDesk/subnetdesk-preflight.yml?branch=master&style=flat-square&label=build" alt="构建状态"></a>
  <a href="LICENCE"><img src="https://img.shields.io/github/license/weimutter6688/SubnetDesk?style=flat-square&color=16a34a" alt="开源许可"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-x64%20%7C%20ARM64-0078D6?style=flat-square&logo=windows11&logoColor=white" alt="Windows">
  <img src="https://img.shields.io/badge/macOS-Intel%20%7C%20Apple%20Silicon-111111?style=flat-square&logo=apple&logoColor=white" alt="macOS">
  <img src="https://img.shields.io/badge/Linux-x86__64%20%7C%20ARM64-FCC624?style=flat-square&logo=linux&logoColor=111111" alt="Linux">
  <img src="https://img.shields.io/badge/Android-APK-3DDC84?style=flat-square&logo=android&logoColor=white" alt="Android">
</p>

<p align="center">
  <a href="README.md">English</a> · 简体中文
</p>

<p align="center">
  <a href="#why-subnetdesk">项目定位</a> ·
  <a href="#features">主要功能</a> ·
  <a href="#download">下载安装</a> ·
  <a href="#quick-start">快速开始</a> ·
  <a href="#build-from-source">源码构建</a>
</p>

<p align="center">
  <img src="assets/screenshots/device-discovery.png" alt="SubnetDesk 局域网设备发现" width="880">
</p>

<a id="why-subnetdesk"></a>

## 💡 为什么选择 SubnetDesk

SubnetDesk 专注于一个明确场景：让本身已经网络互通的设备直接进行远程控制。项目移除了 RustDesk 中的公网设备 ID、信令、中继、云账号、代理和公网自动更新等路径，改为直接连接目标地址，并提供可选的局域网设备发现（默认关闭）。

它适合家庭实验室、办公室、教室、现场运维，以及通过 WireGuard、Tailscale、OpenVPN 等 VPN 互通的私有网络。

| | SubnetDesk | 典型 RustDesk 部署 |
| --- | --- | --- |
| 连接方式 | 直接连接 IP 地址或主机名 | 通过设备 ID 和信令服务连接，可使用中继 |
| 设备发现 | 可选 mDNS，默认关闭 | 设备 ID 或地址簿 |
| 服务依赖 | 无需协调服务器 | 设备 ID 连接通常需要公网或自建信令/中继服务 |
| 适用场景 | 局域网、可路由私网、VPN | 互联网和跨 NAT 访问 |

> [!IMPORTANT]
> SubnetDesk 不提供公网信令或中继服务。两台设备必须位于同一局域网、可路由的私有网络，或通过 VPN 实现网络互通。

<a id="features"></a>

## ✨ 主要功能

| | 功能 | 说明 |
| --- | --- | --- |
| 🔎 | 局域网发现 | 在可信网络手动开启 mDNS；关闭时仅手动输入地址直连。 |
| ⚡ | 快捷连接 | 在桌面端地址栏输入 IP 地址或主机名，按回车连接，TCP 端口可配置。 |
| 🔐 | 访问认证 | 使用用户名和密码保护访问，密码以 Argon2id 哈希保存。 |
| 🔑 | 复用账户 | 保存账户配置，设置默认账户，或为设备绑定专用账户。 |
| 🛡️ | 设备验证 | 首次连接时核对并保存设备指纹，降低误连风险。 |
| 🌐 | 网络白名单 | 通过 CIDR 网段限制允许接入的来源网络。 |
| ⭐ | 最近设备与收藏 | 成功连接的设备重启后仍保留，不依赖记住密码；显示设备名称与地址，支持搜索和排序。 |
| 🎚️ | 连接模式 | 可选择低延迟、文字清晰或画质优先，自适应调整码率和帧率，并在支持的环境中缩放传输分辨率。 |
| 🔄 | 自动重连 | 已建立会话遇到临时网络故障时自动重试，仍执行设备身份验证。 |
| 🩺 | 诊断导出 | 在设置中导出最近日志和版本信息，便于排查问题。 |
| 🖥️ | RustDesk 体验 | 保留远程控制、剪贴板、音频和文件传输等能力，无需公网协调服务器。 |

<a id="download"></a>

## 📦 下载安装

稳定版本请前往 **[GitHub Releases](https://github.com/weimutter6688/SubnetDesk/releases/latest)** 下载。希望体验最新改动时，可以使用 **[Nightly 版本](https://github.com/weimutter6688/SubnetDesk/releases/tag/nightly)** 或 **[GitHub Actions](https://github.com/weimutter6688/SubnetDesk/actions)** 中的构建产物。

| 平台 | 架构 | 安装包 |
| --- | --- | --- |
| Windows | x86_64、ARM64 | `.msi` 安装包（推荐优先使用）、免安装/便携版 `.exe` |
| macOS | Intel、Apple Silicon | `.dmg` |
| Linux | x86_64、ARM64 | `.deb`、`.rpm`、`.AppImage`、`.flatpak`；另提供 x86_64 Arch 包 |
| Android | ARM64、ARMv7、x86_64、通用包 | `.apk` |

> [!NOTE]
> Nightly 属于开发构建，日常使用建议优先选择最新稳定版本。

Windows 桌面发行包包含便携版 `.zip`。请完整解压后运行 `SubnetDesk.exe`，并保留同目录中的依赖库和数据文件。

[Release Build 工作流](https://github.com/weimutter6688/SubnetDesk/actions/workflows/flutter-tag.yml) 无需购买签名证书即可构建 Windows、macOS、Linux 的 x64 和 ARM64 包。可以手动选择平台和架构，也可以推送版本标签；所有选中的构建通过后统一发布，并附带 `SHA256SUMS` 和构建清单。Android 和额外的 Linux 格式不属于默认桌面构建。版本号、未签名包及操作步骤见 [GitHub Release 构建说明](docs/github-releases.md)。

<a id="quick-start"></a>

## 🚀 快速开始

1. 在两台设备上安装并打开 SubnetDesk。
2. 未配置过访问账号的被控端会初始化默认用户名 `subnetrust`、密码 `subnet@local`，默认监听端口为 `21118`。请在 **LAN 设置** 中修改账号密码。发现功能默认关闭，需要广播和扫描时再手动开启。
3. 在控制端选择已发现的设备，或在桌面端地址栏输入主机名/IP 地址（可附带 `:端口`），按 **回车** 发起连接。
4. 首次连接时核对设备指纹。出现登录提示后，选择已保存的账户，或输入被控端的用户名和密码，再按 **回车** 连接。使用 **Tab** 切换用户名和密码输入框后，回车同样可以提交。

默认凭据由所有安装副本共用，请在 LAN 设置中改为自己的密码。已有账号密码会保留；修改或主动清除凭据后，重启不会恢复默认值。这套凭据与操作系统登录账号无关。

<p align="center">
  <img src="assets/screenshots/lan-settings.png" alt="SubnetDesk LAN 设置" width="880">
</p>

### 账户与最近设备

在桌面端点击 **账户** 可管理复用的账户配置。连接时可选择账户，并记住它与目标设备的绑定关系。凭据使用顺序为：本次明确选择的账户、设备绑定账户、该设备记住的密码，最后才是默认账户。

成功连接后，设备会进入最近连接列表，不依赖 **记住密码**，重启应用后仍保留。记录按已验证的设备指纹识别，地址变化时仍对应同一设备；删除后不会重新出现，直到再次成功连接。常用设备可加入收藏，列表支持搜索、排序以及网格/列表视图切换。

### 网络检查清单

- 两台设备之间路由可达。
- 主机防火墙已允许 TCP `21118` 端口，或你自行配置的端口。
- 自动发现使用本地网段内的 mDNS 和 UDP 广播。两端开启发现后，最近连接和收藏中的已知 IPv4 设备也会通过 UDP `21119` 单播探测，可跨网段更新状态。关闭发现仍可手动输入地址直连。
- 被控端的 CIDR 白名单包含控制端所在网段。
- Linux 用户可查看[主机环境说明](docs/linux-host-readiness.md)，了解 SELinux、Wayland 与登录界面的注意事项。

## 🔒 安全模型

- 数据沿设备之间的可达网络路径传输，不依赖公网协调服务。
- 密码使用 Argon2id 哈希保存，不以明文持久化。
- 设备指纹采用首次使用信任机制，可帮助识别目标设备身份的异常变化。
- CIDR 白名单可限制被控端接受连接的来源网络。

当前传输使用 LAN 协议 **v3**，握手签名包含本 fork 的独立前缀，按方向区分
加密 nonce，端口转发和 RDP 隧道在登录后仍保持加密。**控制端与被控端必须一起
更新**；旧 v1/v2 连接会被拒绝，不会降级。更新主机后请重新加载浏览器页面。

程序和内部标识统一使用 SubnetDesk，不包含 GitHub 用户名。Windows 程序为
`SubnetDesk.exe`，服务、安装和配置目录也使用 SubnetDesk，与 RustDesk 隔离。
安装版沿用 SubnetDesk 的升级标识，可升级已有 SubnetDesk 安装。钥匙串改用产品
命名空间，首次连接请核对设备指纹；控制端与被控端都应更新到 .85，浏览器重新加载。
.84 临时命名版本的配置不会自动迁移。详情见[命名与兼容性](docs/fork-identity.md)。

发现功能默认关闭；关闭时不广播设备信息，也不扫描，只保留手动地址连接。
开启后会向本地网络发布设备名、系统和指纹。Web 网关仍默认关闭，默认或未知的
权限档位按只读处理。在共享网络启用访问前，请修改文档中的出厂账号密码。

旧单轮 SHA-256 固定密码认证和原作者自定义配置签名信任已移除。完整清单与范围见
[fork 安全复查](docs/fork-security-review.md)、[构建输入校验](docs/build-inputs.md)、
[Rust 质量门](docs/rust-quality-gates.md)和[上游安全跟踪](docs/upstream-baseline.md)。

合入 RustDesk 或原版 SubnetDesk 的更新，请按[上游更新操作手册](docs/upstream-update-guide.md)
建立独立分支、逐项移植修复并验证测试包，再发布新版本。

与其他远程控制软件一样，请只向可信网络开放监听端口，使用高强度且不重复的密码，并尽可能通过另一条可信渠道核对设备指纹。


### 网段规则与连接质量

LAN 设置会显示默认允许的来源网段：`10.0.0.0/8`、`172.16.0.0/12`、
`192.168.0.0/16`、Tailscale 的 `100.64.0.0/10`、IPv6 的 `fc00::/7`
（包含 Tailscale IPv6），以及回环和链路本地网段。每行一个 CIDR，可添加、修改
或删除，也支持逗号分隔。已有自定义网段会保留。清空并保存后拒绝新的原生客户端
连接，重启不会恢复默认网段。Web 访问未单独配置白名单时继承此列表。
这些规则按来源 IP 判断，不会自动识别某条连接是否经过 VPN 隧道。

连接已知设备时，若首个候选端点仍在等待，**80 毫秒**后即可尝试备用端点，
最多同时尝试两个端点，并核验设备指纹。每个端点的 DNS 查询和 TCP 连接共用
一个超时预算，交替尝试 IPv4/IPv6 地址，在 80 毫秒后启动备用 IP，最多同时
尝试两个解析地址。两层并发限制合计最多四个 TCP 尝试。

### 连接模式与异常恢复

远控工具栏中的 **连接模式** 会按设备保存：

- **低延迟（默认）**：优先降低码率，持续拥堵时降低传输分辨率，网络恢复后逐步提升画质。
- **文字清晰**：保留原始分辨率，帧率上限为 30。
- **画质优先**：保留原始分辨率，帧率上限为 45，并保留更高的码率下限。

画面以 30 帧和较低码率启动，用户选择的画质和自定义 FPS 仍是上限。稳定的 VPN
往返延迟与排队延迟分开处理。自适应缩放不改变桌面几何尺寸和鼠标坐标；控制端与
被控端都需更新，才能完整启用传输尺寸变化与解码恢复。缩放还需要受支持的桌面
渲染器，录像期间保持原始尺寸。

各观看端使用独立编码器和码率/帧率反馈；慢端恢复发送后，即使桌面静止，也会补发
最新画面。像素编码在独立且数量受限的工作线程中执行，发送视频或大剪贴板消息时
仍继续接收键鼠输入。共享 CPU/GPU 饱和、严重网络拥堵仍会限制性能。详细行为与
验证方法见[弱网连接说明](docs/weak-network-tuning.md)和[视频性能修改说明](docs/performance-fixes.md)。

已建立会话遇到临时网络故障后，按 1、2、4、8 秒间隔重试，最长约两分钟。
关闭窗口或手动取消会停止重试；认证、权限、设备指纹错误及主动断开不会触发自动重试。

Windows 安装版中，以系统权限运行且后台服务仍在运行的被控进程，在视频工作线程
连续 30 秒无进展后会由服务重新启动。正常空闲不触发恢复，从休眠恢复后重新给予
宽限期。恢复会中断现有会话；便携模式和界面进程不执行此自动重启。详见
[Windows 空闲后重连恢复说明](docs/idle-reconnect-recovery.md)。

关闭 RDP 后，Windows 可能先进入登录会话，再恢复原来的用户桌面。服务现在会等待
有效会话，避免因暂时无效的会话编号先关闭被控进程。真实会话切换仍可能短暂中断
连接，已建立会话的传输故障由客户端自动重试。日志依据与验证边界见
[Windows 会话切换说明](docs/windows-session-disconnect.md)。

应用已移除 GitHub 更新检查、下载、安装和在线帮助跳转。请通过局域网、VPN
或离线介质分发新版本。构建依赖和仓库文档中的 GitHub 来源地址仍保留。
更新时保留现有配置，具体步骤见[手动更新说明](docs/automatic-updates.md)。

<a id="build-from-source"></a>

## 🛠️ 从源码构建

拉取仓库后执行 Flutter 桌面端构建。共同库已纳入主仓，无需单独拉取子模块，
版本与来源见[共同库记录](docs/common-library-provenance.md)：

```bash
git clone https://github.com/weimutter6688/SubnetDesk.git
cd SubnetDesk
./build.py --flutter --hwcodec
```

构建需要 Rust、Flutter 和对应平台的原生依赖。固定工具版本、目标架构和完整打包步骤以[原生安装包工作流](.github/workflows/flutter-build.yml)为准。

### Linux 构建依赖（Debian/Ubuntu）

Rust 核心需要以下系统包。`libsodium-sys` 会从源码编译 libsodium，所以即使只运行
`cargo test -p hbb_common` 也需要 autotools：

```bash
sudo apt-get install -y build-essential autoconf automake libtool pkg-config clang cmake nasm \
  libgtk-3-dev libxcb-randr0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxdo-dev libxfixes-dev \
  libasound2-dev libpulse-dev libpam0g-dev libva-dev libvdpau-dev libunwind-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libvpx-dev libaom-dev libopus-dev libyuv-dev
```

不使用 vcpkg 时，通过 pkg-config 使用系统编解码库。部分发行版（如 Debian）的 `libyuv-dev`
不带 `libyuv.pc`，构建提示找不到 `libyuv` 时请自行补一个并加入 `PKG_CONFIG_PATH`。
启用 `flutter` 特性还需要 flutter_rust_bridge 生成的 `src/bridge_generated.rs`，
由[桥接工作流](.github/workflows/bridge.yml)生成。

```bash
cargo test --locked -p hbb_common --lib -- --test-threads=1
cargo check --locked --lib --features flutter,linux-pkg-config   # 先生成桥接代码
```

## 🤝 参与贡献

欢迎提交缺陷报告、功能建议和 Pull Request。开始修改前建议先查看现有 [Issues](https://github.com/weimutter6688/SubnetDesk/issues)，并尽量让改动保持聚焦，符合项目面向局域网与 VPN 的产品定位。

## 💬 交流社区

<p align="center">
  <a href="https://discord.gg/MFZG478wPQ">
    <img src="https://img.shields.io/badge/Discord-加入交流社区-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="加入 SubnetDesk Discord 交流社区">
  </a>
</p>

<p align="center">
  欢迎加入 <a href="https://discord.gg/MFZG478wPQ">SubnetDesk Discord 交流社区</a>。
</p>

## 🙏 致谢与许可

SubnetDesk 基于 [RustDesk](https://github.com/rustdesk/rustdesk) 开发并保留其开源基础。本项目独立维护，并非 RustDesk 官方发行版。

项目采用 [GNU Affero General Public License v3.0](LICENCE) 许可。请仅在自己拥有或已获授权的设备上使用远程控制功能。
