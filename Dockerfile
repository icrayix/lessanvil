FROM rust:1.68-alpine as builder
WORKDIR /usr/src/lessanvil
RUN apk add musl-dev
COPY . .
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN cargo install --path .

FROM alpine:latest
COPY --from=builder /usr/local/cargo/bin/lessanvil /usr/local/bin/lessanvil
ENTRYPOINT [ "/usr/local/bin/lessanvil" ]
CMD [ "-w", "/var/world", "--confirm" ]