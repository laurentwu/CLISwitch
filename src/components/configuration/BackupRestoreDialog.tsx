import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RotateCcw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command, errorMessage } from "../../shared/ipc";
import type { BackupMetadata, CliId, RestorePreview, ScanSnapshot } from "../../shared/types";
import { Badge, Button, EmptyState, Modal, Spinner } from "../ui";

export function BackupRestoreDialog({
  open,
  cliId,
  onClose,
  onError,
}: {
  open: boolean;
  cliId?: CliId;
  onClose: () => void;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [restorePreview, setRestorePreview] = useState<RestorePreview>();
  const backups = useQuery({
    queryKey: ["backups", cliId ?? "all"],
    queryFn: () => command<BackupMetadata[]>("list_backups", { cliId: cliId ?? null }),
    enabled: open,
  });
  const prepareRestore = useMutation({
    mutationFn: (backup: BackupMetadata) =>
      command<RestorePreview>("preview_restore", { backupId: backup.id }),
    onSuccess: setRestorePreview,
    onError: (error) => onError(errorMessage(error)),
  });
  const restore = useMutation({
    mutationFn: (preview: RestorePreview) =>
      command<ScanSnapshot>("restore_backup", { previewId: preview.id }),
    onSuccess: (scan) => {
      setRestorePreview(undefined);
      queryClient.setQueryData(["scan"], scan);
      void queryClient.invalidateQueries({ queryKey: ["backups"] });
      void queryClient.invalidateQueries({ queryKey: ["app-snapshot"] });
    },
    onError: (error) => onError(errorMessage(error)),
  });
  return (
    <>
      <Modal
        open={open}
        title={t("config.backups")}
        onClose={() => {
          setRestorePreview(undefined);
          onClose();
        }}
        wide
      >
        {backups.isPending ? <Spinner /> : null}
        {!backups.isPending && !backups.data?.length ? (
          <EmptyState>{t("common.none")}</EmptyState>
        ) : null}
        <div className="backup-list">
          {backups.data?.map((backup) => (
            <article className="backup-row" key={backup.id}>
              <div>
                <strong>{backup.cliId}</strong>
                <div className="path-text">{backup.originalPath}</div>
                <small>{new Date(backup.createdAt).toLocaleString()}</small>
              </div>
              <div className="row-actions">
                {backup.containsCredentials ? (
                  <Badge tone="warn">{t("common.credentials")}</Badge>
                ) : null}
                {!backup.originallyExisted ? (
                  <Badge tone="warn">{t("common.tombstone")}</Badge>
                ) : null}
                <Button
                  variant="secondary"
                  disabled={prepareRestore.isPending || restore.isPending}
                  onClick={() => prepareRestore.mutate(backup)}
                >
                  <RotateCcw size={15} /> {t("config.restore")}
                </Button>
              </div>
            </article>
          ))}
        </div>
      </Modal>
      <Modal
        open={Boolean(restorePreview)}
        title={t("common.confirmRestore")}
        onClose={() => setRestorePreview(undefined)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRestorePreview(undefined)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              disabled={restore.isPending}
              onClick={() => restorePreview && restore.mutate(restorePreview)}
            >
              {t("config.restore")}
            </Button>
          </>
        }
      >
        <div className="path-text">{restorePreview?.targetPath}</div>
        <p>{restorePreview?.restoresTombstone ? t("config.tombstone") : t("config.restore")}</p>
      </Modal>
    </>
  );
}
