FROM rust:1.70-alpine as builder
WORKDIR /usr/src/lessanvil
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY ../ /app
RUN cargo build --release -p lessanvil-cli
RUN strip target/release/lessanvil-cli

FROM alpine:latest
COPY --from=builder /app/target/release/lessanvil-cli /usr/local/bin/lessanvil-cli
ENTRYPOINT [ "/usr/local/bin/lessanvil-cli", "--confirm" ]
CMD [ "-w", "/var/world" ]