import { Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { SavedConfiguration } from "../../shared/types";
import { IconButton, Select } from "../ui";

export function ConfigurationTabs({
  configurations,
  active,
  dirty,
  onSelect,
  onAdd,
}: {
  configurations: SavedConfiguration[];
  active: "current" | string;
  dirty?: boolean;
  onSelect: (id: "current" | string) => void;
  onAdd: () => void;
}) {
  const { t } = useTranslation();
  const ids = ["current", ...configurations.map((item) => item.id)];
  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const index = Math.max(0, ids.indexOf(active));
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? ids.length - 1
          : event.key === "ArrowRight"
            ? (index + 1) % ids.length
            : (index - 1 + ids.length) % ids.length;
    onSelect(ids[next]);
  };
  return (
    <div className="configuration-tabs-wrap">
      <div className="configuration-tabs" role="tablist" onKeyDown={onKeyDown}>
        <button
          role="tab"
          aria-selected={active === "current"}
          className={active === "current" ? "config-tab config-tab-active" : "config-tab"}
          onClick={() => onSelect("current")}
        >
          {t("config.current")}
        </button>
        {configurations.map((configuration) => (
          <button
            key={configuration.id}
            role="tab"
            aria-selected={active === configuration.id}
            className={active === configuration.id ? "config-tab config-tab-active" : "config-tab"}
            onClick={() => onSelect(configuration.id)}
          >
            {configuration.name}
            {dirty && active === configuration.id ? (
              <span className="dirty-marker" aria-label={t("config.unsavedMarker")}>
                •
              </span>
            ) : null}
          </button>
        ))}
        <IconButton label={t("config.add")} onClick={onAdd}>
          <Plus size={18} />
        </IconButton>
      </div>
      {configurations.length > 4 ? (
        <Select
          aria-label={t("config.locate")}
          value={active}
          onChange={(event) => onSelect(event.target.value)}
        >
          <option value="current">{t("config.current")}</option>
          {configurations.map((configuration) => (
            <option key={configuration.id} value={configuration.id}>
              {configuration.name}
            </option>
          ))}
        </Select>
      ) : null}
    </div>
  );
}
