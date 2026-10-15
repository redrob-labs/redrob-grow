#!/usr/bin/env bash
# Take one photo of the shelf into the run's photos/ folder (and the planner's photo_dir).
#
#   scripts/snap.sh [photo_dir] [device]
#
# Default photo_dir is ./photos, device /dev/video0. Uses fswebcam if present, else ffmpeg,
# else libcamera-still (Raspberry Pi camera). File name is the UTC time, so the newest sorts last
# and the engine planner attaches it. Run twice a day from cron:
#
#   0 9,17 * * *  cd /home/pi/shelf && ./scripts/snap.sh photos
set -euo pipefail

dir="${1:-photos}"
dev="${2:-/dev/video0}"
mkdir -p "$dir"
out="$dir/$(date -u +%Y-%m-%dT%H-%M-%SZ).jpg"

if command -v fswebcam >/dev/null 2>&1; then
  fswebcam -q -d "$dev" -r 1280x720 --no-banner --jpeg 85 -S 10 "$out"
elif command -v ffmpeg >/dev/null 2>&1; then
  ffmpeg -loglevel error -y -f v4l2 -video_size 1280x720 -i "$dev" -frames:v 1 -q:v 4 "$out"
elif command -v libcamera-still >/dev/null 2>&1; then
  libcamera-still -n -o "$out" --width 1280 --height 720 -q 85 -t 2000
else
  echo "snap.sh: need fswebcam, ffmpeg or libcamera-still" >&2
  exit 1
fi

# Keep the folder from growing without bound on a small SD card: last 400 photos (~200 days at 2/day).
ls -1t "$dir"/*.jpg 2>/dev/null | tail -n +401 | xargs -r rm -f
echo "$out"
