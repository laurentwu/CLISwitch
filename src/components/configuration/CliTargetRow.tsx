import { useTranslation } from "react-i18next";
import type { CliId, ConfigurationTarget, PublicProvider } from "../../shared/types";
import { CLI_PROTOCOLS } from "../../shared/types";
import { Field, Input, Select } from "../ui";

function compatibleProviders(cliId: CliId, providers: PublicProvider[]) {
  return providers.filter((provider) =>
    provider.kind === "oauth"
      ? (provider.oauthKind === "anthropic" && cliId === "claude-code") ||
        (provider.oauthKind === "codex" && cliId === "codex")
      : provider.connections.some((connection) =>
          CLI_PROTOCOLS[cliId].includes(connection.protocol),
        ),
  );
}

export function makeTarget(
  cliId: CliId,
  provider: PublicProvider,
): ConfigurationTarget | undefined {
  if (provider.kind === "oauth")
    return { targetType: "oauth", cliId, providerId: provider.id, model: "default" };
  const connection = CLI_PROTOCOLS[cliId]
    .map((protocol) => provider.connections.find((item) => item.protocol === protocol))
    .find((item) => item !== undefined);
  if (!connection) return undefined;
  return {
    targetType: "api",
    cliId,
    providerId: provider.id,
    connectionId: connection.id,
    model: connection.defaultModel,
  };
}

export function CliTargetRow({
  cliId,
  target,
  providers,
  onChange,
}: {
  cliId: CliId;
  target: ConfigurationTarget;
  providers: PublicProvider[];
  onChange: (target: ConfigurationTarget) => void;
}) {
  const { t } = useTranslation();
  const compatible = compatibleProviders(cliId, providers);
  const selected = providers.find((provider) => provider.id === target.providerId);
  const connections =
    selected?.kind === "api"
      ? selected.connections.filter((connection) =>
          CLI_PROTOCOLS[cliId].includes(connection.protocol),
        )
      : [];
  return (
    <div className="target-grid">
      <strong>{cliId}</strong>
      <Field label={t("config.provider")}>
        <Select
          value={target.providerId}
          onChange={(event) => {
            const provider = providers.find((item) => item.id === event.target.value);
            const next = provider && makeTarget(cliId, provider);
            if (next) onChange(next);
          }}
        >
          {compatible.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {provider.name}
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
            {connections.map((connection) => (
              <option key={connection.id} value={connection.id}>
                {connection.protocol}
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
