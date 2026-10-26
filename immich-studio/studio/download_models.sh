#!/bin/sh
# Downloads the open-source models once (about 600 MB) into the models volume.
set -e
D="${1:-/models}"
mkdir -p "$D"
cd "$D"
get() { [ -s "$2" ] || { echo "Downloading $2"; curl -fsSL -o "$2.part" "$1" && mv "$2.part" "$2"; }; }
# InsightFace (face finder, recogniser, age) - https://github.com/deepinsight/insightface
[ -d buffalo_l ] || { get https://github.com/deepinsight/insightface/releases/download/v0.7/buffalo_l.zip buffalo_l.zip && unzip -q -o buffalo_l.zip -d buffalo_l && rm buffalo_l.zip; }
[ -d antelopev2 ] || { get https://github.com/deepinsight/insightface/releases/download/v0.7/antelopev2.zip antelopev2.zip && unzip -q -o antelopev2.zip && rm antelopev2.zip; }
# MediaPipe Face Landmarker: 478 face points - Apache 2.0
get https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task face_landmarker.task
# Real-ESRGAN general x4 (BSD-3) - AI upscaling for big prints
get https://huggingface.co/vlad-grig/realesr-general-x4v3-onnx/resolve/main/onnx/model.onnx realesr-general-x4v3.onnx
echo "Models ready in $D"
