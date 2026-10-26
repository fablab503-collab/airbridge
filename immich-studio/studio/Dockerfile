ARG BASE=python:3.13-slim
FROM ${BASE}
# ffmpeg: video audio + odd formats; imagemagick: last-resort format reader;
# libcairo2: SVG; libegl1/libgles2: MediaPipe face mesh
RUN apt-get update && apt-get install -y --no-install-recommends \
        ffmpeg imagemagick libcairo2 libegl1 libgles2 libgl1 libglib2.0-0 curl unzip \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /srv
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY download_models.sh .
COPY app ./app
COPY trained /srv/trained
COPY brand-default /srv/brand-default
ENV STUDIO_MODEL_DIR=/models STUDIO_DATA_DIR=/data STUDIO_GRAPHITE_DIR=/graphite \
    IMMICH_URL=http://immich-server:2283 IMMICH_ML_URL=http://immich-machine-learning:3003
EXPOSE 8000
CMD ["sh", "-c", "./download_models.sh /models && cp -n /srv/trained/* /models/ ; uvicorn app.main:app --host 0.0.0.0 --port 8000"]
