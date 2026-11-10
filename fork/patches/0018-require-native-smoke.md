# 0018 发布前强制验证六平台 Smoke Test

```toml
watch_paths = [
  ".github/workflows/codeks-npm.yml",
  ".github/workflows/codeks-ci.yml",
  "fork/tools/npm_package.py",
  ".github/scripts/codeks_exec_release.py",
]
watch_regex = [
  "workflow_dispatch|needs[.]|if:.*always|Publish.*release|Verify.*target",
]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

0017 补丁对已有 `/5` 的 12 个原生构建 artifact 做严格来源核验，运行恢复构建 [#42730328836](https://github.com/WAcry/codeks/actions/runs/42730328836) 后，`prepare` 通过，两个原生构建 job **按设计被 skipped**，`pack` 真实组装六个平台、生成 npm 包及单独的 Exec Server 并成功上传三个 artifact 集合。可是下游 `smoke` 未执行：GitHub Actions 的默认 `success()` 依赖条件沿 DAG 继承了故意跳过的上游 job。`publish` 也未执行；workflow 却错误显示 `success`。这不是六平台测试通过，只是被错误跳过。

## 改动

- `.github/workflows/codeks-npm.yml` 的 `smoke` job 增加 `if: always() && needs.prepare.result == 'success' && needs.pack.result == 'success'`。只有准备和组装明确成功才允许六个平台的原生 smoke 运行；允许恢复模式的原生编译 job **有意 skipped**，不允许 pack 被 skipped 或失败后继续。该 job 仍维持六平台矩阵和原有 npm/Exec Server 运行时验证。
- 同文件 `publish` job 增加 `if: always() && inputs.publish && needs.prepare.result == 'success' && needs.smoke.result == 'success'`。明确要求用户选择发布、准备及 **完整六平台 smoke** 成功，才能进入带 GitHub `contents:write` 和 npm OIDC trusted publishing 的发布步骤；任何跳过、失败都不得误发布。
- 新增 `finalize` job，`if: always()` 且 `needs: [prepare,pack,smoke,publish]`，强制检查 `prepare`、`pack` 和 `smoke` **均为 success**。当 `publish=true` 还必须验证 `publish` 本身为 success；当 `publish=false`，预期 `publish=skipped` 并不视为失败。任何必须执行的阶段被跳过或失败，`finalize` 主动退出非零，确保 GitHub 整体 workflow 状态准确，不能再把 skip 当做成功。

## 必要性

恢复历史构建本来是为了节省重复编译；如果 CI/CD 默许跳过六平台的 npm、voice、Exec Server 运行测试并返回绿色，后续发布会缺少真正的安全门槛。判断基于 GitHub Actions Job 的实际 `needs.*.result`，而不是看步骤数量或只依赖隐式 `success()`；能同时覆盖常规与恢复模式，维持发布的完整性。

## 何时扩展

- workflow 改变矩阵、pack job 名称、增加 SDK/Exec Server 的独立 smoke 或额外发布环节时，更新 `finalize` 对必须完成的 job 结果检查，不允许新 job skipped 仍显示成功。
- 将原生编译、pack、smoke 进一步拆分复用时，直接检验相应 `needs` 结果，避免传递依赖中 `skipped` 使下游异常跳过；要验证每个原生 target 的真正运行结果。

## 何时移除

若后续采用 GitHub Actions 可复用工作流、平台汇总验证器或其他明确提供相同完整成功判定的机制，可把本补丁合并进新的最终验证阶段；不得删除所有必需 smoke 的强制门禁。

## 冲突处理

保持 0017 的 `reuse_build_run` 与 `reuse_build_tag` 同 SHA 验证不变，继续允许有意跳过原生 build。`pack` 成功后原生 smoke 必须运行，`publish` 只有 smoke 真正成功才执行，`finalize` 无论依赖是否 skipped 都必须检查必要的阶段结果。发现将 `always()` 用在外部可取消任务时，需要审查 cancellation 行为，不能让不完整构建获得绿色状态。

## 验证

- `actionlint .github/workflows/codeks-npm.yml`，检查 `smoke`、`publish`、`finalize` 的 `needs` 引用和上下文都合法。
- 本地提取 `finalize` 的 Bash 脚本，逐例注入 `prepare/pack/smoke/publish` 结果：发布关闭且 smoke 成功 → 成功；发布开启且发布成功 → 成功；smoke skipped → 失败；发布开启但 publish skipped → 失败；smoke failure → 失败。五种情况均通过。
- 用 `codeks/rust-v0.167.0/5` 对应的 12 个原生产物重新触发 `publish=false` 恢复流程，确认实际六个原生 smoke 均执行成功、`finalize` 成功；再用同样 run/tag 的 `publish=true` 完成 trusted npm 和 Release 发布，检查全部资产及真实运行结果。
- 运行 `python3 fork/tools/codeks.py check` 确保补丁元数据符合 fork 规范，不添加 `ci_skip`、不修改 Rust 行为或原生程序。

## 记录

- 2026-11-09 @ rust-v0.167.0：新增。修复恢复模式下六平台 Smoke Test 被间接 skipped 而 workflow 误报成功，增加强制的最终结果验证。
