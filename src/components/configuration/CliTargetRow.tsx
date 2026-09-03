import { useTranslation } from "react-i18next";
import {
  connectionDisplayName,
  connectionsForCli,
  preferredConnectionForCli,
  providerInstanceDisplayName,
  providerSupportsCli,
} from "../../shared/catalog";
import type {
  CliId,
  ConfigurationTarget,
  ProviderCatalog,
  PublicProvider,
} from "../../shared/types";
import { Field, Input, Select } from "../ui";

function compatibleProviders(catalog: ProviderCatalog, cliId: CliId, providers: PublicProvider[]) {
  return providers.filter((provider) => providerSupportsCli(catalog, cliId, provider));
}

export function makeTarget(
  catalog: ProviderCatalog,
  cliId: CliId,
  provider: PublicProvider,
): ConfigurationTarget | undefined {
  if (provider.kind === "oauth")
    return { targetType: "oauth", cliId, providerId: provider.id, model: "default" };
  const compatible = connectionsForCli(catalog, cliId, provider);
  if (!compatible.length) return undefined;
  const connection = preferredConnectionForCli(catalog, cliId, provider);
  return {
    targetType: "api",
    cliId,
    providerId: provider.id,
    connectionId: connection?.id ?? "",
    model: connection?.defaultModel ?? "",
  };
}

export function CliTargetRow({
  cliId,
  target,
  providers,
  catalog,
  onChange,
}: {
  cliId: CliId;
  target: ConfigurationTarget;
  providers: PublicProvider[];
  catalog: ProviderCatalog;
  onChange: (target: ConfigurationTarget) => void;
}) {
  const { t } = useTranslation();
  const compatible = compatibleProviders(catalog, cliId, providers);
  const selected = providers.find((provider) => provider.id === target.providerId);
  const connections = selected?.kind === "api" ? connectionsForCli(catalog, cliId, selected) : [];
  return (
    <div className="target-grid">
      <strong>{cliId}</strong>
      <Field label={t("config.provider")}>
        <Select
          value={target.providerId}
          onChange={(event) => {
            const provider = providers.find((item) => item.id === event.target.value);
            const next = provider && makeTarget(catalog, cliId, provider);
            if (next) onChange(next);
          }}
        >
          {compatible.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {providerInstanceDisplayName(catalog, provider)}
            </option>
          ))}
        </Select>
      </Field>
      {target.targetType === "api" ? (
        <Field label={t("config.protocol")}>
          <Select
            value={target.connectionId}
            onChange={(event) => {
              const connection = connections.find((item) => item.id === event.target.value);
              if (connection)
                onChange({
                  ...target,
                  connectionId: connection.id,
                  model: connection.defaultModel,
                });
            }}
          >
            {!target.connectionId ? <option value="">{t("config.selectEndpoint")}</option> : null}
            {connections.map((connection) => (
              <option key={connection.id} value={connection.id}>
                {selected
                  ? connectionDisplayName(catalog, selected, connection)
                  : connection.protocol}
              </option>
            ))}
          </Select>
        </Field>
      ) : (
        <div className="oauth-target-label">
          OAuth · {selected?.oauthKind}
          {selected?.oauthAccountLabel ? ` · ${selected.oauthAccountLabel}` : ""}
        </div>
      )}
      <Field label={t("config.model")}>
        <Input
          value={target.model}
          onChange={(event) => onChange({ ...target, model: event.target.value })}
        />
      </Field>
    </div>
  );
}
