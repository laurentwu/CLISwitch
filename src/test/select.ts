import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

export async function chooseSelectOption(
  trigger: HTMLElement,
  optionName: string | RegExp,
): Promise<void> {
  const user = userEvent.setup();
  await user.click(trigger);
  await user.click(await screen.findByRole("option", { name: optionName }));
}
