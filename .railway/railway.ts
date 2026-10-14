import { defineRailway, postgres, project, service, volume } from "railway/iac";

export default defineRailway(() => {
  const Postgres = postgres("Postgres", { region: "iad" });
  Postgres.networking = { privateNetworkEndpoint: "postgres" };
  const gatewayData = volume("gateway-data", { alerts: { usage: { "100": {}, "80": {}, "95": {} } }, allowOnlineResize: true, region: "iad", sizeMB: 500 });
  const postgresVolumeZFXv = volume("postgres-volume-zFXv", { alerts: { usage: { "100": {}, "80": {}, "95": {} } }, allowOnlineResize: true, region: "iad", sizeMB: 500 });
  const gateway = service("gateway", {
    build: { buildEnvironment: "V3", builder: "DOCKERFILE", dockerfilePath: "infra/docker/gateway.Dockerfile" },
    healthcheck: "/healthz",
    replicas: { "iad": 1 },
    deploy: { drainingSeconds: 10, overlapSeconds: 0 },
    volumeMounts: { "/data": gatewayData },
  });
  const auth = service("auth", {
    build: { buildEnvironment: "V3", builder: "DOCKERFILE", dockerfilePath: "infra/docker/auth.Dockerfile" },
    healthcheck: "/realms/opencommander",
    healthcheckTimeout: 300,
    replicas: { "iad": 1 },
    deploy: { drainingSeconds: 15, overlapSeconds: 0, restartPolicyMaxRetries: 5 },
  });
  const mcp = service("mcp", {
    build: { buildEnvironment: "V3", builder: "DOCKERFILE", dockerfilePath: "infra/docker/mcp.Dockerfile" },
    healthcheck: "/healthz",
    replicas: { "iad": 1 },
    deploy: { drainingSeconds: 5, overlapSeconds: 0 },
  });

  return project("OpenCommander", {
    variables: { managed: false },
    resources: [Postgres, gateway, auth, mcp, gatewayData, postgresVolumeZFXv],
  });
});
