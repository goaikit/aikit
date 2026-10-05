# Test-only build of RELEASE.2025-02-28T09-55-16Z, pinned to its source commit.
# Go's module proxy and checksum database verify the downloaded module content.
FROM golang:1.23.6-bookworm AS build
ENV CGO_ENABLED=0 GOTOOLCHAIN=local
RUN go install -trimpath github.com/minio/minio@8c2c92f7afdc8386b000c0cb57ecec2ee1f5bcb0

FROM debian:bookworm-slim
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /go/bin/minio /usr/local/bin/minio
EXPOSE 9000
ENTRYPOINT ["minio"]
CMD ["server", "/data"]
