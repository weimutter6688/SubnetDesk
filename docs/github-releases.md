# 在 GitHub 构建 SubnetDesk Release

本仓库的 `SubnetDesk Release Build` 默认构建桌面版，无需购买代码签名证书或配置签名 Secrets。产品名、可执行文件和安装目录均使用 SubnetDesk。

| 平台 | 架构 | 默认产物 |
| --- | --- | --- |
| Windows | x86_64、ARM64 | MSI 安装包、便携 EXE、便携 ZIP |
| macOS | Intel、Apple Silicon | DMG，应用使用 ad-hoc 临时签名 |
| Linux | x86_64、ARM64 | DEB、RPM、SUSE RPM |

Windows 包不带 Authenticode 签名，可能显示未知发布者或 SmartScreen 提示。macOS 临时签名不代表 Apple 开发者认证，也没有公证；首次打开可能需要在系统设置的“隐私与安全性”中允许。Linux 包不要求商业证书。

默认 Linux 桌面包在 Ubuntu 22.04 容器中构建，目标为 glibc 2.35 或更新的兼容发行版；不承诺在 Ubuntu 18.04/20.04 运行。构建容器与 vcpkg 宿主使用同一发行版，避免跨 glibc 版本链接。

Android 不在此默认桌面发布流程中，保留独立构建代码；APK 安装和后续覆盖升级需要固定的自签名 keystore。iOS 没有加入此发行流程，设备安装通常需要 Apple 签名和 provisioning profile。

## 手动构建

1. 打开 [Actions → SubnetDesk Release Build](https://github.com/weimutter6688/SubnetDesk/actions/workflows/flutter-tag.yml)。
2. 点击 **Run workflow**，选择要构建的分支，一般使用 `master`。
3. `platform` 选择 `all`、`windows`、`macos` 或 `linux`；`architecture` 选择 `all`、`x86_64` 或 `aarch64`。
4. `release-tag` 留空即可按源代码生成标签。`publish-release` 默认开启；关闭后只生成 Actions Artifacts。
5. `linux-extras` 默认关闭；打开后额外构建 Arch（仅 x86_64）、AppImage 和 Flatpak，会增加构建时间。

所有选中的平台成功后，工作流检查安装包是否齐全、是否为空和是否重名，再生成 `SHA256SUMS` 和 `build-manifest.json`。版本 Release 先创建草稿，全部文件上传成功后才公开。发布失败的草稿可以从同一提交重新运行构建；已公开的版本不会被重新覆盖。

每个平台的安装包和汇总的 `release-packages-<版本>` 都保留在该次运行的 Artifacts 中，即使关闭 Release 发布也能下载。普通 GitHub 登录账户可以查看完整日志和下载 Artifacts；公开 Release 的安装包可以直接下载。

## 版本号和标签

`Cargo.toml` 的基础版本必须与 `flutter/pubspec.yaml` 中 `+` 前的版本相同。Flutter 的构建号用于 Windows 安装修订号以及所有平台的包名。

例如：

```toml
# Cargo.toml
version = "1.3.0"
```

```yaml
# flutter/pubspec.yaml
version: 1.3.0+86
```

对应包版本为 `1.3.0.86`，默认 Release 标签为 `v1.3.0.86`。这是安装包版本；Rust 原生基础版本仍为 `1.3.0`。

下次发行可以把 Flutter 构建号改为 `87`，提交并推送后再运行工作流，默认生成 `v1.3.0.87`。不要复用已经公开的版本标签。Windows 修订号必须在 1–65535 范围内。

也可以通过推送标签触发全平台、全架构构建：

```bash
git tag v1.3.0.86
git push origin v1.3.0.86
```

请在版本变更提交上创建标签。手填标签必须与源代码版本一致；已有标签必须指向当前构建的提交。兼容三段版本标签和 `v1.3.0-86` 格式，但建议始终使用完整的四段版本标签。

## 下载与校验

成功的正式构建发布到 [GitHub Releases](https://github.com/weimutter6688/SubnetDesk/releases)，发行说明包含各安装包的下载链接。

Windows：

```powershell
Get-FileHash .\subnetdesk-1.3.0.86-x86_64.msi -Algorithm SHA256
```

Linux：

```bash
sha256sum --check SHA256SUMS
```

macOS：

```bash
shasum -a 256 subnetdesk-1.3.0.86-aarch64.dmg
```

对照同一 Release 的 `SHA256SUMS`。`build-manifest.json` 记录提交、运行编号、目标平台、文件大小和校验和；校验和用于检测下载损坏，不等同于发布者的数字签名。

## Nightly 与构建失败

主分支推送、每日计划和手动运行 `SubnetDesk Nightly Build` 会构建桌面测试包。构建失败时保留原有 Nightly 下载；全部成功后才上传替换文件、清理过期附件并更新 `nightly` 标签。附件上传过程不是原子操作，下载时请对照本次校验和。

工作流使用 GitHub 自动提供的 `GITHUB_TOKEN` 上传附件，不需要配置个人 PAT。若发布步骤提示权限不足，检查仓库 **Settings → Actions** 的组织或仓库策略是否允许 `contents: write`。若某个平台失败，查看该运行对应的失败步骤，修复后重新运行。ARM64 和 macOS runner 可能需要排队。

商业证书不是构建前提。复用工作流的 `sign-macos` 默认关闭；只有显式启用并配置自己的证书与公证 Secrets 才执行 Apple 开发者签名。Windows 发布流程已经移除旧远程签名服务调用。
