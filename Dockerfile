#FROM debian:buster-slim
#RUN apt-get install -y pkg-config libssl-dev
#
#FROM rust:1.57.0-alpine as builder
#RUN apk add --no-cache musl-dev
#WORKDIR /opt
#RUN USER=root cargo new --bin thalia
#WORKDIR /opt/thalia
#COPY ./Cargo.toml ./Cargo.toml
#
#RUN cargo build --release
#RUN rm ./src/*.rs
#RUN rm ./target/release/deps/thalia*
#
#ADD ./src ./src
#RUN cargo build --release
#
#FROM scratch
#WORKDIR /opt/thalia
#COPY --from=builder /opt/thalia/target/release/thalia .
#
#EXPOSE 5000
#CMD ["/opt/thalia/thalia"]

FROM rust:1.59.0 as builder

# Install libudev for 'hidapi'
RUN apt-get update -y \
    && apt-get install -y libudev-dev

RUN rustup component add rustfmt

# Build cargo crates
RUN USER=root cargo new --bin thalia
WORKDIR ./thalia
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --release
RUN rm src/*.rs

# Build thalia project
COPY . .
RUN rm ./target/release/deps/thalia*
RUN cargo build --release

# Run-time container
FROM debian:buster-slim
ARG APP=/usr/src/app

# Set specific user for container security
ENV APP_USER=appuser
RUN groupadd $APP_USER \
    && useradd -g $APP_USER $APP_USER \
    && mkdir -p ${APP}

RUN apt-get update -y \
    && apt-get install -y ca-certificates openssl

COPY --from=builder /thalia/target/release/thalia ${APP}/thalia

RUN chown -R $APP_USER:$APP_USER ${APP}

USER $APP_USER
WORKDIR ${APP}
CMD ["./thalia"]