import { Plus } from "lucide-react";
import type { PropsWithChildren } from "react";
import { useTranslation } from "react-i18next";
import type { SavedConfiguration } from "../../shared/types";
import { AppSelect, IconButton } from "../ui";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../ui/primitives/tabs";

export function ConfigurationTabs({
  configurations,
  active,
  dirty,
  onSelect,
  onAdd,
  children,
}: PropsWithChildren<{
  configurations: SavedConfiguration[];
  active: "current" | string;
  dirty?: boolean;
  onSelect: (id: "current" | string) => void;
  onAdd: () => void;
}>) {
  const { t } = useTranslation();
  return (
    <Tabs value={active} onValueChange={onSelect} className="configuration-tabs-root">
      <div className="configuration-tabs-wrap">
        <TabsList variant="line" className="configuration-tabs">
          <TabsTrigger value="current" className="config-tab">
            {t("config.current")}
          </TabsTrigger>
          {configurations.map((configuration) => (
            <TabsTrigger key={configuration.id} value={configuration.id} className="config-tab">
              {configuration.name}
              {dirty && active === configuration.id ? (
                <span className="dirty-marker" aria-label={t("config.unsavedMarker")}>
                  •
                </span>
              ) : null}
            </TabsTrigger>
          ))}
        </TabsList>
        <div className="configuration-tab-actions">
          <IconButton label={t("config.add")} onClick={onAdd}>
            <Plus size={18} />
          </IconButton>
        </div>
        {configurations.length > 4 ? (
          <AppSelect
            aria-label={t("config.locate")}
            value={active}
            onValueChange={onSelect}
            groups={[
              {
                options: [
                  { value: "current", label: t("config.current") },
                  ...configurations.map((configuration) => ({
                    value: configuration.id,
                    label: configuration.name,
                  })),
                ],
              },
            ]}
          />
        ) : null}
      </div>
      <TabsContent value={active}>{children}</TabsContent>
    </Tabs>
  );
}
