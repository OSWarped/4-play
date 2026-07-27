# Security Policy

4-Play is currently pre-release and should not be exposed directly to the public internet.

## Reporting

Please report vulnerabilities privately to the repository owner rather than opening a public issue.

## Security expectations

- Treat seat clients and local networks as untrusted.
- Authenticate every seat, operator, and runtime host.
- Authorize actions by role and resource.
- Never transmit secrets in logs or URLs.
- Bind emulator-control and media services to explicitly configured interfaces.
- Store service credentials outside the repository.
- Validate game-package manifests and filesystem paths.
- Prevent clients from supplying arbitrary emulator command-line arguments.
- Default to least privilege for runtime processes and device access.
