FROM quay.io/keycloak/keycloak:26.8.0 AS build
ENV KC_DB=postgres KC_HEALTH_ENABLED=true KC_FEATURES=cimd,resource-indicators
RUN /opt/keycloak/bin/kc.sh build
FROM quay.io/keycloak/keycloak:26.8.0
COPY --from=build /opt/keycloak/ /opt/keycloak/
COPY infra/keycloak/opencommander-realm.json /opt/keycloak/data/import/opencommander-realm.json
ENV KC_DB=postgres KC_HEALTH_ENABLED=true KC_FEATURES=cimd,resource-indicators
ENTRYPOINT ["/opt/keycloak/bin/kc.sh"]
CMD ["start", "--optimized", "--import-realm"]
