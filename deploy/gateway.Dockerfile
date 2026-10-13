FROM golang:1.23-alpine AS build
WORKDIR /src
COPY gateway-go/go.mod gateway-go/go.sum* ./gateway-go/
WORKDIR /src/gateway-go
RUN go mod download || true
COPY gateway-go ./
RUN go build -o /gateway ./cmd/gateway
FROM alpine:3.20
COPY --from=build /gateway /gateway
EXPOSE 8080
CMD ["/gateway"]
