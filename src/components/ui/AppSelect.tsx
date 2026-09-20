import { forwardRef, type AriaAttributes } from "react";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "./primitives/select";

export type SelectOption = {
  value: string;
  label: string;
  disabled?: boolean;
  description?: string;
};

export type SelectGroupData = { label?: string; options: SelectOption[] };

export type AppSelectProps = Pick<
  AriaAttributes,
  "aria-label" | "aria-labelledby" | "aria-describedby" | "aria-invalid"
> & {
  value: string;
  onValueChange: (value: string) => void;
  groups: SelectGroupData[];
  placeholder?: string;
  allowEmpty?: boolean;
  disabled?: boolean;
  id?: string;
  name?: string;
  onBlur?: () => void;
  className?: string;
};

export const AppSelect = forwardRef<HTMLButtonElement, AppSelectProps>(function AppSelect(
  {
    value,
    onValueChange,
    groups,
    placeholder,
    allowEmpty,
    disabled,
    id,
    name,
    onBlur,
    className,
    ...aria
  },
  ref,
) {
  const allOptions = groups.flatMap((group) => group.options);
  let emptyValue = "__cliswitch_select_empty__";
  while (allOptions.some((option) => option.value === emptyValue)) emptyValue += "_";
  const selected = allOptions.find((option) => option.value === value);
  const radixValue = selected ? value : allowEmpty ? emptyValue : "";

  return (
    <Select
      value={radixValue}
      name={name}
      disabled={disabled}
      onValueChange={(next) => {
        // Radix's native form bridge can emit a change while its options mount.
        // Only actual selectable transitions belong in the controlled business draft.
        if (next === radixValue) return;
        if (allowEmpty && next === emptyValue) onValueChange("");
        else if (allOptions.some((option) => option.value === next && !option.disabled)) {
          onValueChange(next);
        }
      }}
    >
      <SelectTrigger ref={ref} id={id} onBlur={onBlur} className={className} {...aria}>
        <SelectValue placeholder={placeholder}>{selected?.label}</SelectValue>
      </SelectTrigger>
      <SelectContent position="popper" align="start">
        {allowEmpty ? <SelectItem value={emptyValue}>{placeholder ?? "—"}</SelectItem> : null}
        {groups.map((group, groupIndex) => (
          <SelectGroup key={`${group.label ?? "group"}-${groupIndex}`}>
            {group.label ? <SelectLabel>{group.label}</SelectLabel> : null}
            {group.options.map((option) => (
              <SelectItem
                key={option.value}
                value={option.value}
                disabled={option.disabled}
                title={option.description}
              >
                <span className="app-select-option">
                  <span>{option.label}</span>
                  {option.description ? <small>{option.description}</small> : null}
                </span>
              </SelectItem>
            ))}
          </SelectGroup>
        ))}
      </SelectContent>
    </Select>
  );
});
