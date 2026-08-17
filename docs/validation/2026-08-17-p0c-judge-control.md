---
status: Implemented
owners: OpenOJ maintainers
last_reviewed: 2026-08-17
applies_to: P0-C Judge Control commits 582d499 through the current P0-C integration branch
references:
  - README.md
  - ../requirements/functional.md
  - ../requirements/acceptance.md
  - ../architecture/decisions/0004-grpc-over-uds-for-p0-judge-control.md
  - ../operations/deployment-profiles.md
  - ../security/trust-boundaries.md
  - ../superpowers/specs/2026-08-17-p0c-judge-control-design.md
---

# P0-C Judge Control 验证记录（2026-08-17）

## 范围与环境

- 范围：`v0alpha1` Judge Control Protobuf/gRPC 契约、server-owned node policy/clock/lease token、capability claim、renew/cancel、lease-fenced canonical result submit、仅 UDS 的 control-plane/judge-node，以及显式 development mock。
- 环境：Arch Linux，x86_64；Rust/Cargo 1.97.1；debug 构建。
- 数据库：disposable PostgreSQL 18，loopback `127.0.0.1:55432`；连接串只经测试环境变量传入，未写入本记录或测试输出。
- 进程边界：真实 `openoj-control-plane`、`openoj-judge-node` 和 `openoj-cli` 子进程，使用临时 `0700` 目录与 `0600` UDS；不以 in-process transport 替代。

## 需求与证据

| 范围 | 直接证据 | 当前结论 |
|---|---|---|
| `FR-SCHED-001` / `ACC-P0-003` | `postgres_uds_and_two_child_processes_reach_a_terminal_mock_result`；storage 的 capability/claim replay/renew 集成测试 | PostgreSQL、UDS 与独立进程完成 claim、renew、result 持久化；强杀恢复仍待补充。 |
| `FR-JUDGE-003` / `ACC-P0-017` | `async_worker_checks_renewal_before_submitting`、`async_worker_does_not_fabricate_a_result_after_cancel` | worker 执行前检查 renew；cancel 不伪造结果。运行中 microVM 停止未实现。 |
| `FR-RESULT-001` / `ACC-P0-018` | canonical result schema-to-domain conversion、storage result replay 测试、真实进程终态检查 | node/result 的 identity、provenance、lease token 和 operation replay 在服务端 fence。 |
| `NFR-OPEN-001` / `ADR-0004` | `uds_server_negotiates_without_a_tcp_listener`、版本/allowlist 回归、workspace dependency check | P0-C 只创建 UDS listener；未知版本/节点/capability fail closed。 |

## 已执行检查

```text
cargo test -p openoj-control-plane --test uds_negotiate --locked
OPENOJ_TEST_DATABASE_URL=<redacted> cargo test -p openoj-control-plane --test process_e2e --locked
cargo test -p openoj-judge-core --locked
cargo clippy -p openoj-control-plane -p openoj-judge-core -p openoj-judge-node --all-targets --locked -- -D warnings
python3 scripts/check-workspace.py
bash scripts/check-docs.sh
```

UDS 与 PostgreSQL 测试需要允许创建 Unix socket 并访问 disposable loopback database 的环境；受限文件系统沙箱不提供该能力。GitHub Actions 的 PostgreSQL 18 job 运行 workspace test，因此包含该 process regression。

## 未验证

- Firecracker/KVM/jailer、guest、用户代码、Artifact 内容体、网络、TCP/mTLS、跨主机节点身份、性能与生产隔离均未实现或未验证。
- 真实进程强杀、重启后的 claim replay、断线 deadline、运行中取消、五秒排空与 runtime image 回收仍需专门回归；不得将本记录当作这些能力的证明。
- 本记录为 `Implemented`，最终 `Validated` 需要完成全量 P0-C 失败路径、CI 远端结果和维护者审核。
