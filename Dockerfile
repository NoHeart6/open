FROM golang:1.23-alpine AS gbuild
WORKDIR /src/gateway
COPY gateway-go/go.mod ./
RUN go mod download || true
COPY gateway-go/ ./
RUN CGO_ENABLED=0 go build -o /out/gateway ./cmd/gateway

FROM node:24-alpine AS mbuild
WORKDIR /src/mcp
COPY mcp-ts/package.json mcp-ts/package-lock.json ./
RUN npm ci || npm install
COPY mcp-ts/tsconfig.json ./
COPY mcp-ts/src ./src
RUN npx tsc

FROM node:24-alpine
WORKDIR /app
RUN apk add --no-cache ca-certificates
COPY mcp-ts/package.json mcp-ts/package-lock.json ./
RUN npm ci --omit=dev || npm install --omit=dev
COPY --from=mbuild /src/mcp/dist ./dist
COPY --from=gbuild /out/gateway /usr/local/bin/gateway
COPY gateway-go/railway.json /app/railway-gateway.json
COPY mcp-ts/railway.json /app/railway-mcp.json
EXPOSE 8080 8787
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
CMD ["/usr/local/bin/entrypoint.sh"]