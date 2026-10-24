#!/usr/bin/env bash
# App-specific package assertions retained alongside shared packaging lint.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

python3 - <<'PYEOF'
import yaml
manifests = []
for name in ("com.stitchwright.screenfoundry.yml", "com.stitchwright.screenfoundry.bundle.yml"):
    with open("packaging/linux/flatpak/" + name) as source:
        manifest = yaml.safe_load(source)
    assert manifest["id"] == "com.stitchwright.screenfoundry" and manifest["command"] == "screenfoundry", name
    manifests.append(manifest)
for key in ("runtime", "runtime-version", "sdk", "finish-args"):
    assert manifests[0][key] == manifests[1][key], f"Flatpak manifests disagree on {key}"
print("ScreenFoundry package identity ok")
PYEOF
