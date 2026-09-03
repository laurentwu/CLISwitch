import type {
  ApiCliProviderRelation,
  ApiProviderTemplate,
  CatalogProviderInfo,
  CliId,
  ProviderCatalog,
  PublicProvider,
  PublicProviderConnection,
} from "./types";

export function catalogProviderInfos(catalog: ProviderCatalog): CatalogProviderInfo[] {
  return catalog.providerInfo ?? [];
}

export function catalogProviderInfo(
  catalog: ProviderCatalog,
  providerId?: string | null,
): CatalogProviderInfo | undefined {
  if (!providerId) return undefined;
  return catalogProviderInfos(catalog).find((provider) => provider.id === providerId);
}

export function providerDisplayName(
  catalog: ProviderCatalog,
  providerId: string,
  fallback?: string,
): string {
  const provider = catalogProviderInfo(catalog, providerId);
  if (!provider) return fallback ?? providerId;
  return `${provider.name} (${provider.id})`;
}

export function apiTemplate(
  catalog: ProviderCatalog,
  templateId?: string | null,
): ApiProviderTemplate | undefined {
  return catalog.providerTemplates.find(
    (template): template is ApiProviderTemplate =>
      template.mode === "api" && template.id === templateId,
  );
}

export function apiRelations(
  catalog: ProviderCatalog,
  cliId: CliId,
  templateId: string,
): ApiCliProviderRelation[] {
  return catalog.relations.filter(
    (relation): relation is ApiCliProviderRelation =>
      relation.mode === "api" &&
      relation.cliId === cliId &&
      relation.providerTemplateId === templateId,
  );
}

export function connectionsForCli(
  catalog: ProviderCatalog,
  cliId: CliId,
  provider: PublicProvider,
): PublicProviderConnection[] {
  if (provider.kind !== "api") return [];
  const dynamic = catalogProviderInfo(catalog, provider.templateId);
  if (dynamic) {
    if (!dynamic.selectable || !dynamic.supportedClis.includes(cliId)) return [];
    const endpointIds = new Set(
      apiRelations(catalog, cliId, dynamic.id).map((relation) => relation.endpointId),
    );
    const cliProtocols = catalog.clis.find((cli) => cli.id === cliId)?.protocols ?? [];
    return provider.connections.filter((connection) =>
      connection.templateEndpointId
        ? endpointIds.has(connection.templateEndpointId)
        : cliProtocols.includes(connection.protocol),
    );
  }
  if (provider.templateId) {
    // A saved provider can outlive the catalog snapshot which supplied its template. Preserve
    // those resolved connections using the fixed CLI protocol contract; current templates still
    // require their explicit endpoint relations below.
    if (!apiTemplate(catalog, provider.templateId)) {
      const protocols = catalog.clis.find((cli) => cli.id === cliId)?.protocols ?? [];
      return provider.connections.filter((connection) => protocols.includes(connection.protocol));
    }
    const endpointIds = new Set(
      apiRelations(catalog, cliId, provider.templateId).map((relation) => relation.endpointId),
    );
    return provider.connections.filter(
      (connection) =>
        Boolean(connection.templateEndpointId) &&
        endpointIds.has(connection.templateEndpointId as string),
    );
  }
  const protocols = catalog.clis.find((cli) => cli.id === cliId)?.protocols ?? [];
  return protocols.flatMap((protocol) =>
    provider.connections.filter((connection) => connection.protocol === protocol),
  );
}

export function providerSupportsCli(
  catalog: ProviderCatalog,
  cliId: CliId,
  provider: PublicProvider,
): boolean {
  const dynamic = catalogProviderInfo(catalog, provider.templateId);
  if (dynamic) return dynamic.selectable && dynamic.supportedClis.includes(cliId);
  if (provider.kind === "api") return connectionsForCli(catalog, cliId, provider).length > 0;
  if (provider.templateId) {
    return catalog.relations.some(
      (relation) =>
        relation.mode === "auth" &&
        relation.cliId === cliId &&
        relation.providerTemplateId === provider.templateId,
    );
  }
  return Boolean(
    provider.oauthKind &&
    catalog.clis
      .find((cli) => cli.id === cliId)
      ?.authModes.some((mode) => mode.oauthKind === provider.oauthKind),
  );
}

export function preferredConnectionForCli(
  catalog: ProviderCatalog,
  cliId: CliId,
  provider: PublicProvider,
): PublicProviderConnection | undefined {
  const connections = connectionsForCli(catalog, cliId, provider);
  if (connections.length <= 1) return connections[0];
  if (!provider.templateId) return connections[0];
  const defaultEndpoint = apiRelations(catalog, cliId, provider.templateId).find(
    (relation) => relation.default,
  )?.endpointId;
  return connections.find((connection) => connection.templateEndpointId === defaultEndpoint);
}

export function connectionDisplayName(
  catalog: ProviderCatalog,
  provider: PublicProvider,
  connection: PublicProviderConnection,
): string {
  const endpoint = apiTemplate(catalog, provider.templateId)?.endpoints.find(
    (candidate) => candidate.id === connection.templateEndpointId,
  );
  return endpoint?.name ?? connection.protocol;
}
