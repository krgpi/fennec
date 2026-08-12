import type { ReactNode } from "react";

export const inputClass =
  "rounded border border-neutral-300 dark:border-neutral-600 bg-transparent px-2 py-1 text-sm";

export const buttonClass =
  "px-2 py-1 rounded border border-neutral-300 dark:border-neutral-600 text-xs hover:bg-neutral-100 dark:hover:bg-neutral-700 disabled:opacity-50";

export function Section({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1">
      {title && <h3 className="text-xs font-semibold text-neutral-500">{title}</h3>}
      <div className="flex flex-col gap-3 rounded-lg border border-neutral-200 dark:border-neutral-700 p-3">
        {children}
      </div>
    </section>
  );
}

export function ToggleRow({
  label,
  description,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  description?: string;
  checked: boolean;
  disabled?: boolean;
  onChange?: (checked: boolean) => void;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label
        className={`flex items-center gap-2 text-sm ${disabled ? "opacity-50" : ""}`}
      >
        <input
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={(e) => onChange?.(e.target.checked)}
        />
        <span>{label}</span>
      </label>
      {description && <p className="pl-6 text-xs text-neutral-500">{description}</p>}
    </div>
  );
}
