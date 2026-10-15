# codeks

本仓库是 openai/codex 的自用 fork。fork 自己的文档、规范和工具都在 `fork/` 目录，用中文写。

动手前先读：

- `fork/README.md`：仓库模型、版本和命令
- `fork/PATCHING.md`：补丁规范
- 同步上游时再读 `fork/SYNC.md`

必须遵守：

- 对上游文件的每一处改动都属于某个补丁。一个补丁 = 一个提交 = 一份文档 `fork/patches/NNNN-<slug>.md`，提交带 `Codeks-Patch: NNNN` trailer。修改已有补丁用 `python3 fork/tools/codeks.py amend <ID>`。
- 先找不改代码的办法（配置、skills、MCP、插件）。要改代码时优先新建文件，最后才改上游已有的文件。补丁保持最小，不顺手格式化、重构、改名或调整顺序。
- 不改上游的 `README.md`、`CHANGELOG.md`、`docs/` 和已有的 `.github/workflows/*.yml`。
- 本地默认不编译、不运行、不测试，编译和测试由 GitHub Actions 上的 `codeks-ci` 负责。提交前运行 `python3 fork/tools/codeks.py check`。
- 用户要先审查时，用 `python3 fork/tools/codeks.py pr` 开 draft PR。不要在 GitHub 上合并 PR，发布只用 `codeks.py publish`。
- 代码注释用英文，`fork/` 下的文档用中文。
