# 版本管理与发版流程

状态：Review  
最后更新：2026-10-13  
关联文档：[ADR 0005](../03-architecture/adr/0005-online-update-release.md)、[打包操作](../05-operations/build-and-package.md)、[版本总览](../upgrade/README.md)、[验收](../04-quality/online-update-checklist.md)

## 版本级别与唯一来源

沿用 ashell 的 SemVer 治理：PATCH 为兼容修复、性能、文案或内部调整；MINOR 为兼容新功能；MAJOR 为配置、数据、协议或流程不兼容。仅文档/CI/工具修改且不发产物时通常不升级版本。预发布用 beta.1、rc.1 等，每个版本均有独立标签与详情，不覆盖旧标签。

Cargo.toml 的 package.version 是唯一代码来源；Cargo.lock 的 tiny-proxy 包、界面、Windows ProductVersion、macOS TinyProxyReleaseVersion 与标签保持一致。当前版本与发布状态见 [版本总览](../upgrade/README.md)，不在规范文档中维护第二份版本状态。

## 必须同步的资料

1. 根目录 CHANGELOG.md 维护短摘要及 `[X.Y.Z]: docs/upgrade/vX.Y.Z/README.md` 引用，不在 README 重复维护版本历史。
2. docs/upgrade/README.md 维护当前代码版本、日期、状态、详情链接和一句话摘要，新版本置顶。
3. docs/upgrade/vX.Y.Z/README.md 是版本说明权威来源，目录名与标签完全一致；该目录结构为用户要求复刻的发布工具约定。
4. 发版脚本拒绝包版本、锁文件、标签、索引或详情不一致。

## 版本详情内容

必须基于实际代码差异、配置变化及验证记录编写，不只复制提交标题：

- 标题、版本、发布日期、状态、最后更新时间与关联文档；未发布写「未发布」，不虚构日期或标签。
- 版本概述与按实际内容整理的新功能、改进、修复、性能、安全、工程等分类，没有内容不保留空分类。
- 行为与界面变化：默认值、设置、窗口、操作流程和用户可见反馈。
- 配置与数据兼容性：路径、加密、迁移、能否回退和必要备份。
- 升级说明：覆盖安装、退出、备份、权限及平台差异。
- 破坏性变更与已知问题：影响、迁移、回退或临时规避方式。
- 验证结果：实际格式、Clippy、测试、构建和平台验收，未运行项注明原因。
- 变更依据：目标标签、最近真实祖先发布标签及 GitHub compare；首版说明无前置版本。并行发布线不能按数字大小推断祖先。
- 返回版本总览链接。

Release Notes 从详情提取用户可见章节，隐藏验证结果、变更依据和本地导航；应用与 GitHub 使用同一来源。

## 发布步骤

1. 检查真实祖先标签到目标代码的差异，确定升级级别和兼容性。
2. 修改 Cargo.toml，同步 Cargo.lock，仅依赖解析不需编译；不要因版本更新自动运行 cargo check。
3. 完成详情、总览、CHANGELOG、验收场景，回填实际验证结果。
4. 执行 `python scripts/release_notes.py --check-current`。开发校验和手动 Actions 允许 Draft/Review、「未发布」。
5. 获得正式发布确认后，填写发布日期、把详情设为 Approved；执行 `python scripts/release_notes.py --tag vX.Y.Z`，未批准/无日期/缺资料先失败。
6. 仅在用户明确授权 Git 操作与发布后提交、创建并推送新标签。此文档不构成自动提交或正式发布授权。
7. 发布后核对四个应用产物、release-notes.md、update-manifest.json 的版本、大小、URL、SHA-256，完成真实更新往返并记录证据。

已公开标签不得复用。发布失败保留记录，按影响建立新的 PATCH 版本。单版本详情只有经用户明确要求才能加入 `> 发布测试策略：skip`，仅跳过该版本发布工具测试，资料/构建/打包/清单仍检查；后续无标记版本恢复默认测试。

## Actions 行为

check.yml 在 push/PR 执行两平台格式、Clippy、测试、release 构建和 Python 发版工具测试。release.yml 校验资料、执行发布工具测试、构建打包并生成清单。workflow_dispatch 仅上传 artifacts；v* 标签才发布 Release。含 `-` 的标签为 prerelease，不设 latest，应用只检查最新正式版。当前工作流尚未执行。
