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
import { AppSelect, Field, Input } from "../ui";

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
      <Field label={t("config.provider")}>
        <AppSelect
          value={target.providerId}
          onValueChange={(value) => {
            const provider = providers.find((item) => item.id === value);
            const next = provider && makeTarget(catalog, cliId, provider);
            if (next) onChange(next);
          }}
          groups={[
            {
              options: compatible.map((provider) => ({
                value: provider.id,
                label: providerInstanceDisplayName(catalog, provider),
              })),
            },
          ]}
        />
      </Field>
      {target.targetType === "api" ? (
        <Field label={t("config.protocol")}>
          <AppSelect
            value={target.connectionId}
            allowEmpty={!target.connectionId}
            placeholder={t("config.selectEndpoint")}
            onValueChange={(value) => {
              const connection = connections.find((item) => item.id === value);
              if (connection)
                onChange({
                  ...target,
                  connectionId: connection.id,
                  model: connection.defaultModel,
                });
            }}
            groups={[
              {
                options: connections.map((connection) => ({
                  value: connection.id,
                  label: selected
                    ? connectionDisplayName(catalog, selected, connection)
                    : connection.protocol,
                })),
              },
            ]}
          />
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
