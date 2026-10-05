# MinIO integration-test fixture

The published MinIO image used by this test became inaccessible (registry
`unauthorized` errors). Build the same release from upstream source instead of
skipping the S3 integration test or substituting a third-party binary.

The Dockerfile pins MinIO RELEASE.2025-02-28T09-55-16Z to commit
`8c2c92f7afdc8386b000c0cb57ecec2ee1f5bcb0`. Go verifies modules through its checksum
database. The image is for local tests only and is not published or deployed.

On Linux with Docker, from the repository root:

```sh
docker build --tag aikit-test-minio:8c2c92f7 aikit-session-sync/tests/minio
cargo test -p aikit-session-sync --test e2e_minio -- --nocapture
```

All Linux CI jobs that run this test build the image using the shared
`prepare-minio` action. Testcontainers uses the local image. Build, startup, and
S3 assertion failures fail CI; registry errors are never treated as test skips.
The existing platform guard still skips this Linux-container test on Windows
and macOS. The first source build requires network access and takes longer than
a prebuilt-image pull; subsequent local builds reuse Docker layers.
