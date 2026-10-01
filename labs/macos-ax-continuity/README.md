# INCODEX-V1.1-MAC-AX-RC1

本地、未发行的 macOS 授权连续性实验。目标是让两份内容不同的原生应用满足同一证书绑定的 designated requirement，用户对第一份授权后，第二份实际 AX 读取仍可用。

此阶段不改 `/Applications/ChatGPT.app`，不发布或替换当前 1.1.0 Runtime，不写 TCC、不自动授权、不生成 identifier-only 规则、不伪造 OpenAI Team。系统批准由用户完成。稳定身份后端使用实验独立钥匙串，不能污染登录钥匙串；首次生成的身份不得因源码或 RC 编号变化而自动轮换。

失败先行测试先落地；原生 Helper、构建与运行后置条件随后补齐。候选编号不是已安装 Runtime 版本。即使实验通过，也须再证明正式宿主的签名拓扑、恢复/卸载和撤销权限边界。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
