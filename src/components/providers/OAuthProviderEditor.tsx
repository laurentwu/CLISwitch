import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, Files, LogIn, Save, Trash2, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command } from "../../shared/ipc";
import { validateEntityName } from "../../shared/names";
import type {
  AuthProviderTemplate,
  OAuthKind,
  OAuthProviderDetail,
  ProviderCatalog,
  PublicProvider,
} from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { Alert, Badge, Button, Card, Field, Input, Textarea, type ErrorReporter } from "../ui";
import { ProviderTemplateSelect } from "./ProviderTemplateSelect";

export type OAuthProviderDraft = {
  templateId: string;
  kind: OAuthKind;
  name: string;
  rawContent: string;
};

export function OAuthProviderEditor({
  detail,
  publicProvider,
  catalog,
  initialTemplateId,
  initialName,
  initialRaw,
  providers,
  onError,
  onStartFlow,
  onChooseApiTemplate,
  onDuplicate,
  onDelete,
  deleteDisabled = false,
  onCreated,
}: {
  detail?: OAuthProviderDetail;
  publicProvider?: PublicProvider;
  catalog: ProviderCatalog;
  initialTemplateId?: string;
  initialName?: string;
  initialRaw?: string;
  providers: PublicProvider[];
  onError: ErrorReporter;
  onStartFlow: (
    kind: OAuthKind,
    mode: "login" | "import",
    name: string,
    replaceProviderId?: string,
  ) => void;
  onChooseApiTemplate?: (templateId: string, currentName: string) => void;
  onDuplicate?: (draft: OAuthProviderDraft) => void;
  onDelete?: () => void;
  deleteDisabled?: boolean;
  onCreated?: (providerId: string) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const setDirty = useUiStore((state) => state.setDirty);
  const setSaveCurrent = useUiStore((state) => state.setSaveCurrent);
  const authTemplates = catalog.providerTemplates.filter(
    (template): template is AuthProviderTemplate => template.mode === "auth",
  );
  const detailTemplate =
    authTemplates.find((template) => template.id === detail?.templateId) ??
    authTemplates.find((template) => template.authKind === detail?.oauthKind);
  const [templateId, setTemplateId] = useState(
    detailTemplate?.id ?? initialTemplateId ?? authTemplates[0]?.id ?? "",
  );
  const selectedTemplate = authTemplates.find((template) => template.id === templateId);
  const kind = detail?.oauthKind ?? selectedTemplate?.authKind;
  const [name, setName] = useState(detail?.name ?? initialName ?? selectedTemplate?.name ?? "");
  const [raw, setRaw] = useState(detail?.rawContent ?? initialRaw ?? "");
  const [copied, setCopied] = useState(false);
  const nameIssue = validateEntityName(name, providers, detail?.id);
  const initialCreationTemplateId = initialTemplateId ?? authTemplates[0]?.id ?? "";
  const initialCreationName = initialName ?? selectedTemplate?.name ?? "";
  const initialCreationRaw = initialRaw ?? "";
  const dirty = detail
    ? name !== detail.name || raw !== detail.rawContent
    : templateId !== initialCreationTemplateId ||
      name !== initialCreationName ||
      raw !== initialCreationRaw;

  const save = useMutation({
    mutationFn: async () => {
      if (!kind) throw new Error("OAuth template is required");
      if (detail) {
        return command<PublicProvider>("update_oauth_provider", {
          providerId: detail.id,
          expectedRevision: detail.revision,
          name: name.trim(),
          rawContent: raw,
        });
      }
      return command<PublicProvider>("create_oauth_provider", {
        kind,
        name: name.trim(),
        rawContent: raw,
      });
    },
    onSuccess: async (provider) => {
      setDirty(false);
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      if (detail) {
        await queryClient.invalidateQueries({ queryKey: ["provider-secret", detail.id] });
      } else {
        onCreated?.(provider.id);
      }
    },
    onError: (error) => onError(error, detail ? "save" : "create"),
  });
  const saveDisabled =
    Boolean(nameIssue) ||
    !kind ||
    save.isPending ||
    (!detail && !raw.trim()) ||
    Boolean(detail && !dirty);

  useEffect(() => setDirty(dirty), [dirty, setDirty]);
  const saveCurrentRef = useRef<() => Promise<boolean>>(async () => false);
  useEffect(() => {
    saveCurrentRef.current = async () => {
      if (nameIssue || !kind || (!detail && !raw.trim())) return false;
      if (detail && !dirty) return true;
      try {
        await save.mutateAsync();
        return true;
      } catch {
        return false;
      }
    };
  });
  useEffect(() => {
    const saveCurrent = () => saveCurrentRef.current();
    setSaveCurrent(saveCurrent);
    return () => setSaveCurrent(undefined);
  }, [setSaveCurrent]);

  const chooseTemplate = (nextTemplateId: string) => {
    const nextTemplate = authTemplates.find((template) => template.id === nextTemplateId);
    if (!nextTemplate) {
      const apiTemplate = catalog.providerTemplates.find(
        (template) => template.id === nextTemplateId && template.mode === "api",
      );
      onChooseApiTemplate?.(
        nextTemplateId,
        !name.trim() || name === selectedTemplate?.name ? (apiTemplate?.name ?? "") : name,
      );
      return;
    }
    if (!name.trim() || name === selectedTemplate?.name) setName(nextTemplate.name);
    if (selectedTemplate && selectedTemplate.authKind !== nextTemplate.authKind) setRaw("");
    setTemplateId(nextTemplate.id);
  };

  return (
    <div className="editor">
      <header className="editor-header">
        <div>
          <h2>{detail ? detail.name : (selectedTemplate?.name ?? t("providers.addOauth"))}</h2>
          {detail ? (
            <div className="badge-row">
              <Badge>{detail.oauthKind === "anthropic" ? "Anthropic OAuth" : "Codex OAuth"}</Badge>
              <Badge
                tone={
                  detail.manuallyModified
                    ? "warn"
                    : detail.verification.status === "valid"
                      ? "good"
                      : detail.verification.status === "invalid"
                        ? "bad"
                        : detail.verification.status === "never-tested"
                          ? "neutral"
                          : "warn"
                }
              >
                {t(`status.${detail.verification.status}`)}
              </Badge>
            </div>
          ) : null}
        </div>
        <div className="section-actions">
          {detail && onDelete ? (
            <Button variant="danger" disabled={deleteDisabled} onClick={onDelete}>
              <Trash2 size={16} /> {t("common.delete")}
            </Button>
          ) : null}
          {detail && onDuplicate && kind ? (
            <Button
              variant="secondary"
              disabled={Boolean(nameIssue)}
              onClick={() => onDuplicate({ templateId, kind, name: name.trim(), rawContent: raw })}
            >
              <Files size={16} /> {t("common.duplicate")}
            </Button>
          ) : null}
          <Button disabled={saveDisabled} onClick={() => save.mutate()}>
            <Save size={16} /> {t("common.save")}
          </Button>
        </div>
      </header>

      <Card>
        <div className="form-grid two-columns">
          <Field
            label={t("providers.name")}
            hint={
              nameIssue
                ? t(nameIssue === "length" ? "validation.nameLength" : "validation.nameDuplicate")
                : undefined
            }
          >
            <Input value={name} onChange={(event) => setName(event.target.value)} />
          </Field>
          {detail ? (
            <Field label={t("common.account")}>
              <Input readOnly value={detail.accountLabel ?? detail.accountId ?? t("common.none")} />
            </Field>
          ) : (
            <Field label={t("providers.template")} hint={t("providers.templateHint")}>
              <ProviderTemplateSelect
                catalog={catalog}
                value={templateId}
                onChange={chooseTemplate}
              />
            </Field>
          )}
        </div>
        <div className="section-actions">
          <Button
            variant="secondary"
            disabled={Boolean(nameIssue) || !kind}
            onClick={() => kind && onStartFlow(kind, "login", name.trim(), detail?.id)}
          >
            <LogIn size={15} /> {t("providers.login")}
          </Button>
          <Button
            variant="secondary"
            disabled={Boolean(nameIssue) || !kind}
            onClick={() => kind && onStartFlow(kind, "import", name.trim(), detail?.id)}
          >
            <Upload size={15} /> {t("providers.import")}
          </Button>
        </div>
      </Card>

      <Field
        label={t("providers.raw")}
        hint={
          <>
            {t("providers.rawSaveValidation")} {t("providers.clipboardWarning")}
          </>
        }
      >
        <Textarea
          className="secret-editor"
          value={raw}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => {
            setRaw(event.target.value);
            setCopied(false);
          }}
        />
      </Field>
      {detail?.manuallyModified ? (
        <Alert tone="warning" title={t("providers.manualWarning")} />
      ) : null}
      <div className="section-actions">
        <Button
          variant="secondary"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(raw);
              setCopied(true);
            } catch (error) {
              onError(error, "copy");
            }
          }}
        >
          <Copy size={15} /> {copied ? t("common.copied") : t("providers.copyAuth")}
        </Button>
      </div>

      {publicProvider ? (
        <Card>
          <h3>{t("providers.references")}</h3>
          {publicProvider.referencedBy.length ? (
            <ul>
              {publicProvider.referencedBy.map((reference) => (
                <li key={reference}>{reference}</li>
              ))}
            </ul>
          ) : (
            <p>{t("common.none")}</p>
          )}
        </Card>
      ) : null}
    </div>
  );
}
