FROM node:24-alpine AS build
WORKDIR /app
COPY apps/mcp-server/package*.json ./
RUN npm ci
COPY apps/mcp-server/tsconfig.json ./
COPY apps/mcp-server/src/ ./src/
RUN npm run build && npm prune --omit=dev
FROM node:24-alpine
ENV NODE_ENV=production MCP_HOST=0.0.0.0 MCP_PORT=3000
WORKDIR /app
COPY --from=build --chown=node:node /app/package*.json ./
COPY --from=build --chown=node:node /app/node_modules/ ./node_modules/
COPY --from=build --chown=node:node /app/dist/ ./dist/
COPY --chown=node:node releases/windows/ ./releases/windows/
USER node
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s CMD node -e "fetch('http://127.0.0.1:3000/healthz').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"
CMD ["node","dist/index.js"]
