import type { PropsWithChildren } from "react";
import { Boxes, Settings, SlidersHorizontal } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Navigation } from "../../stores/ui";

export function AppLayout({
  navigation,
  onNavigate,
  children,
}: PropsWithChildren<{ navigation: Navigation; onNavigate: (navigation: Navigation) => void }>) {
  const { t } = useTranslation();
  const items: Array<{ id: Navigation; icon: typeof SlidersHorizontal }> = [
    { id: "configuration", icon: SlidersHorizontal },
    { id: "providers", icon: Boxes },
    { id: "settings", icon: Settings },
  ];
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand" aria-label={t("appName")}>
          <span className="brand-mark">CS</span>
          <span>{t("appName")}</span>
        </div>
        <nav className="primary-nav" aria-label={t("accessibility.primaryNavigation")}>
          {items.map(({ id, icon: Icon }) => (
            <button
              key={id}
              className={navigation === id ? "nav-item nav-item-active" : "nav-item"}
              aria-current={navigation === id ? "page" : undefined}
              onClick={() => onNavigate(id)}
            >
              <Icon size={19} />
              <span>{t(`nav.${id}`)}</span>
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">0.1</div>
      </aside>
      <main className="main-content">{children}</main>
    </div>
  );
}
