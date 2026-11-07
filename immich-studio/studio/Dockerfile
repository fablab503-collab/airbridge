# Gimmich Studio: one Rust program (see README.md).
# Step 1 builds it; step 2 is the small image that runs it.
ARG BUILD_IMAGE=rust:1-trixie
ARG RUNTIME_IMAGE=debian:trixie-slim

FROM ${BUILD_IMAGE} AS build
# libheif: HEIC/AVIF photos; cmake + clang: build Whisper (speech-to-text)
RUN apt-get update && apt-get install -y --no-install-recommends libheif-dev cmake clang libclang-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release && cp target/release/gimmich-studio /gimmich-studio

FROM ${RUNTIME_IMAGE}
ARG TARGETARCH
ARG ONNXRUNTIME=1.22.0
# ffmpeg: sound from videos + rare formats; imagemagick, poppler: last-resort readers (old formats, PDF);
# libheif plugins: read/write HEIC and AVIF; curl/unzip: download the AI models on first start
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl unzip ffmpeg imagemagick poppler-utils fonts-dejavu-core \
        libheif1 libheif-plugin-libde265 libheif-plugin-x265 libheif-plugin-dav1d libheif-plugin-aomenc \
    && rm -rf /var/lib/apt/lists/*
# ONNX Runtime (Microsoft, MIT): runs the face, mesh and upscaling models
RUN arch=$([ "$TARGETARCH" = "arm64" ] && echo aarch64 || echo x64) \
    && curl -fsSL "https://github.com/microsoft/onnxruntime/releases/download/v${ONNXRUNTIME}/onnxruntime-linux-${arch}-${ONNXRUNTIME}.tgz" \
       | tar -xz -C /opt && mv /opt/onnxruntime-linux-* /opt/onnxruntime
WORKDIR /srv
COPY --from=build /gimmich-studio /usr/local/bin/gimmich-studio
COPY download_models.sh .
COPY static ./static
COPY trained ./trained
COPY brand-default ./brand-default
ENV STUDIO_MODEL_DIR=/models STUDIO_DATA_DIR=/data STUDIO_GRAPHITE_DIR=/graphite STUDIO_STATIC_DIR=/srv/static \
    STUDIO_TRAINED_DIR=/srv/trained STUDIO_BRAND_DEFAULT_DIR=/srv/brand-default STUDIO_DOWNLOADS_DIR=/downloads \
    ORT_DYLIB_PATH=/opt/onnxruntime/lib/libonnxruntime.so \
    IMMICH_URL=http://immich-server:2283 IMMICH_ML_URL=http://immich-machine-learning:3003
EXPOSE 8000
CMD ["sh", "-c", "./download_models.sh /models && cp -n /srv/trained/* /models/ ; exec gimmich-studio"]
