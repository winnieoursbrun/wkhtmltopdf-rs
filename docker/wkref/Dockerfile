# Rebuild of the wkref:local oracle image (lost when Docker Desktop's data was reset on
# 2026-10-14): Ubuntu 20.04, DejaVu fonts only, poppler-utils, the shared libraries the
# official wkhtmltopdf 0.12.6 linux-amd64 binary links against. The binary itself is
# bind-mounted at run time (/wk/...), as before.
FROM ubuntu:20.04
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN apt-get update -qq \
 && apt-get install -y -qq --no-install-recommends fonts-dejavu-core poppler-utils \
      libfontconfig1 libfreetype6 libxrender1 libxext6 libx11-6 libjpeg-turbo8 libpng16-16 libssl1.1 zlib1g ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /wk
