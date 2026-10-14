# Build with --platform linux/arm64 (see bench/java/run.sh). OpenHTMLtoPDF benchmark image:
# Temurin 21 JRE + the benchmark driver + the same measuring tools as wkbench:local. The Rust
# binaries are bind-mounted at run time, so both engines run in the same container.

FROM maven:3.9-eclipse-temurin-21 AS build
WORKDIR /src
COPY pom.xml .
RUN mvn -q -B dependency:go-offline
COPY src src
RUN mvn -q -B -o package -DskipTests \
 && mkdir -p /out/lib && cp target/convert.jar /out/ && cp target/lib/*.jar /out/lib/ \
 && mvn -q -B -o dependency:list -DoutputFile=/out/dependencies.txt -DincludeScope=runtime

FROM eclipse-temurin:21-jre-noble
ENV DEBIAN_FRONTEND=noninteractive LC_ALL=C.UTF-8 LANG=C.UTF-8
# Fonts: DejaVu only, as in wkbench:local; Convert.java registers them with OpenHTMLtoPDF.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      hyperfine time poppler-utils python3 fonts-dejavu-core fontconfig \
 && rm -rf /var/lib/apt/lists/* \
 && fc-cache -f
COPY --from=build /out /opt/bench
# AppCDS archive for the "java-cds" cold variant: classes loaded by one conversion.
RUN printf '%s' '<!DOCTYPE html><html><head><style>body{font:11pt "DejaVu Serif"}h1{font-family:"DejaVu Sans"}table{border-collapse:collapse}td{border:1px solid #bbb}</style></head><body><h1>Warm</h1><p>Text <em>em</em> <b>b</b></p><table><thead><tr><th>a</th></tr></thead><tr><td>1</td></tr></table></body></html>' > /tmp/w.html \
 && java -XX:ArchiveClassesAtExit=/opt/bench/app.jsa -jar /opt/bench/convert.jar /tmp/w.html /tmp/w.pdf \
 && java -XX:SharedArchiveFile=/opt/bench/app.jsa -Xshare:on -jar /opt/bench/convert.jar /tmp/w.html /tmp/w.pdf \
 && pdfinfo /tmp/w.pdf | grep -q '^Pages: *1$' \
 && rm -f /tmp/w.html /tmp/w.pdf

LABEL org.opencontainers.image.title="wkjava" \
      org.opencontainers.image.description="OpenHTMLtoPDF benchmark driver on Temurin 21 + hyperfine + GNU time + poppler, linux/arm64"
WORKDIR /bench
