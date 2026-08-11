---
title: 参与贡献
description: 选择范围明确的修改，遵守 Koharu 所有权规则，并在 PR 中提供验证证据。
---

# 参与贡献

Koharu 欢迎聚焦的缺陷修复、文档改进、模型移植修正和范围明确的产品工作。

## 编码之前

1. 搜索[现有 Issue](https://github.com/mayocream/koharu/issues) 与 Pull Request。
2. 开始大型行为或架构变更前先创建 [Issue](https://github.com/mayocream/koharu/issues)。
3. 阅读 `AGENTS.md` 以及所有受影响 crate 的 README。
4. 不要提交生成输出、模型权重、数据集、凭据、构建产物或机器特有文件。

## 修改要求

- API 或架构变化时更新仓库内所有使用方，不添加兼容别名。
- 服务商专用默认值和请求行为由对应服务模块拥有。
- 安全公共 API 与 unsafe FFI、动态加载分离。
- 上游模型移植保留影响检查点的结构，并在相同输入上比较结构化输出。
- 在实际目标设备上用代表输入优化，同时报告速度与正确性。

注释应解释所有权、不变量、上游映射或有意差异，不要复述简单代码。

## 验证与 PR

默认只运行与修改对应的最小 debug 检查或聚焦测试一次，不必每次运行无关完整套件。格式化修改过的 Rust 和 TypeScript 文件，并运行 `git diff --check`。

PR 应包含：问题与所有权边界、重要行为或架构变化、命令及结果、可见 UI 变化的截图，以及性能工作的设备、输入、基准、结果和正确性差异。

实质使用生成式 AI 时请披露。提交者必须理解、审查并测试全部内容。未审查的生成代码与低质量自动 PR 可能被直接关闭。

缺陷与计划中的变更请使用 [GitHub Issues](https://github.com/mayocream/koharu/issues)，设计问题与社区支持请使用 [Discord](https://discord.gg/mHvHkxGnUY)。
