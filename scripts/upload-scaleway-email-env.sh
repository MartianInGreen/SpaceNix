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

: "${SCALEWAY_EMAIL_REGION:?SCALEWAY_EMAIL_REGION is required}"
: "${SCALEWAY_EMAIL_SECRET_KEY:?SCALEWAY_EMAIL_SECRET_KEY is required}"
: "${SCALEWAY_EMAIL_PROJECT_ID:?SCALEWAY_EMAIL_PROJECT_ID is required}"
: "${SCALEWAY_EMAIL_FROM_EMAIL:?SCALEWAY_EMAIL_FROM_EMAIL is required}"
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

json_option_bool() {
  if [[ "${1:-}" == "1" || "${1:-}" == "true" ]]; then
    printf '{"some":true}\n'
  elif [[ "${1:-}" == "0" || "${1:-}" == "false" ]]; then
    printf '{"some":false}\n'
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
  update_scaleway_email_config_with_credentials \
  "$(json_string "$SPACETIME_ADMIN_EMAIL")" \
  "$(json_string "$SPACETIME_ADMIN_PASSWORD")" \
  "$(json_string "$SCALEWAY_EMAIL_REGION")" \
  "$(json_option_string "$SCALEWAY_EMAIL_SECRET_KEY")" \
  "$(json_option_string "$SCALEWAY_EMAIL_PROJECT_ID")" \
  "$(json_string "$SCALEWAY_EMAIL_FROM_EMAIL")" \
  "$(json_option_string "${SCALEWAY_EMAIL_FROM_NAME:-}")" \
  "$(json_option_bool "${SCALEWAY_EMAIL_ENABLED:-}")"

echo "Uploaded Scaleway Transactional Email config to $SPACETIME_DATABASE on $SPACETIME_SERVER."
