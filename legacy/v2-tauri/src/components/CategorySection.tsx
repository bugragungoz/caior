import { useState } from "react";
import type { Category, InstallStatus, PackageProgress } from "../types/manifest";
import { PackageCard } from "./PackageCard";

interface CategorySectionProps {
  category: Category;
  selected: Set<string>;
  installStatus: Record<string, InstallStatus>;
  progress: Record<string, PackageProgress>;
  onToggle: (id: string) => void;
  onSelectAll: (ids: string[]) => void;
  onDeselectAll: (ids: string[]) => void;
  disabled: boolean;
}

export function CategorySection({
  category,
  selected,
  installStatus,
  progress,
  onToggle,
  onSelectAll,
  onDeselectAll,
  disabled,
}: CategorySectionProps) {
  const [collapsed, setCollapsed] = useState(false);

  const availableIds = category.packages
    .filter((p) => p.url || p.drive_url)
    .map((p) => p.id);

  const selectedCount = availableIds.filter((id) => selected.has(id)).length;
  const allSelected = availableIds.length > 0 && selectedCount === availableIds.length;

  const handleHeaderToggle = () => {
    if (allSelected) {
      onDeselectAll(availableIds);
    } else {
      onSelectAll(availableIds);
    }
  };

  return (
    <section style={{ marginBottom: "8px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "10px",
          padding: "8px 16px 6px",
          cursor: "pointer",
        }}
        onClick={() => setCollapsed((c) => !c)}
      >
        <span
          className="material-symbols-rounded sz-18"
          style={{ color: "var(--md-on-surface-variant)" }}
        >
          {category.icon}
        </span>
        <h2
          style={{
            flex: 1,
            fontSize: "12px",
            fontWeight: 500,
            letterSpacing: "1.5px",
            textTransform: "uppercase",
            color: "var(--md-on-surface-variant)",
          }}
        >
          {category.label}
          {selectedCount > 0 && (
            <span
              style={{
                marginLeft: "8px",
                fontSize: "11px",
                fontWeight: 500,
                color: "var(--md-primary)",
                letterSpacing: "0",
                textTransform: "none",
              }}
            >
              ({selectedCount} selected)
            </span>
          )}
        </h2>

        {!disabled && availableIds.length > 0 && (
          <button
            className="btn-text"
            style={{ height: "28px", padding: "0 10px", fontSize: "12px" }}
            onClick={(e) => {
              e.stopPropagation();
              handleHeaderToggle();
            }}
          >
            {allSelected ? "None" : "All"}
          </button>
        )}

        <span
          className="material-symbols-rounded sz-18"
          style={{
            color: "var(--md-on-surface-variant)",
            transform: collapsed ? "rotate(-90deg)" : "rotate(0deg)",
            transition: "transform 0.2s",
          }}
        >
          expand_more
        </span>
      </div>

      {!collapsed && (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: "4px",
            padding: "0 12px 4px",
          }}
        >
          {category.packages.map((pkg) => (
            <PackageCard
              key={pkg.id}
              pkg={pkg}
              selected={selected.has(pkg.id)}
              installStatus={installStatus[pkg.id] ?? "unknown"}
              progress={progress[pkg.id] ?? null}
              onToggle={onToggle}
              disabled={disabled}
            />
          ))}
        </div>
      )}
    </section>
  );
}
