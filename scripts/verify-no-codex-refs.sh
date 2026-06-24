#!/usr/bin/env bash
set -euo pipefail

# Fail if any source file outside the allow-list mentions codex/Codex/CODEX_/OpenAI/ChatGPT
# as branding. URLs (chatgpt.com, openai.com) are intentionally retained.

allow_file=scripts/.codex-refs-allowed
pattern='\b(codex|Codex|CODEX_[A-Z_]+|OpenAI|ChatGPT)\b'

# Search code dirs only.
hits=$(grep -rnE "$pattern" crates/ utils/ 2>/dev/null | \
       grep -v -F -f "$allow_file" || true)

if [[ -n "$hits" ]]; then
  echo "verify-no-codex-refs.sh: found unallowed brand references:"
  echo "$hits"
  exit 1
fi

echo "verify-no-codex-refs.sh: OK"
