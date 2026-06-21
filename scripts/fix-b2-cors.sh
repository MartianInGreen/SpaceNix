#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${ENV_FILE:-$SCRIPT_DIR/.env}"

if [[ ! -f "$ENV_FILE" ]]; then
  echo "Missing $ENV_FILE" >&2
  exit 1
fi

set -a
source "$ENV_FILE"
set +a

: "${S3_BUCKET:?S3_BUCKET is required}"
: "${S3_ACCESS_KEY_ID:?S3_ACCESS_KEY_ID is required}"
: "${S3_SECRET_ACCESS_KEY:?S3_SECRET_ACCESS_KEY is required}"

# Comma-separated list of extra origins to ensure are allowed.
# Override with: CORS_ORIGINS="http://localhost:5173,https://spacenix.example.com"
CORS_ORIGINS="${CORS_ORIGINS:-http://localhost:5173}"

python3 - "$S3_BUCKET" "$S3_ACCESS_KEY_ID" "$S3_SECRET_ACCESS_KEY" "$CORS_ORIGINS" << 'PYEOF'
import sys, json, requests

bucket_name, key_id, app_key, origins_csv = sys.argv[1:5]
extra_origins = [o.strip() for o in origins_csv.split(",") if o.strip()]

# 1. Authorize
auth_resp = requests.get(
    "https://api.backblazeb2.com/b2api/v3/b2_authorize_account",
    auth=(key_id, app_key),
)
if auth_resp.status_code != 200:
    print(f"b2_authorize_account failed ({auth_resp.status_code}): {auth_resp.text}", file=sys.stderr)
    sys.exit(1)
auth = auth_resp.json()
auth_token = auth["authorizationToken"]
account_id = auth["accountId"]
storage = auth["apiInfo"]["storageApi"]
api_url = storage["apiUrl"]
allowed_bucket_id = storage.get("bucketId")

# 2. Find the bucket
headers = {"Authorization": auth_token}
if allowed_bucket_id:
    list_resp = requests.post(f"{api_url}/b2api/v3/b2_list_buckets", json={"accountId": account_id, "bucketId": allowed_bucket_id}, headers=headers)
else:
    list_resp = requests.post(f"{api_url}/b2api/v3/b2_list_buckets", json={"accountId": account_id, "bucketName": bucket_name}, headers=headers)
if list_resp.status_code != 200:
    print(f"b2_list_buckets failed ({list_resp.status_code}): {list_resp.text}", file=sys.stderr)
    sys.exit(1)
buckets = list_resp.json().get("buckets", [])
bucket = next((b for b in buckets if b.get("bucketName") == bucket_name), None)
if not bucket:
    print(f"Bucket '{bucket_name}' not found", file=sys.stderr)
    sys.exit(1)
bucket_id = bucket["bucketId"]
existing_rules = bucket.get("corsRules", [])

# 3. Merge: add s3_put + s3_head to each rule, ensure extra origins are present
REQUIRED_OPS = {"s3_get", "s3_put", "s3_head"}
REQUIRED_HEADERS = {"content-type"}

if not existing_rules:
    existing_rules = [{
        "corsRuleName": "spacenix",
        "allowedOrigins": list(extra_origins),
        "allowedOperations": list(REQUIRED_OPS),
        "allowedHeaders": list(REQUIRED_HEADERS),
        "maxAgeSeconds": 3600,
    }]

for rule in existing_rules:
    ops = set(rule.get("allowedOperations", []))
    ops.update(REQUIRED_OPS)
    rule["allowedOperations"] = sorted(ops)

    hdrs = set(rule.get("allowedHeaders", []) or [])
    hdrs.update(REQUIRED_HEADERS)
    if hdrs == {"*"}:
        pass
    elif not hdrs:
        rule["allowedHeaders"] = ["*"]
    else:
        rule["allowedHeaders"] = sorted(hdrs)

    origins = rule.get("allowedOrigins", [])
    if "*" not in origins:
        origins = sorted(set(origins) | set(extra_origins))
    rule["allowedOrigins"] = origins

# 4. Update bucket
update_resp = requests.post(f"{api_url}/b2api/v3/b2_update_bucket", json={
    "accountId": account_id,
    "bucketId": bucket_id,
    "corsRules": existing_rules,
}, headers=headers)

if update_resp.status_code != 200:
    print(f"b2_update_bucket failed ({update_resp.status_code}): {update_resp.text}", file=sys.stderr)
    sys.exit(1)

result = update_resp.json()
print("CORS rules updated. Current rules:")
print(json.dumps(result.get("corsRules", []), indent=2))
PYEOF
