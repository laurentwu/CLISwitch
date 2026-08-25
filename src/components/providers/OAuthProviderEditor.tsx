import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, LogIn, Save, Trash2, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command } from "../../shared/ipc";
import { validateEntityName } from "../../shared/names";
import type { OAuthProviderDetail, PublicProvider } from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import {
  Alert,
  Badge,
  Button,
  Card,
  Field,
  Input,
  Modal,
  Textarea,
  type ErrorReporter,
} from "../ui";

export function OAuthProviderEditor({
  detail,
  publicProvider,
  providers,
  onClose,
  onError,
  onStartFlow,
}: {
  detail: OAuthProviderDetail;
  publicProvider: PublicProvider;
  providers: PublicProvider[];
  onClose: () => void;
  onError: ErrorReporter;
  onStartFlow: (mode: "login" | "import") => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const setDirty = useUiStore((state) => state.setDirty);
  const setSaveCurrent = useUiStore((state) => state.setSaveCurrent);
  const [name, setName] = useState(detail.name);
  const [raw, setRaw] = useState(detail.rawContent);
  const [copied, setCopied] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const nameIssue = validateEntityName(name, providers, detail.id);

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: ["providers"] });
    await queryClient.invalidateQueries({ queryKey: ["provider-secret", detail.id] });
  };
  const dirty = name !== detail.name || raw !== detail.rawContent;
  const saveAll = useMutation({
    mutationFn: async () => {
      let revision = detail.revision;
      if (name !== detail.name) {
        const renamed = await command<PublicProvider>("rename_oauth_provider", {
          providerId: detail.id,
          expectedRevision: revision,
          name: name.trim(),
        });
        revision = renamed.revision;
      }
      if (raw !== detail.rawContent) {
        await command<OAuthProviderDetail>("update_oauth_raw_content", {
          providerId: detail.id,
          expectedRevision: revision,
          rawContent: raw,
        });
      }
    },
    onSuccess: async () => {
      setDirty(false);
      await refresh();
    },
    onError: (error) => onError(error, "save"),
  });
  useEffect(() => setDirty(dirty), [dirty, setDirty]);
  const saveCurrentRef = useRef<() => Promise<boolean>>(async () => false);
  useEffect(() => {
    saveCurrentRef.current = async () => {
      if (nameIssue) return false;
      try {
        await saveAll.mutateAsync();
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
  const remove = useMutation({
    mutationFn: () =>
      command("delete_provider", {
        providerId: detail.id,
        expectedRevision: detail.revision,
      }),
    onSuccess: async () => {
      setDirty(false);
      queryClient.removeQueries({ queryKey: ["provider-secret", detail.id] });
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      onClose();
    },
    onError: (error) => onError(error, "delete"),
  });

  return (
    <div className="editor">
      <header className="editor-header">
        <div>
          <h2>{detail.name}</h2>
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
        </div>
        <Button variant="ghost" onClick={onClose}>
          {t("common.close")}
        </Button>
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
          <Field label={t("common.account")}>
            <Input readOnly value={detail.accountLabel ?? detail.accountId ?? t("common.none")} />
          </Field>
        </div>
        <div className="section-actions">
          <Button
            variant="secondary"
            disabled={Boolean(nameIssue) || !dirty || saveAll.isPending}
            onClick={() => saveAll.mutate()}
          >
            <Save size={15} /> {t("common.save")}
          </Button>
          <Button variant="secondary" onClick={() => onStartFlow("login")}>
            <LogIn size={15} /> {t("providers.login")}
          </Button>
          <Button variant="secondary" onClick={() => onStartFlow("import")}>
            <Upload size={15} /> {t("providers.import")}
          </Button>
        </div>
      </Card>

      <Field
        label={t("providers.raw")}
        hint={
          <>
            {t("providers.rawWarning")} {t("providers.clipboardWarning")}
          </>
        }
      >
        <Textarea
          className="secret-editor"
          value={raw}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => setRaw(event.target.value)}
        />
      </Field>
      {detail.manuallyModified ? (
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
          <Copy size={15} /> {copied ? t("common.copied") : t("common.copy")}
        </Button>
        <Button
          disabled={Boolean(nameIssue) || !dirty || saveAll.isPending}
          onClick={() => saveAll.mutate()}
        >
          <Save size={15} /> {t("common.save")}
        </Button>
      </div>

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
        <Button
          variant="danger"
          disabled={publicProvider.referencedBy.length > 0 || remove.isPending}
          onClick={() => setDeleteOpen(true)}
        >
          <Trash2 size={15} /> {t("common.delete")}
        </Button>
      </Card>
      <Modal
        open={deleteOpen}
        title={t("common.confirmDelete")}
        onClose={() => setDeleteOpen(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleteOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" disabled={remove.isPending} onClick={() => remove.mutate()}>
              {t("common.delete")}
            </Button>
          </>
        }
      >
        <p>
          {detail.name}: {t("providers.deleteWarning")}
        </p>
      </Modal>
    </div>
  );
}
