# Koda security notes

- Provider, GitLab, Jira and MCP credentials must be injected through `*_FILE` secrets or the encrypted secret store. They are never returned by the API.
- Context7 is an explicitly registered remote MCP server. A repository cannot add a server through its own `opencode.json`.
- The egress proxy supports only HTTPS CONNECT and an administrator-provided host allowlist. Production should attach sessions to an internal Docker network so the proxy is the only route.
- Approval is bound to the evidence hash. A changed diff invalidates a previous approval.
- The current MVP is single-tenant and should run with a non-root Docker engine/user. Do not expose the Docker control socket to the public network.
