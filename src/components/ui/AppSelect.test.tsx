import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Controller, useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { chooseSelectOption } from "../../test/select";
import { AppSelect } from "./AppSelect";

const groups = [
  {
    label: "Remote",
    options: [
      { value: "alpha", label: "Alpha provider" },
      {
        value: "unavailable",
        label: "Unavailable provider",
        disabled: true,
        description: "Not supported by this CLI",
      },
    ],
  },
];

describe("AppSelect", () => {
  it("maps its private empty option back to an empty business value", async () => {
    const onValueChange = vi.fn();
    const view = render(
      <AppSelect
        aria-label="Provider"
        value=""
        onValueChange={onValueChange}
        groups={groups}
        allowEmpty
        placeholder="Choose a provider"
      />,
    );

    const trigger = screen.getByRole("combobox", { name: "Provider" });
    expect(trigger).toHaveTextContent("Choose a provider");
    await chooseSelectOption(trigger, "Alpha provider");
    expect(onValueChange).toHaveBeenLastCalledWith("alpha");

    view.rerender(
      <AppSelect
        aria-label="Provider"
        value="alpha"
        onValueChange={onValueChange}
        groups={groups}
        allowEmpty
        placeholder="Choose a provider"
      />,
    );
    await chooseSelectOption(trigger, "Choose a provider");
    expect(onValueChange).toHaveBeenLastCalledWith("");
    expect(onValueChange).not.toHaveBeenCalledWith("__cliswitch_select_empty__");
  });

  it("keeps groups and disabled explanations in the Radix listbox", async () => {
    render(<AppSelect aria-label="Provider" value="" onValueChange={vi.fn()} groups={groups} />);
    await userEvent.click(screen.getByRole("combobox", { name: "Provider" }));

    expect(screen.getByRole("group", { name: "Remote" })).toBeInTheDocument();
    const disabled = screen.getByRole("option", { name: /Unavailable provider/ });
    expect(disabled).toHaveAttribute("data-disabled");
    expect(disabled).toHaveAttribute("title", "Not supported by this CLI");
    expect(disabled).toHaveTextContent("Not supported by this CLI");
  });

  it("works as a controlled React Hook Form field and marks the form dirty", async () => {
    function Harness() {
      const { control, formState } = useForm({ defaultValues: { provider: "" } });
      return (
        <>
          <Controller
            control={control}
            name="provider"
            rules={{ required: true }}
            render={({ field }) => (
              <AppSelect
                aria-label="Provider"
                value={field.value}
                onValueChange={field.onChange}
                onBlur={field.onBlur}
                name={field.name}
                ref={field.ref}
                groups={groups}
              />
            )}
          />
          <output>{formState.isDirty ? "dirty" : "pristine"}</output>
        </>
      );
    }

    render(<Harness />);
    await chooseSelectOption(screen.getByRole("combobox", { name: "Provider" }), "Alpha provider");
    expect(screen.getByText("dirty")).toBeInTheDocument();
  });

  it("keeps a missing selected option empty without replacing business state", () => {
    const onValueChange = vi.fn();
    render(
      <AppSelect
        aria-label="Provider"
        value="removed"
        onValueChange={onValueChange}
        groups={groups}
        placeholder="Choose a provider"
      />,
    );
    expect(screen.getByRole("combobox")).toHaveTextContent("Choose a provider");
    expect(onValueChange).not.toHaveBeenCalled();
  });
});
