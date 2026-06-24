# agentx

Single-binary remote process / filesystem executor.

`agentx` is a hard fork of [codex](https://github.com/openai/codex) `exec-server`
at tag `rust-v0.142.0`, with everything outside the remote-exec-server use
case removed. See `NOTICE` for derivation details.

## Install

```bash
curl -fsSL https://github.com/agentserver/agentx/releases/latest/download/install.sh | sh
```

Or download a tarball from
[releases](https://github.com/agentserver/agentx/releases/latest) and put
`agentx` on your `PATH`.

### macOS

Binaries are unsigned in v0.x. After download:

```bash
xattr -d com.apple.quarantine /usr/local/bin/agentx
```

## Usage

```bash
agentx --remote https://your-gateway/  --environment-id exe_… --name my-laptop \
       --use-agent-identity-auth \
       --agent-identity-authapi-base-url https://your-auth-server/
```

Environment:
- `AGENTX_ACCESS_TOKEN` — Agent Identity JWT.
- `AGENTX_AGENT_IDENTITY_ALLOWED_BASE_URLS` — comma-separated allow-list for
  the chatgpt_base_url config.
- `AGENTX_API_KEY` — bearer for API-key auth mode.
- `AGENTX_API_KEY_ALLOWED_HOSTS` — comma-separated allow-list for the
  --remote URL host in API-key mode.
- `AGENTX_HOME` — config directory (default `~/.agentx`).

## License

Apache-2.0. See LICENSE and NOTICE.
