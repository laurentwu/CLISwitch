import { useTranslation } from "react-i18next";
import { catalogProviderInfos } from "../../shared/catalog";
import type { ProviderCatalog } from "../../shared/types";
import { Select } from "../ui";

export const CUSTOM_PROVIDER_TEMPLATE = "__custom-provider__";

export function ProviderTemplateSelect({
  catalog,
  value,
  onChange,
}: {
  catalog: ProviderCatalog;
  value: string;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  const dynamicProviders = catalogProviderInfos(catalog);
  if (dynamicProviders.length) {
    return (
      <Select value={value} onChange={(event) => onChange(event.target.value)}>
        <option value="">{t("providers.chooseTemplate")}</option>
        <optgroup label={t("providers.templateCategory.api")}>
          {dynamicProviders.map((provider) => (
            <option
              key={provider.id}
              value={provider.id}
              disabled={!provider.selectable}
              title={provider.disabledReason ?? undefined}
            >
              {provider.name} ({provider.id})
              {!provider.selectable && provider.disabledReason
                ? ` — ${provider.disabledReason}`
                : ""}
            </option>
          ))}
          <option value={CUSTOM_PROVIDER_TEMPLATE}>{t("providers.customTemplate")}</option>
        </optgroup>
        <optgroup label={t("providers.templateCategory.oauth")}>
          {catalog.providerTemplates
            .filter((template) => template.mode === "auth")
            .map((template) => (
              <option key={template.id} value={template.id}>
                {template.name}
              </option>
            ))}
        </optgroup>
      </Select>
    );
  }
  const oauthTemplates = catalog.providerTemplates.filter((template) => template.mode === "auth");
  const apiTemplates = catalog.providerTemplates.filter((template) => template.mode === "api");

  return (
    <Select value={value} onChange={(event) => onChange(event.target.value)}>
      <option value="">{t("providers.chooseTemplate")}</option>
      <optgroup label={t("providers.templateCategory.oauth")}>
        {oauthTemplates.map((template) => (
          <option key={template.id} value={template.id}>
            {template.name}
          </option>
        ))}
      </optgroup>
      <optgroup label={t("providers.templateCategory.api")}>
        {apiTemplates.map((template) => (
          <option key={template.id} value={template.id}>
            {template.name}
          </option>
        ))}
        <option value={CUSTOM_PROVIDER_TEMPLATE}>{t("providers.customTemplate")}</option>
      </optgroup>
    </Select>
  );
}
