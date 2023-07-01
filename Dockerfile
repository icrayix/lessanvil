FROM rust:1.70-alpine as builder
WORKDIR /usr/src/lessanvil
RUN apk add musl-dev
COPY . .
RUN cargo install --path cli

FROM alpine:latest
COPY --from=builder /usr/local/cargo/bin/lessanvil-cli /usr/local/bin/lessanvil-cli
ENTRYPOINT [ "/usr/local/bin/lessanvil-cli" ]
CMD [ "-w", "/var/world", "--confirm" ]