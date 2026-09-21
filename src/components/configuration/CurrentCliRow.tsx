import { ArchiveRestore, ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { diagnosticText } from "../../shared/diagnostics";
import { cliDisplayName } from "../../shared/names";
import type { CliId, DetectedCli, DetectedProviderCandidate } from "../../shared/types";
import { CliIcon } from "../CliIcon";
import { Alert, Badge, Button } from "../ui";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../ui/primitives/collapsible";

function statusTone(status: DetectedCli["status"]): "neutral" | "good" | "warn" | "bad" {
  if (status === "detected") return "good";
  if (["unmanaged", "partially-detected", "externally-overridden"].includes(status)) return "warn";
  if (["unreadable", "invalid-config"].includes(status)) return "bad";
  return "neutral";
}

export function CurrentCliRow({
  cliId,
  detected,
  expanded,
  onExpandedChange,
  onOpenBackups,
  onManageCandidate,
}: {
  cliId: CliId;
  detected?: DetectedCli;
  expanded: boolean;
  onExpandedChange: (expanded: boolean) => void;
  onOpenBackups: (cliId: CliId) => void;
  onManageCandidate: (candidate: DetectedProviderCandidate) => void;
}) {
  const { t } = useTranslation();
  const label = cliDisplayName(cliId);
  const tone = detected ? statusTone(detected.status) : "neutral";
  const toggleLabel = t(expanded ? "config.hideDetails" : "config.showDetails", { cli: label });

  return (
    <Collapsible
      open={expanded}
      onOpenChange={onExpandedChange}
      className="current-cli-row"
      data-cli-id={cliId}
    >
      <div className="current-cli-summary">
        <div className="current-cli-identity">
          <CliIcon cliId={cliId} />
          <div className="current-cli-field">
            <h3>{label}</h3>
            <small>{detected?.version ?? "—"}</small>
          </div>
        </div>
        <div className="current-cli-field" aria-label={t("config.status")}>
          <Badge tone={tone}>
            {t(detected ? `status.${detected.status}` : "config.notScanned")}
          </Badge>
        </div>
        <div
          className="current-cli-field current-cli-field-provider"
          aria-label={t("config.provider")}
        >
          <small>{t("config.provider")}</small>
          {detected?.current?.providerName ?? "—"}
        </div>
        <div className="current-cli-field current-cli-field-model" aria-label={t("config.model")}>
          <small>{t("config.model")}</small>
          {detected?.current?.model ?? "—"}
        </div>
        <div className="row-actions" aria-label={t("common.actions")}>
          <Button variant="ghost" disabled={!detected} onClick={() => onOpenBackups(cliId)}>
            <ArchiveRestore size={15} /> {t("config.rowBackups")}
          </Button>
          <CollapsibleTrigger asChild>
            <Button
              variant="ghost"
              aria-label={toggleLabel}
              title={toggleLabel}
              disabled={!detected}
            >
              <ChevronDown
                size={16}
                aria-hidden="true"
                style={{ transform: expanded ? "rotate(180deg)" : undefined }}
              />
            </Button>
          </CollapsibleTrigger>
        </div>
      </div>
      {detected ? (
        <CollapsibleContent className="current-cli-details">
          <dl className="detail-grid">
            <dt>{t("config.executable")}</dt>
            <dd className="path-text">{detected.executablePath ?? "—"}</dd>
            <dt>{t("config.directory")}</dt>
            <dd className="path-text">{detected.configDirectory}</dd>
            <dt>{t("config.protocol")}</dt>
            <dd>{detected.current?.protocol ?? "—"}</dd>
            <dt>{t("providers.authType")}</dt>
            <dd>{detected.current?.authKind ?? "—"}</dd>
          </dl>
        </CollapsibleContent>
      ) : null}
      {detected?.current?.diagnostics.length ||
      detected?.current?.externallyOverridden ||
      detected?.providerCandidates?.length ? (
        <div className="current-cli-messages">
          {detected.current?.externallyOverridden ? (
            <Alert compact tone="warning" title={t("status.externally-overridden")} />
          ) : null}
          {detected.current?.diagnostics.map((message, index) => (
            <Alert
              key={index}
              compact
              tone={tone === "bad" ? "error" : "warning"}
              title={t("config.scanDiagnostic")}
            >
              <p>{diagnosticText(t, message)}</p>
            </Alert>
          ))}
          {detected.providerCandidates?.map((candidate) => (
            <div className="section-actions" key={candidate.id}>
              <Button variant="secondary" onClick={() => onManageCandidate(candidate)}>
                {t("config.manageCandidateNamed", { name: candidate.suggestedName })}
              </Button>
            </div>
          ))}
        </div>
      ) : null}
    </Collapsible>
  );
}
