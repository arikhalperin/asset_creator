#!/bin/sh
# Regenerates the video test fixtures (needs the ffmpeg CLI with libx264 and libvpx).
# Reference PNGs are what `ffmpeg -pix_fmt rgba` outputs for the listed frames.
set -e
cd "$(dirname "$0")"
FF="ffmpeg -hide_banner -loglevel error -y"
SRC="testsrc2=size=160x120:rate=30:duration=3"

# Lossless: RGB frames, FFV1 in Matroska; the source PNGs are the reference.
rm -rf lossless_src && mkdir lossless_src
$FF -f lavfi -i "$SRC" -pix_fmt rgb24 lossless_src/%03d.png
$FF -framerate 30 -i lossless_src/%03d.png -c:v ffv1 -pix_fmt bgr0 lossless.mkv
for n in 0 17 45 89; do cp "lossless_src/$(printf %03d $((n + 1))).png" "lossless_f$n.png"; done
rm -rf lossless_src

# Lossy containers.
$FF -f lavfi -i "$SRC" -c:v libx264 -pix_fmt yuv420p -g 30 -bf 2 clip.mp4
$FF -f lavfi -i "$SRC" -c:v libvpx-vp9 -pix_fmt yuv420p -g 30 -b:v 200k clip.webm
$FF -f lavfi -i "$SRC" -c:v libx264 -pix_fmt yuv420p -g 30 clip.mkv
$FF -f lavfi -i "$SRC" -c:v mpeg4 -q:v 4 -g 30 clip.avi
$FF -f lavfi -i "$SRC" -c:v libx264 -pix_fmt yuv420p -g 30 clip.mov
# BT.709-tagged (the color matrix must be honoured).
$FF -f lavfi -i "$SRC" -c:v libx264 -pix_fmt yuv420p -g 30 -colorspace bt709 -color_primaries bt709 -color_trc bt709 bt709.mp4
# Rotated: stored 160x120 with 90° display rotation (displays 120x160).
$FF -display_rotation 90 -i clip.mov -c copy rotated.mov
for clip in clip.mp4 clip.webm clip.mkv clip.avi clip.mov rotated.mov bt709.mp4; do
  for n in 0 17 45 89; do
    $FF -i "$clip" -vf "select=eq(n\,$n)" -frames:v 1 -pix_fmt rgba "${clip%%.*}_${clip##*.}_f$n.png"
  done
done

# Scene cuts: six visually distinct moving segments; cuts listed in scenes.json.
$FF -f lavfi -i "testsrc2=size=160x120:rate=30:duration=1.5" \
    -f lavfi -i "mandelbrot=size=160x120:rate=30" \
    -f lavfi -i "smptebars=size=160x120:rate=30:duration=1" \
    -f lavfi -i "life=size=160x120:rate=30:mold=10:ratio=0.3:death_color=#103020:life_color=#e0c040" \
    -f lavfi -i "gradients=size=160x120:rate=30:speed=0.05:c0=#ff2020:c1=#2020ff" \
    -f lavfi -i "cellauto=size=160x120:rate=30:rule=110" \
    -filter_complex "[0]format=yuv420p,setsar=1[a];[1]trim=end_frame=40,setpts=PTS-STARTPTS,format=yuv420p,setsar=1[b];[2]format=yuv420p,setsar=1[c];[3]trim=end_frame=50,setpts=PTS-STARTPTS,format=yuv420p,setsar=1[d];[4]trim=end_frame=35,setpts=PTS-STARTPTS,format=yuv420p,setsar=1[e];[5]trim=end_frame=45,setpts=PTS-STARTPTS,format=yuv420p,setsar=1[f];[a][b][c][d][e][f]concat=n=6:v=1,settb=1/30,setpts=N[v]" \
    -map "[v]" -r 30 -c:v libx264 -g 30 -pix_fmt yuv420p scenes.mp4
# testsrc2 45 frames, mandelbrot 40, smptebars 30, life 50, gradients 35, cellauto 45.
printf '{\n  "frames": 245,\n  "cuts": [45, 85, 115, 165, 200]\n}\n' > scenes.json

# Not a video.
head -c 4096 /dev/urandom > corrupt.mp4

# Full-loop fixture: the CC0 Kenney skeleton sprite (tests/fixtures/tagging/
# skeleton_01.png, Tiny Dungeon) scaled 2x with nearest neighbour, walking
# across a plain teal background, 30 frames.
$FF -f lavfi -i "color=c=0x1e465a:size=320x240:rate=30:duration=1" \
    -loop 1 -i ../tagging/skeleton_01.png \
    -filter_complex "[1]scale=96:112:flags=neighbor[s];[0][s]overlay=x='40+t*120':y=64:shortest=1,format=yuv420p" \
    -frames:v 30 -c:v libx264 -g 30 -pix_fmt yuv420p skeleton.mp4

# A 1-minute clip (walk.mp4 looped) for the UI-responsiveness test.
$FF -stream_loop 9 -i walk.mp4 -c copy -t 60 minute.mp4
