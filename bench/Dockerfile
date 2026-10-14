# Build with --platform linux/arm64. Benchmark environment: native linux/arm64 on Apple Silicon, so the original
# wkhtmltopdf and the Rust port run on the same CPU without emulation.
FROM debian:bookworm-slim

ARG WKHTMLTOX_VERSION=0.12.6.1-3
ARG WKHTMLTOX_SHA256=b6606157b27c13e044d0abbe670301f88de4e1782afca4f9c06a5817f3e03a9c

ENV DEBIAN_FRONTEND=noninteractive LC_ALL=C.UTF-8 LANG=C.UTF-8

# Fonts: DejaVu only (the family the Rust port embeds as its fallback set), so both
# binaries resolve the same faces. xfonts-* come in as wkhtmltox dependencies but
# are bitmap fonts that fontconfig does not pick for the CSS generic families.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl hyperfine time poppler-utils python3 \
      fonts-dejavu-core fontconfig \
      imagemagick python3-numpy python3-pil \
      ruby ruby-bundler \
 && curl -fsSLo /tmp/wkhtmltox.deb \
      "https://github.com/wkhtmltopdf/packaging/releases/download/${WKHTMLTOX_VERSION}/wkhtmltox_${WKHTMLTOX_VERSION}.bookworm_arm64.deb" \
 && echo "${WKHTMLTOX_SHA256}  /tmp/wkhtmltox.deb" | sha256sum -c - \
 && apt-get install -y --no-install-recommends /tmp/wkhtmltox.deb \
 && rm -f /tmp/wkhtmltox.deb \
 && apt-get purge -y curl && apt-get autoremove -y \
 && rm -rf /var/lib/apt/lists/* \
 && fc-cache -f \
 && wkhtmltopdf --version | grep -q 'with patched qt'

LABEL org.opencontainers.image.title="wkbench" \
      org.opencontainers.image.description="wkhtmltopdf ${WKHTMLTOX_VERSION} (patched qt) + hyperfine + GNU time + poppler, linux/arm64"

# pdfkit integration run (bench/pdfkit/generate.rb): gems pinned by Gemfile.lock.
# SSIM is computed in Python (numpy): ImageMagick 6.9.11 has no SSIM metric.
COPY pdfkit/Gemfile pdfkit/Gemfile.lock /opt/pdfkit/
ENV BUNDLE_GEMFILE=/opt/pdfkit/Gemfile BUNDLE_FROZEN=true
RUN bundle install --quiet && bundle exec ruby -e 'require "pdfkit"; require "webrick"; puts PDFKit::VERSION'

WORKDIR /bench
