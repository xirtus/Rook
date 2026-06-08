#!/usr/bin/env python3
"""Example: AI agent controlling Rook via stdio IPC."""

import json, subprocess, sys

proc = subprocess.Popen(
    ["cargo", "run", "-p", "rook-cli", "--", "serve", "--ipc", "stdio"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True, cwd=".."
)

req_id = 0

def call(method, params=None):
    global req_id
    req_id += 1
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}}
    proc.stdin.write(json.dumps(req) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    if not line:
        return None
    return json.loads(line)

# 1. List methods
resp = call("project.get")
print(f"Project state: {json.dumps(resp, indent=2)[:500]}...")

# 2. Create new project
resp = call("project.create", {"name": "Agent Test", "canvas": {"width": 1920, "height": 1080}})
print(f"Created: {resp}")

# 3. Import media
resp = call("gallery.import", {"paths": ["test.mp4"]})
print(f"Imported: {resp}")

# 4. Get timeline
resp = call("timeline.get")
print(f"Timeline tracks: {len(resp.get('result', {}).get('snapshot', {}).get('tracks', []))}")

# 5. Export
resp = call("project.export", {"output_path": "output.mp4", "format": "h264"})
print(f"Export job: {resp}")

proc.terminate()
