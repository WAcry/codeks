# 0017 同提交发布组装、恢复与完整验证

```toml
watch_paths = [
  ".github/workflows/codeks-npm.yml",
  "fork/tools/npm_package.py",
  ".github/scripts/codeks_exec_release.py",
  ".github/scripts/test_codeks_exec_release.py",
  ".github/workflows/codeks-ci.yml",
]
watch_regex = [
  "codeks-(base|voice|exec)|workflow_dispatch|actions/download-artifact",
  "workflow_dispatch|needs[.]|if:.*always|Publish.*release|Verify.*target",
]
backport_of = [
]
upstream = [
]
ci_skip = [
]
```

## 背景

2026-11-09 的 `codeks/rust-v0.167.0/5` 发布 workflow（GitHub Actions 运行 [#42594162422](https://github.com/WAcry/codeks/actions/runs/42594162422)）中，**Windows/Linux/macOS x64/ARM64 六个平台的 CLI 与 Voice 原生构建共 12 个 Job 全部成功**，并各自上传构建 artifact。最终 `pack` 组装阶段执行 YAML 内嵌 Python 时，第 16 行新增的独立 Exec Server `subprocess.run` 比同一循环中的其他语句多一个缩进空格，因此报 `IndentationError: unexpected indent`，导致 `smoke` 和 `publish` 未运行。原始二进制并没有编译失败。

直接重新编译十二个平台会浪费已经通过的原生构建；但从不同版本、不同仓库或错误来源拼装构建产物，则会破坏 Voice receipt、SHA256、`STABLE_GIT_COMMIT` 及 fork 补丁的可靠性。因此在原有 workflow 内提供严格受限的同 SHA 恢复路径。

本补丁的恢复流程对已有 `/5` 的 12 个原生构建 artifact 做严格来源核验，运行恢复构建 [#42730328836](https://github.com/WAcry/codeks/actions/runs/42730328836) 后，`prepare` 通过，两个原生构建 job **按设计被 skipped**，`pack` 真实组装六个平台、生成 npm 包及单独的 Exec Server 并成功上传三个 artifact 集合。可是下游 `smoke` 未执行：GitHub Actions 的默认 `success()` 依赖条件沿 DAG 继承了故意跳过的上游 job。`publish` 也未执行；workflow 却错误显示 `success`。这不是六平台测试通过，只是被错误跳过。

## 改动

- `.github/workflows/codeks-npm.yml`：更正内嵌 Python 的 Exec Server `subprocess.run` 与上一级 `for platform` 内其他操作的缩进（原脚本第 16 行多一个空格）。`prepare` 在执行原生构建前新增静态检查：从原 workflow 提取 `Assemble voice and archive complete packages` 的 Python 代码，使用 `ast.parse` 验证语法并确认独立 Exec Server 打包入口存在。原生打包助手和业务代码不变。
- `workflow_dispatch` 新增两个可选输入：`reuse_build_run`（number，默认 0）以及 `reuse_build_tag`（string，默认空）。未填时维持原有 `prepare → 6 CLI + 6 voice → pack → 6 smoke → publish` 全量构建，输入 `publish=true` 的 tag 约束保持不变。
- 两处下载原生产物的 `run-id` 先检查 `reuse_build_run > 0`，只在正数时选择历史运行，否则选择当前 `github.run_id`。GitHub Actions 的输入可能以字符串 `"0"` 进入表达式；直接使用 `||` 会把它当作真值，误请求不存在的 run 0。
- 两项均填写时，`prepare` checkout 现存的 `codeks/rust-v*` tag，拒绝无 tag、只填 tag 或非法 run ID；通过 `gh api` 核对历史运行属于同仓库，其 `head_sha` 与 tag 对应 HEAD **完全一致**，并核对六个平台 `codeks-base-*` 与 `codeks-voice-*` 共 12 份 artifact 存在、未过期、非空；任一不符立即失败。为了阻止跨提交重用，不接受手工指定不同 SHA 或把别的版本的文件冒充当前构建。
- 验证通过时，原生 `binaries`、`voice` jobs 跳过，`pack` 用 `actions/download-artifact` 的 `run-id` 和 `github.token` 读取同一历史 run 的原生 artifact。`pack` 仍运行完整 `voice_package.py assemble`、完整 CLI 的 archive / collect、独立 Exec Server 的 archive、npm pack；紧接着仍由对应平台的六个 runner 做完整 `npm` 与独立 Exec Server 的运行时 smoke。成功后 `publish=true` 才允许上传 npm 与 GitHub Release。
- 恢复任务的 `pack/smoke/publish` 都 checkout **原始 tag**，在 `npm_package.py version/archive/collect/pack` 显式传入原始 tag，以保持版本号、来源提交及 Voice receipt 一致。`RELEASE_TAG` 显式指定 GitHub Release 的准确 tag，而不是把用来触发新 workflow 的 `main` 误当作发布 tag。保留原有 npm trusted publishing（无长期 npm token）与 GitHub `contents:write` 发布权限，未新建第二条复制构建的工作流。

- `.github/workflows/codeks-npm.yml` 的 `smoke` job 增加 `if: always() && needs.prepare.result == 'success' && needs.pack.result == 'success'`。只有准备和组装明确成功才允许六个平台的原生 smoke 运行；允许恢复模式的原生编译 job **有意 skipped**，不允许 pack 被 skipped 或失败后继续。该 job 仍维持六平台矩阵和原有 npm/Exec Server 运行时验证。
- 同文件 `publish` job 增加 `if: always() && inputs.publish && needs.prepare.result == 'success' && needs.smoke.result == 'success'`。明确要求用户选择发布、准备及 **完整六平台 smoke** 成功，才能进入带 GitHub `contents:write` 和 npm OIDC trusted publishing 的发布步骤；任何跳过、失败都不得误发布。
- 新增 `finalize` job，`if: always()` 且 `needs: [prepare,pack,smoke,publish]`，强制检查 `prepare`、`pack` 和 `smoke` **均为 success**。当 `publish=true` 还必须验证 `publish` 本身为 success；当 `publish=false`，预期 `publish=skipped` 并不视为失败。任何必须执行的阶段被跳过或失败，`finalize` 主动退出非零，确保 GitHub 整体 workflow 状态准确，不能再把 skip 当做成功。

## 必要性

问题是确定性 Python 语法错误，单纯重试原工作流无效；重复执行 12 个原生 Job 又延误发布。提供受限恢复模式可用当前 fork 同提交的已编译成果真正完成 `pack → smoke → publish`，同时不会隐藏任何失败测试、替换执行服务器二进制或弱化供应链校验。

恢复历史构建本来是为了节省重复编译；如果 CI/CD 默许跳过六平台的 npm、voice、Exec Server 运行测试并返回绿色，后续发布会缺少真正的安全门槛。判断基于 GitHub Actions Job 的实际 `needs.*.result`，而不是看步骤数量或只依赖隐式 `success()`；能同时覆盖常规与恢复模式，维持发布的完整性。

## 何时扩展

- 当构建矩阵、artifact 命名、tag 版本协议或 `npm_package.py` 入参发生变化时，同时更新十二个 artifact 的完整性验证；`reuse_build_run` 与 `reuse_build_tag` 不能脱钩。
- 增加新的单独发布程序或更改 Voice/Exec Server receipt 时，确保它也来自同 SHA 的原生 artifact，并参加全部平台 smoke；不允许通过跨版本复用规避构建失败。
- 若将内嵌打包脚本移入独立文件，应保留构建前的 `ast.parse`/`py_compile` 语法检查，避免原生构建完成后才发现语法错误。

- workflow 改变矩阵、pack job 名称、增加 SDK/Exec Server 的独立 smoke 或额外发布环节时，更新 `finalize` 对必须完成的 job 结果检查，不允许新 job skipped 仍显示成功。
- 将原生编译、pack、smoke 进一步拆分复用时，直接检验相应 `needs` 结果，避免传递依赖中 `skipped` 使下游异常跳过；要验证每个原生 target 的真正运行结果。

## 何时移除

如果上游/Codeks 采用可证明同 SHA 的持久构建缓存、自动重打包或统一可复用工作流，可收敛为更简洁的入口。不能依靠 force-push 重指向旧发布 tag，不能跳过校验。

若后续采用 GitHub Actions 可复用工作流、平台汇总验证器或其他明确提供相同完整成功判定的机制，可把本补丁合并进新的最终验证阶段；不得删除所有必需 smoke 的强制门禁。

## 冲突处理

以最新上游 Cargo、release 资源、矩阵和规范包结构为准。保留独立 Exec Server 的打包语句与普通组装循环同级；历史产物恢复只应发生在原始 run 与发布 tag commit 相同的情况。检查 `pack` 的 `needs` 条件允许原生 job 成功、或经过验证的历史 run 两条路径，但不能允许跳过的原生构建被误判为成功；若 tag、artifact、版本不一致必须直接失败。

保持 0017 的 `reuse_build_run` 与 `reuse_build_tag` 同 SHA 验证不变，继续允许有意跳过原生 build。`pack` 成功后原生 smoke 必须运行，`publish` 只有 smoke 真正成功才执行，`finalize` 无论依赖是否 skipped 都必须检查必要的阶段结果。发现将 `always()` 用在外部可取消任务时，需要审查 cancellation 行为，不能让不完整构建获得绿色状态。

## 验证

- `actionlint .github/workflows/codeks-npm.yml` 应通过；`prepare` 内嵌 Python 和 `pack` 内嵌 Python 用 `ast.parse` 验证，原生打包测试 `python3 .github/scripts/test_codeks_exec_release.py -v` 通过。专门检查过错位缩进在 `prepare` 前置检查中会被发现。
- 运行 `python3 fork/tools/codeks.py check` 确认新补丁符合规范，且已有 0004/0015 补丁不因改动而消失。
- 用 `/5` 成功原生运行 `42594162422` 的 Linux x64 `base` 与 `voice` artifact 本地执行打包循环；已从真实 artifact 成功生成完整 CLI voice 归档与独立 Exec Server 归档；测试完整合并时需验证六平台输入、产物、SHA256。
- 远端恢复先用 `publish=false` 不发布试运行，核对原生 12 个 Job 确实被跳过、pack/六个 smoke 全部通过，然后用同一 run/tag 的 `publish=true` 发布并核对 npm 版本及 GitHub Release 六个平台的完整 CLI、独立 Exec Server 与 SHA256，避免先发布后发现 smoke 异常。

恢复命令（只复用已通过、未过期的 **0.167.0-codeks.5** 同提交产物）：

```bash
gh workflow run codeks-npm.yml -R WAcry/codeks --ref main \
  -f publish=false -f reuse_build_run=42594162422 \
  -f reuse_build_tag=codeks/rust-v0.167.0/5
```

注意：workflow 本身从含修复的新 `main` 定义执行，但校验、构建产物元数据及 pack/smoke 所用源码均来自原来的 `/5` tag。只修复 workflow 的新提交自身需要全量构建时，应使用正常无 `reuse_build_run` 的发布流程。

- `actionlint .github/workflows/codeks-npm.yml`，检查 `smoke`、`publish`、`finalize` 的 `needs` 引用和上下文都合法。
- 本地提取 `finalize` 的 Bash 脚本，逐例注入 `prepare/pack/smoke/publish` 结果：发布关闭且 smoke 成功 → 成功；发布开启且发布成功 → 成功；smoke skipped → 失败；发布开启但 publish skipped → 失败；smoke failure → 失败。五种情况均通过。
- 用 `codeks/rust-v0.167.0/5` 对应的 12 个原生产物重新触发 `publish=false` 恢复流程，确认实际六个原生 smoke 均执行成功、`finalize` 成功；再用同样 run/tag 的 `publish=true` 完成 trusted npm 和 Release 发布，检查全部资产及真实运行结果。
- 运行 `python3 fork/tools/codeks.py check` 确保补丁元数据符合 fork 规范，不添加 `ci_skip`、不修改 Rust 行为或原生程序。

## 记录

- 2026-11-09 @ rust-v0.167.0：新增。修复独立 Exec Server 打包的 Python 意外缩进，增加普通构建前语法校验，并增加严格绑定源码 SHA、12 份 artifact 和原 tag 的恢复发布能力。
- 2026-11-09 @ rust-v0.167.0：新增。修复恢复模式下六平台 Smoke Test 被间接 skipped 而 workflow 误报成功，增加强制的最终结果验证。
- 2026-11-09 @ rust-v0.167.0：重用 42594162422 的十二份同提交原生构建，42731715932 真实通过全部六平台 npm/Exec Server smoke，42732837385 的 GitHub Release、npm trusted publishing 和最终结果门禁全部成功。
- 2026-11-10 @ rust-v0.167.0：普通发布 42843430249 的十二个原生构建全部成功，组装阶段因输入 `"0"` 被当作有效 run ID 而失败；下载来源改为显式正数判断，保留同提交恢复与全部平台验证。
