FROM node:24-alpine AS mcp
WORKDIR /app/mcp-ts
COPY mcp-ts/package*.json ./
RUN npm ci --omit=dev || npm install --omit=dev
COPY mcp-ts ./
RUN npm run build || npx tsc
CMD ["node","dist/server.js"]
