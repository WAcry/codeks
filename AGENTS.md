# codeks

本仓库是 openai/codex 的自用 fork。fork 自己的文档、规范和工具都在 `fork/` 目录，用中文写。

动手前先读：

- `fork/README.md`：仓库模型、版本和命令
- `fork/PATCHING.md`：补丁规范
- 同步上游时再读 `fork/SYNC.md`

必须遵守：

- 这是公开仓库。写进仓库或发到 GitHub、npm 的内容都会公开，包括代码、注释、文档、提交和 tag 说明、分支名、PR 和评论。只写公开资料里查得到的名称，例如开源项目、公有云和它们的服务：LiteLLM、Azure、AWS 和它们的运行时都可以写。私有的服务、工具、项目、域名、地址、账号和人名都不要写，举例也不行；在对话、本机其他仓库或工具里看到的私有名称，只用来理解需求。需要指代时用泛称，例如“下游项目”“某个 OpenAI 兼容服务”。拿不准是否公开，就当作私有。
- 私有名称清单在本机的 `~/.config/codeks/private-terms`，不进仓库：每行一个正则，不区分大小写，`#` 开头是注释。`check`、`pr`、`publish` 用它扫描补丁内容、提交说明和作者、tag 说明、PR 文字和分支名。遇到清单里没有的私有名称，追加进去。
- 对上游文件的每一处改动都属于某个补丁。一个补丁 = 一个提交 = 一份文档 `fork/patches/NNNN-<slug>.md`，提交带 `Codeks-Patch: NNNN` trailer。修改已有补丁用 `python3 fork/tools/codeks.py amend <ID>`。
- 配置、skills、MCP、插件、bridge 适配和源码补丁都是候选方案。先比较行为覆盖、用户配置负担、实现复杂度、影响文件和同步维护成本，再与用户确定实现位置；配置能实现，不等于必须选配置。用户已明确选择的方案不重复确认。源码补丁优先考虑新文件和适配层，保持最小，不顺手格式化、重构、改名或调整顺序。
- 不改上游的 `README.md`、`CHANGELOG.md`、`docs/` 和已有的 `.github/workflows/*.yml`。
- 本地默认不编译、不运行、不测试，编译和测试由 GitHub Actions 上的 `codeks-ci` 负责。提交前运行 `python3 fork/tools/codeks.py check`。
- 用户要先审查时，用 `python3 fork/tools/codeks.py pr` 开 draft PR。不要在 GitHub 上合并 PR，发布只用 `codeks.py publish`。
- 代码注释用英文，`fork/` 下的文档用中文。
