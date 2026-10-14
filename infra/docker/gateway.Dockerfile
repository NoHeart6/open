FROM golang:1.26.9-alpine AS build
WORKDIR /src
COPY apps/gateway/go.mod apps/gateway/go.sum ./
RUN go mod download
COPY apps/gateway/ ./
RUN CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o /gateway .
FROM alpine:3.23
RUN apk add --no-cache ca-certificates su-exec sqlite && addgroup -g 10001 app && adduser -D -u 10001 -G app app && mkdir /data && chown app:app /data
USER root
COPY --from=build /gateway /usr/local/bin/gateway
COPY --chmod=755 infra/docker/gateway-entrypoint.sh /usr/local/bin/gateway-entrypoint
RUN sed -i 's/\r$//' /usr/local/bin/gateway-entrypoint
ENV GATEWAY_ADDR=0.0.0.0:8080 GATEWAY_DB=/data/gateway.db
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD wget -q -O /dev/null http://127.0.0.1:8080/healthz || exit 1
ENTRYPOINT ["gateway-entrypoint"]
