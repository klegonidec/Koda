# Koda architecture

Koda is a single-instance Rust control plane. It receives Jira and GitLab webhooks, persists an idempotent job, evaluates a project policy, and records a session evidence hash. The React console calls the versioned API from the same origin.

The current Compose profile keeps the OpenCode sidecar for compatibility while the `koda-harness` binary defines the new per-session runner contract. The next hardening step is to launch that binary in a short-lived container and place it on an internal Docker network with `koda-egress` as its only external route.

## Workflows

- `jira_implement`: prepare a controlled change and wait for policy/approval before publishing a Draft MR.
- `mr_review`: run read-only analysis and publish a summary comment.
- `pipeline_analysis`: analyze failed jobs and optionally prepare a corrective Draft MR.

No workflow merges code automatically or accepts an arbitrary repository URL from a webhook payload.
