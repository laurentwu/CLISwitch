import { KeyRound, LogIn, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProviderTemplate } from "../../shared/types";
import { Button, Modal } from "../ui";

export type ProviderTemplateIntent = "add" | "import";

type TemplateGroup = "oauth" | "api" | "coding-plan" | "other";

type TemplateChoice = {
  id: string;
  name: string;
  group: TemplateGroup;
  template?: ProviderTemplate;
};

const groups: TemplateGroup[] = ["oauth", "api", "coding-plan", "other"];

function groupFor(template: ProviderTemplate): TemplateGroup {
  if (template.mode === "auth") return "oauth";
  if (template.category === "api") return "api";
  if (template.category === "coding-plan") return "coding-plan";
  return "other";
}

export function ProviderTemplateDialog({
  open,
  intent,
  templates,
  onClose,
  onSelect,
}: {
  open: boolean;
  intent: ProviderTemplateIntent;
  templates: ProviderTemplate[];
  onClose: () => void;
  onSelect: (template?: ProviderTemplate) => void;
}) {
  const { t } = useTranslation();
  const choices: TemplateChoice[] = templates
    .filter((template) => intent === "add" || template.mode === "auth")
    .map((template) => ({
      id: template.id,
      name: template.name,
      group: groupFor(template),
      template,
    }));

  if (intent === "add") {
    choices.push({
      id: "custom-provider",
      name: t("providers.customTemplate"),
      group: "other",
    });
  }

  return (
    <Modal
      open={open}
      wide
      title={
        intent === "add" ? t("providers.chooseAddTemplate") : t("providers.chooseImportTemplate")
      }
      onClose={onClose}
      footer={
        <Button variant="ghost" onClick={onClose}>
          {t("common.cancel")}
        </Button>
      }
    >
      <p className="template-picker-hint">
        {intent === "add" ? t("providers.addTemplateHint") : t("providers.importTemplateHint")}
      </p>
      <div className="template-groups">
        {groups.map((group) => {
          const groupedChoices = choices.filter((choice) => choice.group === group);
          if (!groupedChoices.length) return null;
          return (
            <section className="template-group" key={group}>
              <h3>{t(`providers.templateCategory.${group}`)}</h3>
              <div className="template-options">
                {groupedChoices.map((choice) => {
                  const operation =
                    intent === "import"
                      ? t("providers.import")
                      : choice.template?.mode === "auth"
                        ? t("providers.login")
                        : t("providers.configureTemplate");
                  return (
                    <button
                      className="template-option"
                      key={choice.id}
                      type="button"
                      onClick={() => onSelect(choice.template)}
                    >
                      <span className="template-option-icon">
                        {intent === "import" ? (
                          <Upload size={18} />
                        ) : choice.template?.mode === "auth" ? (
                          <LogIn size={18} />
                        ) : (
                          <KeyRound size={18} />
                        )}
                      </span>
                      <span className="template-option-copy">
                        <strong>{choice.name}</strong>
                        <small>{operation}</small>
                      </span>
                    </button>
                  );
                })}
              </div>
            </section>
          );
        })}
      </div>
    </Modal>
  );
}
