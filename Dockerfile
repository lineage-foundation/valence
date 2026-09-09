####################################################################################################
## Builder
####################################################################################################
FROM rust:latest AS builder

WORKDIR /valence

COPY ./ .

RUN cargo build --release

####################################################################################################
## Final image
####################################################################################################
FROM cgr.dev/chainguard/glibc-dynamic:latest

USER nonroot

WORKDIR /valence

# Copy the release binary and the optional default config (env vars override it).
COPY --from=builder /valence/target/release/valence ./
COPY --from=builder /valence/config.toml ./

CMD ["/valence/valence"]
