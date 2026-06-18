#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${ENV_FILE:-$SCRIPT_DIR/.env}"
SPACETIME_SERVER="${SPACETIME_SERVER:-maincloud}"
SPACETIME_DATABASE="${SPACETIME_DATABASE:-spacenix-9wfd4}"

if [[ ! -f "$ENV_FILE" ]]; then
  echo "Missing $ENV_FILE" >&2
  exit 1
fi

set -a
source "$ENV_FILE"
set +a

: "${S3_BUCKET:?S3_BUCKET is required}"
: "${S3_REGION:?S3_REGION is required}"
: "${S3_ACCESS_KEY_ID:?S3_ACCESS_KEY_ID is required}"
: "${S3_SECRET_ACCESS_KEY:?S3_SECRET_ACCESS_KEY is required}"
: "${SPACETIME_ADMIN_EMAIL:?SPACETIME_ADMIN_EMAIL is required}"
: "${SPACETIME_ADMIN_PASSWORD:?SPACETIME_ADMIN_PASSWORD is required}"

json_string() {
  python3 -c 'import json, sys; print(json.dumps(sys.argv[1]))' "$1"
}

json_option_string() {
  if [[ -n "${1:-}" ]]; then
    python3 -c 'import json, sys; print(json.dumps({"some": sys.argv[1]}))' "$1"
  else
    printf '{"none":[]}\n'
  fi
}

if [[ "${DRY_RUN:-}" == "1" ]]; then
  set -x
fi

spacetime call \
  --server "$SPACETIME_SERVER" \
  "$SPACETIME_DATABASE" \
  update_s_3_config_with_credentials \
  "$(json_string "$SPACETIME_ADMIN_EMAIL")" \
  "$(json_string "$SPACETIME_ADMIN_PASSWORD")" \
  "$(json_string "$S3_BUCKET")" \
  "$(json_string "$S3_REGION")" \
  "$(json_option_string "${S3_ENDPOINT:-}")" \
  "$(json_option_string "$S3_ACCESS_KEY_ID")" \
  "$(json_option_string "$S3_SECRET_ACCESS_KEY")" \
  "$(json_option_string "${S3_PATH_PREFIX:-}")" \
  "$(json_option_string "${S3_PUBLIC_BASE_URL:-}")"

echo "Uploaded S3 config to $SPACETIME_DATABASE on $SPACETIME_SERVER."
