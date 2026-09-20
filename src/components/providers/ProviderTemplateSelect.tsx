import { useTranslation } from "react-i18next";
import { catalogProviderInfos } from "../../shared/catalog";
import type { ProviderCatalog } from "../../shared/types";
import { AppSelect, type AppSelectProps } from "../ui";

export const CUSTOM_PROVIDER_TEMPLATE = "__custom-provider__";

export function ProviderTemplateSelect({
  catalog,
  value,
  onChange,
  ...triggerProps
}: {
  catalog: ProviderCatalog;
  value: string;
  onChange: (value: string) => void;
} & Pick<
  AppSelectProps,
  "id" | "aria-label" | "aria-labelledby" | "aria-describedby" | "aria-invalid"
>) {
  const { t } = useTranslation();
  const dynamicProviders = catalogProviderInfos(catalog);
  if (dynamicProviders.length) {
    return (
      <AppSelect
        value={value}
        onValueChange={onChange}
        allowEmpty
        placeholder={t("providers.chooseTemplate")}
        groups={[
          {
            label: t("providers.templateCategory.api"),
            options: [
              ...dynamicProviders.map((provider) => ({
                value: provider.id,
                label: `${provider.name} (${provider.id})`,
                disabled: !provider.selectable,
                description: provider.disabledReason ?? undefined,
              })),
              { value: CUSTOM_PROVIDER_TEMPLATE, label: t("providers.customTemplate") },
            ],
          },
          {
            label: t("providers.templateCategory.oauth"),
            options: catalog.providerTemplates
              .filter((template) => template.mode === "auth")
              .map((template) => ({ value: template.id, label: template.name })),
          },
        ]}
        {...triggerProps}
      />
    );
  }
  const oauthTemplates = catalog.providerTemplates.filter((template) => template.mode === "auth");
  const apiTemplates = catalog.providerTemplates.filter((template) => template.mode === "api");

  return (
    <AppSelect
      value={value}
      onValueChange={onChange}
      allowEmpty
      placeholder={t("providers.chooseTemplate")}
      groups={[
        {
          label: t("providers.templateCategory.oauth"),
          options: oauthTemplates.map((template) => ({ value: template.id, label: template.name })),
        },
        {
          label: t("providers.templateCategory.api"),
          options: [
            ...apiTemplates.map((template) => ({ value: template.id, label: template.name })),
            { value: CUSTOM_PROVIDER_TEMPLATE, label: t("providers.customTemplate") },
          ],
        },
      ]}
      {...triggerProps}
    />
  );
}
